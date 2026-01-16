mod config;
mod db;
mod discord;
mod error;
mod poller;
mod riot;

use std::sync::Arc;

use poise::serenity_prelude as serenity;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::db::Repository;
use crate::discord::{Data, ImageGenerator};
use crate::riot::RiotClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tentrackule=debug"));

    let json_logs = std::env::var("LOG_FORMAT")
        .map(|v| v.to_lowercase() == "json")
        .unwrap_or(false);

    if json_logs {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json().with_file(true).with_line_number(true))
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(
                fmt::layer()
                    .with_target(true)
                    .with_file(true)
                    .with_line_number(true)
                    .with_thread_ids(false),
            )
            .init();
    }

    tracing::info!("🦑 Starting Tentrackule 2.0");

    // Load configuration
    let config = Config::from_env()?;
    tracing::info!("⚙️ Configuration loaded");

    // Initialize database
    let db_options: SqliteConnectOptions = config.database_url.parse()?;
    let db_options = db_options.create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(db_options)
        .await?;

    db::run_migrations(&pool).await?;
    let repository = Repository::new(pool.clone());
    tracing::info!("🗄️ Database initialized");

    // Initialize Riot API client
    let riot_client = RiotClient::new(
        config.riot_api_key.clone(),
        config.riot_rate_limit_per_second,
    )?;
    tracing::info!("🔷 Riot API client initialized");

    // Initialize image generator
    let image_gen = Arc::new(ImageGenerator::new(config.ddragon_version.clone()).await?);
    tracing::info!(version = %config.ddragon_version, "🖼️ Image generator initialized");

    // Create shared data for Discord bot
    let data = Data {
        db: repository.clone(),
        riot: riot_client.clone(),
        image_gen: Arc::clone(&image_gen),
    };

    // Build Discord framework
    let framework = discord::create_framework(data);

    // Build Discord client
    let intents = serenity::GatewayIntents::GUILDS;
    let mut client = serenity::ClientBuilder::new(&config.discord_token, intents)
        .framework(framework)
        .await?;

    // Get HTTP client for poller
    let http = Arc::clone(&client.http);

    // Spawn match poller in background
    let poller_db = repository.clone();
    let poller_riot = riot_client.clone();
    let poller_image_gen = Arc::clone(&image_gen);
    let polling_interval = config.polling_interval_secs;

    tokio::spawn(async move {
        poller::start_polling(
            poller_db,
            poller_riot,
            http,
            poller_image_gen,
            polling_interval,
        )
        .await;
    });

    tracing::info!("🔄 Match poller spawned");

    // Start the bot
    tracing::info!("🎮 Starting Discord bot...");
    client.start().await?;

    Ok(())
}
