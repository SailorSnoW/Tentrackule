use poise::serenity_prelude::ChannelType;
use reqwest::StatusCode;
use tokio::sync::oneshot;
use tracing::{debug, info};

use crate::{
    db::DbRequest,
    riot::{
        types::{Region, RiotApiError},
        ApiRequest,
    },
};

use super::{serenity, Context, Error};

fn enter_command_log(command_name: &str) {
    info!("🤖 Executing command: {}", command_name)
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

    debug!("🤖 Requesting Riot API for PUUID of: {}#{}", game_name, tag);
    let (tx, rx) = oneshot::channel();
    ctx.data()
        .api_sender
        .send(ApiRequest::PuuidByAccountId {
            game_name: game_name.clone(),
            tag_line: tag.clone(),
            respond_to: tx,
        })
        .await?;

    let account_data = match rx.await? {
        Ok(account_data) => {
            debug!("🤖 Got following account informations: {:?}", account_data);
            account_data
        }
        Err(err) => {
            tracing::error!("🤖 Riot API error while getting account: {:?}", err);

            match err {
                RiotApiError::Status(StatusCode::NOT_FOUND) => ctx
                    .say("❌ Player not found on Riot servers. Please try with another summoner name/tag.")
                    .await?,
                _ => ctx
                    .say("❌ Something went wrong during summoner API request.")
                    .await?,
            };

            return Ok(());
        }
    };

    debug!("🤖 Registering new tracked player informations in database.");
    let (tx, rx) = oneshot::channel();
    ctx.data()
        .db_sender
        .send(DbRequest::TrackNewAccount {
            account_data,
            guild_id: ctx.guild_id().expect("Is run from a guild"),
            region,
            respond_to: tx,
        })
        .await?;

    if let Err(e) = rx.await? {
        tracing::error!(
            "🤖 Unexpected database error on registering new tracking: {}",
            e
        );
        ctx.say("❌ Internal Error: Something went wrong during database operations.")
            .await?;
        return Ok(());
    };

    ctx.say(format!(
        "🎉 Successfully started to track new summoner: **{}#{}**",
        game_name, tag
    ))
    .await?;
    Ok(())
}

/// Show a list of the current tracked players on this server.
#[poise::command(slash_command, category = "Tracking", ephemeral)]
pub async fn show_tracked(ctx: Context<'_>) -> Result<(), Error> {
    enter_command_log("show_tracked");

    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            ctx.say("❌ This command can only be used inside a guild.")
                .await?;
            return Ok(());
        }
    };

    let (tx, rx) = oneshot::channel();
    ctx.data()
        .db_sender
        .send(DbRequest::GetAllAccountsForGuild {
            guild_id,
            respond_to: tx,
        })
        .await?;

    let response = match rx.await? {
        Ok(accounts) => {
            let mut s: String = "Currently Tracked:\n".to_owned();
            for account in accounts {
                let row = format!("\n- **{}#{}**", account.game_name, account.tag_line);
                s = s + &row;
            }
            s
        }
        Err(e) => {
            tracing::error!("🤖 Error happened during database request: {}", e);
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

    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            ctx.say("❌ this command can only be used inside a guild.")
                .await?;
            return Ok(());
        }
    };

    let (tx, rx) = oneshot::channel();
    ctx.data()
        .db_sender
        .send(DbRequest::SetAlertChannel {
            guild_id,
            channel_id: channel.id,
            respond_to: tx,
        })
        .await?;

    if let Err(e) = rx.await? {
        tracing::error!("🤖 Database error on setting alert channel: {}", e);
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

/// Tell the actual channel where tracking alerts are send.
#[poise::command(slash_command, category = "Settings", ephemeral)]
pub async fn current_alert_channel(ctx: Context<'_>) -> Result<(), Error> {
    enter_command_log("current_alert_channel");

    let guild_id = match ctx.guild_id() {
        Some(id) => id,
        None => {
            ctx.say("❌ This command can only be used inside a guild.")
                .await?;
            return Ok(());
        }
    };

    let (tx, rx) = oneshot::channel();
    ctx.data()
        .db_sender
        .send(DbRequest::GetAlertChannel {
            guild_id,
            respond_to: tx,
        })
        .await?;

    let response = match rx.await? {
        Some(channel_id) => {
            let channel = channel_id
                .to_channel(ctx)
                .await
                .expect("Can retrieve channel informations");
            format!("Current alert channel for this server: {}", channel)
        }
        None => {
            "Alert channel isn't set for this server. You can set it with `/set_alert_channel`."
                .to_string()
        }
    };

    ctx.say(response).await?;
    Ok(())
}
