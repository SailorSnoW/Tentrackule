use async_trait::async_trait;
use bytes::Bytes;
use std::fmt::Debug;

use crate::types::RiotApiResponse;

use super::{
    client::{AccountApi, ApiClientBase, ApiRequest},
    types::AccountDto,
};

pub mod league_v4;
pub mod match_v5;

pub use league_v4::LeagueApi;
pub use match_v5::MatchApi;

/// All APIs required for the entire LoL required scope of the bot.
pub trait LolApiFull: LeagueApi + MatchApi + AccountApi {}

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
    async fn request(&self, path: String) -> RiotApiResponse<Bytes> {
        self.0.request(path).await
    }
}

impl LeagueApi for LolApiClient {}
impl MatchApi for LolApiClient {}
impl LolApiFull for LolApiClient {}

#[async_trait]
impl AccountApi for LolApiClient {
    async fn get_account_by_riot_id(
        &self,
        game_name: String,
        tag_line: String,
    ) -> RiotApiResponse<AccountDto> {
        self.0.get_account_by_riot_id(game_name, tag_line).await
    }
}
