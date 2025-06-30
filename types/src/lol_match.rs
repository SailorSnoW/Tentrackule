use std::sync::Arc;

use poise::serenity_prelude::Colour;
use tentrackule_riot_api::{
    api::types::{LeagueApi, LeagueEntryDto, MatchDto, ParticipantDto},
    types::RiotMatchError,
};
use urlencoding::encode;

use crate::{ddragon_version, traits::CachedLeagueSource, Account, CachedLeague, QueueType};

/// We directly use the API type as it doesn't need any change for the moment.
pub type OnlineLeague = LeagueEntryDto;

pub struct Match {
    pub participants: Vec<MatchParticipant>,
    pub queue_id: u16,
    pub game_duration: u64,
    pub game_creation: u128,
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

impl Match {
    pub fn participant(&self, puuid: &str) -> Option<&MatchParticipant> {
        self.participants.iter().find(|p| p.puuid == puuid)
    }

    pub fn queue_type(&self) -> QueueType {
        self.queue_id.into()
    }

    pub fn participant_info_of(&self, puuid: &str) -> Option<&MatchParticipant> {
        self.participants.iter().find(|p| p.puuid == puuid)
    }

    pub fn to_formatted_match_duration(&self) -> String {
        let minutes = self.game_duration / 60;
        let seconds = self.game_duration % 60;
        format!("{:02}:{:02}", minutes, seconds)
    }

    pub async fn try_into_match_ranked<C>(
        self,
        ranking_of: &Account,
        api: Arc<dyn LeagueApi>,
        cache: &C,
    ) -> Result<MatchRanked, RiotMatchError>
    where
        C: CachedLeagueSource,
    {
        let queue_type: QueueType = self.queue_id.into();
        let cached_league = cache
            .get_league_for(ranking_of.puuid.clone(), queue_type)
            .await
            .map_err(|e| RiotMatchError::CantRetrieveCachedLeague(e))?
            .ok_or(RiotMatchError::NoCachedLeagueFound(
                ranking_of.puuid.clone(),
                self.queue_id,
            ))?;

        let current_leagues = api
            .get_leagues(ranking_of.puuid.clone(), Box::new(ranking_of.region))
            .await
            .map_err(RiotMatchError::RiotApiError)?;
        let current_league = current_leagues
            .into_iter()
            .find(|league| league.queue_type.eq(queue_type.as_str()))
            .ok_or(RiotMatchError::NoApiLeagueFound(
                queue_type.as_str().to_string(),
                ranking_of.puuid.clone(),
            ))?;

        Ok(MatchRanked {
            base: self,
            current_league,
            cached_league,
        })
    }
}

pub struct MatchParticipant {
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

impl MatchParticipant {
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
            encode(&self.riot_id_game_name),
            encode(&self.riot_id_tagline)
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

pub struct MatchRanked {
    pub base: Match,
    pub current_league: OnlineLeague,
    pub cached_league: CachedLeague,
}

impl MatchRanked {
    /// Calculate the gain/loss of LP between the cached value and the new match data.
    /// Returns a positive number for LP gain, negative for LP loss, or None if data is missing.
    pub fn calculate_league_points_difference(&self, won: bool) -> i16 {
        let current_league = &self.current_league;

        let cached = &self.cached_league;

        let mut diff = current_league.league_points as i16 - cached.points as i16;

        if (diff < 0 && won) || (diff > 0 && !won) {
            diff += if won { 100 } else { -100 };
        }

        diff
    }
}
