//! Phase 1 of the HTML scorecard roadmap (`docs/html-scorecard-roadmap.md`).
//!
//! A pure-Rust builder that emits the final, resolved HTML for a match card.
//! It is a faithful port of the design-tool export `Scorecard LoL.dc.html`:
//!   - `themePresets()`  → [`MatchResult::theme_vars`]
//!   - `renderVals()`    → [`ScorecardVm`] (already-formatted view-model)
//!   - HTML + `sc-if` / `sc-for` → [`ScorecardVm::build_html`]
//!
//! The builder is complete (it can render every feature of the template). As of
//! Phase 4, [`ScorecardVm::from_context`] lights up everything from real data:
//! KDA, KP, damage share, gold/min, vision breakdown, multi-kills, champion
//! level, summoner spells + runes, the lane matchup, team objectives, ranked
//! win-rate, the performance **grade** ([`super::grade`]), champion **mastery**
//! (non-ranked footer) and the recent-form **streak** (ranked footer). A block
//! is only omitted when its data is genuinely absent (a remake has no team
//! context, a normal game no rank), exactly like an unmet `sc-if`.
//!
//! The *HTML* is produced with no network and no Chromium, so it is unit-testable
//! in isolation (see the tests at the bottom, which write sample cards to
//! `target/scorecard-samples/` for visual review). Image *assets* referenced by
//! the card are inlined as data URIs separately, by the generator (`image_gen`).

use std::collections::HashMap;

use super::ddragon::DdragonData;
use super::grade;
use super::image_gen::MatchImageContext;
use crate::db::RankInfo;
use crate::riot::{InfoDto, ParticipantDto, TeamDto};

/// Card width in CSS pixels. The renderer viewport (`image_gen`) is derived
/// from this same constant so the two can never drift.
pub const CARD_WIDTH_PX: u32 = 820;

// ============================================================================
// Result / theme
// ============================================================================

/// Match outcome, driving the colour theme (port of `themePresets()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchResult {
    Victory,
    Defeat,
    Remake,
}

impl MatchResult {
    /// The banner label (uppercase, matching the design).
    fn result_label(self) -> &'static str {
        match self {
            MatchResult::Victory => "VICTORY",
            MatchResult::Defeat => "DEFEAT",
            MatchResult::Remake => "REMAKE",
        }
    }

    /// The CSS custom-property block for this outcome, formatted as the content
    /// of a `style="…"` attribute. Verbatim port of `themePresets()[result]`.
    fn theme_vars(self) -> &'static str {
        match self {
            MatchResult::Victory => {
                "--card-bg:linear-gradient(158deg,#0b1a2e,#050c15);--edge:#22405f;--panel-bg:linear-gradient(180deg,#0c1e34,#07131f);--panel-edge:#1c3a58;--ribbon-bg:linear-gradient(90deg,#1c4d8c,#2f6fb8);--ribbon-edge:#c8aa6e;--ribbon-text:#f5efe0;--grade-bg:linear-gradient(150deg,#20334d,#0c1826);--grade-edge:#c8aa6e;--grade-text:#f5efe0;--accent:#c8aa6e;--accent-2:#31d07f;--label:#6a8199;--value:#f0e6d2;--value-soft:#9fb4cc;--footer-edge:rgba(200,170,110,.3);--delta:#31d07f;--track:#0f2236;--track-opp:#33546f;--overlay:linear-gradient(180deg,rgba(20,60,110,.4) 0%,rgba(7,16,26,.1) 30%,rgba(6,12,20,.85) 64%,#060e18 100%);--chip-edge:#7a5a2a;--chip-text:#d8c79a;--chip-bg:rgba(6,12,20,.6);--chip-edge-2:#2c4864;--chip-text-2:#9fb4cc;--death:#e84057;--slash:#3f5470;--divider:#24425f"
            }
            MatchResult::Defeat => {
                "--card-bg:linear-gradient(158deg,#1d0d10,#0c0607);--edge:#6a2b30;--panel-bg:linear-gradient(180deg,#1e0e11,#120709);--panel-edge:#5a2429;--ribbon-bg:linear-gradient(90deg,#8a2a34,#c0424f);--ribbon-edge:#e0526a;--ribbon-text:#ffe4e8;--grade-bg:linear-gradient(150deg,#4a1e22,#1a090b);--grade-edge:#e0526a;--grade-text:#ffe4e8;--accent:#e0526a;--accent-2:#e0526a;--label:#a8737a;--value:#ffe9eb;--value-soft:#e0b4b9;--footer-edge:rgba(224,82,106,.3);--delta:#ff6b7f;--track:#2a1215;--track-opp:#6b4045;--overlay:linear-gradient(180deg,rgba(90,25,32,.4) 0%,rgba(26,8,10,.1) 30%,rgba(18,7,9,.86) 64%,#0c0607 100%);--chip-edge:#8a3d44;--chip-text:#e59aa4;--chip-bg:rgba(20,8,10,.6);--chip-edge-2:#5a2429;--chip-text-2:#e0b4b9;--death:#ff8a99;--slash:#6b3d43;--divider:#5a2429"
            }
            MatchResult::Remake => {
                "--card-bg:linear-gradient(158deg,#181a1f,#0d0e11);--edge:#2f343c;--panel-bg:linear-gradient(180deg,#1a1c21,#101115);--panel-edge:#2a2e35;--ribbon-bg:linear-gradient(90deg,#3a3f47,#565c66);--ribbon-edge:#7a828e;--ribbon-text:#e8eaee;--grade-bg:linear-gradient(150deg,#2a2e35,#141519);--grade-edge:#7a828e;--grade-text:#c8ccd2;--accent:#9aa2ad;--accent-2:#9aa2ad;--label:#6b7178;--value:#e8eaee;--value-soft:#aeb4bd;--footer-edge:rgba(122,130,142,.3);--delta:#aeb4bd;--track:#20232a;--track-opp:#3a3f47;--overlay:linear-gradient(180deg,rgba(60,64,72,.4) 0%,rgba(20,22,26,.1) 30%,rgba(14,15,18,.86) 64%,#0d0e11 100%);--chip-edge:#4a4f57;--chip-text:#aeb4bd;--chip-bg:rgba(14,15,18,.6);--chip-edge-2:#3a3f47;--chip-text-2:#aeb4bd;--death:#c98a90;--slash:#4a4f57;--divider:#3a3f47"
            }
        }
    }
}

// ============================================================================
// View-model (port of `renderVals()` output)
// ============================================================================

/// K / D / A triple.
#[derive(Debug, Clone)]
pub struct Kda {
    pub kills: i32,
    pub deaths: i32,
    pub assists: i32,
}

/// A stat panel value with an optional dimmed sub-label (e.g. `303` + `9.3/m`).
#[derive(Debug, Clone)]
pub struct StatValue {
    pub value: String,
    pub sub: Option<String>,
}

impl StatValue {
    fn new(value: impl Into<String>, sub: Option<String>) -> Self {
        Self {
            value: value.into(),
            sub,
        }
    }
}

/// Ranked footer block (solo/flex). Optional pieces (`winrate`, `progress`,
/// `streak`) are omitted individually when their data is not available yet.
#[derive(Debug, Clone)]
pub struct RankView {
    /// e.g. `Emerald IV`.
    pub tier: String,
    /// Ranked-tier mini-crest (SVG) `src`. `None` for an unknown tier word.
    pub emblem_src: Option<String>,
    pub lp: i32,
    /// Signed LP delta text, e.g. `+19` / `-17`. `None` hides the parenthesis.
    pub delta: Option<String>,
    /// Division progress 0..=100 for the thin bar.
    pub progress: Option<u8>,
    pub winrate: Option<u8>,
    pub wins: Option<i32>,
    pub losses: Option<i32>,
    /// Recent results, `true` = win. Empty = no streak row.
    pub streak: Vec<bool>,
}

/// Champion-mastery footer block (non-ranked queues).
#[derive(Debug, Clone)]
pub struct MasteryView {
    pub level: String,
    pub points: String,
}

/// The summoner-spells + runes + items + trinket strip.
#[derive(Debug, Clone, Default)]
pub struct Loadout {
    /// Resolved `src` for each summoner spell (0..=2).
    pub spell_srcs: Vec<String>,
    pub keystone_src: Option<String>,
    pub secondary_src: Option<String>,
    /// Six main item slots; `None` renders a dashed empty slot.
    pub items: Vec<Option<String>>,
    pub trinket_src: Option<String>,
}

/// Lane matchup panel.
#[derive(Debug, Clone)]
pub struct MatchupView {
    pub label: String,
    pub opp_icon_src: String,
    pub opp_name: String,
    pub opp_kda: String,
    pub rows: Vec<MatchupRow>,
}

/// One comparison row (DMG / CS / Or) inside the matchup panel. `you_pct` is the
/// player's share (0..=100); the opponent takes the remainder.
#[derive(Debug, Clone)]
pub struct MatchupRow {
    pub key: String,
    pub you: String,
    pub opp: String,
    pub you_pct: u8,
}

/// A single team objective (drake / baron / tower), carrying its resolved
/// minimap icon `src`.
#[derive(Debug, Clone)]
pub struct Objective {
    pub icon_src: String,
    pub count: i32,
    pub label: String,
}

/// Team / objectives panel.
#[derive(Debug, Clone)]
pub struct TeamView {
    pub kills: i32,
    pub opp_kills: i32,
    pub objectives: Vec<Objective>,
}

/// Fully-resolved, already-formatted view-model for one card. Everything the
/// builder needs is pre-computed here — [`ScorecardVm::build_html`] is pure
/// string assembly with no business logic.
#[derive(Debug, Clone)]
pub struct ScorecardVm {
    pub result: MatchResult,

    // --- left sidebar ---
    pub champion_splash_src: String,
    pub grade: Option<String>,
    pub player_name: String,
    /// Includes the leading `#`, e.g. `#01010`.
    pub player_tag: String,
    /// e.g. `JUNGLE`; empty hides the role chip (ARAM).
    pub role_upper: String,
    /// Role position glyph (SVG) shown in the role chip and the matchup header.
    /// `None` for ARAM / unknown positions.
    pub role_icon_src: Option<String>,
    pub champion_name: String,
    pub champion_level: Option<i32>,
    /// `Some` → ranked footer block; `None` → mastery/queue block.
    pub rank: Option<RankView>,
    pub mastery: Option<MasteryView>,
    pub queue_label: String,

