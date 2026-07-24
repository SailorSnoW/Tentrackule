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

/// How long to wait for the poller to finish its in-flight cycle on shutdown
/// before closing the pool anyway. Kept short so it fits inside a deploy's
/// SIGTERM→SIGKILL window (Fly's default `kill_timeout` is 5 s).
const SHUTDOWN_DRAIN: Duration = Duration::from_secs(4);

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

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "🦑 Starting Tentrackule"
    );

    // Load configuration
    let config = Config::from_env()?;
    tracing::info!("⚙️ Configuration loaded");

    // Shared runtime metrics, emitted as an hourly stats line (Phase 6). Created
    // before the clients so their API/cache/render activity is counted from boot.
    let metrics = Arc::new(Metrics::new());
    tokio::spawn(Arc::clone(&metrics).run_reporter());

    // Initialize database
    // WAL lets the poller's writes and the slash commands' reads proceed
    // concurrently; sqlx doesn't set a journal mode by default, so the fallback
    // rollback journal would briefly block readers on every write.
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

    // Initialize Riot API client
    let riot_client = RiotClient::new(
        config.riot_api_key.clone(),
        config.riot_rate_limit_per_second,
        config.riot_rate_limit_per_two_min,
        Arc::clone(&metrics),
    )?;
    tracing::info!("🔷 Riot API client initialized");

    // Initialize image generator. The DDragon patch is auto-resolved at boot
    // (unless DDRAGON_VERSION pins it), so read it back for the log line.
    let image_gen = Arc::new(
        ImageGenerator::new(
            config.ddragon_version_override.clone(),
            config.renderer_url.clone(),
            config.renderer_token.clone(),
            Arc::clone(&metrics),
        )
        .await?,
    );
    tracing::info!(
        version = %image_gen.ddragon_version(),
        renderer = config.renderer_url.is_some(),
        "🖼️ Image generator initialized"
    );

    // Keep the DDragon tables current: re-resolve the live patch every 24h and
    // hot-swap them, so a champion released mid-run doesn't 404 (no redeploy).
    tokio::spawn(Arc::clone(&image_gen).run_ddragon_refresh());
    tracing::info!("🖼️ DDragon refresh task spawned");

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

    // Graceful-shutdown plumbing (Phase 6): a watch channel tells the poller to
    // stop, and its JoinHandle lets us drain the in-flight cycle before closing
    // the DB pool.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Spawn match poller in background
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

    // On SIGTERM (Fly deploy) / SIGINT (Ctrl-C): tell the poller to stop, then
    // shut the shards down so `client.start()` below returns and we close cleanly.
    let shard_manager = client.shard_manager.clone();
    tokio::spawn(async move {
        wait_for_shutdown_signal().await;
        tracing::info!("🛑 Shutdown signal received; stopping gracefully");
        let _ = shutdown_tx.send(true);
        shard_manager.shutdown_all().await;
    });

    // Start the bot (blocks until the shard manager is shut down).
    tracing::info!("🎮 Starting Discord bot...");
    client.start().await?;

    // Client stopped → let the poller finish its in-flight cycle (bounded), then
    // close the pool so SQLite checkpoints the WAL cleanly instead of relying on
    // recovery after a hard kill.
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

/// Resolve on the first shutdown signal: SIGTERM (Fly deploy) or SIGINT (Ctrl-C)
/// on Unix, Ctrl-C elsewhere. If the Unix handlers can't be installed, fall back
/// to Ctrl-C alone rather than never resolving.
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
