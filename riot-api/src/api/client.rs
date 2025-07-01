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
use tentrackule_shared::{
    traits::api::{AccountApi, ApiError, ApiRequest},
    Account, Region,
};

use crate::types::RiotApiError;

use super::metrics::RequestMetrics;

/// Basic HTTP client used to perform requests against Riot endpoints.
#[derive(Debug)]
pub struct ApiClientBase {
    pub client: reqwest::Client,
    pub limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
    /// Riot API Key
    key: String,
    pub metrics: Arc<RequestMetrics>,
}

impl ApiClientBase {
    /// Create a new client using the provided Riot API key.
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
    async fn request(&self, path: String) -> Result<Bytes, ApiError> {
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
            StatusCode::OK => {
                res.bytes()
                    .map_err(|e| RiotApiError::Reqwest(e).into())
                    .await
            }
            _ => Err(RiotApiError::Status(res.status()).into()),
        }
    }
}

#[async_trait]
impl AccountApi for ApiClientBase {
    fn route(&self) -> &'static str {
        if cfg!(test) {
            "http://europe.api.riotgames.com/riot/account/v1/accounts"
        } else {
            "https://europe.api.riotgames.com/riot/account/v1/accounts"
        }
    }

    async fn get_account_by_riot_id(
        &self,
        game_name: String,
        tag_line: String,
    ) -> Result<Account, ApiError> {
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
        let account_dto: AccountDto = serde_json::from_slice(&raw).map_err(RiotApiError::Serde)?;

        Ok(account_dto.into())
    }
}

/// Representation of the account data response.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AccountDto {
    pub puuid: String,
    pub game_name: Option<String>,
    pub tag_line: Option<String>,
}

impl From<AccountDto> for Account {
    fn from(value: AccountDto) -> Self {
        Self {
            puuid: value.puuid,
            game_name: value.game_name.unwrap(),
            tag_line: value.tag_line.unwrap(),
            region: Region::Euw,
            last_match_id: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        api::{metrics::RequestMetrics, types::AccountDto},
        types::RiotApiError,
    };

    use super::{ApiClientBase, ApiRequest};
    use dotenv::dotenv;
    use governor::{Quota, RateLimiter};
    use nonzero_ext::nonzero;
    use serde_json::json;
    use std::env;
    use tentrackule_shared::Account;

    #[tokio::test]
    async fn request_propagates_reqwest_error() {
        dotenv().ok();
        env::set_var("RIOT_API_KEY", "TEST_KEY");
        let key = env::var("RIOT_API_KEY")
            .expect("A Riot API Key must be set in environment to create the API Client.");
        let client = ApiClientBase::new(key);

        let bad_url = "ht!tp://invalid-url".to_string(); // incorrect schema

        let res = client.request(bad_url).await;

        assert!(res
            .as_ref()
            .err()
            .and_then(|e| e.downcast_ref::<RiotApiError>())
            .map(|e| matches!(e, RiotApiError::Reqwest(_)))
            .unwrap_or(false));
    }

    #[test]
    fn account_dto_into_account() {
        let dto = AccountDto {
            puuid: "id".to_string(),
            game_name: Some("Name".to_string()),
            tag_line: Some("Tag".to_string()),
        };

        let account: Account = dto.clone().into();
        assert_eq!(account.puuid, dto.puuid);
        assert_eq!(account.game_name, "Name".to_string());
        assert_eq!(account.tag_line, "Tag".to_string());
    }

    #[tokio::test]
    async fn get_account_by_riot_id_local_mock() {
        use httpmock::prelude::*;

        let server = MockServer::start_async().await;
        let mock = server.mock(|when, then| {
            when.method(GET)
                .path("/riot/account/v1/accounts/by-riot-id/Game/Tag");
            then.status(200).json_body(json!({
                "puuid": "puuid1",
                "gameName": "Game",
                "tagLine": "Tag"
            }));
        });

        let client = reqwest::Client::new();
        let quota = Quota::per_minute(nonzero!(100_u32)).allow_burst(nonzero!(20_u32));
        let api = ApiClientBase {
            client,
            limiter: RateLimiter::direct(quota),
            key: "KEY".to_string(),
            metrics: RequestMetrics::new(),
        };

        let route = format!(
            "{}/riot/account/v1/accounts/by-riot-id/Game/Tag",
            server.base_url()
        );
        let raw = api.request(route).await.unwrap();
        let account_dto: AccountDto = serde_json::from_slice(&raw).unwrap();
        let account: Account = account_dto.into();

        mock.assert();
        assert_eq!(account.game_name, "Game".to_string());
        assert_eq!(account.tag_line, "Tag".to_string());
    }
}
