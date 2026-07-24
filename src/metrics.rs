//! Lightweight runtime metrics, logged once an hour (Phase 6).
//!
//! A single shared [`Metrics`] is threaded into the Riot client (API call
//! volume), the image cache (hit-rate), and the renderer (latency), plus the
//! poller (cycle count). [`Metrics::run_reporter`] drains and logs them on a
//! fixed interval, then resets the window so each line covers the last hour —
//! giving the before/after numbers the Phase 3.B/4 exit criteria ask for without
//! standing up a metrics backend.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::time::{MissedTickBehavior, interval};
use tracing::info;

/// How often the stats line is emitted.
const REPORT_INTERVAL: Duration = Duration::from_secs(60 * 60); // 1h

/// Cap on render samples retained per window, so a pathological burst can't grow
/// the latency buffer without bound. Far above the handful of renders/hour seen
/// in practice; extra samples in a window are simply dropped from the percentile.
const MAX_RENDER_SAMPLES: usize = 4096;

/// Shared, cheap-to-update counters for the hourly stats line. All the hot-path
/// updates are relaxed atomics (or a briefly-held mutex for the latency vec), so
/// instrumentation never contends with real work.
#[derive(Debug, Default)]
pub struct Metrics {
    /// Outbound Riot API requests — every HTTP attempt, retries included, so the
    /// number reflects true API volume (Phase 4's "appels/jour").
    riot_api_calls: AtomicU64,
    /// Poll cycles run (one per tick, including idle ticks that polled nobody).
    poll_cycles: AtomicU64,
    /// Asset reads served without a network fetch (in-RAM or disk hit).
    cache_hits: AtomicU64,
    /// Asset reads that fell through to a network fetch.
    cache_misses: AtomicU64,
    /// Successful render round-trip durations (ms) this window, for percentiles.
    render_latencies_ms: Mutex<Vec<u32>>,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// One outbound Riot API request was sent.
    pub fn record_api_call(&self) {
        self.riot_api_calls.fetch_add(1, Ordering::Relaxed);
    }

    /// One poll cycle ran.
    pub fn record_poll_cycle(&self) {
        self.poll_cycles.fetch_add(1, Ordering::Relaxed);
    }

    /// An asset was served from the cache (no network).
    pub fn record_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// An asset read missed the cache and went to the network.
    pub fn record_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Record one successful render's wall-clock round-trip latency.
    pub fn record_render(&self, latency: Duration) {
        let mut samples = self
            .render_latencies_ms
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if samples.len() < MAX_RENDER_SAMPLES {
            samples.push(latency.as_millis().min(u32::MAX as u128) as u32);
        }
    }

    /// Log the stats line every [`REPORT_INTERVAL`], then reset the window. Runs
    /// until the process exits; spawn it on the shared `Arc<Metrics>`.
    pub async fn run_reporter(self: Arc<Self>) {
        let mut ticker = interval(REPORT_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // The first tick fires immediately; skip it (the window would be empty).
        ticker.tick().await;
        loop {
            ticker.tick().await;
            self.report();
        }
    }

    /// Snapshot-and-reset every counter, then emit one structured line. Splitting
    /// this out of the loop keeps it unit-testable.
    fn report(&self) {
        let api_calls = self.riot_api_calls.swap(0, Ordering::Relaxed);
        let cycles = self.poll_cycles.swap(0, Ordering::Relaxed);
        let hits = self.cache_hits.swap(0, Ordering::Relaxed);
        let misses = self.cache_misses.swap(0, Ordering::Relaxed);

        let mut samples = std::mem::take(
            &mut *self
                .render_latencies_ms
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        samples.sort_unstable();

        let calls_per_cycle = if cycles > 0 {
            api_calls as f64 / cycles as f64
        } else {
            0.0
        };
        let cache_total = hits + misses;
        let hit_rate = if cache_total > 0 {
            hits as f64 / cache_total as f64 * 100.0
        } else {
            0.0
        };

        info!(
            api_calls,
            cycles,
            calls_per_cycle = format!("{calls_per_cycle:.1}"),
            cache_hits = hits,
            cache_misses = misses,
            cache_hit_rate = format!("{hit_rate:.1}%"),
            renders = samples.len(),
            render_p50_ms = percentile(&samples, 50),
            render_p95_ms = percentile(&samples, 95),
            // 0 when unreadable (e.g. non-Linux local dev); prod is Linux.
            rss_mb = resident_set_size_mb().unwrap_or(0),
            "📊 hourly stats"
        );
    }
}

/// Nearest-rank `p`-th percentile (0–100) of a **pre-sorted** slice; 0 if empty.
fn percentile(sorted: &[u32], p: u32) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let n = sorted.len();
    // rank = ceil(p * n / 100), 1-based, then clamp to a valid index.
    let rank = (p as usize * n).div_ceil(100);
    let idx = rank.saturating_sub(1).min(n - 1);
    sorted[idx]
}

/// Resident set size in MB from `/proc/self/statm` (Linux). `None` elsewhere
/// (e.g. local macOS dev), where the stats line logs 0.
fn resident_set_size_mb() -> Option<u64> {
    // Fields are page counts: `size resident shared …`; field 1 is resident.
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let resident_pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    // 4 KiB is the page size on x86-64/aarch64 Linux (where the bot runs); good
    // enough for an order-of-magnitude RSS readout without pulling in libc.
    const PAGE_SIZE: u64 = 4096;
    Some(resident_pages * PAGE_SIZE / (1024 * 1024))
}

#[cfg(test)]
mod tests {
    use super::{Metrics, percentile};

    #[test]
    fn percentile_nearest_rank() {
        assert_eq!(percentile(&[], 50), 0, "empty → 0");

        let one = [42];
        assert_eq!(percentile(&one, 50), 42);
        assert_eq!(percentile(&one, 95), 42);

        // 1..=100 sorted: p50 → 50th value, p95 → 95th value (nearest-rank).
        let hundred: Vec<u32> = (1..=100).collect();
        assert_eq!(percentile(&hundred, 50), 50);
        assert_eq!(percentile(&hundred, 95), 95);
        assert_eq!(percentile(&hundred, 100), 100);
    }

    #[test]
    fn render_samples_are_bounded_and_cleared_on_report() {
        let m = Metrics::new();
        for i in 0..(super::MAX_RENDER_SAMPLES + 10) {
            m.record_render(std::time::Duration::from_millis(i as u64));
        }
        assert_eq!(
            m.render_latencies_ms.lock().unwrap().len(),
            super::MAX_RENDER_SAMPLES,
            "buffer must be capped"
        );

        m.report();
        assert!(
            m.render_latencies_ms.lock().unwrap().is_empty(),
            "report drains the latency buffer"
        );
    }

    #[test]
    fn counters_reset_each_report_window() {
        let m = Metrics::new();
        m.record_api_call();
        m.record_api_call();
        m.record_poll_cycle();
        m.record_cache_hit();
        m.record_cache_miss();

        // A report snapshots and zeroes; a second report sees a clean window.
        m.report();
        use std::sync::atomic::Ordering::Relaxed;
        assert_eq!(m.riot_api_calls.load(Relaxed), 0);
        assert_eq!(m.poll_cycles.load(Relaxed), 0);
        assert_eq!(m.cache_hits.load(Relaxed), 0);
        assert_eq!(m.cache_misses.load(Relaxed), 0);
    }
}
