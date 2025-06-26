use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Simple counter used to log the amount of Riot API requests performed.
#[derive(Debug)]
pub struct RequestMetrics {
    start: Instant,
    count: AtomicU64,
}

impl RequestMetrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            start: Instant::now(),
            count: AtomicU64::new(0),
        })
    }

    pub fn inc(&self) {
        self.count.fetch_add(1, Ordering::Relaxed);
    }

    pub async fn log_loop(self: Arc<Self>) {
        let mut interval = tokio::time::interval(Duration::from_secs(60)); // Log per
                                                                           // minutes
        loop {
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
    }
}
