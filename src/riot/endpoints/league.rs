use crate::error::AppError;
use crate::riot::client::RiotClient;
use crate::riot::region::Platform;
use crate::riot::types::LeagueEntryDto;

impl RiotClient {
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
}
