use std::fmt::{self, Display};

use poise::serenity_prelude::Colour;
use serde::Deserialize;

use crate::{UnifiedQueueType, traits::QueueKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueueType {
    Normal,
    Unhandled,
}

impl From<u16> for QueueType {
    fn from(value: u16) -> Self {
        match value {
            1090 => Self::Normal,
            _ => Self::Unhandled,
        }
    }
}

impl Display for QueueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            QueueType::Normal => "NORMAL_TFT",
            QueueType::Unhandled => "UNHANDLED",
        };

        write!(f, "{}", name)
    }
}

impl QueueKind for QueueType {
    fn to_unified(&self) -> UnifiedQueueType {
        UnifiedQueueType::Tft(*self)
    }
}

impl QueueType {
    pub fn as_str(&self) -> &'static str {
        match self {
            QueueType::Normal => "NORMAL_TFT",
            QueueType::Unhandled => "UNHANDLED",
        }
    }
}

/// Representation of the match data response.
#[derive(Deserialize, Debug, Clone)]
pub struct Match {
    pub metadata: Metadata,
    pub info: Info,
}

impl Match {
    pub fn participant(&self, puuid: &str) -> Option<&Participant> {
        self.info.participants.iter().find(|p| p.puuid == puuid)
    }
    pub fn queue_type(&self) -> QueueType {
        self.info.queue_id.into()
    }
    pub fn to_trackergg_url(&self) -> String {
        format!("https://tracker.gg/tft/match/{}", self.metadata.match_id)
    }
}

/// Representation of the match metadata data response.
#[derive(Deserialize, Debug, Clone)]
pub struct Metadata {
    pub match_id: String,
}

/// Representation of the match info data response.
#[derive(Deserialize, Debug, Clone)]
pub struct Info {
    pub participants: Vec<Participant>,
    pub queue_id: u16,
    #[serde(rename = "gameCreation")]
    pub game_creation: u128,
    pub tft_set_number: u8,
}

/// Representation of the participant data response.
#[derive(Deserialize, Debug, Clone)]
pub struct Participant {
    pub puuid: String,
    pub companion: Companion,
    pub gold_left: u16,
    pub placement: u8,
    pub total_damage_to_players: u16,
    pub last_round: u16,
    pub units: Vec<Unit>,

    #[serde(rename = "riotIdGameName")]
    pub riot_id_game_name: String,
    #[serde(rename = "riotIdTagline")]
    pub riot_id_tagline: String,
}

impl Participant {
    pub fn to_placement_string(&self) -> String {
        match self.placement {
            1 => "1st".to_string(),
            2 => "2nd".to_string(),
            3 => "3rd".to_string(),
            x => format!("{}th", x),
        }
    }

    pub fn to_place_string(&self) -> String {
        format!("{} place", self.to_placement_string())
    }

    pub fn to_place_title_string(&self) -> String {
        let emoji = match self.placement {
            1 => Some(" 🥇"),
            2 => Some(" 🥈"),
            3 => Some(" 🥉"),
            _ => None,
        };

        format!("{}{}", self.to_place_string(), emoji.unwrap_or(""))
    }

    pub fn to_win_colour(&self) -> Colour {
        if self.placement <= 4 {
            Colour::from_rgb(39, 98, 218)
        } else {
            Colour::from_rgb(226, 54, 112)
        }
    }
}

pub trait UnitsFilter: IntoIterator {
    fn best_unit(&self) -> Option<&Unit>;
}

#[derive(Deserialize, Debug, Clone)]
pub struct Unit {
    pub character_id: String,
    #[serde(rename = "itemNames")]
    pub item_names: Vec<String>,
    pub rarity: u8,
    pub tier: u8,
}

impl UnitsFilter for Vec<Unit> {
    fn best_unit(&self) -> Option<&Unit> {
        self.iter().max_by(|a, b| {
            (a.tier, a.rarity, a.item_names.len()).cmp(&(b.tier, b.rarity, b.item_names.len()))
        })
    }
}

impl Display for Unit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tier_emojis = match self.tier {
            1 => " ⭐",
            2 => " ⭐⭐",
            3 => " ⭐⭐⭐",
            _ => "",
        };
        let character_name = self
            .character_id
            .rsplit('_')
            .next()
            .unwrap_or(&self.character_id);

        write!(f, "{}{}", character_name, tier_emojis)
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Companion {
    #[serde(rename = "item_ID")]
    pub item_id: u32,
    #[serde(rename = "skin_ID")]
    pub skin_id: u32,
}
