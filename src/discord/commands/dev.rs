use poise::ChoiceParameter;
use poise::serenity_prelude::CreateAttachment;

use crate::db::{Player, RankInfo};
use crate::discord::bot::Context;
use crate::discord::image_gen::{MasteryInfo, MatchImageContext};
use crate::error::AppError;
use crate::riot::{
    InfoDto, ObjectiveCountDto, ObjectivesDto, ParticipantDto, PerkSelectionDto, PerkStyleDto,
    PerksDto, TeamDto,
};

#[derive(Debug, Clone, Copy, ChoiceParameter)]
pub enum TestQueueType {
    #[name = "Normal Blind (430)"]
    NormalBlind,
    #[name = "Normal Draft (400)"]
    NormalDraft,
    #[name = "Quickplay (490)"]
    Quickplay,
    #[name = "Ranked Solo/Duo (420)"]
    RankedSolo,
    #[name = "Ranked Flex (440)"]
    RankedFlex,
    #[name = "ARAM (450)"]
    Aram,
}

impl TestQueueType {
    fn queue_id(self) -> i32 {
        match self {
            TestQueueType::NormalBlind => 430,
            TestQueueType::NormalDraft => 400,
            TestQueueType::Quickplay => 490,
            TestQueueType::RankedSolo => 420,
            TestQueueType::RankedFlex => 440,
            TestQueueType::Aram => 450,
        }
    }
}

