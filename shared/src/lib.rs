use std::{
    env,
    fmt::{self, Display},
    sync::LazyLock,
};

use serde::Deserialize;
use tracing::info;
use traits::{
    QueueKind,
    api::{LeaguePoints, LeagueQueueType, LeagueRank},
};
use uuid::Uuid;

pub mod errors;
pub mod lol_match;
pub mod tft_match;
pub mod traits;

/// Loaded once at startup to avoid repeated environment lookups.
pub static DDRAGON_VERSION: LazyLock<String> =
    LazyLock::new(|| env::var("DDRAGON_VERSION").unwrap_or_else(|_| "15.12.1".to_string()));

pub fn init_ddragon_version() {
    LazyLock::force(&DDRAGON_VERSION);
    info!("Using Riot Ddragon assets v{}", DDRAGON_VERSION.as_str())
}

fn ddragon_version() -> &'static str {
    DDRAGON_VERSION.as_str()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, poise::ChoiceParameter)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnifiedQueueType {
    Lol(lol_match::QueueType),
    Tft(tft_match::QueueType),
}

impl Display for UnifiedQueueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Lol(x) => x.to_string(),
            Self::Tft(x) => x.to_string(),
        };

        write!(f, "{}", name)
    }
}

impl QueueKind for UnifiedQueueType {
    fn to_unified(&self) -> UnifiedQueueType {
        *self
    }
}

/// Representation of an account tracked by the bot stored in the database.
#[derive(Debug, Clone)]
pub struct Account {
    pub id: Uuid,
    pub puuid: Option<String>,
    pub puuid_tft: Option<String>,
    pub game_name: String,
    pub tag_line: String,
    pub region: Region,
    pub last_match_id: String,
}

/// Representation of a league used by the bot which is stored in the database.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct League {
    pub queue_type: String,
    pub league_points: u16,
    pub wins: u16,
    pub losses: u16,
    pub rank: String,
    pub tier: String,
}

impl League {
    pub fn is_ranked_solo_duo(&self) -> bool {
        self.queue_type.eq("RANKED_SOLO_5x5")
    }

    pub fn is_ranked_flex(&self) -> bool {
        self.queue_type.eq("RANKED_FLEX_SR")
    }
}

impl LeaguePoints for League {
    fn league_points(&self) -> u16 {
        self.league_points
    }
}
impl LeagueRank for League {
    fn rank(&self) -> String {
        self.rank.clone()
    }
    fn tier(&self) -> String {
        self.tier.clone()
    }
}
impl LeagueQueueType for League {
    fn queue_type(&self) -> String {
        self.queue_type.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_type_and_region_conversions() {
        let q = lol_match::QueueType::from(420u16);
        assert!(matches!(q, lol_match::QueueType::SoloDuo));
        assert_eq!(q.to_string(), "RANKED_SOLO_5x5");
        assert!(matches!(
            lol_match::QueueType::from(999u16),
            lol_match::QueueType::Unhandled
        ));

        assert_eq!(Region::Euw.to_endpoint(), "euw1.api.riotgames.com");
        assert_eq!(
            Region::Na.to_global_endpoint(),
            "americas.api.riotgames.com"
        );
        let s: String = Region::Na.into();
        assert_eq!(s, "NA");
        assert_eq!(Region::try_from("euw".to_string()).unwrap(), Region::Euw);
    }
}
