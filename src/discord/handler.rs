use tracing::warn;

use super::*;

pub async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
) -> Result<(), Error> {
    if let serenity::FullEvent::Ready { data_about_bot, .. } = event {
        info!("🤖 [DISCORD] connected as {}", data_about_bot.user.name);
        info!(
            "🎮 [DISCORD] joined {} guild(s)",
            data_about_bot.guilds.len()
        );
        ctx.set_activity(Some(ActivityData::playing("League of Legends")));
    }

    if let serenity::FullEvent::GuildCreate { guild, is_new } = event {
        if matches!(is_new, Some(true)) {
            let welcome_message = "👋 Thanks for using Tentrackule 🦑! ".to_owned()
                + "You should first configure the alert channel with `/set_alert_channel` to be able to receive tracking alerts.";

            if let Some(channel_id) = guild.system_channel_id {
                if let Err(e) = channel_id.say(ctx, &welcome_message).await {
                    warn!("⚠️ [DISCORD] Couldn't send the welcome message: {}", e);
                }
            } else if let Some(first_text) = guild
                .channels
                .values()
                .find(|c| c.kind == serenity::ChannelType::Text)
            {
                let _ = first_text.id.say(ctx, &welcome_message).await;
            }
        }
    }

    Ok(())
}
