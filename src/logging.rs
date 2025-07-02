//! Simple tracing subscriber setup used by the application.

use std::{env, sync::OnceLock};

use tracing_appender::{
    non_blocking,
    non_blocking::NonBlocking,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    EnvFilter,
    fmt::{fmt, time::ChronoLocal, writer::MakeWriterExt},
};

/// Guard to ensure buffered logs are flushed on shutdown.
static LOG_GUARD: OnceLock<non_blocking::WorkerGuard> = OnceLock::new();

pub fn init() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let builder = fmt()
        .with_env_filter(env_filter)
        .with_timer(ChronoLocal::new("%Y-%m-%d %H:%M:%S".to_string()))
        .with_target(false)
        .with_ansi(true)
        .with_level(true);

    if let Ok(dir) = env::var("LOG_DIR") {
        let stdout = std::io::stdout.with_max_level(tracing::Level::INFO);
        let writer = stdout.and(init_file_writer(dir));

        builder.with_writer(writer).init();
    } else {
        builder.init();
    }

    tracing::info!("logger initialized");
}

fn init_file_writer(dir: String) -> NonBlocking {
    let max_files = env::var("LOG_MAX_FILES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok());

    let mut file_builder = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("tentrackule.log");

    if let Some(n) = max_files {
        file_builder = file_builder.max_log_files(n);
    }

    let file_appender = file_builder.build(&dir).expect("failed to create log file");

    let (file_writer, guard) = non_blocking(file_appender);

    LOG_GUARD.set(guard).expect("LOG_GUARD already set");

    file_writer
}
