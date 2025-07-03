use async_trait::async_trait;
use bytes::Bytes;
use tentrackule_shared::{
    Account, Region,
    tft_match::{self, Match},
    traits::api::{AccountApi, ApiError, ApiRequest, MatchApi},
};

use crate::types::RiotApiError;

use super::client::ApiClientBase;

/// High level client implementing all LoL related APIs used by the bot.
#[derive(Debug)]
pub struct TftApiClient(ApiClientBase);

impl TftApiClient {
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
impl ApiRequest for TftApiClient {
    async fn request(&self, path: String) -> Result<Bytes, ApiError> {
        self.0.request(path).await
    }
}

#[async_trait]
impl AccountApi for TftApiClient {
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
impl MatchApi<Match> for TftApiClient {
    async fn get_last_match_id(
        &self,
        puuid: String,
        region: Region,
    ) -> Result<Option<String>, ApiError> {
        tracing::trace!(
            "[TFT-MATCH-V1 API] get_last_match_id {} in {:?}",
            puuid,
            region
        );

        let params = "?start=0&count=1";
        let path = format!(
            "https://{}/tft/match/v1/matches/by-puuid/{}/ids/{}",
            region.to_global_endpoint(),
            puuid,
            params
        );

        let raw = self.request(path).await?;
        let seq: Vec<String> = serde_json::from_slice(&raw).map_err(RiotApiError::Serde)?;

        Ok(seq.first().cloned())
    }

    async fn get_match(
        &self,
        match_id: String,
        region: Region,
    ) -> Result<tft_match::Match, ApiError> {
        tracing::trace!("[TFT-MATCH-V1 API] get_match {} in {:?}", match_id, region);

        let path = format!(
            "https://{}/tft/match/v1/matches/{}",
            region.to_global_endpoint(),
            match_id,
        );

        let raw = self.request(path).await?;
        Ok(serde_json::from_slice(&raw).map_err(RiotApiError::Serde)?)
    }
}
