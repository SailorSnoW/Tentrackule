use poise::serenity_prelude::Colour;
use serde::Deserialize;

/// A call to Riot API can either result in a success with the success type or fail with a
/// status code for the request.
pub type RiotApiResponse<T> = Result<T, reqwest::StatusCode>;

#[derive(Debug, Clone)]
pub struct MatchDtoWithLeagueInfo {
    pub match_data: MatchDto,
    pub league_data: Option<LeagueEntryDto>,
}

impl MatchDtoWithLeagueInfo {
    pub fn new(match_data: MatchDto, league_data: Option<LeagueEntryDto>) -> Self {
        Self {
            match_data,
            league_data,
        }
    }
}

/// Representation of the match data response.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MatchDto {
    pub info: InfoDto,
}

impl MatchDto {
    pub fn queue_type(&self) -> QueueType {
        self.info.queue_id.into()
    }

    pub fn participant_info_of(&self, puuid: &str) -> Option<&ParticipantDto> {
        self.info.participants.iter().find(|p| p.puuid == puuid)
    }

    pub fn to_formatted_match_duration(&self) -> String {
        let minutes = self.info.game_duration / 60;
        let seconds = self.info.game_duration % 60;
        format!("{:02}:{:02}", minutes, seconds)
    }
}

/// Representation of the match info data response.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct InfoDto {
    pub participants: Vec<ParticipantDto>,
    pub queue_id: u16,
    pub game_duration: u64,
    pub game_creation: u64,
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

impl ParticipantDto {
    pub fn to_profile_icon_picture_url(&self) -> String {
        format!(
            "https://ddragon.leagueoflegends.com/cdn/15.7.1/img/profileicon/{}.png",
            self.profile_icon
        )
    }
    pub fn to_champion_picture_url(&self) -> String {
        format!(
            "https://ddragon.leagueoflegends.com/cdn/15.7.1/img/champion/{}.png",
            self.champion_name
        )
    }
    pub fn to_dpm_profile_url(&self) -> String {
        format!(
            "https://dpm.lol/{}-{}",
            self.riot_id_game_name, self.riot_id_tagline
        )
    }
    pub fn to_title_win_string(&self) -> String {
        match self.win {
            true => "Victory".to_string(),
            false => "Defeat".to_string(),
        }
    }
    pub fn to_formatted_win_string(&self) -> String {
        match self.win {
            true => "won".to_string(),
            false => "lost".to_string(),
        }
    }

    pub fn to_win_colour(&self) -> Colour {
        match self.win {
            true => Colour::from_rgb(39, 98, 218),
            false => Colour::from_rgb(226, 54, 112),
        }
    }
}

/// Representation of the account data response.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AccountDto {
    pub puuid: String,
    pub game_name: Option<String>,
    pub tag_line: Option<String>,
}

/// Representation of the league entry response.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LeagueEntryDto {
    pub queue_type: String,
    pub tier: String,
    pub rank: String,
    pub league_points: u8,
}

impl LeagueEntryDto {
    pub fn is_ranked_solo_duo(&self) -> bool {
        self.queue_type.eq("RANKED_SOLO_5x5")
    }
}

#[derive(Debug, Clone, poise::ChoiceParameter)]
pub enum Region {
    Na,
    Euw,
    Eune,
    Oce,
    Ru,
    Tr,
    Br,
    Lan,
    Las,
    Jp,
    Kr,
    Tw,
}

impl Region {
    pub fn to_global_endpoint(&self) -> String {
        match self {
            Region::Lan => "americas.api.riotgames.com".to_string(),
            Region::Las => "americas.api.riotgames.com".to_string(),
            Region::Na => "americas.api.riotgames.com".to_string(),
            Region::Br => "americas.api.riotgames.com".to_string(),
            Region::Euw => "europe.api.riotgames.com".to_string(),
            Region::Eune => "europe.api.riotgames.com".to_string(),
            Region::Tr => "europe.api.riotgames.com".to_string(),
            Region::Ru => "europe.api.riotgames.com".to_string(),
            Region::Kr => "asia.api.riotgames.com".to_string(),
            Region::Jp => "asia.api.riotgames.com".to_string(),
            Region::Oce => "sea.api.riotgames.com".to_string(),
            Region::Tw => "sea.api.riotgames.com".to_string(),
        }
    }

    pub fn to_endpoint(&self) -> String {
        match self {
            Region::Lan => "la1.api.riotgames.com".to_string(),
            Region::Las => "la2.api.riotgames.com".to_string(),
            Region::Na => "na1.api.riotgames.com".to_string(),
            Region::Br => "br1.api.riotgames.com".to_string(),
            Region::Euw => "euw1.api.riotgames.com".to_string(),
            Region::Eune => "eun1.api.riotgames.com".to_string(),
            Region::Tr => "tr1.api.riotgames.com".to_string(),
            Region::Ru => "ru.api.riotgames.com".to_string(),
            Region::Kr => "kr.api.riotgames.com".to_string(),
            Region::Jp => "jp1.api.riotgames.com".to_string(),
            Region::Oce => "oc1.api.riotgames.com".to_string(),
            Region::Tw => "tw2.api.riotgames.com".to_string(),
        }
    }
}

impl From<Region> for String {
    fn from(region: Region) -> Self {
        match region {
            Region::Lan => "LAN".to_string(),
            Region::Las => "LAS".to_string(),
            Region::Na => "NA".to_string(),
            Region::Br => "BR".to_string(),
            Region::Euw => "EUW".to_string(),
            Region::Eune => "EUNE".to_string(),
            Region::Tr => "TR".to_string(),
            Region::Ru => "RU".to_string(),
            Region::Kr => "KR".to_string(),
            Region::Jp => "JP".to_string(),
            Region::Oce => "OCE".to_string(),
            Region::Tw => "TW".to_string(),
        }
    }
}

impl TryFrom<String> for Region {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        match value.to_uppercase().as_str() {
            "LAN" => Ok(Region::Lan),
            "LAS" => Ok(Region::Las),
            "NA" => Ok(Region::Na),
            "BR" => Ok(Region::Br),
            "EUW" => Ok(Region::Euw),
            "EUNE" => Ok(Region::Eune),
            "TR" => Ok(Region::Tr),
            "RU" => Ok(Region::Ru),
            "KR" => Ok(Region::Kr),
            "JP" => Ok(Region::Jp),
            "OCE" => Ok(Region::Oce),
            "TW" => Ok(Region::Tw),
            _ => Err(format!("Unknown region: {}", value)),
        }
    }
}

pub enum QueueType {
    /// Ranked Solo/Duo
    SoloDuo,
    Unhandled,
}

impl From<u16> for QueueType {
    fn from(value: u16) -> Self {
        match value {
            420 => Self::SoloDuo,
            _ => Self::Unhandled,
        }
    }
}