    // --- right main column ---
    pub meta_line: String,
    pub kda: Kda,
    pub kda_ratio: String,
    pub kp: Option<u8>,
    pub kp_sub: Option<String>,
    pub multi: Option<String>,
    pub multi_sub: Option<String>,
    pub cs: StatValue,
    pub gold: StatValue,
    pub damage_value: String,
    pub damage_share: Option<u8>,
    pub vision: StatValue,
    pub loadout: Loadout,
    pub matchup: Option<MatchupView>,
    pub team: Option<TeamView>,
}

impl ScorecardVm {
    /// Whether the KDA multi-kill chip should show (port of `showMultiChip`).
    fn show_multi_chip(&self) -> bool {
        self.multi.is_some() && self.result != MatchResult::Remake
    }

    /// Whether the bottom row (matchup / team) has any panel to show.
    fn has_bottom(&self) -> bool {
        self.matchup.is_some() || self.team.is_some()
    }

    /// Every external image URL referenced by the card (splash, summoner spells,
    /// runes, items, trinket, matchup icon). Used to pre-fetch and inline them as
    /// data URIs before rendering.
    pub fn image_urls(&self) -> Vec<String> {
        let l = &self.loadout;
        let mut urls = vec![self.champion_splash_src.clone()];
        urls.extend(self.role_icon_src.iter().cloned());
        urls.extend(l.spell_srcs.iter().cloned());
        urls.extend(l.keystone_src.iter().cloned());
        urls.extend(l.secondary_src.iter().cloned());
        urls.extend(l.items.iter().flatten().cloned());
        urls.extend(l.trinket_src.iter().cloned());
        if let Some(r) = &self.rank {
            urls.extend(r.emblem_src.iter().cloned());
        }
        if let Some(m) = &self.matchup {
            urls.push(m.opp_icon_src.clone());
        }
        if let Some(t) = &self.team {
            urls.extend(t.objectives.iter().map(|o| o.icon_src.clone()));
        }
        urls
    }

    /// Replace each image URL that appears in `map` with its inlined data URI.
    /// URLs missing from the map keep their original value, so the renderer can
    /// still fetch them directly as a fallback.
    pub fn apply_asset_map(&mut self, map: &HashMap<String, String>) {
        let swap = |s: &mut String| {
            if let Some(uri) = map.get(s.as_str()) {
                *s = uri.clone();
            }
        };
        swap(&mut self.champion_splash_src);
        if let Some(s) = self.role_icon_src.as_mut() {
            swap(s);
        }
        for s in &mut self.loadout.spell_srcs {
            swap(s);
        }
        if let Some(s) = self.loadout.keystone_src.as_mut() {
            swap(s);
        }
        if let Some(s) = self.loadout.secondary_src.as_mut() {
            swap(s);
        }
        for slot in self.loadout.items.iter_mut().flatten() {
            swap(slot);
        }
        if let Some(s) = self.loadout.trinket_src.as_mut() {
            swap(s);
        }
        if let Some(r) = self.rank.as_mut()
            && let Some(s) = r.emblem_src.as_mut()
        {
            swap(s);
        }
        if let Some(m) = &mut self.matchup {
            swap(&mut m.opp_icon_src);
        }
        if let Some(t) = self.team.as_mut() {
            for o in &mut t.objectives {
                swap(&mut o.icon_src);
            }
        }
    }
}

// ============================================================================
// Data wiring: MatchImageContext → ScorecardVm
// ============================================================================

const DDRAGON: &str = super::ddragon::DDRAGON_CDN;

/// CommunityDragon "latest" root, for assets DDragon doesn't carry: role
/// position glyphs, ranked-tier crests and minimap objective icons.
const CDRAGON: &str = "https://raw.communitydragon.org/latest";

impl ScorecardVm {
    /// Build the view-model from a match context and the boot-time Data Dragon
    /// tables. Features that still require DB state (grade, mastery, streak) are
    /// left off; their template blocks are then omitted by [`build_html`].
    pub fn from_context(ctx: &MatchImageContext<'_>, ddragon: &DdragonData) -> Self {
        let p = ctx.participant;
        let info = ctx.match_info;
        let version = ddragon.version();
        // Reconcile the match-v5 champion name with the canonical DDragon key so
        // the splash resolves for every champion (e.g. `FiddleSticks`). Skin is
        // not exposed by match-v5, so the base skin (`_0`) is all we can show.
        let champion_key = ddragon.champion_key(&p.champion_name);

        let result = if info.game_ended_in_early_surrender {
            MatchResult::Remake
        } else if p.win {
            MatchResult::Victory
        } else {
            MatchResult::Defeat
        };

        // Team-relative stats (KP, damage share, matchup, team kills, objectives),
        // grouped by `teamId`. Skips cleanly for anything that isn't a real 5v5.
        let team = TeamDerived::compute(info, p, ddragon);

        let role_upper = p.position_display().to_uppercase();

        // Ranked footer only when we actually have a rank for this queue.
        let rank = if info.is_ranked() {
            ctx.new_rank.map(|r| {
                let delta = calculate_lp_diff(ctx.old_rank, ctx.new_rank);
                let tier = r.display_tier();
                RankView {
                    emblem_src: rank_emblem_src(&tier),
                    tier,
                    lp: r.lp,
                    delta: delta.map(format_signed),
                    progress: division_progress(&r.tier, r.lp),
                    winrate: win_rate(r.wins, r.losses),
                    wins: r.wins,
                    losses: r.losses,
                    // Recent-form bar from the match-history table (Phase 4).
                    streak: ctx.streak.clone(),
                }
            })
        } else {
            None
        };

        let meta_line = [
            info.queue_name().to_string(),
            info.duration_formatted(),
            format!("Patch {}", info.patch_version()),
        ]
        .join(" · ");

        let items = p.items();
        let loadout = Loadout {
            spell_srcs: [p.summoner1_id, p.summoner2_id]
                .into_iter()
                .filter_map(|key| ddragon.spell_src(key))
                .collect(),
            keystone_src: p.keystone_perk_id().and_then(|id| ddragon.keystone_src(id)),
            secondary_src: p
                .secondary_style_id()
                .and_then(|id| ddragon.secondary_src(id)),
            items: items[..6]
                .iter()
                .map(|&id| (id > 0).then(|| item_src(version, id)))
                .collect(),
            trinket_src: (items[6] > 0).then(|| item_src(version, items[6])),
        };

        let gold_sub = {
            let minutes = info.game_duration as f64 / 60.0;
            if minutes > 0.0 {
                Some(format!(
                    "{}/m",
                    (p.gold_earned as f64 / minutes).round() as i64
                ))
            } else {
                None
            }
        };

        let (multi, multi_sub) = match p.multi_kill() {
            Some((label, sub)) => (Some(label), sub),
            None => (None, None),
        };

        // Ward breakdown only when there's something to show (a 3-min remake has none).
        let vision_sub = (p.wards_placed + p.wards_killed + p.detector_wards_placed > 0)
            .then(|| p.vision_breakdown());

        // Performance grade (Phase 4): a role-relative composite of the same
        // derived stats. It needs real team context (KP, damage share), so it
        // rides on `team` being present — off for remakes / non-5v5, exactly
        // like the KP and damage-share panels.
        let grade = team.as_ref().map(|t| {
            let minutes = info.game_duration as f64 / 60.0;
            let vision_per_min = if minutes > 0.0 {
                p.vision_score as f64 / minutes
            } else {
                0.0
            };
            grade::evaluate(&grade::GradeInput {
                role: &p.team_position,
                kda: p.kda_ratio(),
                kp: t.kp,
                cs_per_min: p.cs_per_minute(info.game_duration),
                damage_share: t.damage_share,
                vision_per_min,
            })
            .label()
            .to_string()
        });

        Self {
            result,
            champion_splash_src: format!("{DDRAGON}/img/champion/loading/{champion_key}_0.jpg"),
            grade,
            player_name: ctx.player.game_name.clone(),
            player_tag: format!("#{}", ctx.player.tag_line),
            role_upper,
            role_icon_src: position_icon_src(&p.team_position),
            champion_name: p.champion_name.clone(),
            champion_level: (p.champ_level > 0).then_some(p.champ_level),
            rank,
            // Non-ranked footer: champion mastery (Phase 4). Shown only when the
            // rank block is absent (`build_html` prefers rank when present).
            mastery: ctx.mastery.map(|m| MasteryView {
                level: m.level.to_string(),
                points: format_short(m.points as i64),
            }),
            queue_label: info.queue_name().to_string(),
            meta_line,
            kda: Kda {
                kills: p.kills,
                deaths: p.deaths,
                assists: p.assists,
            },
            kda_ratio: format!("{:.2}", p.kda_ratio()),
            kp: team.as_ref().map(|t| t.kp),
            kp_sub: team.as_ref().map(|t| t.kp_sub.clone()),
            multi,
            multi_sub,
            cs: StatValue::new(
                p.cs_total().to_string(),
                Some(format!("{:.1}/m", p.cs_per_minute(info.game_duration))),
            ),
            gold: StatValue::new(format_short(p.gold_earned), gold_sub),
            damage_value: format_short(p.total_damage_dealt_to_champions),
            damage_share: team.as_ref().map(|t| t.damage_share),
            vision: StatValue::new(p.vision_score.to_string(), vision_sub),
            loadout,
            matchup: team.as_ref().and_then(|t| t.matchup.clone()),
            team: team.map(|t| TeamView {
                kills: t.team_kills,
                opp_kills: t.opp_kills,
                objectives: t.objectives,
            }),
        }
    }
}

/// Team-relative derivations computed by grouping participants on `teamId`.
struct TeamDerived {
    kp: u8,
    kp_sub: String,
    damage_share: u8,
    team_kills: i32,
    opp_kills: i32,
    matchup: Option<MatchupView>,
    objectives: Vec<Objective>,
}

