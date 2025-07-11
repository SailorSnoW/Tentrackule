//! Slash command implementations used by the Discord bot.

use poise::serenity_prelude::ChannelType;
use tentrackule_shared::{Account, QueueType, Region};
use tracing::{debug, info};

use super::{Context, Error, serenity};

/// Error message shown when a command is used outside of a guild context.
const GUILD_ONLY_ERR: &str = "❌ This command can only be used inside a guild.";

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum QueueAlertType {
    #[name = "Ranked Solo/Duo"]
    SoloDuo,
    #[name = "Ranked Flex"]
    Flex,
    #[name = "Normal Draft"]
    NormalDraft,
    #[name = "ARAM"]
    Aram,
}

impl From<QueueAlertType> for QueueType {
    fn from(q: QueueAlertType) -> Self {
        match q {
            QueueAlertType::SoloDuo => QueueType::SoloDuo,
            QueueAlertType::Flex => QueueType::Flex,
            QueueAlertType::NormalDraft => QueueType::NormalDraft,
            QueueAlertType::Aram => QueueType::Aram,
        }
    }
}

/// Return the [`GuildId`] of the context or notify the user if the command was
/// run outside a guild.
async fn require_guild(ctx: &Context<'_>) -> Option<serenity::GuildId> {
    match ctx.guild_id() {
        Some(id) => Some(id),
        None => {
            let _ = ctx.say(GUILD_ONLY_ERR).await;
            None
        }
    }
}

fn enter_command_log(command_name: &str) {
    info!("/{} invoked", command_name)
}

/// Track a new player and start receiving alerts on new game results in your server.
#[poise::command(slash_command, category = "Tracking", ephemeral)]
pub async fn track(
    ctx: Context<'_>,
    game_name: String,
    tag: String,
    region: Region,
) -> Result<(), Error> {
    enter_command_log("track");

    let Some(guild_id) = require_guild(&ctx).await else {
        return Ok(());
    };

    debug!("[CMD] fetching PUUID for {}#{}", game_name, tag);

    let api_account_data = ctx
        .data()
        .account_api
        .get_account_by_riot_id(game_name.clone(), tag.clone())
        .await?;

    let cached_account = Account {
        puuid: api_account_data.puuid,
        game_name: api_account_data.game_name,
        tag_line: api_account_data.tag_line,
        region,
        last_match_id: Default::default(),
    };

    debug!("[CMD] storing tracking data in DB");

    if let Err(e) = ctx.data().db.insert_account(cached_account, guild_id).await {
        tracing::error!("DB error while tracking player: {}", e);
        let _ = ctx
            .say("❌ Internal Error: Something went wrong during database operations.")
            .await;
        return Ok(());
    }

    ctx.say(format!(
        "🎉 Successfully started to track new summoner: **{}#{}**",
        game_name, tag
    ))
    .await?;
    Ok(())
}

/// Stop tracking a player in this server.
#[poise::command(slash_command, category = "Tracking", ephemeral)]
pub async fn untrack(ctx: Context<'_>, game_name: String, tag: String) -> Result<(), Error> {
    enter_command_log("untrack");

    let Some(guild_id) = require_guild(&ctx).await else {
        return Ok(());
    };

    debug!("fetching PUUID for {}#{}", game_name, tag);
    let account_data = ctx
        .data()
        .account_api
        .get_account_by_riot_id(game_name.clone(), tag.clone())
        .await?;

    if let Err(e) = ctx
        .data()
        .db
        .remove_account(account_data.puuid, guild_id)
        .await
    {
        tracing::error!("DB error while untracking player: {}", e);
        ctx.say("❌ Internal Error: Something went wrong during database operations.")
            .await?;
        return Ok(());
    }

    ctx.say(format!(
        "🗑️ Successfully stopped tracking summoner: **{}#{}**",
        game_name, tag
    ))
    .await?;
    Ok(())
}

