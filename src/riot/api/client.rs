use std::{fmt::Debug, sync::Arc};

use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use nonzero_ext::nonzero;
use reqwest::StatusCode;
use serde::{de::DeserializeOwned, Deserialize};

use crate::riot::types::{RiotApiError, RiotApiResponse};

use super::metrics::RequestMetrics;

pub struct ApiClient {
    pub client: reqwest::Client,
    pub limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
    /// Riot API Key
    key: String,
    metrics: Arc<RequestMetrics>,
}

impl ApiClient {
    pub fn new(key: String, metrics: Arc<RequestMetrics>) -> Self {
        let q = Quota::per_minute(nonzero!(100_u32)).allow_burst(nonzero!(20_u32));

        Self {
            client: reqwest::Client::new(),
            limiter: RateLimiter::direct(q),
            key,
            metrics,
        }
    }

    // Account-V1 endpoint
    const ACCOUNT_ROUTE: &'static str = "https://europe.api.riotgames.com/riot/account/v1/accounts";

    pub async fn request<T: DeserializeOwned + Debug>(&self, path: String) -> RiotApiResponse<T> {
        self.metrics.inc();

        let res = self
            .client
            .get(path)
            .header("X-Riot-Token", &self.key)
            .send()
            .await
            .map_err(RiotApiError::Reqwest)?;
        match res.status() {
            StatusCode::OK => res.json().await.map_err(RiotApiError::Reqwest),
            _ => Err(RiotApiError::Status(res.status())),
        }
    }

    pub async fn get_account_by_riot_id(
        &self,
        game_name: String,
        tag_line: String,
    ) -> RiotApiResponse<AccountDto> {
        tracing::trace!(
            "[RIOT::CLIENT] get_account_by_riot_id {}#{}",
            game_name,
            tag_line
        );
        let path = format!(
            "{}/by-riot-id/{}/{}",
            Self::ACCOUNT_ROUTE,
            game_name,
            tag_line
        );

        self.request(path).await
    }
}

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
    use dotenv::dotenv;

    use super::ApiClient;
    use crate::riot::api::metrics::RequestMetrics;

    fn api_key() -> String {
        dotenv().ok();
        std::env::var("RIOT_API_KEY").unwrap()
    }

    #[tokio::test]
    #[ignore = "API Key required"]
    async fn get_account_by_riot_id_works() {
        let metrics = RequestMetrics::new();
        let client = ApiClient::new(api_key(), metrics);

        let account = client
            .get_account_by_riot_id("Chalop".to_string(), "3012".to_string())
            .await
            .unwrap();

        assert_eq!(
            account.puuid,
            "jG0VKFsMuF2aWaQoiDxJ1brhlXyMY7kj4HfIAucciWH_9YVdWVpbQDIRhJWQQGhP89qCrp5EwLxl3Q"
        );
        assert_eq!(account.game_name, Some("Chalop".to_string()));
        assert_eq!(account.tag_line, Some("3012".to_string()))
    }

    #[tokio::test]
    async fn request_propagates_reqwest_error() {
        let fake_key = "RGAPI-INVALID-KEY".to_string();
        let metrics = super::super::metrics::RequestMetrics::new();
        let client = super::ApiClient::new(fake_key, metrics);

        let bad_url = "ht!tp://invalid-url".to_string(); // incorrect schema

        let res: super::RiotApiResponse<()> = client.request(bad_url).await;

        assert!(matches!(res, Err(super::RiotApiError::Reqwest(_))));
    }
}