/// [DEV] Send a test alert image
#[poise::command(slash_command, guild_only, owners_only, rename = "dev_test_alert")]
pub async fn dev_test_alert(
    ctx: Context<'_>,
    #[description = "Queue type to test"] queue_type: TestQueueType,
    #[description = "Simulate a win?"] win: Option<bool>,
    #[description = "Simulate a remake?"] remake: Option<bool>,
) -> Result<(), AppError> {
    ctx.defer().await?;

    let win = win.unwrap_or(true);
    let remake = remake.unwrap_or(false);
    let queue_id = queue_type.queue_id();

    // Fake player data
    let player = Player {
        id: 0,
        puuid: "test-puuid-12345".to_string(),
        game_name: "TestPlayer".to_string(),
        tag_line: "EUW".to_string(),
        region: "EUW1".to_string(),
        last_match_id: None,
        last_rank_solo_tier: Some("GOLD".to_string()),
        last_rank_solo_rank: Some("II".to_string()),
        last_rank_solo_lp: Some(45),
        last_rank_flex_tier: Some("SILVER".to_string()),
        last_rank_flex_rank: Some("I".to_string()),
        last_rank_flex_lp: Some(75),
    };

    // ARAM has no lanes, so blank the positions to exercise the "no matchup /
    // no role chip" variant; every other queue keeps its lane assignments.
    let pos = |p: &str| if queue_id == 450 { "" } else { p }.to_string();

    // Fake tracked participant (blue-side mid). Carries the full Phase 3 data set
    // so the dev card shows spells, runes, multi-kills, vision breakdown, etc.
    let participant = ParticipantDto {
        puuid: "test-puuid-12345".to_string(),
        team_position: pos("MIDDLE"),
        champion_name: "Ahri".to_string(),
        kills: 8,
        deaths: 3,
        assists: 12,
        total_damage_dealt_to_champions: 28500,
        total_minions_killed: 185,
        neutral_minions_killed: 12,
        vision_score: 42,
        gold_earned: 12450,
        win,
        // Example items: Luden's, Sorc Shoes, Shadowflame, Rabadon, Void Staff, Zhonya, ward
        item0: 6655,
        item1: 3020,
        item2: 4645,
        item3: 3089,
        item4: 3135,
        item5: 3157,
        item6: 3364,
        team_id: 100,
        champ_level: 16,
        double_kills: 2,
        triple_kills: 1,
        summoner1_id: 4,  // Flash
        summoner2_id: 14, // Ignite
        perks: PerksDto {
            styles: vec![
                // Domination / Electrocute keystone.
                PerkStyleDto {
                    style: 8100,
                    selections: vec![PerkSelectionDto { perk: 8112 }],
                },
                // Sorcery secondary tree.
                PerkStyleDto {
                    style: 8200,
                    selections: vec![],
                },
            ],
        },
        wards_placed: 14,
        wards_killed: 5,
        detector_wards_placed: 8,
        ..Default::default()
    };

    // Nine more players to make a full 5v5, so KP, damage share, the lane matchup
    // and the team/objectives panel all light up from "real" data.
    let opp_win = !win;
    let participants = vec![
        participant.clone(),
        demo_participant(
            "blue-top",
            100,
            &pos("TOP"),
            "Garen",
            win,
            5,
            4,
            7,
            18_000,
            210,
            11_000,
        ),
        demo_participant(
            "blue-jgl",
            100,
            &pos("JUNGLE"),
            "LeeSin",
            win,
            6,
            5,
            11,
            16_000,
            150,
            10_500,
        ),
        demo_participant(
            "blue-bot",
            100,
            &pos("BOTTOM"),
            "Jinx",
            win,
            11,
            3,
            9,
            32_000,
            240,
            13_500,
        ),
        demo_participant(
            "blue-sup",
            100,
            &pos("UTILITY"),
            "Thresh",
            win,
            2,
            6,
            21,
            9_000,
            35,
            8_500,
        ),
        demo_participant(
            "red-mid",
            200,
            &pos("MIDDLE"),
            "Zed",
            opp_win,
            7,
            8,
            5,
            24_000,
            195,
            11_800,
        ),
        demo_participant(
            "red-top",
            200,
            &pos("TOP"),
            "Darius",
            opp_win,
            6,
            7,
            4,
            21_000,
            205,
            11_200,
        ),
        demo_participant(
            "red-jgl",
            200,
            &pos("JUNGLE"),
            "Elise",
            opp_win,
            5,
            7,
            8,
            15_000,
            140,
            9_800,
        ),
        demo_participant(
            "red-bot",
            200,
            &pos("BOTTOM"),
            "Caitlyn",
            opp_win,
            9,
            6,
            6,
            30_000,
            250,
            13_000,
        ),
        demo_participant(
            "red-sup",
            200,
            &pos("UTILITY"),
            "Lulu",
            opp_win,
            3,
            6,
            14,
            8_500,
            40,
            8_200,
        ),
    ];

    // Fake match info
    let match_info = InfoDto {
        game_duration: if remake { 180 } else { 1847 },
        game_version: "14.24.632.8043".to_string(),
        game_ended_in_early_surrender: remake,
        participants,
        queue_id,
        teams: vec![
            TeamDto {
                team_id: 100,
                objectives: objectives(3, 1, 8),
            },
            TeamDto {
                team_id: 200,
                objectives: objectives(2, 0, 4),
            },
        ],
    };

    // Old/new rank for ranked games
    let (old_rank, new_rank) = if match_info.is_solo_queue() {
        let old = RankInfo {
            tier: "GOLD".to_string(),
            rank: "II".to_string(),
            lp: 45,
            ..Default::default()
        };
        let new = RankInfo {
            tier: "GOLD".to_string(),
            rank: "II".to_string(),
            lp: if win { 67 } else { 28 },
            wins: Some(if win { 33 } else { 32 }),
            losses: Some(if win { 23 } else { 24 }),
        };
        (Some(old), Some(new))
    } else if match_info.is_flex_queue() {
        let old = RankInfo {
            tier: "SILVER".to_string(),
            rank: "I".to_string(),
            lp: 75,
            ..Default::default()
        };
        let new = RankInfo {
            tier: if win {
                "GOLD".to_string()
            } else {
                "SILVER".to_string()
            },
            rank: if win {
                "IV".to_string()
            } else {
                "I".to_string()
            },
            lp: if win { 15 } else { 58 },
            wins: Some(if win { 18 } else { 17 }),
            losses: Some(15),
        };
        (Some(old), Some(new))
    } else {
        (None, None)
    };

    // Showcase the Phase 4 footer blocks: a recent-form streak bar on ranked
    // cards, champion mastery on everything else.
    let is_ranked = match_info.is_ranked();
    let streak = if is_ranked {
        vec![true, false, true, true, win]
    } else {
        Vec::new()
    };
    let mastery = (!is_ranked).then_some(MasteryInfo {
        level: 7,
        points: 142_384,
    });

    let image_ctx = MatchImageContext {
        player: &player,
        participant: &participant,
        match_info: &match_info,
        old_rank: old_rank.as_ref(),
        new_rank: new_rank.as_ref(),
        streak,
        mastery,
    };

    let image_data = ctx
        .data()
        .image_gen
        .generate_match_image(&image_ctx)
        .await?;
    let attachment = CreateAttachment::bytes(image_data, "match_result.png");

    ctx.send(poise::CreateReply::default().attachment(attachment))
        .await?;

    Ok(())
}

/// Compact builder for a filler roster participant (only the fields the card
/// reads for team/matchup derivations are meaningful; the rest default to zero).
#[allow(clippy::too_many_arguments)]
fn demo_participant(
    puuid: &str,
    team_id: i32,
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
        puuid: puuid.to_string(),
        team_position: position.to_string(),
        champion_name: champion.to_string(),
        kills,
        deaths,
        assists,
        total_damage_dealt_to_champions: damage,
        total_minions_killed: cs,
        vision_score: 20,
        gold_earned: gold,
        win,
        team_id,
        champ_level: 16,
        ..Default::default()
    }
}

fn objectives(dragon: i32, baron: i32, tower: i32) -> ObjectivesDto {
    ObjectivesDto {
        dragon: ObjectiveCountDto { kills: dragon },
        baron: ObjectiveCountDto { kills: baron },
        tower: ObjectiveCountDto { kills: tower },
    }
}
