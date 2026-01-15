use std::fmt;
use std::str::FromStr;

use poise::ChoiceParameter;

use crate::error::AppError;

/// Platform routing values for Riot API (Summoner-v4, League-v4)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ChoiceParameter)]
pub enum Platform {
    #[name = "BR - Brazil"]
    BR1,
    #[name = "LAN - Latin America North"]
    LA1,
    #[name = "LAS - Latin America South"]
    LA2,
    #[name = "NA - North America"]
    NA1,
    #[name = "JP - Japan"]
    JP1,
    #[name = "KR - Korea"]
    KR,
    #[name = "EUNE - EU Nordic & East"]
    EUN1,
    #[name = "EUW - EU West"]
    EUW1,
    #[name = "ME - Middle East"]
    ME1,
    #[name = "RU - Russia"]
    RU,
    #[name = "TR - Turkey"]
    TR1,
    #[name = "OCE - Oceania"]
    OC1,
    #[name = "PH - Philippines"]
    PH2,
    #[name = "SG - Singapore"]
    SG2,
    #[name = "TH - Thailand"]
    TH2,
    #[name = "TW - Taiwan"]
    TW2,
    #[name = "VN - Vietnam"]
    VN2,
}

impl Platform {
    pub fn base_url(&self) -> String {
        format!("https://{}.api.riotgames.com", self.as_str())
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::BR1 => "br1",
            Self::LA1 => "la1",
            Self::LA2 => "la2",
            Self::NA1 => "na1",
            Self::JP1 => "jp1",
            Self::KR => "kr",
            Self::EUN1 => "eun1",
            Self::EUW1 => "euw1",
            Self::ME1 => "me1",
            Self::RU => "ru",
            Self::TR1 => "tr1",
            Self::OC1 => "oc1",
            Self::PH2 => "ph2",
            Self::SG2 => "sg2",
            Self::TH2 => "th2",
            Self::TW2 => "tw2",
            Self::VN2 => "vn2",
        }
    }

    pub fn to_region(self) -> Region {
        match self {
            Self::BR1 | Self::LA1 | Self::LA2 | Self::NA1 => Region::Americas,
            Self::JP1 | Self::KR => Region::Asia,
            Self::EUN1 | Self::EUW1 | Self::ME1 | Self::RU | Self::TR1 => Region::Europe,
            Self::OC1 | Self::PH2 | Self::SG2 | Self::TH2 | Self::TW2 | Self::VN2 => Region::Sea,
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::BR1 => "Brazil",
            Self::LA1 => "Latin America North",
            Self::LA2 => "Latin America South",
            Self::NA1 => "North America",
            Self::JP1 => "Japan",
            Self::KR => "Korea",
            Self::EUN1 => "EU Nordic & East",
            Self::EUW1 => "EU West",
            Self::ME1 => "Middle East",
            Self::RU => "Russia",
            Self::TR1 => "Turkey",
            Self::OC1 => "Oceania",
            Self::PH2 => "Philippines",
            Self::SG2 => "Singapore",
            Self::TH2 => "Thailand",
            Self::TW2 => "Taiwan",
            Self::VN2 => "Vietnam",
        }
    }
}

impl FromStr for Platform {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "BR" | "BR1" => Ok(Self::BR1),
            "LAN" | "LA1" => Ok(Self::LA1),
            "LAS" | "LA2" => Ok(Self::LA2),
            "NA" | "NA1" => Ok(Self::NA1),
            "JP" | "JP1" => Ok(Self::JP1),
            "KR" => Ok(Self::KR),
            "EUNE" | "EUN" | "EUN1" => Ok(Self::EUN1),
            "EUW" | "EUW1" => Ok(Self::EUW1),
            "ME" | "ME1" => Ok(Self::ME1),
            "RU" => Ok(Self::RU),
            "TR" | "TR1" => Ok(Self::TR1),
            "OCE" | "OC" | "OC1" => Ok(Self::OC1),
            "PH" | "PH2" => Ok(Self::PH2),
            "SG" | "SG2" => Ok(Self::SG2),
            "TH" | "TH2" => Ok(Self::TH2),
            "TW" | "TW2" => Ok(Self::TW2),
            "VN" | "VN2" => Ok(Self::VN2),
            _ => Err(AppError::InvalidRegion(s.to_string())),
        }
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str().to_uppercase())
    }
}

/// Regional routing values for Riot API (Account-v1, Match-v5)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Region {
    Americas,
    Asia,
    Europe,
    Sea,
}

impl Region {
    pub fn base_url(&self) -> String {
        format!("https://{}.api.riotgames.com", self.as_str())
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Americas => "americas",
            Self::Asia => "asia",
            Self::Europe => "europe",
            Self::Sea => "sea",
        }
    }
}

impl fmt::Display for Region {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
