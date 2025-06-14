use commands::{current_alert_channel, set_alert_channel, show_tracked, track};
use poise::serenity_prelude as serenity;
use serenity::*;
use std::env;
use tokio::sync::mpsc;
use tracing::{error, info};

use handler::event_handler;

use crate::{db::DbRequest, riot::ApiRequest};

pub use alert_sender::{AlertSender, AlertSenderMessage, AlertSenderTx};

mod alert_sender;
mod commands;
mod embeds;
mod handler;

// Types used by all command functions
type Error = Box<dyn std::error::Error + Send + Sync>;
#[allow(unused)]
type Context<'a> = poise::Context<'a, Data, Error>;

pub struct DiscordBot {
    pub client: Client,
}

impl DiscordBot {
    pub async fn new(
        db_sender: mpsc::Sender<DbRequest>,
        api_sender: mpsc::Sender<ApiRequest>,
    ) -> Self {
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
                ],
                event_handler: |ctx, event, framework, _| {
                    Box::pin(event_handler(ctx, event, framework))
                },
                ..Default::default()
            })
            .setup(|ctx, _ready, framework| {
                Box::pin(async move {
                    poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                    Ok(Data {
                        db_sender,
                        api_sender,
                    })
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

    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
    }

    async fn run(mut self) {
        info!("🌐 [DISCORD] connecting to gateway");
        if let Err(why) = self.client.start().await {
            error!("❌ [DISCORD] connection failed: {why:?}");
            panic!()
        }
    }
}

// Custom user data passed to all command functions
#[derive(Debug)]
pub struct Data {
    db_sender: mpsc::Sender<DbRequest>,
    api_sender: mpsc::Sender<ApiRequest>,
}
