use serde::Deserialize;
use tentrackule_shared::lol_match::{Match, MatchParticipant};

/// Representation of the match data response.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MatchDto {
    pub info: InfoDto,
}

impl From<MatchDto> for Match {
    fn from(value: MatchDto) -> Self {
        Self {
            participants: value
                .info
                .participants
                .into_iter()
                .map(|participant| participant.into())
                .collect(),
            queue_id: value.info.queue_id,
            game_duration: value.info.game_duration,
            game_creation: value.info.game_creation,
        }
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

impl From<ParticipantDto> for MatchParticipant {
    fn from(value: ParticipantDto) -> Self {
        Self {
            puuid: value.puuid,
            champion_name: value.champion_name,
            team_position: value.team_position,
            win: value.win,
            kills: value.kills,
            deaths: value.deaths,
            assists: value.assists,
            profile_icon: value.profile_icon,
            riot_id_game_name: value.riot_id_game_name,
            riot_id_tagline: value.riot_id_tagline,
        }
    }
}
