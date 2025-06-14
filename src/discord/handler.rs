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

    Ok(())
}