impl TeamDerived {
    fn compute(info: &InfoDto, me: &ParticipantDto, ddragon: &DdragonData) -> Option<Self> {
        // A remake (early surrender) is too short for a meaningful breakdown.
        if info.game_ended_in_early_surrender {
            return None;
        }

        let version = ddragon.version();
        let allies: Vec<&ParticipantDto> = info
            .participants
            .iter()
            .filter(|p| p.team_id == me.team_id)
            .collect();
        let enemies: Vec<&ParticipantDto> = info
            .participants
            .iter()
            .filter(|p| p.team_id != me.team_id)
            .collect();

        // Only trust the grouping for a genuine 5v5.
        if allies.len() != 5 || enemies.len() != 5 {
            return None;
        }

        let team_kills: i32 = allies.iter().map(|p| p.kills).sum();
        let opp_kills: i32 = enemies.iter().map(|p| p.kills).sum();
        let team_dmg: i64 = allies
            .iter()
            .map(|p| p.total_damage_dealt_to_champions)
            .sum();

        let ka = me.kills + me.assists;
        let kp = percent(ka as i64, team_kills as i64);

        // Lane opponent: same non-empty team position on the other side.
        let matchup = enemies
            .iter()
            .find(|p| !me.team_position.is_empty() && p.team_position == me.team_position)
            .map(|opp| MatchupView {
                label: "Matchup".into(),
                opp_icon_src: champion_square_src(
                    version,
                    &ddragon.champion_key(&opp.champion_name),
                ),
                opp_name: opp.champion_name.clone(),
                opp_kda: format!("{}/{}/{}", opp.kills, opp.deaths, opp.assists),
                rows: vec![
                    MatchupRow {
                        key: "DMG".into(),
                        you: format_short(me.total_damage_dealt_to_champions),
                        opp: format_short(opp.total_damage_dealt_to_champions),
                        you_pct: share(
                            me.total_damage_dealt_to_champions,
                            opp.total_damage_dealt_to_champions,
                        ),
                    },
                    MatchupRow {
                        key: "CS".into(),
                        you: me.cs_total().to_string(),
                        opp: opp.cs_total().to_string(),
                        you_pct: share(me.cs_total() as i64, opp.cs_total() as i64),
                    },
                    MatchupRow {
                        key: "Gold".into(),
                        you: format_short(me.gold_earned),
                        opp: format_short(opp.gold_earned),
                        you_pct: share(me.gold_earned, opp.gold_earned),
                    },
                ],
            });

        // Objectives from the team summary (`teams[].objectives`), if present.
        let objectives = info
            .team(me.team_id)
            .map(team_objectives)
            .unwrap_or_default();

        Some(Self {
            kp,
            kp_sub: format!("{}/{}", ka, team_kills),
            damage_share: percent(me.total_damage_dealt_to_champions, team_dmg),
            team_kills,
            opp_kills,
            matchup,
            objectives,
        })
    }
}

/// Drakes / Nashor / towers row for the team panel, from a `TeamDto` summary.
fn team_objectives(t: &TeamDto) -> Vec<Objective> {
    vec![
        Objective {
            icon_src: objective_icon_src("dragon"),
            count: t.objectives.dragon.kills,
            label: "Drakes".into(),
        },
        Objective {
            icon_src: objective_icon_src("baron"),
            count: t.objectives.baron.kills,
            label: "Nashor".into(),
        },
        Objective {
            icon_src: objective_icon_src("tower"),
            count: t.objectives.tower.kills,
            label: "Towers".into(),
        },
    ]
}

// ============================================================================
// HTML assembly
// ============================================================================

impl ScorecardVm {
    /// Emit the final, self-contained HTML document (fonts + card). Screenshot
    /// the `#card-root` element for a tight crop.
    pub fn build_html(&self) -> String {
        let mut s = String::with_capacity(8 * 1024);
        s.push_str(
            "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">\n\
<link rel=\"preconnect\" href=\"https://fonts.gstatic.com\" crossorigin>\n\
<link href=\"",
        );
        s.push_str(&self.font_css_url());
        s.push_str(
            "\" rel=\"stylesheet\">\n\
<style>body{margin:0;padding:40px;background:#101216;font-family:'Barlow Semi Condensed',system-ui,sans-serif}</style>\n\
<script>addEventListener('error',function(e){var t=e.target;if(t&&t.tagName==='IMG'){t.style.visibility='hidden';}},true);</script>\n\
</head>\n<body>\n",
        );

        s.push_str("<div id=\"card-root\" style=\"");
        s.push_str(self.result.theme_vars());
        s.push_str(";display:inline-block\">\n");
        s.push_str("  <div style=\"width:");
        s.push_str(&CARD_WIDTH_PX.to_string());
        s.push_str("px;display:flex;font-family:'Barlow Semi Condensed',sans-serif;color:var(--value,#f0e6d2);background:var(--card-bg,linear-gradient(158deg,#0b1a2e,#050c15));border:1px solid var(--edge,#22405f);border-radius:14px;overflow:hidden;box-shadow:0 26px 60px -22px rgba(0,0,0,.85),inset 0 1px 0 rgba(255,255,255,.05)\">\n");

        self.push_sidebar(&mut s);
        self.push_main(&mut s);

        s.push_str("  </div>\n");
        s.push_str("</div>\n</body>\n</html>\n");
        s
    }

    /// Google Fonts stylesheet URL for the card. Cinzel + Barlow + Noto Sans JP
    /// are always requested; the Korean (`KR`) and Simplified-Chinese (`SC`) Noto
    /// subsets are pulled in only when the player name actually contains those
    /// scripts, so a Latin/Japanese name never requests the heavier CJK CSS.
    /// (Google's `unicode-range` subsetting still limits the download to the glyph
    /// ranges the card renders, whichever families are listed.)
    fn font_css_url(&self) -> String {
        let mut families = String::from(
            "family=Cinzel:wght@600;700\
             &family=Barlow+Semi+Condensed:wght@400;500;600;700\
             &family=Noto+Sans+JP:wght@500;700",
        );
        // The player name is the only field carrying arbitrary user text; champion
        // and rune labels are Latin DDragon keys.
        if contains_hangul(&self.player_name) {
            families.push_str("&family=Noto+Sans+KR:wght@500;700");
        }
        if contains_han(&self.player_name) {
            families.push_str("&family=Noto+Sans+SC:wght@500;700");
        }
        format!("https://fonts.googleapis.com/css2?{families}&display=swap")
    }

    fn push_sidebar(&self, s: &mut String) {
        s.push_str("    <div style=\"position:relative;width:248px;flex:none;overflow:hidden;border-right:1px solid var(--edge,#22405f)\">\n");
        s.push_str("      <img src=\"");
        s.push_str(&escape_html(&self.champion_splash_src));
        s.push_str("\" style=\"position:absolute;inset:0;width:100%;height:100%;object-fit:cover;object-position:center 10%\">\n");
        s.push_str("      <div style=\"position:absolute;inset:0;background:var(--overlay,linear-gradient(180deg,rgba(20,60,110,.4),rgba(6,12,20,.85) 64%,#060e18))\"></div>\n");

        // Ribbon + grade badge row.
        s.push_str("      <div style=\"position:relative;display:flex;align-items:flex-start;justify-content:space-between;padding:14px 14px 0\">\n");
        s.push_str("        <span style=\"padding:5px 11px;background:var(--ribbon-bg,linear-gradient(90deg,#1c4d8c,#2f6fb8));border:1px solid var(--ribbon-edge,#c8aa6e);border-radius:5px;font:700 11px/1 Cinzel,serif;letter-spacing:.2em;color:var(--ribbon-text,#f5efe0)\">");
        s.push_str(self.result.result_label());
        s.push_str("</span>\n");
        if let Some(grade) = &self.grade {
            s.push_str("        <div style=\"position:relative;width:52px;height:52px;display:flex;align-items:center;justify-content:center\">\n");
            s.push_str("          <div style=\"position:absolute;inset:3px;background:var(--grade-bg,linear-gradient(150deg,#20334d,#0c1826));border:2px solid var(--grade-edge,#c8aa6e);border-radius:11px;transform:rotate(45deg);box-shadow:0 0 16px -3px rgba(120,120,120,.35)\"></div>\n");
            s.push_str("          <span style=\"position:relative;font:700 22px/1 Cinzel,serif;color:var(--grade-text,#f5efe0)\">");
            s.push_str(&escape_html(grade));
            s.push_str("</span>\n        </div>\n");
        }
        s.push_str("      </div>\n");

        // Bottom-anchored identity + footer block.
        s.push_str(
            "      <div style=\"position:absolute;left:0;right:0;bottom:0;padding:0 14px 14px\">\n",
        );
        s.push_str("        <div style=\"font:700 19px/1.1 'Noto Sans JP','Noto Sans KR','Noto Sans SC','Barlow Semi Condensed',sans-serif;color:var(--value,#f5efe0)\">");
        s.push_str(&escape_html(&self.player_name));
        s.push_str(
            "<span style=\"color:var(--value-soft,#8aa0b8);font-weight:600;font-size:14px\">",
        );
        s.push_str(&escape_html(&self.player_tag));
        s.push_str("</span></div>\n");

        // Role + champion chips.
        s.push_str("        <div style=\"display:flex;gap:6px;margin-top:7px\">\n");
        if !self.role_upper.is_empty() {
            s.push_str("          <span style=\"display:inline-flex;align-items:center;gap:4px;padding:2px 7px;border:1px solid var(--chip-edge,#7a5a2a);border-radius:4px;font:700 9px/1.4;letter-spacing:.14em;color:var(--chip-text,#c8aa6e);text-transform:uppercase\">");
            if let Some(src) = &self.role_icon_src {
                s.push_str("<img src=\"");
                s.push_str(&escape_html(src));
                s.push_str("\" style=\"width:16px;height:16px;object-fit:contain;filter:brightness(0) invert(0.78)\">");
            }
            s.push_str(&escape_html(&self.role_upper));
            s.push_str("</span>\n");
        }
        s.push_str("          <span style=\"padding:2px 7px;background:var(--chip-bg,rgba(6,12,20,.6));border:1px solid var(--chip-edge-2,#2c4864);border-radius:4px;font:700 9px/1.4;letter-spacing:.08em;color:var(--chip-text-2,#9fb4cc);text-transform:uppercase;white-space:nowrap\">");
        s.push_str(&escape_html(&self.champion_name));
        if let Some(level) = self.champion_level {
            s.push_str(" · Lvl ");
            s.push_str(&level.to_string());
        }
        s.push_str("</span>\n        </div>\n");

        s.push_str("        <div style=\"height:1px;background:var(--footer-edge,rgba(200,170,110,.25));margin:12px 0\"></div>\n");

        if let Some(rank) = &self.rank {
            self.push_ranked_block(s, rank);
        } else {
            self.push_mastery_block(s);
        }

        s.push_str("      </div>\n");
        s.push_str("    </div>\n");
    }

