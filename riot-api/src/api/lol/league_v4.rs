use std::sync::Arc;

use serde::Deserialize;

use crate::{
    api::client::ApiClient,
    types::{LeaguePoints, Region, RiotApiResponse},
};

#[derive(Debug)]
pub struct LeagueV4Api(Arc<ApiClient>);

impl LeagueV4Api {
    pub fn new(api_client: Arc<ApiClient>) -> Self {
        Self(api_client)
    }

    pub async fn get_leagues(
        &self,
        puuid: String,
        region: Region,
    ) -> RiotApiResponse<Vec<LeagueEntryDto>> {
        tracing::trace!("[RIOT::CLIENT] get_league {} in {:?}", puuid, region);

        let path = format!(
            "https://{}/lol/league/v4/entries/by-puuid/{}",
            region.to_endpoint(),
            puuid,
        );

        self.0.request(path).await
    }
}

/// Representation of the league entry response.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LeagueEntryDto {
    pub queue_type: String,
    pub tier: String,
    pub rank: String,
    pub league_points: LeaguePoints,
}

impl LeagueEntryDto {
    pub fn is_ranked_solo_duo(&self) -> bool {
        self.queue_type.eq("RANKED_SOLO_5x5")
    }
}
