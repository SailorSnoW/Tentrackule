use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use reqwest::Client;
use serde::de::DeserializeOwned;
use tracing::{debug, error, trace, warn};

use crate::config::USER_AGENT;
use crate::error::AppError;
use crate::metrics::Metrics;

type GovernorRateLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

const MAX_ATTEMPTS: u32 = 3;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(2);
const SERVER_ERROR_RETRY_DELAY: Duration = Duration::from_secs(1);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(120);

#[derive(Clone, Debug)]
pub struct RiotClient {
    http: Client,
    api_key: String,
    limiter_per_second: Arc<GovernorRateLimiter>,
    limiter_per_window: Arc<GovernorRateLimiter>,
    metrics: Arc<Metrics>,
}

impl RiotClient {
    pub fn new(
        api_key: String,
        rate_limit_per_second: NonZeroU32,
        rate_limit_per_two_min: NonZeroU32,
        metrics: Arc<Metrics>,
    ) -> Result<Self, AppError> {
        let limiter_per_second = Arc::new(RateLimiter::direct(Quota::per_second(
            rate_limit_per_second,
        )));

        let window_quota =
            Quota::with_period(Duration::from_secs(120) / rate_limit_per_two_min.get())
                .expect("120s / N is a non-zero period for N >= 1")
                .allow_burst(rate_limit_per_two_min);
        let limiter_per_window = Arc::new(RateLimiter::direct(window_quota));

        let http = Client::builder().user_agent(USER_AGENT).build()?;

        Ok(Self {
            http,
            api_key,
            limiter_per_second,
            limiter_per_window,
            metrics,
        })
    }

    pub async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, AppError> {
        let endpoint = url
            .split("api.riotgames.com")
            .nth(1)
            .and_then(|s| s.split('?').next())
            .unwrap_or(url);

        let mut attempt: u32 = 0;
        loop {
            attempt += 1;

            self.limiter_per_window.until_ready().await;
            self.limiter_per_second.until_ready().await;

            trace!(endpoint, attempt, "🔷 API request");
            self.metrics.record_api_call();

            let response = self
                .http
                .get(url)
                .header("X-Riot-Token", &self.api_key)
                .send()
                .await?;

            let status = response.status();
            let code = status.as_u16();

            if status.is_success() {
                debug!(endpoint, status = code, "🔷 ✅ API success");
                return Ok(response.json::<T>().await?);
            }

            if code == 429 && attempt < MAX_ATTEMPTS {
                let delay = retry_after(&response)
                    .unwrap_or(DEFAULT_RETRY_DELAY)
                    .min(MAX_RETRY_DELAY);
                warn!(
                    endpoint,
                    status = 429,
                    attempt,
                    delay_s = delay.as_secs(),
                    "🔷 ⚠️ Rate limited; retrying after Retry-After"
                );
                tokio::time::sleep(delay).await;
                continue;
            }
            if status.is_server_error() && attempt < MAX_ATTEMPTS {
                warn!(
                    endpoint,
                    status = code,
                    attempt,
                    "🔷 ⚠️ Server error; retrying"
                );
                tokio::time::sleep(SERVER_ERROR_RETRY_DELAY).await;
                continue;
            }

            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            match code {
                404 => {
                    debug!(endpoint, status = 404, "🔷 Not found");
                }
                429 => {
                    warn!(
                        endpoint,
                        status = 429,
                        attempt,
                        "🔷 ⚠️ Rate limited (retries exhausted)"
                    );
                }
                403 => {
                    error!(endpoint, status = 403, "🔷 ❌ Forbidden - check API key");
                }
                _ => {
                    error!(
                        endpoint,
                        status = code,
                        error_message = %message,
                        "🔷 ❌ API error"
                    );
                }
            }

            return Err(AppError::RiotApi {
                status: code,
                message,
            });
        }
    }
}

fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    let secs: u64 = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(secs))
}