    fn push_ranked_block(&self, s: &mut String, rank: &RankView) {
        s.push_str("        <div>\n");
        s.push_str("          <div style=\"display:flex;align-items:center;gap:10px\">\n");
        if let Some(src) = &rank.emblem_src {
            s.push_str("            <img src=\"");
            s.push_str(&escape_html(src));
            s.push_str("\" style=\"width:52px;height:52px;flex:none;object-fit:contain;filter:drop-shadow(0 1px 5px rgba(0,0,0,.6))\">\n");
        }
        s.push_str("            <div>\n");
        s.push_str("              <div style=\"font:700 13px/1.1 Cinzel,serif;color:var(--value,#e6d9b8);white-space:nowrap\">");
        s.push_str(&escape_html(&rank.tier));
        s.push_str("</div>\n");
        s.push_str("              <div style=\"font:700 11px/1;margin-top:3px;color:var(--value,#cddcec)\">");
        s.push_str(&rank.lp.to_string());
        s.push_str(" LP");
        if let Some(delta) = &rank.delta {
            s.push_str(" <span style=\"color:var(--delta,#31d07f)\">(");
            s.push_str(&escape_html(delta));
            s.push_str(")</span>");
        }
        s.push_str("</div>\n            </div>\n");

        if let (Some(wr), Some(wins), Some(losses)) = (rank.winrate, rank.wins, rank.losses) {
            s.push_str("            <div style=\"margin-left:auto;text-align:right\">\n");
            s.push_str("              <div style=\"font:700 13px/1;color:var(--value,#f0e6d2)\">");
            s.push_str(&wr.to_string());
            s.push_str("% <span style=\"font-size:9px;color:var(--label,#6a8199);font-weight:600\">WR</span></div>\n");
            s.push_str("              <div style=\"font:600 9px/1;margin-top:3px;color:var(--value-soft,#8aa0b8)\">");
            s.push_str(&wins.to_string());
            s.push_str("W · ");
            s.push_str(&losses.to_string());
            s.push_str("L</div>\n            </div>\n");
        }
        s.push_str("          </div>\n");

        if let Some(progress) = rank.progress {
            s.push_str("          <div style=\"margin-top:10px;height:5px;border-radius:3px;background:var(--track,#12283f);overflow:hidden\"><div style=\"width:");
            s.push_str(&progress.to_string());
            s.push_str(
                "%;height:100%;background:linear-gradient(90deg,#1c8f6b,#31d07f)\"></div></div>\n",
            );
        }

        if !rank.streak.is_empty() {
            s.push_str("          <div style=\"display:flex;gap:3px;margin-top:8px\">");
            for win in &rank.streak {
                let color = if *win { "#1e9e6a" } else { "#c0455f" };
                s.push_str("<span style=\"flex:1;height:5px;border-radius:2px;background:");
                s.push_str(color);
                s.push_str("\"></span>");
            }
            s.push_str("</div>\n");
        }

        s.push_str("        </div>\n");
    }

    fn push_mastery_block(&self, s: &mut String) {
        s.push_str("        <div style=\"display:flex;align-items:center;gap:10px\">\n");
        if let Some(mastery) = &self.mastery {
            s.push_str("          <div style=\"width:26px;height:26px;flex:none;transform:rotate(45deg);border-radius:5px;background:linear-gradient(135deg,#e6c974,#8a6a24);border:1px solid #f0dfa0;box-shadow:0 0 10px -2px rgba(200,170,110,.5)\"></div>\n");
            s.push_str("          <div>\n");
            s.push_str("            <div style=\"font:700 13px/1.1 Cinzel,serif;color:var(--value,#e6d9b8)\">Mastery ");
            s.push_str(&escape_html(&mastery.level));
            s.push_str("</div>\n");
            s.push_str("            <div style=\"font:600 10px/1;margin-top:3px;color:var(--value-soft,#9fb4cc)\">");
            s.push_str(&escape_html(&mastery.points));
            s.push_str(" pts</div>\n          </div>\n");
        }
        s.push_str("          <span style=\"margin-left:auto;padding:3px 9px;background:var(--chip-bg,rgba(6,12,20,.6));border:1px solid var(--chip-edge-2,#2c4864);border-radius:20px;font:700 9px/1.4;letter-spacing:.1em;color:var(--chip-text-2,#9fb4cc);text-transform:uppercase\">");
        s.push_str(&escape_html(&self.queue_label));
        s.push_str("</span>\n        </div>\n");
    }

    fn push_main(&self, s: &mut String) {
        s.push_str("    <div style=\"flex:1;min-width:0;display:flex;flex-direction:column;padding:14px 16px\">\n");

        // Header row.
        s.push_str("      <div style=\"display:flex;align-items:center;justify-content:space-between;margin-bottom:11px\">\n");
        s.push_str("        <span style=\"font:700 10px/1;letter-spacing:.16em;color:var(--label,#6a8199);text-transform:uppercase\">Match Summary</span>\n");
        s.push_str("        <span style=\"font:600 10.5px/1;letter-spacing:.05em;color:var(--value-soft,#8fb2da);text-transform:uppercase\">");
        s.push_str(&escape_html(&self.meta_line));
        s.push_str("</span>\n      </div>\n");

        // KDA + chips.
        s.push_str(
            "      <div style=\"display:flex;align-items:flex-end;gap:14px;margin-bottom:13px\">\n",
        );
        s.push_str("        <div style=\"font:700 38px/0.85 'Barlow Semi Condensed';color:var(--value,#f5efe0)\">");
        s.push_str(&self.kda.kills.to_string());
        s.push_str("<span style=\"color:var(--slash,#3f5470);margin:0 6px;font-weight:500\">/</span><span style=\"color:var(--death,#e84057)\">");
        s.push_str(&self.kda.deaths.to_string());
        s.push_str("</span><span style=\"color:var(--slash,#3f5470);margin:0 6px;font-weight:500\">/</span>");
        s.push_str(&self.kda.assists.to_string());
        s.push_str("</div>\n");
        s.push_str("        <div style=\"display:flex;flex-direction:column;gap:6px;padding-bottom:3px\">\n");
        s.push_str("          <span style=\"font:700 14px/1;color:var(--accent,#c8aa6e)\">");
        s.push_str(&escape_html(&self.kda_ratio));
        s.push_str(" KDA</span>\n");
        s.push_str("          <div style=\"display:flex;gap:6px\">\n");
        if let Some(kp) = self.kp {
            s.push_str("            <span style=\"padding:2px 7px;border-radius:4px;background:var(--chip-bg,rgba(200,170,110,.12));border:1px solid var(--chip-edge,#7a5a2a);font:700 9.5px/1.4;letter-spacing:.08em;color:var(--chip-text,#d8c79a);white-space:nowrap;text-transform:uppercase\">");
            s.push_str(&kp.to_string());
            s.push_str("% KP</span>\n");
        }
        if self.show_multi_chip()
            && let Some(multi) = &self.multi
        {
            s.push_str("            <span style=\"padding:2px 7px;border-radius:4px;background:var(--accent,#c8aa6e);font:700 9.5px/1.4;letter-spacing:.08em;color:#1a1206;white-space:nowrap;text-transform:uppercase\">");
            s.push_str(&escape_html(multi));
            s.push_str("</span>\n");
        }
        s.push_str("          </div>\n        </div>\n      </div>\n");

        self.push_stats_grid(s);
        self.push_loadout(s);
        if self.has_bottom() {
            self.push_bottom(s);
        }

        s.push_str("    </div>\n");
    }

    fn push_stats_grid(&self, s: &mut String) {
        s.push_str("      <div style=\"display:grid;grid-template-columns:repeat(3,1fr);gap:8px;margin-bottom:10px\">\n");
        self.push_stat_panel(s, "CS", &self.cs);
        self.push_stat_panel(s, "Gold", &self.gold);
        self.push_damage_panel(s);
        self.push_stat_panel(s, "Vision", &self.vision);
        self.push_kp_panel(s);
        self.push_multi_panel(s);
        s.push_str("      </div>\n");
    }

    /// A generic value + optional sub stat panel.
    fn push_stat_panel(&self, s: &mut String, label: &str, stat: &StatValue) {
        s.push_str(PANEL_OPEN);
        push_panel_label(s, label);
        s.push_str(
            "<div style=\"font:700 19px/1 'Barlow Semi Condensed';color:var(--value,#f0e6d2)\">",
        );
        s.push_str(&escape_html(&stat.value));
        if let Some(sub) = &stat.sub {
            s.push_str(
                " <span style=\"font-size:10px;color:var(--label,#6a8199);font-weight:600\">",
            );
            s.push_str(&escape_html(sub));
            s.push_str("</span>");
        }
        s.push_str("</div></div>\n");
    }

    fn push_damage_panel(&self, s: &mut String) {
        s.push_str(PANEL_OPEN);
        push_panel_label(s, "Damage");
        s.push_str(
            "<div style=\"font:700 19px/1 'Barlow Semi Condensed';color:var(--value,#f0e6d2)\">",
        );
        s.push_str(&escape_html(&self.damage_value));
        s.push_str("</div>");
        if let Some(share) = self.damage_share {
            s.push_str("<div style=\"margin-top:5px;height:4px;border-radius:2px;background:var(--track,#12283f);overflow:hidden\"><div style=\"width:");
            s.push_str(&share.to_string());
            s.push_str("%;height:100%;background:var(--accent,#c8aa6e)\"></div></div><div style=\"margin-top:3px;font:600 9px/1;color:var(--label,#6a8199)\">");
            s.push_str(&share.to_string());
            s.push_str("% team</div>");
        }
        s.push_str("</div>\n");
    }

