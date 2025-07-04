use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tracing::{Instrument, info_span};

/// Simple counter used to log the amount of Riot API requests performed.
#[derive(Debug)]
pub struct RequestMetrics {
    start: Instant,
    count: AtomicU64,
    name: &'static str,
}

impl RequestMetrics {
    pub fn new(name: &'static str) -> Arc<Self> {
        Arc::new(Self {
            start: Instant::now(),
            count: AtomicU64::new(0),
            name,
        })
    }

    pub fn inc(&self) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn log_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_secs(60)); // Log per
        // minutes
        loop {
            let span = info_span!("📊 ", client = self.name);
            async {
                interval.tick().await;
                let total = self.count.load(Ordering::Relaxed);
                let elapsed_min = self.start.elapsed().as_secs_f64() / 60.0;
                let avg = if elapsed_min > 0.0 {
                    total as f64 / elapsed_min
                } else {
                    0.0
                };
                tracing::info!("{} requests executed (avg {:.2} req/min)", total, avg);
            }
            .instrument(span)
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn inc_increases_count() {
        let metrics = RequestMetrics::new("test");
        metrics.inc();
        metrics.inc();

        let metrics = Arc::try_unwrap(metrics).expect("arc should be unique");
        assert_eq!(metrics.count.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn log_loop_runs_once() {
        tokio::time::pause();

        let metrics = RequestMetrics::new("test");
        let cloned = metrics.clone();
        let handle = tokio::spawn(async move { cloned.log_loop().await });

        tokio::time::advance(Duration::from_secs(61)).await;
        handle.abort();
        let _ = handle.await;
    }
}
