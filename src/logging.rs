//! Simple tracing subscriber setup used by the application.

use tracing_subscriber::{fmt, EnvFilter};

pub fn init() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    fmt()
        .with_env_filter(env_filter)
        .without_time()
        .with_target(false)
        .with_ansi(true)
        .with_level(true)
        .init();

    tracing::info!("🔊 [LOG] logger initialized");
}
