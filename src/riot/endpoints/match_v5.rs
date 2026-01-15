use crate::error::AppError;
use crate::riot::client::RiotClient;
use crate::riot::region::Region;
use crate::riot::types::MatchDto;

impl RiotClient {
    /// Get list of match IDs by PUUID
    /// Uses regional routing (americas, europe, asia, sea)
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

    /// Get match details by match ID
    /// Uses regional routing (americas, europe, asia, sea)
    pub async fn get_match(&self, region: Region, match_id: &str) -> Result<MatchDto, AppError> {
        let url = format!("{}/lol/match/v5/matches/{}", region.base_url(), match_id);

        self.get(&url).await
    }
}
