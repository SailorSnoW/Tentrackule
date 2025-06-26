//! Simple tracing subscriber setup used by the application.

use std::env;

use tracing_appender::{
    non_blocking,
    rolling::{RollingFileAppender, Rotation},
};
use tracing_subscriber::{
    fmt::{fmt, writer::MakeWriterExt},
    EnvFilter,
};

/// Guard to ensure buffered logs are flushed on shutdown.
static mut LOG_GUARD: Option<non_blocking::WorkerGuard> = None;

pub fn init() {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let builder = fmt()
        .with_env_filter(env_filter)
        .without_time()
        .with_target(false)
        .with_ansi(true)
        .with_level(true);

    if let Ok(dir) = env::var("LOG_DIR") {
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

        // Safety: the guard is stored globally to flush logs on exit.
        unsafe {
            LOG_GUARD = Some(guard);
        }

        let stdout = std::io::stdout.with_max_level(tracing::Level::INFO);
        let writer = stdout.and(file_writer);

        builder.with_writer(writer).init();
    } else {
        builder.init();
    }

    tracing::info!("logger initialized");
}
