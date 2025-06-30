use async_trait::async_trait;
use serde::Deserialize;

use crate::{
    api::{client::ApiRequest, traits::RegionEndpoint},
    types::{RiotApiError, RiotApiResponse},
};

#[async_trait]
pub trait MatchApi: ApiRequest {
    async fn get_last_match_id(
        &self,
        puuid: String,
        region: impl RegionEndpoint,
    ) -> RiotApiResponse<Option<String>> {
        tracing::trace!("[MatchV5 API] get_last_match_id {} in {:?}", puuid, region);

        let params = "?start=0&count=1";
        let path = format!(
            "https://{}/lol/match/v5/matches/by-puuid/{}/ids/{}",
            region.to_global_endpoint(),
            puuid,
            params
        );

        let raw = self.request(path).await?;
        let seq: Vec<String> = serde_json::from_slice(&raw).map_err(RiotApiError::Serde)?;

        Ok(seq.first().cloned())
    }

    async fn get_match(
        &self,
        match_id: String,
        region: impl RegionEndpoint,
    ) -> RiotApiResponse<MatchDto> {
        tracing::trace!("[MatchV5 API] get_match {} in {:?}", match_id, region);

        let path = format!(
            "https://{}/lol/match/v5/matches/{}",
            region.to_global_endpoint(),
            match_id,
        );

        let raw = self.request(path).await?;
        serde_json::from_slice(&raw).map_err(RiotApiError::Serde)
    }
}

/// Representation of the match data response.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MatchDto {
    pub info: InfoDto,
}

impl MatchDto {
    pub fn queue_type<T: From<u16>>(&self) -> T {
        T::from(self.info.queue_id)
    }
}

/// Representation of the match info data response.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InfoDto {
    pub participants: Vec<ParticipantDto>,
    pub queue_id: u16,
    pub game_duration: u64,
    pub game_creation: u128,
}

/// Representation of the participant data response.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantDto {
    pub puuid: String,
    pub champion_name: String,
    pub team_position: String,
    pub win: bool,
    pub kills: u16,
    pub deaths: u16,
    pub assists: u16,
    pub profile_icon: u16,
    pub riot_id_game_name: String,
    pub riot_id_tagline: String,
}
