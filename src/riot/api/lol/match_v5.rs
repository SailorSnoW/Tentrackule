use std::{env, sync::Arc};

use poise::serenity_prelude::Colour;
use serde::Deserialize;

use crate::riot::{
    api::client::ApiClient,
    types::{QueueType, Region, RiotApiResponse},
};

pub struct MatchV5Api(Arc<ApiClient>);

impl MatchV5Api {
    pub fn new(api_client: Arc<ApiClient>) -> Self {
        Self(api_client)
    }

    pub async fn get_last_match_id(
        &self,
        puuid: String,
        region: Region,
    ) -> RiotApiResponse<Option<String>> {
        tracing::trace!("[RIOT::CLIENT] get_last_match_id {} in {:?}", puuid, region);

        let params = "?start=0&count=1";
        let path = format!(
            "https://{}/lol/match/v5/matches/by-puuid/{}/ids/{}",
            region.to_global_endpoint(),
            puuid,
            params
        );

        let seq: Vec<String> = self.0.request(path).await?;

        Ok(seq.first().cloned())
    }

    pub async fn get_match(&self, match_id: String, region: Region) -> RiotApiResponse<MatchDto> {
        tracing::trace!("[RIOT::CLIENT] get_match {} in {:?}", match_id, region);

        let path = format!(
            "https://{}/lol/match/v5/matches/{}",
            region.to_global_endpoint(),
            match_id,
        );

        self.0.request(path).await
    }
}

fn ddragon_version() -> String {
    env::var("DDRAGON_VERSION").unwrap_or_else(|_| "15.12.1".to_string())
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
    pub fn to_normalized_role(&self) -> String {
        match self.team_position.as_str() {
            "TOP" => "Top".to_string(),
            "JUNGLE" => "Jungle".to_string(),
            "MIDDLE" => "Mid".to_string(),
            "BOTTOM" => "AD Carry".to_string(),
            "UTILITY" => "Support".to_string(),
            _ => "".to_string(),
        }
    }
    pub fn to_profile_icon_picture_url(&self) -> String {
        format!(
            "https://ddragon.leagueoflegends.com/cdn/{}/img/profileicon/{}.png",
            ddragon_version(),
            self.profile_icon
        )
    }
    pub fn to_champion_picture_url(&self) -> String {
        let mut champion_name = self.champion_name.clone();
        if self.champion_name == "FiddleSticks" {
            champion_name = "Fiddlesticks".to_string()
        }
        format!(
            "https://ddragon.leagueoflegends.com/cdn/{}/img/champion/{}.png",
            ddragon_version(),
            champion_name
        )
    }
    pub fn to_dpm_profile_url(&self) -> String {
        format!(
            "https://dpm.lol/{}-{}",
            self.riot_id_game_name, self.riot_id_tagline
        )
    }
    pub fn to_title_win_string(&self, lp_info: Option<i16>) -> String {
        let lp_info_str = match lp_info {
            Some(p) => format!(" ({:+} LPs)", p),
            None => "".to_string(),
        };
        let result_str = match self.win {
            true => "Victory".to_string(),
            false => "Defeat".to_string(),
        };

        format!("{}{}", result_str, lp_info_str)
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

#[cfg(test)]
pub use tests::dummy_match;

#[cfg(test)]
mod tests {
    use crate::riot::api::lol::match_v5::ParticipantDto;

    use super::*;

    fn dummy_participant(puuid: &str) -> ParticipantDto {
        ParticipantDto {
            puuid: puuid.into(),
            champion_name: "Lux".into(),
            team_position: "MIDDLE".into(),
            win: true,
            kills: 5,
            deaths: 2,
            assists: 8,
            profile_icon: 1234,
            riot_id_game_name: "Game".into(),
            riot_id_tagline: "Tag".into(),
        }
    }

    pub fn dummy_match() -> MatchDto {
        MatchDto {
            info: InfoDto {
                participants: vec![],
                queue_id: 420,
                game_duration: 0,
                game_creation: 0,
            },
        }
    }

    #[test]
    fn match_helpers_work() {
        let participant = dummy_participant("abc");
        let match_data = MatchDto {
            info: InfoDto {
                participants: vec![participant.clone()],
                queue_id: 420,
                game_duration: 125,
                game_creation: 0,
            },
        };

        assert!(matches!(match_data.queue_type(), QueueType::SoloDuo));
        assert!(match_data.participant_info_of("abc").is_some());
        assert!(match_data.participant_info_of("missing").is_none());
        assert_eq!(match_data.to_formatted_match_duration(), "02:05");
    }

    #[test]
    fn participant_helpers_work() {
        let p = dummy_participant("abc");

        assert_eq!(p.to_normalized_role(), "Mid");
        assert_eq!(
            p.to_profile_icon_picture_url(),
            "https://ddragon.leagueoflegends.com/cdn/15.12.1/img/profileicon/1234.png"
        );
        assert_eq!(
            p.to_champion_picture_url(),
            "https://ddragon.leagueoflegends.com/cdn/15.12.1/img/champion/Lux.png"
        );
        assert_eq!(p.to_dpm_profile_url(), "https://dpm.lol/Game-Tag");
        assert_eq!(p.to_title_win_string(Some(12)), "Victory (+12 LPs)");
        assert_eq!(p.to_formatted_win_string(), "won");
        assert_eq!(p.to_win_colour(), Colour::from_rgb(39, 98, 218));
    }

    #[test]
    fn fiddlesticks_picture_url_work() {
        let mut p = dummy_participant("abc");
        p.champion_name = "FiddleSticks".to_string();

        assert_eq!(
            p.to_champion_picture_url(),
            "https://ddragon.leagueoflegends.com/cdn/15.12.1/img/champion/Fiddlesticks.png"
        );
    }
}
