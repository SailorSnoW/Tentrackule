use std::env;
use std::num::NonZeroU32;

use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct Config {
    pub discord_token: String,
    pub riot_api_key: String,
    pub database_url: String,
    pub polling_interval_secs: u64,
    pub riot_rate_limit_per_second: NonZeroU32,
    pub ddragon_version: String,
}

impl Config {
    pub fn from_env() -> Result<Self, AppError> {
        dotenvy::dotenv().ok();

        const DEFAULT_POLLING_INTERVAL_SECS: u64 = 60;
        const DEFAULT_RIOT_RATE_LIMIT_PER_SECOND: u32 = 20;
        const DEFAULT_DDRAGON_VERSION: &str = "16.1.1";

        let discord_token = env::var("DISCORD_TOKEN")
            .map_err(|_| AppError::Config("DISCORD_TOKEN must be set".into()))?;

        let riot_api_key = env::var("RIOT_API_KEY")
            .map_err(|_| AppError::Config("RIOT_API_KEY must be set".into()))?;

        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:tentrackule.db".into());

        let polling_interval_secs = env::var("POLLING_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_POLLING_INTERVAL_SECS);

        let riot_rate_limit_per_second = env::var("RIOT_RATE_LIMIT_PER_SECOND")
            .ok()
            .and_then(|v| v.parse().ok())
            .and_then(NonZeroU32::new)
            .unwrap_or_else(|| {
                NonZeroU32::new(DEFAULT_RIOT_RATE_LIMIT_PER_SECOND)
                    .unwrap_or(NonZeroU32::MIN)
            });

        let ddragon_version =
            env::var("DDRAGON_VERSION").unwrap_or_else(|_| DEFAULT_DDRAGON_VERSION.into());

        Ok(Self {
            discord_token,
            riot_api_key,
            database_url,
            polling_interval_secs,
            riot_rate_limit_per_second,
            ddragon_version,
        })
    }
}
