use super::client::RiotClient;
use super::region::{Platform, Region};
use super::types::{AccountDto, AccountRegionDto, ChampionMasteryDto, LeagueEntryDto, MatchDto};
use crate::error::AppError;

impl RiotClient {
    pub async fn get_account_by_riot_id(
        &self,
        region: Region,
        game_name: &str,
        tag_line: &str,
    ) -> Result<AccountDto, AppError> {
        let url = format!(
            "{}/riot/account/v1/accounts/by-riot-id/{}/{}",
            region.base_url(),
            urlencoding::encode(game_name),
            urlencoding::encode(tag_line)
        );

        self.get(&url).await.map_err(|e| {
            if matches!(&e, AppError::RiotApi { status: 404, .. }) {
                AppError::PlayerNotFound {
                    game_name: game_name.to_string(),
                    tag_line: tag_line.to_string(),
                }
            } else {
                e
            }
        })
    }

    pub async fn get_active_platform(
        &self,
        region: Region,
        puuid: &str,
    ) -> Result<Platform, AppError> {
        let url = format!(
            "{}/riot/account/v1/region/by-game/lol/by-puuid/{}",
            region.base_url(),
            puuid
        );

        let dto: AccountRegionDto = self.get(&url).await?;
        dto.region.parse()
    }

    pub async fn get_league_entries_by_puuid(
        &self,
        platform: Platform,
        puuid: &str,
    ) -> Result<Vec<LeagueEntryDto>, AppError> {
        let url = format!(
            "{}/lol/league/v4/entries/by-puuid/{}",
            platform.base_url(),
            puuid
        );

        self.get(&url).await
    }

    pub async fn get_champion_mastery(
        &self,
        platform: Platform,
        puuid: &str,
        champion_id: i64,
    ) -> Result<ChampionMasteryDto, AppError> {
        let url = format!(
            "{}/lol/champion-mastery/v4/champion-masteries/by-puuid/{}/by-champion/{}",
            platform.base_url(),
            puuid,
            champion_id
        );

        self.get(&url).await
    }

    pub async fn get_match_ids(
        &self,
        region: Region,
        puuid: &str,
        count: u32,
    ) -> Result<Vec<String>, AppError> {
        let url = format!(
            "{}/lol/match/v5/matches/by-puuid/{}/ids?count={}",
            region.base_url(),
            puuid,
            count
        );

        self.get(&url).await
    }

    pub async fn get_match(&self, region: Region, match_id: &str) -> Result<MatchDto, AppError> {
        let url = format!("{}/lol/match/v5/matches/{}", region.base_url(), match_id);

        self.get(&url).await
    }
}
