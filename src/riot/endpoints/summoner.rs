use crate::error::AppError;
use crate::riot::{Platform, RiotClient, SummonerDto};

impl RiotClient {
    /// Get summoner by PUUID (for profile icon)
    pub async fn get_summoner_by_puuid(
        &self,
        platform: Platform,
        puuid: &str,
    ) -> Result<SummonerDto, AppError> {
        let url = format!(
            "https://{}.api.riotgames.com/lol/summoner/v4/summoners/by-puuid/{}",
            platform.as_str(),
            puuid
        );

        self.get(&url).await
    }
}
