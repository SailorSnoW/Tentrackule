use poise::serenity_prelude::{self as serenity, Mentionable};
use tracing::{info, instrument};

use crate::discord::bot::Context;
use crate::error::AppError;

/// Configure the bot for this server
#[poise::command(
    slash_command,
    guild_only,
    required_permissions = "MANAGE_GUILD",
    subcommands("channel")
)]
pub async fn config(_ctx: Context<'_>) -> Result<(), AppError> {
    // Parent command, subcommands handle the actual work
    Ok(())
}

/// Set the channel for game alerts
#[poise::command(slash_command, guild_only, required_permissions = "MANAGE_GUILD")]
#[instrument(
    skip(ctx),
    fields(
        guild_id,
        user_id = %ctx.author().id,
        channel_id = %channel.id
    )
)]
pub async fn channel(
    ctx: Context<'_>,
    #[description = "Channel for game alerts"]
    #[channel_types("Text")]
    channel: serenity::GuildChannel,
) -> Result<(), AppError> {
    let guild_id = ctx
        .guild_id()
        .ok_or(AppError::Config("Must be used in a guild".into()))?;
    tracing::Span::current().record("guild_id", guild_id.get());

    ctx.data()
        .db
        .set_guild_alert_channel(guild_id.get(), channel.id.get())
        .await?;

    let embed = serenity::CreateEmbed::new()
        .title("Configuration Updated")
        .description(format!(
            "Game alerts will now be sent to {}",
            channel.mention()
        ))
        .color(0x00ff00);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    info!("Alert channel configured");

    Ok(())
}