    fn push_kp_panel(&self, s: &mut String) {
        s.push_str(PANEL_OPEN);
        push_panel_label(s, "KP");
        s.push_str(
            "<div style=\"font:700 19px/1 'Barlow Semi Condensed';color:var(--value,#f0e6d2)\">",
        );
        match self.kp {
            Some(kp) => {
                s.push_str(&kp.to_string());
                s.push('%');
                if let Some(sub) = &self.kp_sub {
                    s.push_str(" <span style=\"font-size:10px;color:var(--label,#6a8199);font-weight:600\">");
                    s.push_str(&escape_html(sub));
                    s.push_str("</span>");
                }
            }
            None => s.push('—'),
        }
        s.push_str("</div></div>\n");
    }

    fn push_multi_panel(&self, s: &mut String) {
        s.push_str(PANEL_OPEN);
        push_panel_label(s, "Multi-kills");
        s.push_str(
            "<div style=\"font:700 15px/1 'Barlow Semi Condensed';color:var(--accent,#c8aa6e)\">",
        );
        match &self.multi {
            Some(multi) => {
                s.push_str(&escape_html(multi));
                if let Some(sub) = &self.multi_sub {
                    s.push_str(" <span style=\"font-size:10px;color:var(--label,#6a8199);font-weight:600\">");
                    s.push_str(&escape_html(sub));
                    s.push_str("</span>");
                }
            }
            None => s.push('—'),
        }
        s.push_str("</div></div>\n");
    }

    fn push_loadout(&self, s: &mut String) {
        let l = &self.loadout;
        s.push_str("      <div style=\"display:flex;align-items:center;gap:7px;background:var(--panel-bg,linear-gradient(180deg,#0c1e34,#07131f));border:1px solid var(--panel-edge,#1c3a58);border-radius:9px;padding:8px 10px;margin-bottom:10px\">\n");

        let mut need_divider = false;

        // Spells + runes group.
        let has_spells_runes =
            !l.spell_srcs.is_empty() || l.keystone_src.is_some() || l.secondary_src.is_some();
        if has_spells_runes {
            for src in &l.spell_srcs {
                s.push_str("        <img src=\"");
                s.push_str(&escape_html(src));
                s.push_str("\" style=\"width:32px;height:32px;border-radius:5px;border:1px solid var(--panel-edge,#35506f)\">\n");
            }
            if let Some(src) = &l.keystone_src {
                s.push_str("        <div style=\"width:32px;height:32px;border-radius:50%;background:radial-gradient(circle,#141c26,#0a0f16);border:1px solid var(--chip-edge,#7a5a2a);display:flex;align-items:center;justify-content:center\"><img src=\"");
                s.push_str(&escape_html(src));
                s.push_str("\" style=\"width:27px;height:27px\"></div>\n");
            }
            if let Some(src) = &l.secondary_src {
                s.push_str("        <div style=\"width:32px;height:32px;border-radius:50%;background:radial-gradient(circle,#141c26,#0a0f16);border:1px solid var(--panel-edge,#35506f);display:flex;align-items:center;justify-content:center\"><img src=\"");
                s.push_str(&escape_html(src));
                s.push_str("\" style=\"width:26px;height:26px\"></div>\n");
            }
            need_divider = true;
        }

        // Items group.
        if !l.items.is_empty() {
            if need_divider {
                push_loadout_divider(s);
            }
            for slot in &l.items {
                match slot {
                    Some(src) => {
                        s.push_str("        <img src=\"");
                        s.push_str(&escape_html(src));
                        s.push_str("\" style=\"width:32px;height:32px;border-radius:5px;border:1px solid var(--panel-edge,#2c4864)\">\n");
                    }
                    None => s.push_str("        <div style=\"width:32px;height:32px;border-radius:5px;border:1px dashed var(--panel-edge,#2c4864);background:rgba(0,0,0,.22)\"></div>\n"),
                }
            }
            need_divider = true;
        }

        // Trinket group.
        if let Some(src) = &l.trinket_src {
            if need_divider {
                push_loadout_divider(s);
            }
            s.push_str("        <img src=\"");
            s.push_str(&escape_html(src));
            s.push_str(
                "\" style=\"width:32px;height:32px;border-radius:5px;border:1px solid #4a6a3a\">\n",
            );
        }

        s.push_str("      </div>\n");
    }

    fn push_bottom(&self, s: &mut String) {
        s.push_str("      <div style=\"display:flex;gap:8px\">\n");
        if let Some(matchup) = &self.matchup {
            self.push_matchup_panel(s, matchup);
        }
        if let Some(team) = &self.team {
            self.push_team_panel(s, team);
        }
        s.push_str("      </div>\n");
    }

    fn push_matchup_panel(&self, s: &mut String, m: &MatchupView) {
        s.push_str("        <div style=\"flex:1.5;background:var(--panel-bg,linear-gradient(180deg,#0c1e34,#07131f));border:1px solid var(--panel-edge,#1c3a58);border-radius:9px;padding:9px 12px\">\n");
        s.push_str("          <div style=\"display:flex;align-items:center;justify-content:space-between;margin-bottom:9px\">\n");
        s.push_str("            <span style=\"display:inline-flex;align-items:center;gap:5px;font:700 9px/1;letter-spacing:.12em;color:var(--label,#6a8199);text-transform:uppercase\">");
        if let Some(src) = &self.role_icon_src {
            s.push_str("<img src=\"");
            s.push_str(&escape_html(src));
            s.push_str("\" style=\"width:17px;height:17px;object-fit:contain;filter:brightness(0) invert(0.55)\">");
        }
        s.push_str(&escape_html(&m.label));
        s.push_str("</span>\n");
        s.push_str(
            "            <span style=\"display:flex;align-items:center;gap:6px\"><img src=\"",
        );
        s.push_str(&escape_html(&m.opp_icon_src));
        s.push_str("\" style=\"width:18px;height:18px;border-radius:4px;border:1px solid var(--panel-edge,#4a3030)\"><span style=\"font:700 10px/1;color:var(--value,#cddcec)\">");
        s.push_str(&escape_html(&m.opp_name));
        s.push(' ');
        s.push_str(&escape_html(&m.opp_kda));
        s.push_str("</span></span>\n          </div>\n");

        for row in &m.rows {
            let opp_pct = 100u8.saturating_sub(row.you_pct);
            s.push_str("          <div style=\"display:flex;align-items:center;gap:7px;margin-bottom:6px\"><span style=\"width:30px;font:700 8px/1;letter-spacing:.06em;color:var(--label,#6a8199);text-transform:uppercase\">");
            s.push_str(&escape_html(&row.key));
            s.push_str("</span><span style=\"width:38px;text-align:right;font:700 10px/1;color:var(--accent,#c8aa6e)\">");
            s.push_str(&escape_html(&row.you));
            s.push_str("</span><div style=\"flex:1;display:flex;height:5px;border-radius:3px;overflow:hidden;background:var(--track,#0f2236)\"><div style=\"flex:");
            s.push_str(&row.you_pct.to_string());
            s.push_str(";background:var(--accent,#c8aa6e)\"></div><div style=\"flex:");
            s.push_str(&opp_pct.to_string());
            s.push_str(";background:var(--track-opp,#33546f)\"></div></div><span style=\"width:32px;font:700 10px/1;color:var(--value-soft,#7d93ac)\">");
            s.push_str(&escape_html(&row.opp));
            s.push_str("</span></div>\n");
        }

        s.push_str("        </div>\n");
    }

    fn push_team_panel(&self, s: &mut String, t: &TeamView) {
        s.push_str("        <div style=\"flex:1;background:var(--panel-bg,linear-gradient(180deg,#0c1e34,#07131f));border:1px solid var(--panel-edge,#1c3a58);border-radius:9px;padding:9px 12px\">\n");
        s.push_str("          <div style=\"font:700 9px/1;letter-spacing:.12em;color:var(--label,#6a8199);text-transform:uppercase;margin-bottom:9px\">Team · Objectives</div>\n");
        s.push_str("          <div style=\"display:flex;align-items:baseline;gap:6px;margin-bottom:11px\"><span style=\"font:700 22px/1;color:var(--accent-2,#31d07f)\">");
        s.push_str(&t.kills.to_string());
        s.push_str("</span><span style=\"font:700 9px/1;letter-spacing:.08em;color:var(--label,#6a8199);text-transform:uppercase\">kills · opp. ");
        s.push_str(&t.opp_kills.to_string());
        s.push_str("</span></div>\n");

        if !t.objectives.is_empty() {
            s.push_str("          <div style=\"display:flex;gap:13px;flex-wrap:wrap\">\n");
            for ob in &t.objectives {
                s.push_str("            <div style=\"display:flex;align-items:center;gap:5px\"><img src=\"");
                s.push_str(&escape_html(&ob.icon_src));
                s.push_str("\" style=\"width:16px;height:16px;object-fit:contain;flex:none\"><span style=\"font:700 13px/1;color:var(--value,#e6edf5)\">");
                s.push_str(&ob.count.to_string());
                s.push_str("</span><span style=\"font:600 9px/1;letter-spacing:.04em;color:var(--label,#6a8199);text-transform:uppercase\">");
                s.push_str(&escape_html(&ob.label));
                s.push_str("</span></div>\n");
            }
            s.push_str("          </div>\n");
        }

        s.push_str("        </div>\n");
    }
}

/// Shared opening markup for a stat panel.
const PANEL_OPEN: &str = "        <div style=\"display:flex;flex-direction:column;justify-content:center;gap:6px;background:var(--panel-bg,linear-gradient(180deg,#0c1e34,#07131f));border:1px solid var(--panel-edge,#1c3a58);border-radius:9px;padding:10px 11px\">";

fn push_panel_label(s: &mut String, label: &str) {
    s.push_str("<div style=\"font:700 9px/1;letter-spacing:.12em;color:var(--label,#6a8199);text-transform:uppercase\">");
    s.push_str(&escape_html(label));
    s.push_str("</div>");
}

fn push_loadout_divider(s: &mut String) {
    s.push_str("        <div style=\"width:1px;height:28px;background:var(--divider,#24425f);margin:0 3px\"></div>\n");
}

// ============================================================================
// Helpers
// ============================================================================

/// Escape the five HTML-significant characters. Applied to every dynamic field
/// so Riot IDs (which can contain `< > & " '`) can never break the markup.
fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

/// Whether `s` contains any Hangul (Korean) code point — Jamo or syllable blocks.
fn contains_hangul(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(c,
            '\u{1100}'..='\u{11FF}'   // Jamo
            | '\u{3130}'..='\u{318F}' // Compatibility Jamo
            | '\u{AC00}'..='\u{D7A3}' // Syllables
        )
    })
}

