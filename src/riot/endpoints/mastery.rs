use crate::error::AppError;
use crate::riot::client::RiotClient;
use crate::riot::region::Platform;
use crate::riot::types::ChampionMasteryDto;

impl RiotClient {
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
}
