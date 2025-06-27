use tentrackule_riot_api::types::{LeaguePoints, Region};

/// Representation of an account tracked by the bot stored in the database.
#[derive(Debug, Clone)]
pub struct Account {
    pub puuid: String,
    pub game_name: String,
    pub tag_line: String,
    pub region: Region,
    pub last_match_id: String,
}

#[derive(Debug, Clone)]
pub struct League {
    pub points: LeaguePoints,
    pub wins: u16,
    pub losses: u16,
}
