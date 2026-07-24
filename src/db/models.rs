use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct Player {
    pub id: i64,
    pub puuid: String,
    pub game_name: String,
    pub tag_line: String,
    pub region: String,
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
        stored_rank(
            &self.last_rank_solo_tier,
            &self.last_rank_solo_rank,
            self.last_rank_solo_lp,
        )
    }

    pub fn flex_rank_info(&self) -> Option<RankInfo> {
        stored_rank(
            &self.last_rank_flex_tier,
            &self.last_rank_flex_rank,
            self.last_rank_flex_lp,
        )
    }
}

fn stored_rank(tier: &Option<String>, rank: &Option<String>, lp: Option<i32>) -> Option<RankInfo> {
    Some(RankInfo {
        tier: tier.clone()?,
        rank: rank.clone()?,
        lp: lp?,
        ..Default::default()
    })
}

#[derive(Debug, Clone, Default)]
pub struct RankInfo {
    pub tier: String,
    pub rank: String,
    pub lp: i32,
    pub wins: Option<i32>,
    pub losses: Option<i32>,
}

impl RankInfo {
    pub fn display_tier(&self) -> String {
        let lower = self.tier.to_lowercase();
        let mut chars = lower.chars();
        let tier = match chars.next() {
            None => String::new(),
            Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        };
        format!("{tier} {}", self.rank)
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct Guild {
    pub id: i64,
    pub alert_channel_id: Option<i64>,
}

#[derive(Debug, Clone, FromRow)]
pub struct TrackedPlayer {
    #[sqlx(flatten)]
    pub player: Player,
    pub last_activity_secs: i64,
}
