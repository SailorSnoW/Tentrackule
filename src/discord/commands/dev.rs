use poise::ChoiceParameter;
use poise::serenity_prelude::CreateAttachment;

use crate::db::{Player, RankInfo};
use crate::discord::bot::Context;
use crate::discord::image_gen::{ImageGenerator, MasteryInfo, MatchImageContext};
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

    let pos = |p: &str| if queue_id == 450 { "" } else { p }.to_string();

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
        summoner1_id: 4,
        summoner2_id: 14,
        perks: PerksDto {
            styles: vec![
                PerkStyleDto {
                    style: 8100,
                    selections: vec![PerkSelectionDto { perk: 8112 }],
                },
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

    let mut participants = vec![participant.clone()];
    participants.extend(DEMO_TEAM.map(
        |(puuid, team_id, position, champion, kills, deaths, assists, damage, cs, gold)| {
            ParticipantDto {
                puuid: puuid.to_string(),
                team_id,
                team_position: pos(position),
                champion_name: champion.to_string(),
                win: (team_id == 100) == win,
                kills,
                deaths,
                assists,
                total_damage_dealt_to_champions: damage,
                total_minions_killed: cs,
                gold_earned: gold,
                vision_score: 20,
                champ_level: 16,
                ..Default::default()
            }
        },
    ));

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
    let attachment = CreateAttachment::bytes(image_data, ImageGenerator::ATTACHMENT_FILENAME);

    ctx.send(poise::CreateReply::default().attachment(attachment))
        .await?;

    Ok(())
}

type DemoRow = (
    &'static str,
    i32,
    &'static str,
    &'static str,
    i32,
    i32,
    i32,
    i64,
    i32,
    i64,
);

const DEMO_TEAM: [DemoRow; 9] = [
    (
        "blue-top", 100, "TOP", "Garen", 5, 4, 7, 18_000, 210, 11_000,
    ),
    (
        "blue-jgl", 100, "JUNGLE", "LeeSin", 6, 5, 11, 16_000, 150, 10_500,
    ),
    (
        "blue-bot", 100, "BOTTOM", "Jinx", 11, 3, 9, 32_000, 240, 13_500,
    ),
    (
        "blue-sup", 100, "UTILITY", "Thresh", 2, 6, 21, 9_000, 35, 8_500,
    ),
    (
        "red-mid", 200, "MIDDLE", "Zed", 7, 8, 5, 24_000, 195, 11_800,
    ),
    (
        "red-top", 200, "TOP", "Darius", 6, 7, 4, 21_000, 205, 11_200,
    ),
    (
        "red-jgl", 200, "JUNGLE", "Elise", 5, 7, 8, 15_000, 140, 9_800,
    ),
    (
        "red-bot", 200, "BOTTOM", "Caitlyn", 9, 6, 6, 30_000, 250, 13_000,
    ),
    (
        "red-sup", 200, "UTILITY", "Lulu", 3, 6, 14, 8_500, 40, 8_200,
    ),
];

fn objectives(dragon: i32, baron: i32, tower: i32) -> ObjectivesDto {
    ObjectivesDto {
        dragon: ObjectiveCountDto { kills: dragon },
        baron: ObjectiveCountDto { kills: baron },
        tower: ObjectiveCountDto { kills: tower },
    }
}
