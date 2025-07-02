use async_trait::async_trait;
use bytes::Bytes;
use match_v5::MatchDto;
use std::fmt::Debug;
use tentrackule_shared::{
    Account, League, Region,
    lol_match::Match,
    traits::api::{AccountApi, ApiError, ApiRequest, LeagueApi, LolApiFull, MatchApi},
};

use crate::types::RiotApiError;

use super::client::ApiClientBase;

pub mod match_v5;

/// High level client implementing all LoL related APIs used by the bot.
#[derive(Debug)]
pub struct LolApiClient(ApiClientBase);

impl LolApiClient {
    /// Create a new API client using the provided key.
    pub fn new(api_key: String) -> Self {
        Self(ApiClientBase::new(api_key))
    }

    /// Spawn a task logging periodic metrics about requests.
    pub fn start_metrics_logging(&self) {
        let metrics = self.0.metrics.clone();
        tokio::spawn(async move { metrics.log_loop().await });
    }
}

#[async_trait]
impl ApiRequest for LolApiClient {
    async fn request(&self, path: String) -> Result<Bytes, ApiError> {
        self.0.request(path).await
    }
}

#[async_trait]
impl LeagueApi for LolApiClient {
    async fn get_leagues(&self, puuid: String, region: Region) -> Result<Vec<League>, ApiError> {
        tracing::trace!("[LeagueV4 API] get_league {} in {:?}", puuid, region);

        let path = format!(
            "https://{}/lol/league/v4/entries/by-puuid/{}",
            region.to_endpoint(),
            puuid,
        );

        let raw = self.request(path).await?;
        serde_json::from_slice(&raw).map_err(|e| RiotApiError::Serde(e).into())
    }
}

#[async_trait]
impl AccountApi for LolApiClient {
    fn route(&self) -> &'static str {
        self.0.route()
    }

    async fn get_account_by_riot_id(
        &self,
        game_name: String,
        tag_line: String,
    ) -> Result<Account, ApiError> {
        self.0.get_account_by_riot_id(game_name, tag_line).await
    }
}

#[async_trait]
impl MatchApi for LolApiClient {
    async fn get_last_match_id(
        &self,
        puuid: String,
        region: Region,
    ) -> Result<Option<String>, ApiError> {
        tracing::trace!("[MatchV5 API] get_last_match_id {} in {:?}", puuid, region);

        let params = "?start=0&count=1";
        let path = format!(
            "https://{}/lol/match/v5/matches/by-puuid/{}/ids/{}",
            region.to_global_endpoint(),
            puuid,
            params
        );

        let raw = self.request(path).await?;
        let seq: Vec<String> = serde_json::from_slice(&raw).map_err(RiotApiError::Serde)?;

        Ok(seq.first().cloned())
    }

    async fn get_match(&self, match_id: String, region: Region) -> Result<Match, ApiError> {
        tracing::trace!("[MatchV5 API] get_match {} in {:?}", match_id, region);

        let path = format!(
            "https://{}/lol/match/v5/matches/{}",
            region.to_global_endpoint(),
            match_id,
        );

        let raw = self.request(path).await?;
        let match_dto: MatchDto = serde_json::from_slice(&raw).map_err(RiotApiError::Serde)?;

        Ok(match_dto.into())
    }
}

impl LolApiFull for LolApiClient {}
