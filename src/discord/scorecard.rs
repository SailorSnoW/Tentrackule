use std::collections::HashMap;
use std::fmt::{self, Write as _};
use std::sync::Arc;

use super::ddragon::DdragonData;
use super::fonts::LATIN_FONT_CSS_URL;
use super::grade;
use super::image_gen::MatchImageContext;
use crate::db::RankInfo;
use crate::riot::{InfoDto, ParticipantDto, TeamDto};

pub const CARD_WIDTH_PX: u32 = 820;
pub const FONTS_READY_SELECTOR: &str = "html[data-fonts-ready]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchResult {
    Victory,
    Defeat,
    Remake,
}

impl MatchResult {
    fn result_label(self) -> &'static str {
        match self {
            MatchResult::Victory => "VICTORY",
            MatchResult::Defeat => "DEFEAT",
            MatchResult::Remake => "REMAKE",
        }
    }

    fn theme_vars(self) -> &'static str {
        match self {
            MatchResult::Victory => include_str!("themes/victory.css"),
            MatchResult::Defeat => include_str!("themes/defeat.css"),
            MatchResult::Remake => include_str!("themes/remake.css"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Kda {
    pub kills: i32,
    pub deaths: i32,
    pub assists: i32,
}

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

#[derive(Debug, Clone)]
pub struct RankView {
    pub tier: String,
    pub emblem_src: Option<String>,
    pub lp: i32,
    pub delta: Option<String>,
    pub progress: Option<u8>,
    pub winrate: Option<u8>,
    pub wins: Option<i32>,
    pub losses: Option<i32>,
    pub streak: Vec<bool>,
}

#[derive(Debug, Clone)]
pub struct MasteryView {
    pub level: String,
    pub points: String,
}

#[derive(Debug, Clone, Default)]
pub struct Loadout {
    pub spell_srcs: Vec<String>,
    pub keystone_src: Option<String>,
    pub secondary_src: Option<String>,
    pub items: Vec<Option<String>>,
    pub trinket_src: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MatchupView {
    pub label: String,
    pub opp_icon_src: String,
    pub opp_name: String,
    pub opp_kda: String,
    pub rows: Vec<MatchupRow>,
}

#[derive(Debug, Clone)]
pub struct MatchupRow {
    pub key: String,
    pub you: String,
    pub opp: String,
    pub you_pct: u8,
}

#[derive(Debug, Clone)]
pub struct Objective {
    pub icon_src: String,
    pub count: i32,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct TeamView {
    pub kills: i32,
    pub opp_kills: i32,
    pub objectives: Vec<Objective>,
}

#[derive(Debug, Clone)]
pub struct ScorecardVm {
    pub result: MatchResult,

    pub champion_splash_src: String,
    pub grade: Option<String>,
    pub player_name: String,
    pub player_tag: String,
    pub role_upper: String,
    pub role_icon_src: Option<String>,
    pub champion_name: String,
    pub champion_level: Option<i32>,
    pub rank: Option<RankView>,
    pub mastery: Option<MasteryView>,
    pub queue_label: String,

    pub meta_line: String,
    pub kda: Kda,
    pub kda_ratio: String,
    pub kp: Option<StatValue>,
    pub multi: Option<StatValue>,
    pub cs: StatValue,
    pub gold: StatValue,
    pub damage_value: String,
    pub damage_share: Option<u8>,
    pub vision: StatValue,
    pub loadout: Loadout,
    pub matchup: Option<MatchupView>,
    pub team: Option<TeamView>,

    inlined_font_css: Option<Arc<str>>,
}

impl ScorecardVm {
    pub fn set_inlined_font_css(&mut self, css: Arc<str>) {
        self.inlined_font_css = Some(css);
    }

    fn show_multi_chip(&self) -> bool {
        self.multi.is_some() && self.result != MatchResult::Remake
    }

    fn has_bottom(&self) -> bool {
        self.matchup.is_some() || self.team.is_some()
    }

    fn image_srcs_mut(&mut self) -> Vec<&mut String> {
        let l = &mut self.loadout;
        let mut srcs = vec![&mut self.champion_splash_src];
        srcs.extend(self.role_icon_src.as_mut());
        srcs.extend(l.spell_srcs.iter_mut());
        srcs.extend(l.keystone_src.as_mut());
        srcs.extend(l.secondary_src.as_mut());
        srcs.extend(l.items.iter_mut().flatten());
        srcs.extend(l.trinket_src.as_mut());
        srcs.extend(self.rank.as_mut().and_then(|r| r.emblem_src.as_mut()));
        srcs.extend(self.matchup.as_mut().map(|m| &mut m.opp_icon_src));
        srcs.extend(
            self.team
                .as_mut()
                .into_iter()
                .flat_map(|t| t.objectives.iter_mut().map(|o| &mut o.icon_src)),
        );
        srcs
    }

    pub fn image_urls(&mut self) -> Vec<String> {
        self.image_srcs_mut()
            .into_iter()
            .map(|s| s.clone())
            .collect()
    }

    pub fn apply_asset_map(&mut self, map: &HashMap<String, String>) {
        for src in self.image_srcs_mut() {
            if let Some(uri) = map.get(src.as_str()) {
                *src = uri.clone();
            }
        }
    }
}

const DDRAGON: &str = super::ddragon::DDRAGON_CDN;

const CDRAGON: &str = "https://raw.communitydragon.org/latest";

impl ScorecardVm {
    pub fn from_context(ctx: &MatchImageContext<'_>, ddragon: &DdragonData) -> Self {
        let p = ctx.participant;
        let info = ctx.match_info;
        let version = ddragon.version();
        let champion_key = ddragon.champion_key(&p.champion_name);

        let result = if info.game_ended_in_early_surrender {
            MatchResult::Remake
        } else if p.win {
            MatchResult::Victory
        } else {
            MatchResult::Defeat
        };

        let team = TeamDerived::compute(info, p, ddragon);

        let role_upper = p.position_display().to_uppercase();

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
                    streak: ctx.streak.clone(),
                }
            })
        } else {
            None
        };

        let queue_label = info.queue_name();
        let meta_line = [
            queue_label.to_string(),
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

        let minutes = info.game_minutes();
        let per_minute = |total: f64| (minutes > 0.0).then(|| total / minutes);

        let gold_sub =
            per_minute(p.gold_earned as f64).map(|rate| format!("{}/m", rate.round() as i64));

        let multi = p
            .multi_kill()
            .map(|(label, sub)| StatValue::new(label, sub));

        let vision_sub = (p.wards_placed + p.wards_killed + p.detector_wards_placed > 0)
            .then(|| p.vision_breakdown());

        let grade = team.as_ref().map(|t| {
            grade::evaluate(&grade::GradeInput {
                role: &p.team_position,
                kda: p.kda_ratio(),
                kp: t.kp,
                cs_per_min: p.cs_per_minute(info.game_duration),
                damage_share: t.damage_share,
                vision_per_min: per_minute(p.vision_score as f64).unwrap_or(0.0),
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
            mastery: ctx.mastery.map(|m| MasteryView {
                level: m.level.to_string(),
                points: format_short(m.points as i64),
            }),
            queue_label: queue_label.to_string(),
            meta_line,
            kda: Kda {
                kills: p.kills,
                deaths: p.deaths,
                assists: p.assists,
            },
            kda_ratio: format!("{:.2}", p.kda_ratio()),
            kp: team
                .as_ref()
                .map(|t| StatValue::new(format!("{}%", t.kp), Some(t.kp_sub.clone()))),
            multi,
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
            inlined_font_css: None,
        }
    }
}

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

impl ScorecardVm {
    pub fn build_html(&self) -> String {
        let mut s = String::with_capacity(8 * 1024);
        self.write_html(&mut s)
            .expect("writing to a String is infallible");
        s
    }

    fn write_html(&self, s: &mut String) -> fmt::Result {
        s.push_str("<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n");
        self.write_font_head(s)?;
        s.push_str("<style>\n");
        s.push_str(include_str!("themes/card.css"));
        s.push_str(
            "</style>\n\
<script>addEventListener('error',function(e){var t=e.target;if(t&&t.tagName==='IMG'){t.style.visibility='hidden';}},true);addEventListener('load',function(){document.fonts.ready.then(function(){document.documentElement.setAttribute('data-fonts-ready','1');});});</script>\n\
</head>\n<body>\n",
        );

        writeln!(
            s,
            "<div id=\"card-root\" style=\"{}\">",
            self.result.theme_vars()
        )?;
        s.push_str("  <div class=\"card\">\n");

        self.write_sidebar(s)?;
        self.write_main(s)?;

        s.push_str("  </div>\n");
        s.push_str("</div>\n</body>\n</html>\n");
        Ok(())
    }

    fn write_font_head(&self, s: &mut String) -> fmt::Result {
        match &self.inlined_font_css {
            Some(css) => writeln!(s, "<style>{css}</style>"),
            None => {
                s.push_str(
                    "<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">\n\
<link rel=\"preconnect\" href=\"https://fonts.gstatic.com\" crossorigin>\n",
                );
                writeln!(s, "<link href=\"{LATIN_FONT_CSS_URL}\" rel=\"stylesheet\">")
            }
        }
    }

    fn write_sidebar(&self, s: &mut String) -> fmt::Result {
        s.push_str("    <div class=\"side\">\n");
        writeln!(
            s,
            "      <img class=\"splash\" src=\"{}\">",
            Esc(&self.champion_splash_src)
        )?;
        s.push_str("      <div class=\"veil\"></div>\n");

        s.push_str("      <div class=\"side-top\">\n");
        writeln!(
            s,
            "        <span class=\"ribbon\">{}</span>",
            self.result.result_label()
        )?;
        if let Some(grade) = &self.grade {
            s.push_str("        <div class=\"grade\">\n");
            s.push_str("          <div class=\"grade-gem\"></div>\n");
            writeln!(
                s,
                "          <span class=\"grade-txt\">{}</span>",
                Esc(grade)
            )?;
            s.push_str("        </div>\n");
        }
        s.push_str("      </div>\n");

        s.push_str("      <div class=\"side-btm\">\n");
        writeln!(
            s,
            "        <div class=\"name\">{}<span class=\"tag\">{}</span></div>",
            Esc(&self.player_name),
            Esc(&self.player_tag)
        )?;

        s.push_str("        <div class=\"chips\">\n");
        if !self.role_upper.is_empty() {
            s.push_str("          <span class=\"chip-role\">");
            if let Some(src) = &self.role_icon_src {
                write!(s, "<img class=\"role-ico\" src=\"{}\">", Esc(src))?;
            }
            writeln!(s, "{}</span>", Esc(&self.role_upper))?;
        }
        write!(
            s,
            "          <span class=\"chip-champ\">{}",
            Esc(&self.champion_name)
        )?;
        if let Some(level) = self.champion_level {
            write!(s, " · Lvl {level}")?;
        }
        s.push_str("</span>\n        </div>\n");

        s.push_str("        <div class=\"rule\"></div>\n");

        match &self.rank {
            Some(rank) => write_ranked_block(s, rank)?,
            None => self.write_mastery_block(s)?,
        }

        s.push_str("      </div>\n");
        s.push_str("    </div>\n");
        Ok(())
    }

    fn write_mastery_block(&self, s: &mut String) -> fmt::Result {
        s.push_str("        <div class=\"rank-row\">\n");
        if let Some(mastery) = &self.mastery {
            s.push_str("          <div class=\"mastery-gem\"></div>\n");
            s.push_str("          <div>\n");
            writeln!(
                s,
                "            <div class=\"mastery-lvl\">Mastery {}</div>",
                Esc(&mastery.level)
            )?;
            writeln!(
                s,
                "            <div class=\"mastery-pts\">{} pts</div>",
                Esc(&mastery.points)
            )?;
            s.push_str("          </div>\n");
        }
        writeln!(
            s,
            "          <span class=\"chip-queue\">{}</span>",
            Esc(&self.queue_label)
        )?;
        s.push_str("        </div>\n");
        Ok(())
    }

    fn write_main(&self, s: &mut String) -> fmt::Result {
        s.push_str("    <div class=\"main\">\n");

        s.push_str("      <div class=\"head\">\n");
        s.push_str("        <span class=\"head-l\">Match Summary</span>\n");
        writeln!(
            s,
            "        <span class=\"head-r\">{}</span>",
            Esc(&self.meta_line)
        )?;
        s.push_str("      </div>\n");

        s.push_str("      <div class=\"kda-row\">\n");
        writeln!(
            s,
            "        <div class=\"kda\">{kills}<span class=\"slash\">/</span><span class=\"death\">{deaths}</span><span class=\"slash\">/</span>{assists}</div>",
            kills = self.kda.kills,
            deaths = self.kda.deaths,
            assists = self.kda.assists
        )?;
        s.push_str("        <div class=\"kda-side\">\n");
        writeln!(
            s,
            "          <span class=\"ratio\">{} KDA</span>",
            Esc(&self.kda_ratio)
        )?;
        s.push_str("          <div class=\"kda-chips\">\n");
        if let Some(kp) = &self.kp {
            writeln!(
                s,
                "            <span class=\"chip-kp\">{} KP</span>",
                Esc(&kp.value)
            )?;
        }
        if self.show_multi_chip()
            && let Some(multi) = &self.multi
        {
            writeln!(
                s,
                "            <span class=\"chip-multi\">{}</span>",
                Esc(&multi.value)
            )?;
        }
        s.push_str("          </div>\n        </div>\n      </div>\n");

        self.write_stats_grid(s)?;
        self.write_loadout(s)?;
        if self.has_bottom() {
            self.write_bottom(s)?;
        }

        s.push_str("    </div>\n");
        Ok(())
    }

    fn write_stats_grid(&self, s: &mut String) -> fmt::Result {
        s.push_str("      <div class=\"grid\">\n");
        write_stat_panel(s, "CS", Some(&self.cs), "val")?;
        write_stat_panel(s, "Gold", Some(&self.gold), "val")?;
        self.write_damage_panel(s)?;
        write_stat_panel(s, "Vision", Some(&self.vision), "val")?;
        write_stat_panel(s, "KP", self.kp.as_ref(), "val")?;
        write_stat_panel(s, "Multi-kills", self.multi.as_ref(), "val-accent")?;
        s.push_str("      </div>\n");
        Ok(())
    }

    fn write_damage_panel(&self, s: &mut String) -> fmt::Result {
        s.push_str(PANEL_OPEN);
        s.push_str("<div class=\"lbl\">Damage</div>");
        write!(s, "<div class=\"val\">{}</div>", Esc(&self.damage_value))?;
        if let Some(share) = self.damage_share {
            write!(
                s,
                "<div class=\"bar\"><div class=\"bar-fill\" style=\"width:{share}%\"></div></div><div class=\"bar-lbl\">{share}% team</div>"
            )?;
        }
        s.push_str("</div>\n");
        Ok(())
    }

    fn write_loadout(&self, s: &mut String) -> fmt::Result {
        let l = &self.loadout;
        s.push_str("      <div class=\"panel loadout\">\n");

        let mut need_divider = false;

        let has_spells_runes =
            !l.spell_srcs.is_empty() || l.keystone_src.is_some() || l.secondary_src.is_some();
        if has_spells_runes {
            for src in &l.spell_srcs {
                writeln!(s, "        <img class=\"slot\" src=\"{}\">", Esc(src))?;
            }
            if let Some(src) = &l.keystone_src {
                writeln!(
                    s,
                    "        <div class=\"rune\"><img class=\"rune-ico\" src=\"{}\"></div>",
                    Esc(src)
                )?;
            }
            if let Some(src) = &l.secondary_src {
                writeln!(
                    s,
                    "        <div class=\"rune-2\"><img class=\"rune-ico-2\" src=\"{}\"></div>",
                    Esc(src)
                )?;
            }
            need_divider = true;
        }

        if !l.items.is_empty() {
            if need_divider {
                s.push_str(LOADOUT_DIVIDER);
            }
            for slot in &l.items {
                match slot {
                    Some(src) => writeln!(s, "        <img class=\"slot\" src=\"{}\">", Esc(src))?,
                    None => s.push_str("        <div class=\"slot-empty\"></div>\n"),
                }
            }
            need_divider = true;
        }

        if let Some(src) = &l.trinket_src {
            if need_divider {
                s.push_str(LOADOUT_DIVIDER);
            }
            writeln!(s, "        <img class=\"trinket\" src=\"{}\">", Esc(src))?;
        }

        s.push_str("      </div>\n");
        Ok(())
    }

    fn write_bottom(&self, s: &mut String) -> fmt::Result {
        s.push_str("      <div class=\"bottom\">\n");
        if let Some(matchup) = &self.matchup {
            self.write_matchup_panel(s, matchup)?;
        }
        if let Some(team) = &self.team {
            write_team_panel(s, team)?;
        }
        s.push_str("      </div>\n");
        Ok(())
    }

    fn write_matchup_panel(&self, s: &mut String, m: &MatchupView) -> fmt::Result {
        s.push_str("        <div class=\"panel matchup\">\n");
        s.push_str("          <div class=\"matchup-head\">\n");
        s.push_str("            <span class=\"lbl matchup-lbl\">");
        if let Some(src) = &self.role_icon_src {
            write!(s, "<img class=\"role-ico-sm\" src=\"{}\">", Esc(src))?;
        }
        writeln!(s, "{}</span>", Esc(&m.label))?;
        writeln!(
            s,
            "            <span class=\"opp\"><img class=\"opp-ico\" src=\"{icon}\"><span class=\"opp-txt\">{name} {kda}</span></span>",
            icon = Esc(&m.opp_icon_src),
            name = Esc(&m.opp_name),
            kda = Esc(&m.opp_kda)
        )?;
        s.push_str("          </div>\n");

        for row in &m.rows {
            let opp_pct = 100u8.saturating_sub(row.you_pct);
            writeln!(
                s,
                "          <div class=\"row\"><span class=\"row-k\">{key}</span><span class=\"row-you\">{you}</span><div class=\"row-bar\"><div class=\"row-you-f\" style=\"flex:{you_pct}\"></div><div class=\"row-opp-f\" style=\"flex:{opp_pct}\"></div></div><span class=\"row-opp\">{opp}</span></div>",
                key = Esc(&row.key),
                you = Esc(&row.you),
                you_pct = row.you_pct,
                opp = Esc(&row.opp)
            )?;
        }

        s.push_str("        </div>\n");
        Ok(())
    }
}

const PANEL_OPEN: &str = "        <div class=\"panel stat\">";
const LOADOUT_DIVIDER: &str = "        <div class=\"divider\"></div>\n";

fn write_stat_panel(
    s: &mut String,
    label: &str,
    stat: Option<&StatValue>,
    value_class: &str,
) -> fmt::Result {
    write!(s, "{PANEL_OPEN}<div class=\"lbl\">{}</div>", Esc(label))?;
    write!(s, "<div class=\"{value_class}\">")?;
    match stat {
        Some(stat) => {
            write!(s, "{}", Esc(&stat.value))?;
            if let Some(sub) = &stat.sub {
                write!(s, " <span class=\"sub\">{}</span>", Esc(sub))?;
            }
        }
        None => s.push('—'),
    }
    s.push_str("</div></div>\n");
    Ok(())
}

fn write_ranked_block(s: &mut String, rank: &RankView) -> fmt::Result {
    s.push_str("        <div>\n");
    s.push_str("          <div class=\"rank-row\">\n");
    if let Some(src) = &rank.emblem_src {
        writeln!(s, "            <img class=\"emblem\" src=\"{}\">", Esc(src))?;
    }
    s.push_str("            <div>\n");
    writeln!(
        s,
        "              <div class=\"tier\">{}</div>",
        Esc(&rank.tier)
    )?;
    write!(s, "              <div class=\"lp\">{} LP", rank.lp)?;
    if let Some(delta) = &rank.delta {
        write!(s, " <span class=\"delta\">({})</span>", Esc(delta))?;
    }
    s.push_str("</div>\n            </div>\n");

    if let (Some(wr), Some(wins), Some(losses)) = (rank.winrate, rank.wins, rank.losses) {
        s.push_str("            <div class=\"wr\">\n");
        writeln!(
            s,
            "              <div class=\"wr-pct\">{wr}% <span class=\"wr-lbl\">WR</span></div>"
        )?;
        writeln!(
            s,
            "              <div class=\"wl\">{wins}W · {losses}L</div>"
        )?;
        s.push_str("            </div>\n");
    }
    s.push_str("          </div>\n");

    if let Some(progress) = rank.progress {
        writeln!(
            s,
            "          <div class=\"track\"><div class=\"track-fill\" style=\"width:{progress}%\"></div></div>"
        )?;
    }

    if !rank.streak.is_empty() {
        s.push_str("          <div class=\"streak\">");
        for win in &rank.streak {
            let color = if *win { "#1e9e6a" } else { "#c0455f" };
            write!(
                s,
                "<span class=\"streak-cell\" style=\"background:{color}\"></span>"
            )?;
        }
        s.push_str("</div>\n");
    }

    s.push_str("        </div>\n");
    Ok(())
}

fn write_team_panel(s: &mut String, t: &TeamView) -> fmt::Result {
    s.push_str("        <div class=\"panel team\">\n");
    s.push_str("          <div class=\"lbl team-lbl\">Team · Objectives</div>\n");
    writeln!(
        s,
        "          <div class=\"team-kills\"><span class=\"team-k\">{}</span><span class=\"team-k-lbl\">kills · opp. {}</span></div>",
        t.kills, t.opp_kills
    )?;

    if !t.objectives.is_empty() {
        s.push_str("          <div class=\"objs\">\n");
        for ob in &t.objectives {
            writeln!(
                s,
                "            <div class=\"obj\"><img class=\"obj-ico\" src=\"{icon}\"><span class=\"obj-n\">{count}</span><span class=\"obj-lbl\">{label}</span></div>",
                icon = Esc(&ob.icon_src),
                count = ob.count,
                label = Esc(&ob.label)
            )?;
        }
        s.push_str("          </div>\n");
    }

    s.push_str("        </div>\n");
    Ok(())
}

struct Esc<'a>(&'a str);

impl fmt::Display for Esc<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut rest = self.0;
        while let Some(i) = rest.find(['&', '<', '>', '"', '\'']) {
            f.write_str(&rest[..i])?;
            f.write_str(match rest.as_bytes()[i] {
                b'&' => "&amp;",
                b'<' => "&lt;",
                b'>' => "&gt;",
                b'"' => "&quot;",
                _ => "&#39;",
            })?;
            rest = &rest[i + 1..];
        }
        f.write_str(rest)
    }
}

fn item_src(version: &str, id: i32) -> String {
    format!("{DDRAGON}/{version}/img/item/{id}.png")
}

fn champion_square_src(version: &str, key: &str) -> String {
    format!("{DDRAGON}/{version}/img/champion/{key}.png")
}

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

fn objective_icon_src(name: &str) -> String {
    format!("{CDRAGON}/game/assets/ux/minimap/icons/{name}.png")
}

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

fn percent(part: i64, whole: i64) -> u8 {
    if whole <= 0 {
        0
    } else {
        ((part as f64 / whole as f64) * 100.0)
            .round()
            .clamp(0.0, 100.0) as u8
    }
}

fn share(you: i64, opp: i64) -> u8 {
    let total = you + opp;
    if total <= 0 { 50 } else { percent(you, total) }
}

fn win_rate(wins: Option<i32>, losses: Option<i32>) -> Option<u8> {
    let (w, l) = (wins?, losses?);
    (w + l > 0).then(|| percent(w as i64, (w + l) as i64))
}

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
mod tests;
