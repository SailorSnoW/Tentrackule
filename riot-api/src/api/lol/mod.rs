use async_trait::async_trait;
use bytes::Bytes;
use std::fmt::Debug;
use tracing::warn;

use crate::types::{LeaguePoints, RiotApiResponse};

use super::{
    client::{AccountApi, ApiClientBase, ApiRequest},
    types::AccountDto,
};

pub mod league_v4;
pub mod match_v5;

pub use league_v4::LeagueApi;
use league_v4::LeagueEntryDto;
pub use match_v5::MatchApi;
use match_v5::MatchDto;

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

/// Match data enriched with league information and cached LPs.
#[derive(Debug, Clone)]
pub struct MatchDtoWithLeagueInfo {
    pub match_data: MatchDto,
    pub league_data: Option<LeagueEntryDto>,
    pub cached_league_points: Option<LeaguePoints>,
}

impl MatchDtoWithLeagueInfo {
    pub fn new(
        match_data: MatchDto,
        league_data: Option<LeagueEntryDto>,
        cached_league_points: Option<LeaguePoints>,
    ) -> Self {
        Self {
            match_data,
            league_data,
            cached_league_points,
        }
    }

    /// Calculate the gain/loss of LP between the cached value and the new match data.
    /// Returns a positive number for LP gain, negative for LP loss, or None if data is missing.
    pub fn calculate_league_points_difference(&self, won: bool) -> Option<i16> {
        let Some(league_data) = &self.league_data else {
            warn!("no league data for LP diff");
            return None;
        };

        let Some(cached) = self.cached_league_points else {
            warn!("cached LPs missing, diff ignored");
            return None;
        };

        let mut diff = league_data.league_points as i16 - cached as i16;

        if (diff < 0 && won) || (diff > 0 && !won) {
            diff += if won { 100 } else { -100 };
        }

        Some(diff)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use match_v5::dummy_match;

    fn dummy_league_entry(lp: LeaguePoints) -> LeagueEntryDto {
        LeagueEntryDto {
            queue_type: "RANKED_SOLO_5x5".to_string(),
            tier: "GOLD".to_string(),
            rank: "IV".to_string(),
            league_points: lp,
        }
    }
    #[test]
    fn league_difference_is_calculated() {
        let match_data = dummy_match();
        let league_data = Some(dummy_league_entry(100));

        let match_with_info = MatchDtoWithLeagueInfo::new(match_data, league_data, Some(90));

        assert_eq!(
            match_with_info.calculate_league_points_difference(true),
            Some(10)
        );
    }

    #[test]
    fn win_with_rank_up_adjusts_difference() {
        let match_data = dummy_match();
        let league_data = Some(dummy_league_entry(20));

        let match_with_info = MatchDtoWithLeagueInfo::new(match_data, league_data, Some(90));

        assert_eq!(
            match_with_info.calculate_league_points_difference(true),
            Some(30)
        );
    }

    #[test]
    fn loss_with_rank_down_adjusts_difference() {
        let match_data = dummy_match();
        let league_data = Some(dummy_league_entry(80));

        let match_with_info = MatchDtoWithLeagueInfo::new(match_data, league_data, Some(20));

        assert_eq!(
            match_with_info.calculate_league_points_difference(false),
            Some(-40)
        );
    }

    #[test]
    fn returns_none_when_data_missing() {
        let match_data = dummy_match();

        let with_no_league = MatchDtoWithLeagueInfo::new(match_data.clone(), None, Some(90));
        assert_eq!(
            with_no_league.calculate_league_points_difference(true),
            None
        );

        let with_no_cached =
            MatchDtoWithLeagueInfo::new(match_data, Some(dummy_league_entry(100)), None);
        assert_eq!(
            with_no_cached.calculate_league_points_difference(true),
            None
        );
    }
}
