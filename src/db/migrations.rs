use sqlx::SqlitePool;
use tracing::info;

use crate::error::AppError;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS players (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    puuid TEXT UNIQUE NOT NULL,
    game_name TEXT NOT NULL,
    tag_line TEXT NOT NULL,
    region TEXT NOT NULL,
    profile_icon_id INTEGER,
    last_match_id TEXT,
    last_rank_solo_tier TEXT,
    last_rank_solo_rank TEXT,
    last_rank_solo_lp INTEGER,
    last_rank_flex_tier TEXT,
    last_rank_flex_rank TEXT,
    last_rank_flex_lp INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS guilds (
    id INTEGER PRIMARY KEY,
    alert_channel_id INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE IF NOT EXISTS guild_players (
    guild_id INTEGER NOT NULL,
    player_id INTEGER NOT NULL,
    added_by INTEGER NOT NULL,
    added_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (guild_id, player_id),
    FOREIGN KEY (guild_id) REFERENCES guilds(id) ON DELETE CASCADE,
    FOREIGN KEY (player_id) REFERENCES players(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_players_puuid ON players(puuid);
CREATE INDEX IF NOT EXISTS idx_guild_players_guild ON guild_players(guild_id);
"#;

pub async fn run_migrations(pool: &SqlitePool) -> Result<(), AppError> {
    sqlx::raw_sql(SCHEMA).execute(pool).await?;
    info!("🗄️ Database migrations completed");
    Ok(())
}
