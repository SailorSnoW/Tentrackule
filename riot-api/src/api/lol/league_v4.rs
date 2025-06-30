use async_trait::async_trait;
use serde::Deserialize;

use crate::{
    api::{client::ApiRequest, traits::RegionEndpoint},
    types::{RiotApiError, RiotApiResponse},
};

#[async_trait]
pub trait LeagueApi: ApiRequest {
    async fn get_leagues(
        &self,
        puuid: String,
        region: Box<dyn RegionEndpoint>,
    ) -> RiotApiResponse<Vec<LeagueEntryDto>> {
        tracing::trace!("[LeagueV4 API] get_league {} in {:?}", puuid, region);

        let path = format!(
            "https://{}/lol/league/v4/entries/by-puuid/{}",
            region.to_endpoint(),
            puuid,
        );

        let raw = self.request(path).await?;
        serde_json::from_slice(&raw).map_err(RiotApiError::Serde)
    }
}

/// Representation of the league entry response.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LeagueEntryDto {
    pub queue_type: String,
    pub tier: String,
    pub rank: String,
    pub league_points: u16,
    pub wins: u16,
    pub losses: u16,
}

impl LeagueEntryDto {
    pub fn is_ranked_solo_duo(&self) -> bool {
        self.queue_type.eq("RANKED_SOLO_5x5")
    }

    pub fn is_ranked_flex(&self) -> bool {
        self.queue_type.eq("RANKED_FLEX_SR")
    }
}
