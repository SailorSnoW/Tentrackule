use serde::Deserialize;

// ============================================================================
// Account-v1
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDto {
    pub puuid: String,
    pub game_name: Option<String>,
    pub tag_line: Option<String>,
}

// ============================================================================
// Summoner-v4
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummonerDto {
    pub profile_icon_id: i32,
}

// ============================================================================
// League-v4
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeagueEntryDto {
    pub queue_type: String,
    pub tier: String,
    pub rank: String,
    pub league_points: i32,
}

impl LeagueEntryDto {
    pub fn is_solo_queue(&self) -> bool {
        self.queue_type == "RANKED_SOLO_5x5"
    }

    pub fn is_flex_queue(&self) -> bool {
        self.queue_type == "RANKED_FLEX_SR"
    }
}

// ============================================================================
// Match-v5
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDto {
    pub info: InfoDto,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InfoDto {
    pub game_duration: i64,
    pub game_version: String,
    #[serde(default)]
    pub game_ended_in_early_surrender: bool,
    pub participants: Vec<ParticipantDto>,
    pub queue_id: i32,
}

impl InfoDto {
    /// Queue IDs we support: Normal (400, 430, 490), Ranked (420, 440), ARAM (450)
    pub fn is_supported(&self) -> bool {
        matches!(self.queue_id, 400 | 420 | 430 | 440 | 450 | 490)
    }

    /// Extract short patch version (e.g., "14.24" from "14.24.632.8043")
    pub fn patch_version(&self) -> &str {
        self.game_version
            .match_indices('.')
            .nth(1)
            .map(|(idx, _)| &self.game_version[..idx])
            .unwrap_or(&self.game_version)
    }

    pub fn duration_formatted(&self) -> String {
        let minutes = self.game_duration / 60;
        let seconds = self.game_duration % 60;
        format!("{}:{:02}", minutes, seconds)
    }

    pub fn queue_name(&self) -> &'static str {
        match self.queue_id {
            400 => "Normal Draft",
            420 => "Ranked Solo/Duo",
            430 => "Normal Blind",
            440 => "Ranked Flex",
            450 => "ARAM",
            490 => "Quickplay",
            _ => "Other",
        }
    }

    pub fn is_ranked(&self) -> bool {
        matches!(self.queue_id, 420 | 440)
    }

    pub fn is_solo_queue(&self) -> bool {
        self.queue_id == 420
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParticipantDto {
    pub puuid: String,
    pub team_position: String,
    pub champion_name: String,
    pub kills: i32,
    pub deaths: i32,
    pub assists: i32,
    pub total_damage_dealt_to_champions: i64,
    pub total_minions_killed: i32,
    pub neutral_minions_killed: i32,
    pub vision_score: i32,
    pub gold_earned: i64,
    pub win: bool,
    // Items (6 slots + ward)
    pub item0: i32,
    pub item1: i32,
    pub item2: i32,
    pub item3: i32,
    pub item4: i32,
    pub item5: i32,
    pub item6: i32,
}

impl ParticipantDto {
    pub fn kda_ratio(&self) -> f64 {
        if self.deaths == 0 {
            (self.kills + self.assists) as f64
        } else {
            (self.kills + self.assists) as f64 / self.deaths as f64
        }
    }

    pub fn cs_total(&self) -> i32 {
        self.total_minions_killed + self.neutral_minions_killed
    }

    pub fn cs_per_minute(&self, game_duration_secs: i64) -> f64 {
        if game_duration_secs == 0 {
            0.0
        } else {
            self.cs_total() as f64 / (game_duration_secs as f64 / 60.0)
        }
    }

    pub fn position_display(&self) -> &'static str {
        match self.team_position.as_str() {
            "TOP" => "Top",
            "JUNGLE" => "Jungle",
            "MIDDLE" => "Mid",
            "BOTTOM" => "ADC",
            "UTILITY" => "Support",
            _ => "",
        }
    }

    /// Returns all item IDs (0 = empty slot)
    pub fn items(&self) -> [i32; 7] {
        [
            self.item0, self.item1, self.item2, self.item3, self.item4, self.item5, self.item6,
        ]
    }

    pub fn gold_formatted(&self) -> String {
        if self.gold_earned >= 1_000 {
            format!("{:.1}k", self.gold_earned as f64 / 1_000.0)
        } else {
            self.gold_earned.to_string()
        }
    }
}
