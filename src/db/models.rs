use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct Player {
    pub id: i64,
    pub puuid: String,
    pub game_name: String,
    pub tag_line: String,
    pub region: String,
    pub profile_icon_id: Option<i32>,
    pub last_match_id: Option<String>,
    pub last_rank_solo_tier: Option<String>,
    pub last_rank_solo_rank: Option<String>,
    pub last_rank_solo_lp: Option<i32>,
    pub last_rank_flex_tier: Option<String>,
    pub last_rank_flex_rank: Option<String>,
    pub last_rank_flex_lp: Option<i32>,
}

impl Player {
    pub fn riot_id(&self) -> String {
        format!("{}#{}", self.game_name, self.tag_line)
    }

    pub fn solo_rank_info(&self) -> Option<RankInfo> {
        match (
            &self.last_rank_solo_tier,
            &self.last_rank_solo_rank,
            self.last_rank_solo_lp,
        ) {
            (Some(tier), Some(rank), Some(lp)) => Some(RankInfo {
                tier: tier.clone(),
                rank: rank.clone(),
                lp,
            }),
            _ => None,
        }
    }

    pub fn flex_rank_info(&self) -> Option<RankInfo> {
        match (
            &self.last_rank_flex_tier,
            &self.last_rank_flex_rank,
            self.last_rank_flex_lp,
        ) {
            (Some(tier), Some(rank), Some(lp)) => Some(RankInfo {
                tier: tier.clone(),
                rank: rank.clone(),
                lp,
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RankInfo {
    pub tier: String,
    pub rank: String,
    pub lp: i32,
}

#[derive(Debug, Clone, FromRow)]
pub struct Guild {
    pub id: i64,
    pub alert_channel_id: Option<i64>,
}
