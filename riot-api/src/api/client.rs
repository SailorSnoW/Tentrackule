use std::{fmt::Debug, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use futures::TryFutureExt;
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use nonzero_ext::nonzero;
use reqwest::StatusCode;
use serde::Deserialize;

use crate::types::{RiotApiError, RiotApiResponse};

use super::metrics::RequestMetrics;

#[async_trait]
pub trait ApiRequest: Send + Sync + Debug {
    async fn request(&self, path: String) -> RiotApiResponse<Bytes>;
}

#[async_trait]
pub trait AccountApi: ApiRequest {
    fn route(&self) -> &'static str {
        "https://europe.api.riotgames.com/riot/account/v1/accounts"
    }

    async fn get_account_by_riot_id(
        &self,
        game_name: String,
        tag_line: String,
    ) -> RiotApiResponse<AccountDto> {
        tracing::trace!(
            "[AccountV1 API] get_account_by_riot_id {}#{}",
            game_name,
            tag_line
        );
        let path = format!(
            "{}/by-riot-id/{}/{}",
            Self::route(self),
            game_name,
            tag_line
        );

        let raw = self.request(path).await?;
        serde_json::from_slice(&raw).map_err(RiotApiError::Serde)
    }
}

#[derive(Debug)]
pub struct ApiClientBase {
    pub client: reqwest::Client,
    pub limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
    /// Riot API Key
    key: String,
    pub metrics: Arc<RequestMetrics>,
}

impl ApiClientBase {
    pub fn new(api_key: String) -> Self {
        let q = Quota::per_minute(nonzero!(100_u32)).allow_burst(nonzero!(20_u32));

        Self {
            client: reqwest::Client::new(),
            limiter: RateLimiter::direct(q),
            key: api_key,
            metrics: RequestMetrics::new(),
        }
    }
}

#[async_trait]
impl ApiRequest for ApiClientBase {
    async fn request(&self, path: String) -> RiotApiResponse<Bytes> {
        // Ensure we do not enforce the RIOT API rate limits before doing any request
        self.limiter.until_ready().await;
        self.metrics.inc();

        let res = self
            .client
            .get(path)
            .header("X-Riot-Token", &self.key)
            .send()
            .await
            .map_err(RiotApiError::Reqwest)?;
        match res.status() {
            StatusCode::OK => res.bytes().map_err(RiotApiError::Reqwest).await,
            _ => Err(RiotApiError::Status(res.status())),
        }
    }
}

#[async_trait]
impl AccountApi for ApiClientBase {}

/// Representation of the account data response.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AccountDto {
    pub puuid: String,
    pub game_name: Option<String>,
    pub tag_line: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{AccountApi, ApiClientBase, ApiRequest};
    use dotenv::dotenv;
    use std::env;

    #[tokio::test]
    #[ignore = "API Key required"]
    async fn get_account_by_riot_id_works() {
        let key = env::var("RIOT_API_KEY")
            .expect("A Riot API Key must be set in environment to create the API Client.");
        let client = ApiClientBase::new(key);

        let account = client
            .get_account_by_riot_id("Le Conservateur".to_string(), "3012".to_string())
            .await
            .unwrap();

        assert_eq!(
            account.puuid,
            "jG0VKFsMuF2aWaQoiDxJ1brhlXyMY7kj4HfIAucciWH_9YVdWVpbQDIRhJWQQGhP89qCrp5EwLxl3Q"
        );
        assert_eq!(account.game_name, Some("Le Conservateur".to_string()));
        assert_eq!(account.tag_line, Some("3012".to_string()))
    }

    #[tokio::test]
    async fn request_propagates_reqwest_error() {
        dotenv().ok();
        env::set_var("RIOT_API_KEY", "TEST_KEY");
        let key = env::var("RIOT_API_KEY")
            .expect("A Riot API Key must be set in environment to create the API Client.");
        let client = ApiClientBase::new(key);

        let bad_url = "ht!tp://invalid-url".to_string(); // incorrect schema

        let res = client.request(bad_url).await;

        assert!(matches!(res, Err(super::RiotApiError::Reqwest(_))));
    }
}