/// Show a list of the current tracked players on this server.
#[poise::command(slash_command, category = "Tracking", ephemeral)]
pub async fn show_tracked(ctx: Context<'_>) -> Result<(), Error> {
    enter_command_log("show_tracked");

    let Some(guild_id) = require_guild(&ctx).await else {
        return Ok(());
    };

    let response = match ctx.data().db.get_accounts_for(guild_id).await {
        Ok(accounts) => {
            let mut s: String = "Currently Tracked:\n".to_owned();
            for account in accounts {
                let row = format!("\n- **{}#{}**", account.game_name, account.tag_line);
                s = s + &row;
            }
            s
        }
        Err(e) => {
            tracing::error!("DB query error: {}", e);
            "❌ Internal Error: Couldn't retrieve tracked players for this server.".to_string()
        }
    };

    ctx.say(response).await?;
    Ok(())
}

/// Change the channel where the bot should send tracking alerts.
#[poise::command(slash_command, category = "Settings", ephemeral)]
pub async fn set_alert_channel(
    ctx: Context<'_>,
    #[description = "The text channel where to send tracking alerts."]
    channel: serenity::GuildChannel,
) -> Result<(), Error> {
    enter_command_log("set_alert_channel");

    if channel.kind != ChannelType::Text {
        ctx.say("❌ Specified channel need to be a Text channel where messages can be sent !")
            .await?;
        return Ok(());
    }

    let Some(guild_id) = require_guild(&ctx).await else {
        return Ok(());
    };

    if let Err(e) = ctx.data().db.set_alert_channel(guild_id, channel.id).await {
        tracing::error!("DB error while setting alert channel: {}", e);
        ctx.say("❌ Internal Error: Couldn't update alert channel.")
            .await?;
        return Ok(());
    }

    let response = format!(
        "🎉 Successfully set alerts diffusion to channel {}",
        channel
    );
    ctx.say(response).await?;
    Ok(())
}

/// Enable or disable alerts for a specific queue type in this server.
#[poise::command(slash_command, category = "Settings", ephemeral)]
pub async fn set_queue_alert(
    ctx: Context<'_>,
    #[description = "Queue type"] queue: QueueAlertType,
    #[description = "Enable or disable alerts"] enabled: bool,
) -> Result<(), Error> {
    enter_command_log("set_queue_alert");

    let Some(guild_id) = require_guild(&ctx).await else {
        return Ok(());
    };

    if let Err(e) = ctx
        .data()
        .db
        .set_queue_alert_enabled(guild_id, queue.into(), enabled)
        .await
    {
        tracing::error!("DB error while setting queue alert: {}", e);
        ctx.say("❌ Internal Error: Couldn't update queue alert setting.")
            .await?;
        return Ok(());
    }

    let status = if enabled { "enabled" } else { "disabled" };
    ctx.say(format!("✅ Alerts for {:?} are now {}.", queue, status))
        .await?;
    Ok(())
}

/// Tell the actual channel where tracking alerts are send.
#[poise::command(slash_command, category = "Settings", ephemeral)]
pub async fn current_alert_channel(ctx: Context<'_>) -> Result<(), Error> {
    enter_command_log("current_alert_channel");

    let Some(guild_id) = require_guild(&ctx).await else {
        return Ok(());
    };

    let response = match ctx.data().db.get_alert_channel(guild_id).await {
        Ok(Some(channel_id)) => {
            let channel = channel_id
                .to_channel(ctx)
                .await
                .expect("Can retrieve channel informations");
            format!("Current alert channel for this server: {}", channel)
        }
        Ok(None) => {
            "Alert channel isn't set for this server. You can set it with `/set_alert_channel`."
                .to_string()
        }
        Err(e) => {
            tracing::error!("DB query error: {}", e);
            "❌ Internal Error: Couldn't get the alert channel for this server.".to_string()
        }
    };

    ctx.say(response).await?;
    Ok(())
}
