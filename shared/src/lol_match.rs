use std::{
    fmt::{self, Display},
    sync::Arc,
};

use poise::serenity_prelude::Colour;
use tracing::warn;
use urlencoding::encode;

use crate::{
    Account, League, QueueTyped, UnifiedQueueType, ddragon_version,
    errors::RiotMatchError,
    traits::{
        CachedLeagueSource, QueueKind,
        api::{LeagueApi, LeaguePoints, LeagueQueueType},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueueType {
    /// Ranked Solo/Duo
    SoloDuo,
    /// Ranked Flex
    Flex,
    /// 5v5 Normal Draft Picks
    NormalDraft,
    /// 5v5 Howling Abyss ARAM
    Aram,
    Unhandled,
}

impl From<u16> for QueueType {
    fn from(value: u16) -> Self {
        match value {
            400 => Self::NormalDraft,
            420 => Self::SoloDuo,
            440 => Self::Flex,
            450 => Self::Aram,
            _ => Self::Unhandled,
        }
    }
}

impl Display for QueueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            QueueType::SoloDuo => "RANKED_SOLO_5x5",
            QueueType::Flex => "RANKED_FLEX_SR",
            QueueType::NormalDraft => "",
            QueueType::Aram => "",
            QueueType::Unhandled => "UNHANDLED",
        };

        write!(f, "{}", name)
    }
}
impl QueueKind for QueueType {
    fn to_unified(&self) -> UnifiedQueueType {
        UnifiedQueueType::Lol(*self)
    }

    fn is_ranked(&self) -> bool {
        matches!(self, Self::SoloDuo | Self::Flex)
    }
}

#[derive(Debug, Clone)]
pub struct Match {
    pub participants: Vec<MatchParticipant>,
    pub queue_id: u16,
    pub game_duration: u64,
    pub game_creation: u128,
}

const MAX_REMAKE_TIME: u64 = 240;

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

    pub async fn try_into_match_ranked<Api, Cache>(
        self,
        ranking_of: &Account,
        api: Arc<dyn LeagueApi>,
        cache: &Cache,
    ) -> Result<MatchRanked<Self>, RiotMatchError>
    where
        Cache: CachedLeagueSource,
    {
        let queue_type: QueueType = self.queue_id.into();
        let maybe_cached_league = cache
            .get_league_for(ranking_of.id, &queue_type)
            .await
            .map_err(|e| RiotMatchError::CantRetrieveCachedLeague(e))?;

        if maybe_cached_league.is_none() {
            warn!(
                "No cached league is existing for puuid: {} with queue_id: {}.",
                ranking_of.id, self.queue_id
            )
        }

        let current_leagues = api
            .get_leagues(
                ranking_of.puuid.clone().unwrap_or_default(),
                ranking_of.region,
            )
            .await
            .map_err(|e| RiotMatchError::RiotApiError(e))?;
        let current_league = current_leagues
            .into_iter()
            .find(|league| league.queue_type().eq(&queue_type.to_string()))
            .ok_or(RiotMatchError::NoApiLeagueFound(
                queue_type.to_string(),
                ranking_of.puuid.clone().unwrap_or_default(),
            ))?;

        Ok(MatchRanked {
            base: self,
            current_league,
            cached_league: maybe_cached_league,
        })
    }
}

#[derive(Debug, Clone)]
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
    pub fn to_title_win_string(&self, game_duration: u64) -> String {
        if game_duration < MAX_REMAKE_TIME {
            return "Remake".to_string();
        }

        match self.win {
            true => "Victory".to_string(),
            false => "Defeat".to_string(),
        }
    }
    pub fn to_formatted_win_string(&self, game_duration: u64) -> String {
        // Remake
        if game_duration < MAX_REMAKE_TIME {
            return "remaked".to_string();
        }

        match self.win {
            true => "won".to_string(),
            false => "lost".to_string(),
        }
    }

    pub fn to_win_colour(&self, game_duration: u64) -> Colour {
        // Remake
        if game_duration < MAX_REMAKE_TIME {
            return Colour::from_rgb(128, 128, 128); // Grey
        }

        match self.win {
            true => Colour::from_rgb(39, 98, 218),
            false => Colour::from_rgb(226, 54, 112),
        }
    }
}

pub struct MatchRanked<T> {
    pub base: T,
    pub current_league: League,
    pub cached_league: Option<League>,
}

impl<T> MatchRanked<T> {
    /// Calculate the gain/loss of LP between the cached value and the new match data.
    /// Returns a positive number for LP gain, negative for LP loss, or None if data is missing.
    pub fn calculate_league_points_difference(&self, won: bool) -> Option<i16> {
        let current_league = &self.current_league;
        let cached = self.cached_league.as_ref()?;

        let mut diff = current_league.league_points() as i16 - cached.league_points as i16;

        if (diff < 0 && won) || (diff > 0 && !won) {
            diff += if won { 100 } else { -100 };
        }

        Some(diff)
    }

    pub async fn from_match<Api, Cache>(
        match_data: &T,
        ranking_of: &Account,
        cache: Cache,
        api: Arc<Api>,
    ) -> Result<Self, RiotMatchError>
    where
        T: QueueTyped + Clone,
        Api: LeagueApi,
        Cache: CachedLeagueSource,
    {
        let queue_type = match_data.queue_type();

        let maybe_cached_league = cache
            .get_league_for(ranking_of.id, &queue_type)
            .await
            .map_err(|e| RiotMatchError::CantRetrieveCachedLeague(e))?;

        if maybe_cached_league.is_none() {
            warn!(
                "No cached league is existing for puuid: {} with queue type: {}.",
                ranking_of.id,
                match_data.queue_type().to_string()
            )
        }

        let current_leagues = api
            .get_leagues(
                ranking_of.puuid.clone().unwrap_or_default(),
                ranking_of.region,
            )
            .await
            .map_err(|e| RiotMatchError::RiotApiError(e))?;
        let current_league = current_leagues
            .into_iter()
            .find(|league| league.queue_type().eq(&queue_type.to_string()))
            .ok_or(RiotMatchError::NoApiLeagueFound(
                queue_type.to_string(),
                ranking_of.puuid.clone().unwrap_or_default(),
            ))?;

        Ok(MatchRanked {
            base: match_data.clone(),
            current_league,
            cached_league: maybe_cached_league,
        })
    }
}
