use poise::serenity_prelude as serenity;

use crate::discord::bot::Context;
use crate::error::AppError;

/// List all tracked players in this server
#[poise::command(slash_command, guild_only)]
pub async fn list(ctx: Context<'_>) -> Result<(), AppError> {
    let guild_id = ctx
        .guild_id()
        .ok_or(AppError::Config("Must be used in a guild".into()))?;

    let players = ctx.data().db.get_guild_players(guild_id.get()).await?;

    if players.is_empty() {
        ctx.say("No players are being tracked in this server.\nUse `/track` to add players.")
            .await?;
        return Ok(());
    }

    let mut description = String::new();
    for player in &players {
        description.push_str(&format!(
            "- **{}#{}** ({})\n",
            player.game_name,
            player.tag_line,
            player.region.to_uppercase()
        ));
    }

    let embed = serenity::CreateEmbed::new()
        .title(format!("Tracked Players ({})", players.len()))
        .description(description)
        .color(0x0099ff);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    Ok(())
}
