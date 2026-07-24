use std::env;
use std::fmt::Display;
use std::num::NonZeroU32;
use std::str::FromStr;

use tracing::warn;

use crate::error::AppError;

/// User agent sent on all outbound HTTP (Riot API, DDragon, renderer), derived
/// from the crate version so it can never drift from `Cargo.toml`.
pub const USER_AGENT: &str = concat!("Tentrackule/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone)]
pub struct Config {
    pub discord_token: String,
    pub riot_api_key: String,
    pub database_url: String,
    pub polling_interval_secs: u64,
    pub riot_rate_limit_per_second: NonZeroU32,
    /// Second Riot rate-limit window. Personal keys allow 20 req/s *and* 100
    /// req/2 min; the per-second quota alone doesn't cover the slower window.
    pub riot_rate_limit_per_two_min: NonZeroU32,
    /// Emergency override for the Data Dragon patch. Normally unset: the version
    /// is auto-resolved from `versions.json` at boot and on the periodic refresh
    /// (Phase 5). Set it only to pin a specific patch when auto-resolution must
    /// be forced (e.g. a fresh patch broke assets).
    pub ddragon_version_override: Option<String>,
    /// Base URL of the Browserless HTML→PNG renderer, e.g.
    /// `http://tentrackule-renderer.internal:3000`. When unset, the bot falls
    /// back to a text embed instead of a rendered card (see `image_gen`).
    pub renderer_url: Option<String>,
    /// Auth token for the renderer (`?token=…`). Paired with `renderer_url`.
    pub renderer_token: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self, AppError> {
        dotenvy::dotenv().ok();

        const DEFAULT_POLLING_INTERVAL_SECS: u64 = 60;
        const DEFAULT_RIOT_RATE_LIMIT_PER_SECOND: u32 = 20;
        const DEFAULT_RIOT_RATE_LIMIT_PER_TWO_MIN: u32 = 100;

        let discord_token = env::var("DISCORD_TOKEN")
            .map_err(|_| AppError::Config("DISCORD_TOKEN must be set".into()))?;

        let riot_api_key = env::var("RIOT_API_KEY")
            .map_err(|_| AppError::Config("RIOT_API_KEY must be set".into()))?;

        let database_url =
            env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:tentrackule.db".into());

        let polling_interval_secs =
            parse_env("POLLING_INTERVAL_SECS", DEFAULT_POLLING_INTERVAL_SECS);

        let nonzero = |n: u32| NonZeroU32::new(n).unwrap_or(NonZeroU32::MIN);
        let riot_rate_limit_per_second = parse_env(
            "RIOT_RATE_LIMIT_PER_SECOND",
            nonzero(DEFAULT_RIOT_RATE_LIMIT_PER_SECOND),
        );

        let riot_rate_limit_per_two_min = parse_env(
            "RIOT_RATE_LIMIT_PER_TWO_MIN",
            nonzero(DEFAULT_RIOT_RATE_LIMIT_PER_TWO_MIN),
        );

        // Unset by default → the patch is auto-resolved from DDragon at boot.
        // A blank value is treated as unset so an empty `fly.toml` entry doesn't
        // pin an invalid patch.
        let ddragon_version_override = non_empty_env("DDRAGON_VERSION");

        // Optional: only set in prod. Empty strings are treated as unset so a
        // blank `fly.toml` entry doesn't produce a broken URL.
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

/// Read an env var, mapping both "unset" and "" to `None`.
fn non_empty_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|v| !v.trim().is_empty())
}

/// Parse a scalar env var, falling back to `default`. Unset or blank is a normal
/// "not configured" and stays silent; a value that is *present but invalid*
/// (unparseable — for [`NonZeroU32`] this includes `0` and negatives) logs a
/// warning naming the var, the bad value, and the default, instead of the old
/// silent `parse().ok()` swallow (Phase 6, config stricte).
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

    // Each test uses a distinct key so the shared process environment can't race
    // between parallel tests. `set_var`/`remove_var` are `unsafe` in edition 2024.

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
        // The whole point of "config stricte" for the rate limits: a `0` is a
        // parse error for NonZeroU32, so we keep the safe default rather than
        // silently install an unusable limiter.
        unsafe { std::env::set_var("TENTRACKULE_TEST_ZERO", "0") };
        let default = NonZeroU32::new(20).unwrap();
        assert_eq!(parse_env("TENTRACKULE_TEST_ZERO", default), default);
        unsafe { std::env::remove_var("TENTRACKULE_TEST_ZERO") };
    }
}
