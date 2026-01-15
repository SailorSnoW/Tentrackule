use std::num::NonZeroU32;
use std::sync::Arc;

use governor::clock::DefaultClock;
use governor::state::{InMemoryState, NotKeyed};
use governor::{Quota, RateLimiter};
use reqwest::Client;
use serde::de::DeserializeOwned;
use tracing::{debug, error, trace, warn};

use crate::error::AppError;

type GovernorRateLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

#[derive(Clone, Debug)]
pub struct RiotClient {
    http: Client,
    api_key: String,
    rate_limiter: Arc<GovernorRateLimiter>,
}

impl RiotClient {
    pub fn new(api_key: String, rate_limit_per_second: NonZeroU32) -> Self {
        let quota = Quota::per_second(rate_limit_per_second);
        let rate_limiter = Arc::new(RateLimiter::direct(quota));

        let http = Client::builder()
            .user_agent("Tentrackule/2.0")
            .build()
            .expect("Failed to build HTTP client");

        Self {
            http,
            api_key,
            rate_limiter,
        }
    }

    pub async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, AppError> {
        // Wait for rate limiter
        self.rate_limiter.until_ready().await;

        // Extract endpoint for logging (remove base URL and query params)
        let endpoint = url
            .split("api.riotgames.com")
            .nth(1)
            .and_then(|s| s.split('?').next())
            .unwrap_or(url);

        trace!(endpoint, "🔷 API request");

        let response = self
            .http
            .get(url)
            .header("X-Riot-Token", &self.api_key)
            .send()
            .await?;

        let status = response.status();

        if status.is_success() {
            debug!(endpoint, status = status.as_u16(), "🔷 ✅ API success");
            let body = response.json::<T>().await?;
            Ok(body)
        } else {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            match status.as_u16() {
                404 => {
                    debug!(endpoint, status = 404, "🔷 Not found");
                }
                429 => {
                    warn!(endpoint, status = 429, "🔷 ⚠️ Rate limited");
                }
                403 => {
                    error!(endpoint, status = 403, "🔷 ❌ Forbidden - check API key");
                }
                _ => {
                    error!(
                        endpoint,
                        status = status.as_u16(),
                        error_message = %message,
                        "🔷 ❌ API error"
                    );
                }
            }

            Err(AppError::RiotApi {
                status: status.as_u16(),
                message,
            })
        }
    }
}
