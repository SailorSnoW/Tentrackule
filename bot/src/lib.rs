//! Discord bot implementation responsible for interacting with users.
//!
//! This crate exposes the [`DiscordBot`] type which wraps a Serenity client and
//! provides the command handlers used to configure tracking.

use commands::{current_alert_channel, set_alert_channel, show_tracked, track, untrack};
use poise::serenity_prelude as serenity;
use serenity::*;
use std::{env, fmt::Debug, sync::Arc};
use tentrackule_shared::traits::{api::AccountApi, CacheFull};
use tracing::{error, info};

use handler::event_handler;

mod commands;
mod handler;

// Types use by all command functions
/// Error type shared by all slash commands.
type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

/// Wrapper around a Serenity [`Client`] with all command handlers registered.
pub struct DiscordBot(Client);

impl DiscordBot {
    pub fn client(&self) -> &Client {
        &self.0
    }

    pub async fn new(db: Arc<dyn CacheFull>, account_api: Arc<dyn AccountApi>) -> Self {
        let token =
            env::var("DISCORD_BOT_TOKEN").expect("Expected a discord bot token in the environment");
        let intents = GatewayIntents::non_privileged();
        let framework = poise::Framework::builder()
            .options(poise::FrameworkOptions {
                commands: vec![
                    set_alert_channel(),
                    current_alert_channel(),
                    track(),
                    show_tracked(),
                    untrack(),
                ],
                event_handler: |ctx, event, framework, _| {
                    Box::pin(event_handler(ctx, event, framework))
                },
                ..Default::default()
            })
            .setup(|ctx, _ready, framework| {
                Box::pin(async move {
                    poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                    Ok(Data { db, account_api })
                })
            })
            .build();
        let client_builder = ClientBuilder::new(token, intents).framework(framework);

        info!("initializing bot");
        let client = client_builder
            .await
            .expect("Discord client creation should success.");

        DiscordBot(client)
    }

    pub fn start(self) -> tokio::task::JoinHandle<Result<(), serenity::Error>> {
        tokio::spawn(async move { self.run().await })
    }

    async fn run(mut self) -> Result<(), serenity::Error> {
        info!("connecting to gateway");
        if let Err(why) = self.0.start().await {
            error!("connection failed: {why:?}");
            return Err(why);
        }

        Ok(())
    }
}

/// Custom data passed to all command functions.
#[derive(Debug)]
pub struct Data {
    db: Arc<dyn CacheFull>,
    account_api: Arc<dyn AccountApi>,
}
