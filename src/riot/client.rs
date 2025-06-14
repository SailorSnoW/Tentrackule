use std::{fmt::Debug, sync::Arc};

use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use nonzero_ext::nonzero;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;

use super::types::{AccountDto, LeagueEntryDto, MatchDto, Region, RiotApiError, RiotApiResponse};

pub struct RiotClient {
    pub client: reqwest::Client,
    pub limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
    /// Riot API Key
    key: String,
    metrics: Arc<super::metrics::RequestMetrics>,
}

impl RiotClient {
    pub fn new(key: String, metrics: Arc<super::metrics::RequestMetrics>) -> Self {
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

        request(self.client.clone(), self.key.clone(), path, &self.metrics).await
    }

    // Match-V5 endpoint
    pub async fn get_last_match_id(
        &self,
        puuid: String,
        region: Region,
    ) -> RiotApiResponse<Option<String>> {
        tracing::trace!("[RIOT::CLIENT] get_last_match_id {} in {:?}", puuid, region);

        let params = "?start=0&count=1";
        let path = format!(
            "https://{}/lol/match/v5/matches/by-puuid/{}/ids/{}",
            region.to_global_endpoint(),
            puuid,
            params
        );

        let seq: Vec<String> =
            request(self.client.clone(), self.key.clone(), path, &self.metrics).await?;

        Ok(seq.first().cloned())
    }

    pub async fn get_match(&self, match_id: String, region: Region) -> RiotApiResponse<MatchDto> {
        tracing::trace!("[RIOT::CLIENT] get_match {} in {:?}", match_id, region);

        let path = format!(
            "https://{}/lol/match/v5/matches/{}",
            region.to_global_endpoint(),
            match_id,
        );

        request(self.client.clone(), self.key.clone(), path, &self.metrics).await
    }

    // LEAGUE-V4 endpoint
    pub async fn get_leagues(
        &self,
        puuid: String,
        region: Region,
    ) -> RiotApiResponse<Vec<LeagueEntryDto>> {
        tracing::trace!("[RIOT::CLIENT] get_league {} in {:?}", puuid, region);

        let path = format!(
            "https://{}/lol/league/v4/entries/by-puuid/{}",
            region.to_endpoint(),
            puuid,
        );

        request(self.client.clone(), self.key.clone(), path, &self.metrics).await
    }
}

/// Helper function which wrap the shared requests logic.
async fn request<T: DeserializeOwned + Debug>(
    client: reqwest::Client,
    key: String,
    path: String,
    metrics: &super::metrics::RequestMetrics,
) -> Result<T, RiotApiError> {
    metrics.inc();

    let res = client
        .get(path)
        .header("X-Riot-Token", key)
        .send()
        .await
        .map_err(RiotApiError::Reqwest)?;
    match res.status() {
        StatusCode::OK => res.json().await.map_err(RiotApiError::Reqwest),
        _ => Err(RiotApiError::Status(res.status())),
    }
}

#[cfg(test)]
mod tests {
    use crate::riot::types::Region;

    use super::RiotClient;
    use dotenv::dotenv;

    fn api_key() -> String {
        dotenv().ok();
        std::env::var("RIOT_API_KEY").unwrap()
    }

    #[tokio::test]
    #[ignore = "API Key required"]
    async fn get_account_by_riot_id_works() {
        let metrics = super::super::metrics::RequestMetrics::new();
        let client = RiotClient::new(api_key(), metrics);

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
    #[ignore = "API Key required"]
    async fn get_last_match_id_works() {
        let metrics = super::super::metrics::RequestMetrics::new();
        let client = RiotClient::new(api_key(), metrics);

        let puuid =
            "jG0VKFsMuF2aWaQoiDxJ1brhlXyMY7kj4HfIAucciWH_9YVdWVpbQDIRhJWQQGhP89qCrp5EwLxl3Q"
                .to_string();

        let match_id = client.get_last_match_id(puuid, Region::Euw).await.unwrap();

        println!("Last Match ID fetched: {:?}", match_id);

        assert!(!match_id.unwrap().is_empty());
    }

    #[tokio::test]
    #[ignore = "API Key required"]
    async fn get_match_works() {
        let metrics = super::super::metrics::RequestMetrics::new();
        let client = RiotClient::new(api_key(), metrics);

        let match_id = "EUW1_7349112729".to_string();

        let match_data = client.get_match(match_id, Region::Euw).await.unwrap();

        println!("Match data fetched: {:?}", match_data);
    }

    #[tokio::test]
    #[ignore = "API Key required"]
    async fn get_league() {
        let metrics = super::super::metrics::RequestMetrics::new();
        let client = RiotClient::new(api_key(), metrics);

        let puuid =
            "jG0VKFsMuF2aWaQoiDxJ1brhlXyMY7kj4HfIAucciWH_9YVdWVpbQDIRhJWQQGhP89qCrp5EwLxl3Q"
                .to_string();

        let leagues = client.get_leagues(puuid, Region::Euw).await.unwrap();

        println!("Leagues data fetched: {:?}", leagues);
    }

    #[tokio::test]
    async fn request_propagates_reqwest_error() {
        let client = reqwest::Client::new();
        let metrics = super::super::metrics::RequestMetrics::new();
        let res: super::RiotApiResponse<()> =
            super::request(client, String::new(), "invalid-url".into(), &metrics).await;

        assert!(matches!(res, Err(super::RiotApiError::Reqwest(_))));
    }
}
