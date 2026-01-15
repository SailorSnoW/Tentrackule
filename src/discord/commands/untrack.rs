use poise::serenity_prelude as serenity;
use tracing::{info, instrument};

use crate::discord::bot::Context;
use crate::error::AppError;

/// Stop tracking a League of Legends player
#[poise::command(slash_command, guild_only)]
#[instrument(
    skip(ctx),
    fields(
        guild_id,
        user_id = %ctx.author().id,
        riot_id = %format!("{}#{}", game_name, tag_line)
    )
)]
pub async fn untrack(
    ctx: Context<'_>,
    #[description = "Game name (before the #)"] game_name: String,
    #[description = "Tag line (after the #)"] tag_line: String,
) -> Result<(), AppError> {
    let guild_id = ctx
        .guild_id()
        .ok_or(AppError::Config("Must be used in a guild".into()))?;
    tracing::Span::current().record("guild_id", guild_id.get());

    // Find player in database
    let player = ctx
        .data()
        .db
        .get_player_by_riot_id(&game_name, &tag_line)
        .await?
        .ok_or(AppError::PlayerNotFound {
            game_name: game_name.clone(),
            tag_line: tag_line.clone(),
        })?;

    // Remove from guild
    let removed = ctx
        .data()
        .db
        .remove_player_from_guild(guild_id.get(), player.id)
        .await?;

    if !removed {
        return Err(AppError::PlayerNotTracked);
    }

    let embed = serenity::CreateEmbed::new()
        .title("Player Untracked")
        .description(format!(
            "Stopped tracking **{}#{}**",
            player.game_name, player.tag_line
        ))
        .color(0xff6600);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    info!(player_id = player.id, "Player untracked successfully");

    Ok(())
}