/// Whether `s` contains any Han (CJK Unified) ideograph — covers Chinese and
/// Japanese kanji. Japanese kana are served by the always-loaded JP subset, so
/// they don't need detecting here.
fn contains_han(s: &str) -> bool {
    s.chars()
        .any(|c| matches!(c, '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}'))
}

fn item_src(version: &str, id: i32) -> String {
    format!("{DDRAGON}/{version}/img/item/{id}.png")
}

fn champion_square_src(version: &str, key: &str) -> String {
    format!("{DDRAGON}/{version}/img/champion/{key}.png")
}

/// Role position glyph (SVG) for a Riot `teamPosition` (`JUNGLE` → the jungle
/// icon). `None` for an empty/unknown position (ARAM), which drops the role
/// chip and the matchup header icon.
fn position_icon_src(team_position: &str) -> Option<String> {
    let key = match team_position {
        "TOP" => "top",
        "JUNGLE" => "jungle",
        "MIDDLE" => "middle",
        "BOTTOM" => "bottom",
        "UTILITY" => "utility",
        _ => return None,
    };
    Some(format!(
        "{CDRAGON}/plugins/rcp-fe-lol-static-assets/global/default/svg/position-{key}.svg"
    ))
}

/// Ranked-tier mini-crest (SVG) for a tier label like `Emerald IV` → the emerald
/// crest. `None` when the leading tier word isn't one of the ten known tiers.
fn rank_emblem_src(tier: &str) -> Option<String> {
    let key = tier.split_whitespace().next().unwrap_or("").to_lowercase();
    matches!(
        key.as_str(),
        "iron"
            | "bronze"
            | "silver"
            | "gold"
            | "platinum"
            | "emerald"
            | "diamond"
            | "master"
            | "grandmaster"
            | "challenger"
    )
    .then(|| {
        format!(
            "{CDRAGON}/plugins/rcp-fe-lol-static-assets/global/default/images/ranked-mini-crests/{key}.svg"
        )
    })
}

/// Minimap objective icon (PNG) — `dragon` / `baron` / `tower`.
fn objective_icon_src(name: &str) -> String {
    format!("{CDRAGON}/game/assets/ux/minimap/icons/{name}.png")
}

