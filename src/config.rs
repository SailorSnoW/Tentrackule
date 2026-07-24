use std::env;
use std::fmt::Display;
use std::num::NonZeroU32;
use std::str::FromStr;

use tracing::warn;

use crate::error::AppError;

pub const USER_AGENT: &str = concat!("Tentrackule/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone)]
pub struct Config {
    pub discord_token: String,
    pub riot_api_key: String,
    pub database_url: String,
    pub polling_interval_secs: u64,
    pub riot_rate_limit_per_second: NonZeroU32,
    pub riot_rate_limit_per_two_min: NonZeroU32,
    pub ddragon_version_override: Option<String>,
    pub renderer_url: Option<String>,
    pub renderer_token: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, AppError> {
        dotenvy::dotenv().ok();

        const DEFAULT_POLLING_INTERVAL_SECS: u64 = 60;
        const DEFAULT_RIOT_RATE_LIMIT_PER_SECOND: NonZeroU32 = NonZeroU32::new(20).unwrap();
        const DEFAULT_RIOT_RATE_LIMIT_PER_TWO_MIN: NonZeroU32 = NonZeroU32::new(100).unwrap();

        let discord_token = env::var("DISCORD_TOKEN")
            .map_err(|_| AppError::Config("DISCORD_TOKEN must be set".into()))?;

        let riot_api_key = env::var("RIOT_API_KEY")
            .map_err(|_| AppError::Config("RIOT_API_KEY must be set".into()))?;

        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:tentrackule.db".into());

        let polling_interval_secs =
            parse_env("POLLING_INTERVAL_SECS", DEFAULT_POLLING_INTERVAL_SECS);

        let riot_rate_limit_per_second = parse_env(
            "RIOT_RATE_LIMIT_PER_SECOND",
            DEFAULT_RIOT_RATE_LIMIT_PER_SECOND,
        );

        let riot_rate_limit_per_two_min = parse_env(
            "RIOT_RATE_LIMIT_PER_TWO_MIN",
            DEFAULT_RIOT_RATE_LIMIT_PER_TWO_MIN,
        );

        let ddragon_version_override = non_empty_env("DDRAGON_VERSION");

        let renderer_url = non_empty_env("RENDERER_URL");
        let renderer_token = non_empty_env("RENDERER_TOKEN");

        Ok(Self {
            discord_token,
            riot_api_key,
            database_url,
            polling_interval_secs,
            riot_rate_limit_per_second,
            riot_rate_limit_per_two_min,
            ddragon_version_override,
            renderer_url,
            renderer_token,
        })
    }
}

fn non_empty_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn parse_env<T: FromStr + Display + Copy>(key: &str, default: T) -> T {
    match env::var(key) {
        Ok(raw) if !raw.trim().is_empty() => raw.trim().parse::<T>().unwrap_or_else(|_| {
            warn!(
                env = key,
                value = %raw,
                default = %default,
                "⚙️ ⚠️ Invalid env value; falling back to default"
            );
            default
        }),
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_env;
    use std::num::NonZeroU32;

    #[test]
    fn unset_uses_default_silently() {
        assert_eq!(parse_env("TENTRACKULE_TEST_UNSET", 60u64), 60);
    }

    #[test]
    fn valid_value_is_parsed() {
        unsafe { std::env::set_var("TENTRACKULE_TEST_VALID", "120") };
        assert_eq!(parse_env("TENTRACKULE_TEST_VALID", 60u64), 120);
        unsafe { std::env::remove_var("TENTRACKULE_TEST_VALID") };
    }

    #[test]
    fn invalid_value_falls_back_to_default() {
        unsafe { std::env::set_var("TENTRACKULE_TEST_INVALID", "not-a-number") };
        assert_eq!(parse_env("TENTRACKULE_TEST_INVALID", 60u64), 60);
        unsafe { std::env::remove_var("TENTRACKULE_TEST_INVALID") };
    }

    #[test]
    fn blank_value_is_treated_as_unset() {
        unsafe { std::env::set_var("TENTRACKULE_TEST_BLANK", "   ") };
        assert_eq!(parse_env("TENTRACKULE_TEST_BLANK", 60u64), 60);
        unsafe { std::env::remove_var("TENTRACKULE_TEST_BLANK") };
    }

    #[test]
    fn zero_is_invalid_for_nonzero_and_uses_default() {
        unsafe { std::env::set_var("TENTRACKULE_TEST_ZERO", "0") };
        let default = NonZeroU32::new(20).unwrap();
        assert_eq!(parse_env("TENTRACKULE_TEST_ZERO", default), default);
        unsafe { std::env::remove_var("TENTRACKULE_TEST_ZERO") };
    }
}
