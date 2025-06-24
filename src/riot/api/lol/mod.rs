use std::sync::Arc;
use tracing::warn;

use crate::riot::types::LeaguePoints;

use super::client::ApiClient;

pub mod league_v4;
pub mod match_v5;

use league_v4::LeagueEntryDto;
pub use league_v4::LeagueV4Api;
use match_v5::MatchDto;
pub use match_v5::MatchV5Api;

#[derive(Debug)]
pub struct LolApi {
    pub client: Arc<ApiClient>,
    pub match_v5: MatchV5Api,
    pub league_v4: LeagueV4Api,
}

impl LolApi {
    pub fn new(api_client: Arc<ApiClient>) -> Self {
        let api = Self {
            client: api_client.clone(),
            match_v5: MatchV5Api::new(api_client.clone()),
            league_v4: LeagueV4Api::new(api_client),
        };

        // Start the metrics logger
        let metrics = api.client.metrics.clone();
        tokio::spawn(async move {
            metrics.log_loop().await;
        });

        api
    }
}

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
            warn!("⚠️ [RIOT] no league data for LP diff");
            return None;
        };

        let Some(cached) = self.cached_league_points else {
            warn!("⚠️ [RIOT] cached LPs missing, diff ignored");
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
