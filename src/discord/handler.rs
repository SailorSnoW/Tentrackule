use alert_sender::AlertSender;

use crate::riot::result_poller::ResultPoller;

use super::*;

pub async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    framework: poise::FrameworkContext<'_, Data, Error>,
) -> Result<(), Error> {
    if let serenity::FullEvent::Ready { data_about_bot, .. } = event {
        info!(
            "🤖 Bot succesfuly connected to user: {}",
            data_about_bot.user.name
        );
        info!(
            "🤖 Bot is present in {} guild(s).",
            data_about_bot.guilds.len()
        );
        ctx.set_activity(Some(ActivityData::playing("League of Legends")));
    }

    Ok(())
}
