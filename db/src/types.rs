use tentrackule_riot_api::types::{LeaguePoints, Region};

/// Representation of an account tracked by the bot stored in the database.
#[derive(Debug, Clone)]
pub struct Account {
    pub puuid: String,
    pub game_name: String,
    pub tag_line: String,
    pub region: Region,
    pub last_match_id: String,
    /// Cached league points for the `RANKED_SOLO_5x5` queue.
    pub cached_league_points: Option<LeaguePoints>,
}
