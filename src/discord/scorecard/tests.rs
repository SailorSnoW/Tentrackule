use super::*;
use crate::db::Player;
use crate::discord::ddragon::DdragonData;
use crate::discord::image_gen::{MasteryInfo, MatchImageContext};
use crate::riot::{ObjectiveCountDto, ObjectivesDto};

const DDRAGON_VER: &str = "14.24.1";

fn stat(v: &Option<StatValue>) -> Option<(&str, Option<&str>)> {
    v.as_ref().map(|s| (s.value.as_str(), s.sub.as_deref()))
}

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
        kp: Some(StatValue::new("71%", Some("20/43".into()))),
        multi: Some(StatValue::new("1 Triple", Some("+2 double".into()))),
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
        inlined_font_css: None,
    }
}

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

        assert!(html.contains(label), "missing banner label for {name}");
        assert!(html.contains("Matchup"));
        assert!(html.contains("40.1k"));
        assert!(html.contains("Emerald IV"));
        assert_eq!(html.matches("id=\"card-root\"").count(), 1);
        assert!(html.contains("fonts.googleapis.com"));

        std::fs::write(format!("{out_dir}/{name}.html"), &html).unwrap();
    }
}

#[test]
fn inlines_assets_via_map() {
    let mut vm = sample(MatchResult::Victory);
    let urls = vm.image_urls();
    assert_eq!(urls.len(), 18);
    assert!(urls.contains(&vm.champion_splash_src));
    assert!(urls.iter().any(|u| u.contains("/img/item/")));

    let map: HashMap<String, String> = urls
        .iter()
        .enumerate()
        .map(|(i, u)| (u.clone(), format!("data:image/png;base64,inlined{i}")))
        .collect();
    vm.apply_asset_map(&map);
    let html = vm.build_html();

    assert!(!html.contains("ddragon.leagueoflegends.com"));
    assert!(html.contains("data:image/png;base64,inlined"));
}

#[test]
fn remake_hides_multi_kill_chip() {
    let mut vm = sample(MatchResult::Remake);
    vm.multi = Some(StatValue::new("1 Triple", None));
    let html = vm.build_html();
    assert!(
        !html.contains("class=\"chip-multi\""),
        "multi chip leaked on remake"
    );
}

#[test]
fn escapes_dynamic_fields() {
    let mut vm = sample(MatchResult::Victory);
    vm.player_name = "<script>&\"'".into();
    let html = vm.build_html();
    assert!(html.contains("&lt;script&gt;&amp;&quot;&#39;"));
    assert!(
        !html.contains("<script>&\"'"),
        "player name injected raw markup"
    );
}

