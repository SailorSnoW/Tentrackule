use poise::ChoiceParameter;
use poise::serenity_prelude::CreateAttachment;

use crate::db::{Player, RankInfo};
use crate::discord::bot::Context;
use crate::discord::image_gen::MatchImageContext;
use crate::error::AppError;
use crate::riot::{InfoDto, ParticipantDto};

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
#[poise::command(slash_command, guild_only, rename = "dev_test_alert")]
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
        profile_icon_id: Some(4658),
        last_match_id: None,
        last_rank_solo_tier: Some("GOLD".to_string()),
        last_rank_solo_rank: Some("II".to_string()),
        last_rank_solo_lp: Some(45),
        last_rank_flex_tier: Some("SILVER".to_string()),
        last_rank_flex_rank: Some("I".to_string()),
        last_rank_flex_lp: Some(75),
    };

    // Fake participant data
    let participant = ParticipantDto {
        puuid: "test-puuid-12345".to_string(),
        team_position: "MIDDLE".to_string(),
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
    };

    // Fake match info
    let match_info = InfoDto {
        game_duration: if remake { 180 } else { 1847 },
        game_version: "14.24.632.8043".to_string(),
        game_ended_in_early_surrender: remake,
        participants: vec![participant.clone()],
        queue_id,
    };

    // Old/new rank for ranked games
    let (old_rank, new_rank) = if queue_id == 420 {
        let old = RankInfo {
            tier: "GOLD".to_string(),
            rank: "II".to_string(),
            lp: 45,
        };
        let new = RankInfo {
            tier: "GOLD".to_string(),
            rank: "II".to_string(),
            lp: if win { 67 } else { 28 },
        };
        (Some(old), Some(new))
    } else if queue_id == 440 {
        let old = RankInfo {
            tier: "SILVER".to_string(),
            rank: "I".to_string(),
            lp: 75,
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
        };
        (Some(old), Some(new))
    } else {
        (None, None)
    };

    let image_ctx = MatchImageContext {
        player: &player,
        participant: &participant,
        match_info: &match_info,
        old_rank: old_rank.as_ref(),
        new_rank: new_rank.as_ref(),
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
