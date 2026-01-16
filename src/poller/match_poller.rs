use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude::{ChannelId, CreateAttachment, CreateMessage, Http};
use tokio::time::interval;
use tracing::{Span, debug, error, info, instrument, warn};

use crate::db::{Player, RankInfo, Repository};
use crate::discord::image_gen::{ImageGenerator, MatchImageContext};
use crate::error::AppError;
use crate::riot::{Platform, RiotClient};

#[derive(Debug, thiserror::Error)]
enum PollerError {
    #[error(transparent)]
    App(#[from] AppError),
    #[error("Player {player_puuid} not found in match {match_id}")]
    PlayerNotFoundInMatch {
        player_puuid: String,
        match_id: String,
    },
}

pub async fn start_polling(
    db: Repository,
    riot: RiotClient,
    http: Arc<Http>,
    image_gen: Arc<ImageGenerator>,
    interval_secs: u64,
) {
    let mut interval = interval(Duration::from_secs(interval_secs));

    info!(interval_secs, "🔄 Match poller started");

    loop {
        interval.tick().await;

        if let Err(e) = poll_players(&db, &riot, &http, &image_gen).await {
            error!(error = ?e, "🔄 ❌ Polling cycle failed");
        }
    }
}

#[instrument(skip_all, fields(player_count))]
async fn poll_players(
    db: &Repository,
    riot: &RiotClient,
    http: &Http,
    image_gen: &ImageGenerator,
) -> Result<(), PollerError> {
    let players = db.get_all_tracked_players().await?;

    if players.is_empty() {
        debug!("🔄 No players tracked, skipping poll cycle");
        return Ok(());
    }

    Span::current().record("player_count", players.len());
    info!(count = players.len(), "🔄 Polling {} player(s)", players.len());

    for player in players {
        if let Err(e) = check_player_match(db, riot, http, image_gen, &player).await {
            warn!(
                error = ?e,
                player_id = player.id,
                riot_id = %player.riot_id(),
                "🔄 ⚠️ Failed to check player match"
            );
        }
    }

    Ok(())
}

#[instrument(
    skip(db, riot, http, image_gen, player),
    fields(
        player_id = player.id,
        riot_id = %player.riot_id(),
        region = %player.region
    )
)]
async fn check_player_match(
    db: &Repository,
    riot: &RiotClient,
    http: &Http,
    image_gen: &ImageGenerator,
    player: &Player,
) -> Result<(), PollerError> {
    let platform: Platform = player.region.parse()?;
    let region = platform.to_region();

    // Get latest match ID
    let match_ids = riot.get_match_ids(region, &player.puuid, 1).await?;

    let Some(latest_match_id) = match_ids.first() else {
        debug!("🔄 No matches found");
        return Ok(());
    };

    // Check if this is a new match
    if player.last_match_id.as_deref() == Some(latest_match_id) {
        return Ok(());
    }

    // Get match details
    let match_data = riot.get_match(region, latest_match_id).await?;

    // Skip unsupported game modes
    if !match_data.info.is_supported() {
        debug!(
            queue_id = match_data.info.queue_id,
            match_id = latest_match_id,
            "🔄 Skipping unsupported queue"
        );
        // Still update last_match_id so we don't check this match again
        db.update_player_last_match(player.id, latest_match_id)
            .await?;
        return Ok(());
    }

    info!(
        match_id = latest_match_id,
        queue = match_data.info.queue_name(),
        "🔄 ✅ New match detected"
    );

    // Find the player's participant data
    let participant = match_data
        .info
        .participants
        .iter()
        .find(|p| p.puuid == player.puuid)
        .ok_or_else(|| PollerError::PlayerNotFoundInMatch {
            player_puuid: player.puuid.clone(),
            match_id: latest_match_id.to_string(),
        })?;

    // Get current rank if ranked game
    let old_rank = if match_data.info.is_solo_queue() {
        player.solo_rank_info()
    } else if match_data.info.queue_id == 440 {
        player.flex_rank_info()
    } else {
        None
    };

    // Fetch new rank info and profile icon
    let (new_solo_rank, new_flex_rank) = fetch_rank_info(riot, platform, &player.puuid).await?;

    // Update profile icon (may have changed)
    if let Ok(summoner) = riot.get_summoner_by_puuid(platform, &player.puuid).await {
        let _ = db
            .update_player_profile_icon(player.id, summoner.profile_icon_id)
            .await;
    }

    let new_rank = if match_data.info.is_solo_queue() {
        new_solo_rank.as_ref()
    } else if match_data.info.queue_id == 440 {
        new_flex_rank.as_ref()
    } else {
        None
    };

    // Build image
    let ctx = MatchImageContext {
        player,
        participant,
        match_info: &match_data.info,
        old_rank: old_rank.as_ref(),
        new_rank,
    };

    let image_data = match image_gen.generate_match_image(&ctx).await {
        Ok(data) => data,
        Err(e) => {
            error!(error = ?e, "🖼️ ❌ Failed to generate match image");
            return Err(e.into());
        }
    };

    let image_data: Arc<[u8]> = image_data.into();

    // Get all guilds tracking this player
    let guilds = db.get_guilds_tracking_player(player.id).await?;

    // Send image to all guilds
    for guild in guilds {
        if let Some(channel_id) = guild.alert_channel_id {
            let channel = ChannelId::new(channel_id as u64);
            let attachment = CreateAttachment::bytes(image_data.as_ref(), "match_result.png");
            let message = CreateMessage::new().add_file(attachment);

            if let Err(e) = channel.send_message(http, message).await {
                error!(
                    error = ?e,
                    guild_id = guild.id,
                    channel_id,
                    "🎮 ❌ Failed to send alert message"
                );
            } else {
                debug!(guild_id = guild.id, channel_id, "🎮 ✅ Alert sent");
            }
        }
    }

    // Update player in database
    db.update_player_last_match(player.id, latest_match_id)
        .await?;
    db.update_player_rank(player.id, new_solo_rank.as_ref(), new_flex_rank.as_ref())
        .await?;

    Ok(())
}

async fn fetch_rank_info(
    riot: &RiotClient,
    platform: Platform,
    puuid: &str,
) -> Result<(Option<RankInfo>, Option<RankInfo>), PollerError> {
    let entries = riot.get_league_entries_by_puuid(platform, puuid).await?;

    let mut solo_rank = None;
    let mut flex_rank = None;

    for entry in entries {
        let rank_info = RankInfo {
            tier: entry.tier.clone(),
            rank: entry.rank.clone(),
            lp: entry.league_points,
        };

        if entry.is_solo_queue() {
            solo_rank = Some(rank_info);
        } else if entry.is_flex_queue() {
            flex_rank = Some(rank_info);
        }
    }

    Ok((solo_rank, flex_rank))
}
