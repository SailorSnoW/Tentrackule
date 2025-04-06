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

        let (tx, rx) = mpsc::channel(100);

        // Need to be spawned before the result poller in case the result poller try to send a
        // message to send an alert.
        AlertSender::new(
            rx,
            ctx.clone(),
            framework.user_data().await.db_sender.clone(),
        )
        .start();
        // We spawn the result poller when the bot is ready to operate.
        ResultPoller::new(
            framework.user_data().await.api_sender.clone(),
            framework.user_data().await.db_sender.clone(),
            tx,
        )
        .spawn();
    }

    Ok(())
}