/// Short numeric format used for damage/gold: `999`, `1.2k`, `1.0M`.
fn format_short(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn format_signed(n: i32) -> String {
    if n > 0 {
        format!("+{n}")
    } else {
        n.to_string()
    }
}

/// `round(100 * part / whole)`, saturating to a `u8`. `0` when `whole <= 0`.
fn percent(part: i64, whole: i64) -> u8 {
    if whole <= 0 {
        0
    } else {
        ((part as f64 / whole as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8
    }
}

/// Player's share of a head-to-head total, `50` when both are zero.
fn share(you: i64, opp: i64) -> u8 {
    let total = you + opp;
    if total <= 0 { 50 } else { percent(you, total) }
}

/// Rounded win-rate percentage from optional W/L counts (`None` if unavailable
/// or no games played).
fn win_rate(wins: Option<i32>, losses: Option<i32>) -> Option<u8> {
    let (w, l) = (wins?, losses?);
    (w + l > 0).then(|| percent(w as i64, (w + l) as i64))
}

/// Division-progress bar value (0..=100) from LP. Apex tiers (Master and above)
/// have no 100-LP division, so the bar is hidden there.
fn division_progress(tier: &str, lp: i32) -> Option<u8> {
    let apex = matches!(
        tier.to_uppercase().as_str(),
        "MASTER" | "GRANDMASTER" | "CHALLENGER"
    );
    (!apex).then(|| lp.clamp(0, 100) as u8)
}

fn calculate_lp_diff(old_rank: Option<&RankInfo>, new_rank: Option<&RankInfo>) -> Option<i32> {
    let old = old_rank?;
    let new = new_rank?;
    Some(rank_to_lp(new) - rank_to_lp(old))
}

fn rank_to_lp(rank: &RankInfo) -> i32 {
    let tier_value = match rank.tier.to_uppercase().as_str() {
        "IRON" => 0,
        "BRONZE" => 400,
        "SILVER" => 800,
        "GOLD" => 1200,
        "PLATINUM" => 1600,
        "EMERALD" => 2000,
        "DIAMOND" => 2400,
        "MASTER" => 2800,
        "GRANDMASTER" => 3200,
        "CHALLENGER" => 3600,
        _ => 0,
    };
    let division_value = match rank.rank.as_str() {
        "IV" => 0,
        "III" => 100,
        "II" => 200,
        "I" => 300,
        _ => 0,
    };
    tier_value + division_value + rank.lp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Player;
    use crate::discord::ddragon::DdragonData;
    use crate::discord::image_gen::{MasteryInfo, MatchImageContext};
    use crate::riot::{ObjectiveCountDto, ObjectivesDto};

    const DDRAGON_VER: &str = "14.24.1";

    /// Build a `TeamDto` with the three objective counts the card renders.
    fn team_dto(team_id: i32, dragon: i32, baron: i32, tower: i32) -> TeamDto {
        TeamDto {
            team_id,
            objectives: ObjectivesDto {
                dragon: ObjectiveCountDto { kills: dragon },
                baron: ObjectiveCountDto { kills: baron },
                tower: ObjectiveCountDto { kills: tower },
            },
        }
    }

    /// A fully-populated view-model mirroring the design's `defaultMatch()`, so
    /// `build_html` can be validated against the POC screenshot for every theme.
    fn sample(result: MatchResult) -> ScorecardVm {
        let item = |id: i32| Some(item_src(DDRAGON_VER, id));
        ScorecardVm {
            result,
            champion_splash_src: format!("{DDRAGON}/img/champion/loading/Viego_0.jpg"),
            grade: Some("S+".into()),
            player_name: "れいん".into(),
            player_tag: "#01010".into(),
            role_upper: "JUNGLE".into(),
            role_icon_src: position_icon_src("JUNGLE"),
            champion_name: "Viego".into(),
            champion_level: Some(16),
            rank: Some(RankView {
                tier: "Emerald IV".into(),
                emblem_src: rank_emblem_src("Emerald IV"),
                lp: 63,
                delta: Some("+19".into()),
                progress: Some(63),
                winrate: Some(58),
                wins: Some(32),
                losses: Some(23),
                streak: vec![true, true, false, true, true],
            }),
            mastery: Some(MasteryView {
                level: "7".into(),
                points: "142k".into(),
            }),
            queue_label: "Ranked Solo/Duo".into(),
            meta_line: "Ranked Solo/Duo · 32:35 · Patch 16.14".into(),
            kda: Kda {
                kills: 16,
                deaths: 1,
                assists: 4,
            },
            kda_ratio: "20.00".into(),
            kp: Some(71),
            kp_sub: Some("20/43".into()),
            multi: Some("1 Triple".into()),
            multi_sub: Some("+2 double".into()),
            cs: StatValue::new("303", Some("9.3/m".into())),
            gold: StatValue::new("15.2k", Some("466/m".into())),
            damage_value: "40.1k".into(),
            damage_share: Some(34),
            vision: StatValue::new("21", Some("12·7·4".into())),
            loadout: Loadout {
                spell_srcs: vec![
                    format!("{DDRAGON}/{DDRAGON_VER}/img/spell/SummonerFlash.png"),
                    format!("{DDRAGON}/{DDRAGON_VER}/img/spell/SummonerSmite.png"),
                ],
                keystone_src: Some(format!(
                    "{DDRAGON}/img/perk-images/Styles/Precision/Conqueror/Conqueror.png"
                )),
                secondary_src: Some(format!(
                    "{DDRAGON}/img/perk-images/Styles/7201_Precision.png"
                )),
                items: vec![
                    item(3153),
                    item(3006),
                    item(6672),
                    item(3031),
                    item(3072),
                    item(3026),
                ],
                trinket_src: item(3364),
            },
            matchup: Some(MatchupView {
                label: "Matchup".into(),
                opp_icon_src: champion_square_src(DDRAGON_VER, "Khazix"),
                opp_name: "Kha'Zix".into(),
                opp_kda: "4/6/3".into(),
                rows: vec![
                    MatchupRow {
                        key: "DMG".into(),
                        you: "40.1k".into(),
                        opp: "22.4k".into(),
                        you_pct: 64,
                    },
                    MatchupRow {
                        key: "CS".into(),
                        you: "303".into(),
                        opp: "210".into(),
                        you_pct: 59,
                    },
                    MatchupRow {
                        key: "Gold".into(),
                        you: "15.2k".into(),
                        opp: "11.1k".into(),
                        you_pct: 58,
                    },
                ],
            }),
            team: Some(TeamView {
                kills: 43,
                opp_kills: 38,
                objectives: vec![
                    Objective {
                        icon_src: objective_icon_src("dragon"),
                        count: 3,
                        label: "Drakes".into(),
                    },
                    Objective {
                        icon_src: objective_icon_src("baron"),
                        count: 1,
                        label: "Nashor".into(),
                    },
                    Objective {
                        icon_src: objective_icon_src("tower"),
                        count: 8,
                        label: "Towers".into(),
                    },
                ],
            }),
        }
    }

    /// Writes the three themed sample cards to `target/scorecard-samples/` and
    /// checks the theme + a few resolved values are present. Open the files in a
    /// browser (or screenshot `#card-root`) to visually validate against the POC.
    #[test]
    fn renders_sample_cards_for_every_theme() {
        let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/target/scorecard-samples");
        std::fs::create_dir_all(out_dir).unwrap();

        for (result, name, label) in [
            (MatchResult::Victory, "victory", "VICTORY"),
            (MatchResult::Defeat, "defeat", "DEFEAT"),
            (MatchResult::Remake, "remake", "REMAKE"),
        ] {
            let html = sample(result).build_html();

            // Banner label + a couple of resolved values are present.
            assert!(html.contains(label), "missing banner label for {name}");
            assert!(html.contains("Matchup"));
            assert!(html.contains("40.1k"));
            assert!(html.contains("Emerald IV"));
            // Card must be self-contained: exactly one root, fonts linked.
            assert_eq!(html.matches("id=\"card-root\"").count(), 1);
            assert!(html.contains("fonts.googleapis.com"));

            std::fs::write(format!("{out_dir}/{name}.html"), &html).unwrap();
        }
    }

    #[test]
    fn inlines_assets_via_map() {
        let vm = sample(MatchResult::Victory);
        let urls = vm.image_urls();
        // splash + role icon + 2 spells + keystone + secondary + 6 items + trinket
        // + rank emblem + matchup icon + 3 objective icons.
        assert_eq!(urls.len(), 18);
        assert!(urls.contains(&vm.champion_splash_src));
        assert!(urls.iter().any(|u| u.contains("/img/item/")));

        // Map every URL to a fake data URI and apply it.
        let map: HashMap<String, String> = urls
            .iter()
            .enumerate()
            .map(|(i, u)| (u.clone(), format!("data:image/png;base64,inlined{i}")))
            .collect();
        let mut vm = vm;
        vm.apply_asset_map(&map);
        let html = vm.build_html();

        // Every DDragon URL is now inlined; none leaks into the markup.
        assert!(!html.contains("ddragon.leagueoflegends.com"));
        assert!(html.contains("data:image/png;base64,inlined"));
    }

    #[test]
    fn remake_hides_multi_kill_chip() {
        let mut vm = sample(MatchResult::Remake);
        vm.multi = Some("1 Triple".into());
        let html = vm.build_html();
        // The multi-kill *panel* still shows the value, but the KDA chip (gold
        // background) must be suppressed on a remake.
        assert!(
            !html.contains("color:#1a1206"),
            "multi chip leaked on remake"
        );
    }

    #[test]
    fn escapes_dynamic_fields() {
        let mut vm = sample(MatchResult::Victory);
        vm.player_name = "<script>&\"'".into();
        let html = vm.build_html();
        assert!(html.contains("&lt;script&gt;&amp;&quot;&#39;"));
        // The raw payload must never appear unescaped. (The card's own
        // error-handler `<script>` in the head is the only legitimate script tag,
        // so we check for the exact injected string rather than any `<script>`.)
        assert!(
            !html.contains("<script>&\"'"),
            "player name injected raw markup"
        );
    }

    #[test]
    fn omits_blocks_when_data_is_absent() {
        // A minimal victory VM with everything optional turned off.
        let vm = ScorecardVm {
            result: MatchResult::Victory,
            champion_splash_src: format!("{DDRAGON}/img/champion/loading/Viego_0.jpg"),
            grade: None,
            player_name: "Solo".into(),
            player_tag: "#EUW".into(),
            role_upper: String::new(), // ARAM-style: no role chip
            role_icon_src: None,
            champion_name: "Viego".into(),
            champion_level: None,
            rank: None,
            mastery: None,
            queue_label: "ARAM".into(),
            meta_line: "ARAM · 18:20 · Patch 16.14".into(),
            kda: Kda {
                kills: 5,
                deaths: 5,
                assists: 5,
            },
            kda_ratio: "2.00".into(),
            kp: None,
            kp_sub: None,
            multi: None,
            multi_sub: None,
            cs: StatValue::new("120", Some("6.5/m".into())),
            gold: StatValue::new("9.8k", Some("533/m".into())),
            damage_value: "18.0k".into(),
            damage_share: None,
            vision: StatValue::new("8", None),
            loadout: Loadout {
                items: vec![
                    Some(item_src(DDRAGON_VER, 3153)),
                    None,
                    None,
                    None,
                    None,
                    None,
                ],
                ..Default::default()
            },
            matchup: None,
            team: None,
        };
        let html = vm.build_html();

        // No grade badge, no role chip, KP/Multi panels degrade to an em dash,
        // and the whole bottom row is gone.
        assert!(!html.contains("rotate(45deg)")); // grade badge / mastery diamond absent
        assert!(!html.contains("JUNGLE"));
        assert!(html.contains("—"));
        assert!(!html.contains("Matchup"));
        assert!(!html.contains("Team · Objectives"));
        // Queue label still shows in the mastery/queue fallback block.
        assert!(html.contains("ARAM"));
        // A dashed empty item slot is rendered.
        assert!(html.contains("1px dashed"));
    }

    #[allow(clippy::too_many_arguments)]
    fn participant(
        puuid: &str,
        position: &str,
        champion: &str,
        win: bool,
        kills: i32,
        deaths: i32,
        assists: i32,
        damage: i64,
        cs: i32,
        gold: i64,
    ) -> ParticipantDto {
        ParticipantDto {
            puuid: puuid.into(),
            team_position: position.into(),
            champion_name: champion.into(),
            kills,
            deaths,
            assists,
            total_damage_dealt_to_champions: damage,
            total_minions_killed: cs,
            vision_score: 20,
            gold_earned: gold,
            win,
            // Blue (100) / red (200) split, mirroring `win` in the fixtures.
            team_id: if win { 100 } else { 200 },
            item0: 3153,
            item1: 3006,
            item6: 3364,
            ..Default::default()
        }
    }

    fn player() -> Player {
        Player {
            id: 1,
            puuid: "me".into(),
            game_name: "れいん".into(),
            tag_line: "01010".into(),
            region: "EUW1".into(),
            last_match_id: None,
            last_rank_solo_tier: None,
            last_rank_solo_rank: None,
            last_rank_solo_lp: None,
            last_rank_flex_tier: None,
            last_rank_flex_rank: None,
            last_rank_flex_lp: None,
        }
    }

    /// End-to-end wiring: a real-ish 5v5 must light up KP, damage share, the lane
    /// matchup and team kills purely from the participant list (no DTO changes).
    #[test]
    fn from_context_derives_team_stats() {
        let mut participants = vec![
            participant("me", "JUNGLE", "Viego", true, 16, 1, 4, 40_000, 303, 15_000),
            participant("a2", "TOP", "Garen", true, 5, 3, 10, 20_000, 200, 10_000),
            participant("a3", "MIDDLE", "Ahri", true, 12, 4, 8, 30_000, 250, 12_000),
            participant("a4", "BOTTOM", "Jinx", true, 8, 2, 12, 35_000, 280, 13_000),
            participant("a5", "UTILITY", "Thresh", true, 2, 5, 20, 8_000, 40, 8_000),
            participant(
                "e1", "JUNGLE", "Khazix", false, 4, 6, 3, 22_000, 210, 11_000,
            ),
            participant("e2", "TOP", "Darius", false, 6, 7, 4, 24_000, 220, 11_500),
            participant("e3", "MIDDLE", "Zed", false, 9, 8, 5, 28_000, 240, 12_500),
            participant(
                "e4", "BOTTOM", "Caitlyn", false, 10, 6, 6, 33_000, 270, 13_500,
            ),
            participant("e5", "UTILITY", "Lulu", false, 9, 5, 15, 9_000, 45, 8_500),
        ];
        // Enrich "me" (participants[0]) with the Phase 3 data set so spells,
        // runes, vision breakdown, multi-kills and champ level light up too.
        {
            let me = &mut participants[0];
            me.champ_level = 16;
            me.summoner1_id = 4; // Flash (in the stub)
            me.summoner2_id = 11; // Smite
            me.perks = crate::riot::PerksDto {
                styles: vec![
                    crate::riot::PerkStyleDto {
                        style: 8000, // Precision (keystone tree)
                        selections: vec![crate::riot::PerkSelectionDto { perk: 8010 }],
                    },
                    crate::riot::PerkStyleDto {
                        style: 8100, // Domination (secondary tree)
                        selections: vec![],
                    },
                ],
            };
            me.wards_placed = 12;
            me.wards_killed = 7;
            me.detector_wards_placed = 4;
            me.triple_kills = 1;
            me.double_kills = 2;
        }

        // Team kills = 16+5+12+8+2 = 43; opp = 4+6+9+10+9 = 38.
        let info = InfoDto {
            game_duration: 1955, // 32:35
            game_version: "16.14.632.8043".into(),
            game_ended_in_early_surrender: false,
            participants,
            queue_id: 420, // ranked solo
            teams: vec![team_dto(100, 3, 1, 8), team_dto(200, 2, 0, 4)],
        };
        let new_rank = RankInfo {
            tier: "EMERALD".into(),
            rank: "IV".into(),
            lp: 63,
            wins: Some(32),
            losses: Some(23),
        };
        let old_rank = RankInfo {
            tier: "EMERALD".into(),
            rank: "IV".into(),
            lp: 44,
            ..Default::default()
        };
        let player = player();
        let ctx = MatchImageContext {
            player: &player,
            participant: &info.participants[0],
            match_info: &info,
            old_rank: Some(&old_rank),
            new_rank: Some(&new_rank),
            streak: vec![true, false, true, true, true],
            mastery: None,
        };

        let vm = ScorecardVm::from_context(&ctx, &DdragonData::stub("16.14.1"));

        assert_eq!(vm.result, MatchResult::Victory);
        // KP = (16+4)/43 = 46.5% → 47; sub "20/43".
        assert_eq!(vm.kp, Some(47));
        assert_eq!(vm.kp_sub.as_deref(), Some("20/43"));
        // Team damage = 40+20+30+35+8 = 133k → share 40k/133k ≈ 30%.
        assert_eq!(vm.damage_share, Some(30));
        // Ranked block populated with the derived tier + LP delta + win-rate.
        let rank = vm.rank.as_ref().expect("ranked block");
        assert_eq!(rank.tier, "Emerald IV");
        assert_eq!(rank.delta.as_deref(), Some("+19"));
        assert_eq!(rank.progress, Some(63));
        // 32 / (32 + 23) = 58.18% → 58.
        assert_eq!(rank.winrate, Some(58));
        // Streak bar flows straight through from the context (Phase 4).
        assert_eq!(rank.streak, vec![true, false, true, true, true]);
        // Lane opponent is the enemy jungler.
        let matchup = vm.matchup.as_ref().expect("matchup");
        assert_eq!(matchup.label, "Matchup");
        assert_eq!(matchup.opp_name, "Khazix");
        assert_eq!(matchup.opp_kda, "4/6/3");
        // Team panel kills + objectives from `teams[]`.
        let team = vm.team.as_ref().expect("team");
        assert_eq!(team.kills, 43);
        assert_eq!(team.opp_kills, 38);
        let counts: Vec<i32> = team.objectives.iter().map(|o| o.count).collect();
        assert_eq!(counts, vec![3, 1, 8]); // Drakes / Nashor / Towers

        // Extended stats lit up from the enriched participant.
        assert_eq!(vm.champion_level, Some(16));
        assert_eq!(vm.loadout.spell_srcs.len(), 2);
        assert!(vm.loadout.keystone_src.is_some());
        assert!(vm.loadout.secondary_src.is_some());
        assert_eq!(vm.vision.sub.as_deref(), Some("12·7·4"));
        assert_eq!(vm.multi.as_deref(), Some("1 Triple"));
        assert_eq!(vm.multi_sub.as_deref(), Some("+2 double"));
        // Grade (Phase 4) now scores this carry jungle game: KDA/CS/damage are
        // capped-high but a 47% KP and low vision keep it just under S+ → S.
        assert_eq!(vm.grade.as_deref(), Some("S"));

        // And the whole thing renders without panicking. Dump it next to the
        // hand-built samples so a `from_context` card can be screenshot-reviewed
        // (real DDragon asset URLs, resolved live by Chromium).
        let html = vm.build_html();
        assert!(html.contains("Matchup"));
        let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/target/scorecard-samples");
        std::fs::create_dir_all(out_dir).unwrap();
        std::fs::write(format!("{out_dir}/from_context.html"), &html).unwrap();
    }

    /// A remake must not trust win-grouping (both teams can share `win`), so the
    /// team-derived values stay off.
    #[test]
    fn from_context_remake_skips_team_stats() {
        let participants: Vec<ParticipantDto> = (0..10)
            .map(|i| {
                participant(
                    &format!("p{i}"),
                    "JUNGLE",
                    "Viego",
                    false, // everyone "loses" a remake
                    0,
                    0,
                    0,
                    100,
                    5,
                    500,
                )
            })
            .collect();
        let info = InfoDto {
            game_duration: 180,
            game_version: "16.14.632.8043".into(),
            game_ended_in_early_surrender: true,
            participants,
            queue_id: 420,
            teams: Vec::new(),
        };
        let player = player();
        let ctx = MatchImageContext {
            player: &player,
            participant: &info.participants[0],
            match_info: &info,
            old_rank: None,
            new_rank: None,
            streak: Vec::new(),
            mastery: None,
        };
        let vm = ScorecardVm::from_context(&ctx, &DdragonData::stub("16.14.1"));
        assert_eq!(vm.result, MatchResult::Remake);
        assert!(vm.kp.is_none());
        assert!(vm.damage_share.is_none());
        assert!(vm.matchup.is_none());
        assert!(vm.team.is_none());
        // Grade rides on team context, so a remake has none.
        assert!(vm.grade.is_none());
        // Still renders.
        let _ = vm.build_html();
    }

    /// Non-ranked queues swap the rank block for champion mastery: the mastery
    /// data must flow through and render, with no rank block, while grade still
    /// scores (it's a real 5v5, just unranked).
    #[test]
    fn from_context_shows_mastery_for_non_ranked() {
        let participants = vec![
            participant("me", "MIDDLE", "Ahri", true, 10, 3, 8, 30_000, 240, 12_000),
            participant("a2", "TOP", "Garen", true, 5, 3, 10, 20_000, 200, 10_000),
            participant("a3", "JUNGLE", "LeeSin", true, 6, 4, 9, 18_000, 160, 10_500),
            participant("a4", "BOTTOM", "Jinx", true, 8, 2, 12, 35_000, 280, 13_000),
            participant("a5", "UTILITY", "Thresh", true, 2, 5, 20, 8_000, 40, 8_000),
            participant("e1", "MIDDLE", "Zed", false, 7, 8, 5, 28_000, 240, 12_500),
            participant("e2", "TOP", "Darius", false, 6, 7, 4, 24_000, 220, 11_500),
            participant("e3", "JUNGLE", "Elise", false, 5, 7, 8, 15_000, 140, 9_800),
            participant(
                "e4", "BOTTOM", "Caitlyn", false, 10, 6, 6, 33_000, 270, 13_500,
            ),
            participant("e5", "UTILITY", "Lulu", false, 3, 6, 14, 9_000, 45, 8_500),
        ];
        let info = InfoDto {
            game_duration: 1600,
            game_version: "16.14.632.8043".into(),
            game_ended_in_early_surrender: false,
            participants,
            queue_id: 430, // Normal Blind — not ranked
            teams: vec![team_dto(100, 2, 1, 6), team_dto(200, 1, 0, 3)],
        };
        let player = player();
        let ctx = MatchImageContext {
            player: &player,
            participant: &info.participants[0],
            match_info: &info,
            old_rank: None,
            new_rank: None,
            streak: Vec::new(),
            mastery: Some(MasteryInfo {
                level: 7,
                points: 142_384,
            }),
        };
        let vm = ScorecardVm::from_context(&ctx, &DdragonData::stub("16.14.1"));

        // Non-ranked: rank block absent, mastery shown in its place.
        assert!(vm.rank.is_none());
        let mastery = vm.mastery.as_ref().expect("mastery block");
        assert_eq!(mastery.level, "7");
        assert_eq!(mastery.points, "142.4k");
        // A real 5v5 still yields a grade (mid carry game here).
        assert!(vm.grade.is_some());

        // The mastery footer actually renders (build_html prefers it when the
        // rank block is absent).
        let html = vm.build_html();
        assert!(html.contains("Mastery 7"));
        assert!(html.contains("142.4k pts"));

        // Dump next to the ranked sample for screenshot review of the mastery
        // footer (real DDragon asset URLs, resolved live by Chromium).
        let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/target/scorecard-samples");
        std::fs::create_dir_all(out_dir).unwrap();
        std::fs::write(format!("{out_dir}/from_context_mastery.html"), &html).unwrap();
    }

    /// ARAM (queue 450) has no lanes: Riot returns empty positions, so the lane
    /// matchup and the role chip must both drop out — while the rest of the 5v5
    /// derivations (KP, team panel, grade) and the mastery footer still render.
    #[test]
    fn from_context_aram_hides_matchup_and_role() {
        let participants = vec![
            participant("me", "", "Ahri", true, 10, 3, 8, 30_000, 240, 12_000),
            participant("a2", "", "Garen", true, 5, 3, 10, 20_000, 200, 10_000),
            participant("a3", "", "LeeSin", true, 6, 4, 9, 18_000, 160, 10_500),
            participant("a4", "", "Jinx", true, 8, 2, 12, 35_000, 280, 13_000),
            participant("a5", "", "Thresh", true, 2, 5, 20, 8_000, 40, 8_000),
            participant("e1", "", "Zed", false, 7, 8, 5, 28_000, 240, 12_500),
            participant("e2", "", "Darius", false, 6, 7, 4, 24_000, 220, 11_500),
            participant("e3", "", "Elise", false, 5, 7, 8, 15_000, 140, 9_800),
            participant("e4", "", "Caitlyn", false, 10, 6, 6, 33_000, 270, 13_500),
            participant("e5", "", "Lulu", false, 3, 6, 14, 9_000, 45, 8_500),
        ];
        let info = InfoDto {
            game_duration: 1100,
            game_version: "16.14.632.8043".into(),
            game_ended_in_early_surrender: false,
            participants,
            queue_id: 450, // ARAM — unranked, no lanes
            teams: vec![team_dto(100, 0, 0, 4), team_dto(200, 0, 0, 2)],
        };
        let player = player();
        let ctx = MatchImageContext {
            player: &player,
            participant: &info.participants[0],
            match_info: &info,
            old_rank: None,
            new_rank: None,
            streak: Vec::new(),
            mastery: Some(MasteryInfo {
                level: 5,
                points: 88_000,
            }),
        };
        let vm = ScorecardVm::from_context(&ctx, &DdragonData::stub("16.14.1"));

        // No lanes → no matchup and no role chip …
        assert!(vm.matchup.is_none());
        assert!(vm.role_upper.is_empty());
        // … but it is still a genuine 5v5, so the team stats and grade stand,
        // and an unranked queue shows the mastery footer instead of a rank.
        assert!(vm.rank.is_none());
        assert!(vm.team.is_some());
        assert!(vm.kp.is_some());
        assert!(vm.grade.is_some());
        assert!(vm.mastery.is_some());

        let html = vm.build_html();
        assert!(
            !html.contains("Matchup"),
            "matchup panel leaked into an ARAM card"
        );
        assert!(html.contains("Mastery"));
    }

    /// The Google Fonts request stays lean for Latin/Japanese names and only pulls
    /// the heavy Korean/Chinese Noto subsets in when the name actually needs them.
    #[test]
    fn font_url_adds_cjk_subsets_on_demand() {
        let mut vm = sample(MatchResult::Victory);

        vm.player_name = "Faker".into();
        let latin = vm.font_css_url();
        assert!(latin.contains("Noto+Sans+JP"));
        assert!(!latin.contains("Noto+Sans+KR"));
        assert!(!latin.contains("Noto+Sans+SC"));

        vm.player_name = "페이커".into(); // Hangul
        assert!(vm.font_css_url().contains("Noto+Sans+KR"));

        vm.player_name = "李相赫".into(); // Han
        assert!(vm.font_css_url().contains("Noto+Sans+SC"));

        assert!(contains_hangul("한글") && !contains_hangul("kana かな"));
        assert!(contains_han("漢字") && !contains_han("plain"));
    }

    /// Golden-file snapshot of the full rendered HTML for each theme. Regenerate
    /// after an intentional markup change with `UPDATE_SNAPSHOTS=1 cargo test`.
    #[test]
    fn html_matches_golden_snapshot() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/discord/snapshots");
        for (result, name) in [
            (MatchResult::Victory, "victory"),
            (MatchResult::Defeat, "defeat"),
            (MatchResult::Remake, "remake"),
        ] {
            let html = sample(result).build_html();
            let path = format!("{dir}/scorecard_{name}.html");

            if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
                std::fs::create_dir_all(dir).unwrap();
                std::fs::write(&path, &html).unwrap();
            }

            let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!("snapshot {name} unreadable ({e}); regenerate with UPDATE_SNAPSHOTS=1")
            });
            assert_eq!(
                html, expected,
                "HTML snapshot drift for {name}; re-run with UPDATE_SNAPSHOTS=1 if intended"
            );
        }
    }
}