#[test]
fn omits_blocks_when_data_is_absent() {
    let vm = ScorecardVm {
        result: MatchResult::Victory,
        champion_splash_src: format!("{DDRAGON}/img/champion/loading/Viego_0.jpg"),
        grade: None,
        player_name: "Solo".into(),
        player_tag: "#EUW".into(),
        role_upper: String::new(),
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
        multi: None,
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
        inlined_font_css: None,
    };
    let html = vm.build_html();

    assert!(!html.contains("class=\"grade-gem\""));
    assert!(!html.contains("class=\"mastery-gem\""));
    assert!(!html.contains("JUNGLE"));
    assert!(html.contains("—"));
    assert!(!html.contains("Matchup"));
    assert!(!html.contains("Team · Objectives"));
    assert!(html.contains("ARAM"));
    assert!(html.contains("class=\"slot-empty\""));
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
    {
        let me = &mut participants[0];
        me.champ_level = 16;
        me.summoner1_id = 4;
        me.summoner2_id = 11;
        me.perks = crate::riot::PerksDto {
            styles: vec![
                crate::riot::PerkStyleDto {
                    style: 8000,
                    selections: vec![crate::riot::PerkSelectionDto { perk: 8010 }],
                },
                crate::riot::PerkStyleDto {
                    style: 8100,
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

    let info = InfoDto {
        game_duration: 1955,
        game_version: "16.14.632.8043".into(),
        game_ended_in_early_surrender: false,
        participants,
        queue_id: 420,
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
    assert_eq!(stat(&vm.kp), Some(("47%", Some("20/43"))));
    assert_eq!(vm.damage_share, Some(30));
    let rank = vm.rank.as_ref().expect("ranked block");
    assert_eq!(rank.tier, "Emerald IV");
    assert_eq!(rank.delta.as_deref(), Some("+19"));
    assert_eq!(rank.progress, Some(63));
    assert_eq!(rank.winrate, Some(58));
    assert_eq!(rank.streak, vec![true, false, true, true, true]);
    let matchup = vm.matchup.as_ref().expect("matchup");
    assert_eq!(matchup.label, "Matchup");
    assert_eq!(matchup.opp_name, "Khazix");
    assert_eq!(matchup.opp_kda, "4/6/3");
    let team = vm.team.as_ref().expect("team");
    assert_eq!(team.kills, 43);
    assert_eq!(team.opp_kills, 38);
    let counts: Vec<i32> = team.objectives.iter().map(|o| o.count).collect();
    assert_eq!(counts, vec![3, 1, 8]);

    assert_eq!(vm.champion_level, Some(16));
    assert_eq!(vm.loadout.spell_srcs.len(), 2);
    assert!(vm.loadout.keystone_src.is_some());
    assert!(vm.loadout.secondary_src.is_some());
    assert_eq!(vm.vision.sub.as_deref(), Some("12·7·4"));
    assert_eq!(stat(&vm.multi), Some(("1 Triple", Some("+2 double"))));
    assert_eq!(vm.grade.as_deref(), Some("S"));

    let html = vm.build_html();
    assert!(html.contains("Matchup"));
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/target/scorecard-samples");
    std::fs::create_dir_all(out_dir).unwrap();
    std::fs::write(format!("{out_dir}/from_context.html"), &html).unwrap();
}

#[test]
fn from_context_remake_skips_team_stats() {
    let participants: Vec<ParticipantDto> = (0..10)
        .map(|i| {
            participant(
                &format!("p{i}"),
                "JUNGLE",
                "Viego",
                false,
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
    assert!(vm.grade.is_none());
    let _ = vm.build_html();
}

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
        queue_id: 430,
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

    assert!(vm.rank.is_none());
    let mastery = vm.mastery.as_ref().expect("mastery block");
    assert_eq!(mastery.level, "7");
    assert_eq!(mastery.points, "142.4k");
    assert!(vm.grade.is_some());

    let html = vm.build_html();
    assert!(html.contains("Mastery 7"));
    assert!(html.contains("142.4k pts"));

    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/target/scorecard-samples");
    std::fs::create_dir_all(out_dir).unwrap();
    std::fs::write(format!("{out_dir}/from_context_mastery.html"), &html).unwrap();
}

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
        queue_id: 450,
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

    assert!(vm.matchup.is_none());
    assert!(vm.role_upper.is_empty());
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

#[test]
fn card_names_rely_on_the_preinstalled_cjk_font() {
    let mut vm = sample(MatchResult::Victory);

    for name in ["Faker", "れいん", "페이커", "李相赫"] {
        vm.player_name = name.into();
        let html = vm.build_html();
        assert!(html.contains(name), "missing player name {name}");
        assert!(html.contains("'Noto Sans CJK JP'"));
        assert!(!html.contains("family=Noto"), "no CJK web font for {name}");
    }
}

#[test]
fn html_signals_fonts_ready_after_load() {
    let html = sample(MatchResult::Victory).build_html();
    assert_eq!(FONTS_READY_SELECTOR, "html[data-fonts-ready]");
    assert!(html.contains(
        "addEventListener('load',function(){document.fonts.ready.then(function(){\
         document.documentElement.setAttribute('data-fonts-ready','1');});});"
    ));
}

#[tokio::test]
#[ignore = "hits the real Cloudflare Browser Run API (needs CLOUDFLARE_ACCOUNT_ID + CLOUDFLARE_API_TOKEN)"]
async fn renders_real_cards_on_cloudflare() {
    dotenvy::dotenv().ok();
    let account_id = std::env::var("CLOUDFLARE_ACCOUNT_ID").expect("CLOUDFLARE_ACCOUNT_ID");
    let api_token = std::env::var("CLOUDFLARE_API_TOKEN").expect("CLOUDFLARE_API_TOKEN");
    let out_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/target/scorecard-samples");
    std::fs::create_dir_all(out_dir).unwrap();

    let metrics = Arc::new(crate::metrics::Metrics::new());
    let generator = crate::discord::ImageGenerator::new(
        Some(DDRAGON_VER.into()),
        Some(account_id.clone()),
        Some(api_token.clone()),
        Arc::clone(&metrics),
    )
    .await
    .expect("image generator");

    for (label, player_name) in [
        ("latin", "Faker"),
        ("latin_ext", "Żółć Çelik"),
        ("kana", "れいん"),
        ("hangul", "페이커"),
        ("han", "李相赫"),
        ("cyrillic", "Хайд"),
    ] {
        let mut vm = sample(MatchResult::Victory);
        vm.player_name = player_name.into();

        let started = std::time::Instant::now();
        let jpeg = generator
            .render_scorecard(vm)
            .await
            .unwrap_or_else(|e| panic!("{label} render failed: {e:?}"));

        assert_eq!(&jpeg[..3], [0xFF, 0xD8, 0xFF], "{label} is not a JPEG");
        std::fs::write(format!("{out_dir}/cloudflare_{label}.jpg"), &jpeg).unwrap();
        eprintln!(
            "{label}: {} KB in {:.1}s",
            jpeg.len() / 1024,
            started.elapsed().as_secs_f32()
        );
    }

    let renderer =
        crate::discord::renderer::Renderer::from_config(Some(account_id), Some(api_token))
            .expect("renderer");
    let started = std::time::Instant::now();
    let jpeg = renderer
        .render(
            &reqwest::Client::new(),
            &metrics,
            &sample(MatchResult::Defeat).build_html(),
            false,
        )
        .await
        .expect("font <link> fallback render failed");
    std::fs::write(format!("{out_dir}/cloudflare_link_fallback.jpg"), &jpeg).unwrap();
    eprintln!(
        "link_fallback: {} KB in {:.1}s",
        jpeg.len() / 1024,
        started.elapsed().as_secs_f32()
    );
}

#[test]
fn build_html_inlines_fonts_when_set() {
    let mut vm = sample(MatchResult::Victory);

    let linked = vm.build_html();
    assert!(linked.contains("fonts.googleapis.com"));
    assert!(!linked.contains("data:font/woff2"));

    vm.set_inlined_font_css(std::sync::Arc::from(
        "@font-face{font-family:'Cinzel';src:url(data:font/woff2;base64,AAAA) format('woff2')}",
    ));
    let inlined = vm.build_html();
    assert!(
        !inlined.contains("fonts.googleapis.com"),
        "Google Fonts <link> must be gone once fonts are inlined"
    );
    assert!(inlined.contains("data:font/woff2"));
}

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
