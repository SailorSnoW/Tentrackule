use commands::{current_alert_channel, set_alert_channel, show_tracked, track, untrack};
use poise::serenity_prelude as serenity;
use serenity::*;
use std::{env, sync::Arc};
use tentrackule_db::SharedDatabase;
use tentrackule_riot_api::api::client::AccountApi;
use tracing::{error, info};

use handler::event_handler;

pub use alert_dispatcher::AlertDispatcher;

mod alert_dispatcher;
mod commands;
mod handler;
mod message_sender;

// Types used by all command functions
type Error = Box<dyn std::error::Error + Send + Sync>;
#[allow(unused)]
type Context<'a> = poise::Context<'a, Data, Error>;

pub struct DiscordBot {
    pub client: Client,
}

impl DiscordBot {
    pub async fn new(db: SharedDatabase, account_api: Arc<dyn AccountApi>) -> Self {
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

        info!("🤖 [DISCORD] initializing bot");
        let client = client_builder
            .await
            .expect("Discord client creation should success.");

        Self { client }
    }

    pub fn start(self) -> tokio::task::JoinHandle<Result<(), serenity::Error>> {
        tokio::spawn(async move { self.run().await })
    }

    async fn run(mut self) -> Result<(), serenity::Error> {
        info!("🌐 [DISCORD] connecting to gateway");
        if let Err(why) = self.client.start().await {
            error!("❌ [DISCORD] connection failed: {why:?}");
            return Err(why);
        }

        Ok(())
    }
}

// Custom user data passed to all command functions
#[derive(Debug)]
pub struct Data {
    db: SharedDatabase,
    account_api: Arc<dyn AccountApi>,
}
