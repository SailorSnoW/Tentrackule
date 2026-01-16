use poise::serenity_prelude as serenity;
use tracing::{info, instrument, warn};

use crate::discord::bot::Context;
use crate::error::AppError;
use crate::riot::Platform;

/// Track a League of Legends player
#[poise::command(slash_command, guild_only)]
#[instrument(
    skip(ctx),
    fields(
        guild_id,
        user_id = %ctx.author().id,
        riot_id = %format!("{}#{}", game_name, tag_line),
        region = %region
    )
)]
pub async fn track(
    ctx: Context<'_>,
    #[description = "Game name (before the #)"] game_name: String,
    #[description = "Tag line (after the #)"] tag_line: String,
    #[description = "Server region"] region: Platform,
) -> Result<(), AppError> {
    let guild_id = ctx
        .guild_id()
        .ok_or(AppError::Config("Must be used in a guild".into()))?;
    let user_id = ctx.author().id;

    tracing::Span::current().record("guild_id", guild_id.get());

    let platform = region;
    let riot_region = platform.to_region();

    // Defer response since API calls might take a moment
    ctx.defer().await?;

    // Get account from Riot API
    let account = ctx
        .data()
        .riot
        .get_account_by_riot_id(riot_region, &game_name, &tag_line)
        .await?;

    let puuid = &account.puuid;
    let actual_game_name = account.game_name.as_deref().unwrap_or(&game_name);
    let actual_tag_line = account.tag_line.as_deref().unwrap_or(&tag_line);

    // Get summoner info for profile icon
    let summoner = ctx
        .data()
        .riot
        .get_summoner_by_puuid(platform, puuid)
        .await?;

    // Save to database
    let player = ctx
        .data()
        .db
        .get_or_create_player(puuid, actual_game_name, actual_tag_line, platform.as_str())
        .await?;

    // Update profile icon
    ctx.data()
        .db
        .update_player_profile_icon(player.id, summoner.profile_icon_id)
        .await?;

    // If player has no last_match_id, fetch and store it to avoid alerting on old games
    if player.last_match_id.is_none() {
        let riot_region = platform.to_region();
        match ctx
            .data()
            .riot
            .get_match_ids(riot_region, puuid, 1)
            .await
        {
            Ok(match_ids) => {
                if let Some(last_match_id) = match_ids.first() {
                    ctx.data()
                        .db
                        .update_player_last_match(player.id, last_match_id)
                        .await?;
                    info!(last_match_id, "Initialized player's last_match_id");
                }
            }
            Err(e) => {
                // Non-fatal: player might not have any matches yet
                warn!(error = %e, "Could not fetch last match ID for new player");
            }
        }
    }

    // Check if already tracked in this guild
    if ctx
        .data()
        .db
        .is_player_tracked_in_guild(guild_id.get(), player.id)
        .await?
    {
        ctx.say(format!(
            "**{}#{}** is already being tracked in this server.",
            actual_game_name, actual_tag_line
        ))
        .await?;
        return Ok(());
    }

    // Link player to guild
    ctx.data()
        .db
        .add_player_to_guild(guild_id.get(), player.id, user_id.get())
        .await?;

    // Build response embed
    let embed = serenity::CreateEmbed::new()
        .title("Player Tracked")
        .description(format!(
            "Now tracking **{}#{}** on **{}**",
            actual_game_name,
            actual_tag_line,
            platform.display_name()
        ))
        .color(0x00ff00)
        .field("PUUID", &puuid[..8], true)
        .field("Region", platform.to_string(), true);

    ctx.send(poise::CreateReply::default().embed(embed)).await?;

    info!(player_id = player.id, "Player tracked successfully");

    Ok(())
}
