mod config;
mod db;
mod discord;
mod error;
mod metrics;
mod poller;
mod riot;

use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude as serenity;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use tokio::sync::watch;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::db::Repository;
use crate::discord::{Data, ImageGenerator};
use crate::metrics::Metrics;
use crate::riot::RiotClient;

const SHUTDOWN_DRAIN: Duration = Duration::from_secs(4);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "🦑 Starting Tentrackule"
    );

    let config = Config::from_env()?;
    tracing::info!("⚙️ Configuration loaded");

    let metrics = Arc::new(Metrics::new());
    tokio::spawn(Arc::clone(&metrics).run_reporter());

    let db_options: SqliteConnectOptions = config.database_url.parse()?;
    let db_options = db_options
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(db_options)
        .await?;

    db::run_migrations(&pool).await?;
    let repository = Repository::new(pool.clone());
    tracing::info!("🗄️ Database initialized");

    let riot_client = RiotClient::new(
        config.riot_api_key.clone(),
        config.riot_rate_limit_per_second,
        config.riot_rate_limit_per_two_min,
        Arc::clone(&metrics),
    )?;
    tracing::info!("🔷 Riot API client initialized");

    let image_gen = Arc::new(
        ImageGenerator::new(
            config.ddragon_version_override.clone(),
            config.cloudflare_account_id.clone(),
            config.cloudflare_api_token.clone(),
            Arc::clone(&metrics),
        )
        .await?,
    );
    tracing::info!(
        version = %image_gen.ddragon_version(),
        renderer = image_gen.has_renderer(),
        "🖼️ Image generator initialized"
    );

    tokio::spawn(Arc::clone(&image_gen).run_ddragon_refresh());
    tracing::info!("🖼️ DDragon refresh task spawned");

    let data = Data {
        db: repository.clone(),
        riot: riot_client.clone(),
        image_gen: Arc::clone(&image_gen),
    };

    let framework = discord::create_framework(data);

    let intents = serenity::GatewayIntents::GUILDS;
    let mut client = serenity::ClientBuilder::new(&config.discord_token, intents)
        .framework(framework)
        .await?;

    let http = Arc::clone(&client.http);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    let poller_db = repository.clone();
    let poller_riot = riot_client.clone();
    let poller_image_gen = Arc::clone(&image_gen);
    let poller_metrics = Arc::clone(&metrics);
    let polling_interval = config.polling_interval_secs;

    let poller_handle = tokio::spawn(async move {
        poller::start_polling(
            poller_db,
            poller_riot,
            http,
            poller_image_gen,
            polling_interval,
            poller_metrics,
            shutdown_rx,
        )
        .await;
    });

    tracing::info!("🔄 Match poller spawned");

    let shard_manager = client.shard_manager.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        tracing::info!("🛑 Shutdown signal received; stopping gracefully");
        let _ = shutdown_tx.send(true);
        shard_manager.shutdown_all().await;
    });

    tracing::info!("🎮 Starting Discord bot...");
    client.start().await?;

    tracing::info!("🛑 Discord client stopped; draining poller");
    if tokio::time::timeout(SHUTDOWN_DRAIN, poller_handle)
        .await
        .is_err()
    {
        tracing::warn!(
            timeout_s = SHUTDOWN_DRAIN.as_secs(),
            "🛑 ⚠️ Poller did not drain in time; closing anyway"
        );
    }
    pool.close().await;
    tracing::info!("👋 Shutdown complete");

    Ok(())
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) {
            (Ok(mut term), Ok(mut int)) => {
                tokio::select! {
                    _ = term.recv() => {}
                    _ = int.recv() => {}
                }
            }
            _ => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
