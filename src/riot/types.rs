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
// League-v4
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LeagueEntryDto {
    pub queue_type: String,
    pub tier: String,
    pub rank: String,
    pub league_points: i32,
    #[serde(default)]
    pub wins: i32,
    #[serde(default)]
    pub losses: i32,
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
// Champion-mastery-v4
// ============================================================================

/// A player's mastery on one champion. We only surface level + points; the
/// endpoint exposes no games-played count (that stays omitted on the card).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChampionMasteryDto {
    pub champion_level: i32,
    pub champion_points: i32,
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
    /// Per-team summary (win + objective kill counts). Absent from very old
    /// match payloads, so it defaults to empty and the objectives block is then
    /// simply omitted.
    #[serde(default)]
    pub teams: Vec<TeamDto>,
}

/// One team's objective tally (match-v5 `teams[]`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamDto {
    pub team_id: i32,
    #[serde(default)]
    pub objectives: ObjectivesDto,
}

/// The `objectives` sub-object; we only surface the ones the card renders.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectivesDto {
    #[serde(default)]
    pub baron: ObjectiveCountDto,
    #[serde(default)]
    pub dragon: ObjectiveCountDto,
    #[serde(default)]
    pub tower: ObjectiveCountDto,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectiveCountDto {
    #[serde(default)]
    pub kills: i32,
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

    pub fn is_flex_queue(&self) -> bool {
        self.queue_id == 440
    }

    /// Team summary for `team_id` (100 / 200), if present in the payload.
    pub fn team(&self, team_id: i32) -> Option<&TeamDto> {
        self.teams.iter().find(|t| t.team_id == team_id)
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
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
    // --- Phase 3 additions (all defaulted for resilience against partial payloads) ---
    /// `100` or `200`; used to group allies/enemies reliably.
    #[serde(default)]
    pub team_id: i32,
    #[serde(default)]
    pub champ_level: i32,
    #[serde(default)]
    pub double_kills: i32,
    #[serde(default)]
    pub triple_kills: i32,
    #[serde(default)]
    pub quadra_kills: i32,
    #[serde(default)]
    pub penta_kills: i32,
    /// Summoner-spell numeric keys (e.g. `4` = Flash); mapped to DDragon ids.
    #[serde(default)]
    pub summoner1_id: i32,
    #[serde(default)]
    pub summoner2_id: i32,
    #[serde(default)]
    pub perks: PerksDto,
    #[serde(default)]
    pub wards_placed: i32,
    #[serde(default)]
    pub wards_killed: i32,
    #[serde(default)]
    pub detector_wards_placed: i32,
}

/// Minimal slice of match-v5 `perks` — just enough to resolve the keystone and
/// the secondary rune style for the loadout strip.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PerksDto {
    #[serde(default)]
    pub styles: Vec<PerkStyleDto>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerkStyleDto {
    /// Rune style id (e.g. `8000` = Precision).
    #[serde(default)]
    pub style: i32,
    #[serde(default)]
    pub selections: Vec<PerkSelectionDto>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PerkSelectionDto {
    /// Individual rune id (the first selection of the primary style is the keystone).
    #[serde(default)]
    pub perk: i32,
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

    /// `wardsPlaced·wardsKilled·detectorWardsPlaced`, e.g. `12·7·4`.
    pub fn vision_breakdown(&self) -> String {
        format!(
            "{}·{}·{}",
            self.wards_placed, self.wards_killed, self.detector_wards_placed
        )
    }

    /// The keystone rune id (first selection of the primary style), if any.
    pub fn keystone_perk_id(&self) -> Option<i32> {
        self.perks
            .styles
            .first()?
            .selections
            .first()
            .map(|s| s.perk)
            .filter(|&p| p > 0)
    }

    /// The secondary rune style id, if any.
    pub fn secondary_style_id(&self) -> Option<i32> {
        self.perks.styles.get(1).map(|s| s.style).filter(|&s| s > 0)
    }

    /// Highest-tier multi-kill headline + an optional "doubles" sub-count, e.g.
    /// `("1 Triple", Some("+2 double"))`. `None` when no multi-kill was scored.
    pub fn multi_kill(&self) -> Option<(String, Option<String>)> {
        let (count, name) = [
            (self.penta_kills, "Penta"),
            (self.quadra_kills, "Quadra"),
            (self.triple_kills, "Triple"),
            (self.double_kills, "Double"),
        ]
        .into_iter()
        .find(|(c, _)| *c > 0)?;

        let sub = (name != "Double" && self.double_kills > 0)
            .then(|| format!("+{} double", self.double_kills));
        Some((format!("{count} {name}"), sub))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_kill_picks_highest_tier_with_doubles_sub() {
        let p = ParticipantDto {
            triple_kills: 1,
            double_kills: 2,
            ..Default::default()
        };
        assert_eq!(
            p.multi_kill(),
            Some(("1 Triple".to_string(), Some("+2 double".to_string())))
        );

        // A penta outranks everything; doubles still show as the sub.
        let p = ParticipantDto {
            penta_kills: 1,
            triple_kills: 3,
            double_kills: 1,
            ..Default::default()
        };
        assert_eq!(
            p.multi_kill(),
            Some(("1 Penta".to_string(), Some("+1 double".to_string())))
        );

        // Doubles only → no sub.
        let p = ParticipantDto {
            double_kills: 2,
            ..Default::default()
        };
        assert_eq!(p.multi_kill(), Some(("2 Double".to_string(), None)));

        // No multi-kills at all.
        assert_eq!(ParticipantDto::default().multi_kill(), None);
    }

    #[test]
    fn vision_breakdown_formats_wards() {
        let p = ParticipantDto {
            wards_placed: 12,
            wards_killed: 7,
            detector_wards_placed: 4,
            ..Default::default()
        };
        assert_eq!(p.vision_breakdown(), "12·7·4");
    }

    #[test]
    fn perk_ids_extracted_from_styles() {
        let p = ParticipantDto {
            perks: PerksDto {
                styles: vec![
                    PerkStyleDto {
                        style: 8000,
                        selections: vec![PerkSelectionDto { perk: 8010 }],
                    },
                    PerkStyleDto {
                        style: 8100,
                        selections: vec![],
                    },
                ],
            },
            ..Default::default()
        };
        assert_eq!(p.keystone_perk_id(), Some(8010));
        assert_eq!(p.secondary_style_id(), Some(8100));

        // Empty perks resolve to nothing (icons omitted).
        let empty = ParticipantDto::default();
        assert_eq!(empty.keystone_perk_id(), None);
        assert_eq!(empty.secondary_style_id(), None);
    }
}
