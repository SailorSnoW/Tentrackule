use crate::riot::types::{LeaguePoints, Region};

#[derive(Debug, Clone)]
pub struct Account {
    pub puuid: String,
    pub game_name: String,
    pub tag_line: String,
    pub region: Region,
    pub last_match_id: String,
    pub cached_lol_solo_duo_lps: Option<LeaguePoints>,
}
