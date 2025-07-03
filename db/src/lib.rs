//! Database API definition and SQLite based storage layer used by the bot.
//!
//! This crate defines the [`Database`] type and helpers to interact with it
//! asynchronously.

use std::{collections::HashMap, env, error::Error, path::Path, sync::Arc};

use async_trait::async_trait;
use migrations::DbMigration;
use poise::serenity_prelude::{ChannelId, GuildId};
use rusqlite::{Connection, OptionalExtension, params};
use tentrackule_shared::{
    Account, League,
    traits::{
        CacheFull, CachedAccountGuildSource, CachedAccountSource, CachedLeagueSource,
        CachedSettingSource, CachedSourceError, QueueKind,
    },
};
use tokio::sync::{Mutex, OnceCell};
use tracing::{debug, info};

mod migrations;

/// Thread-safe wrapper around a SQLite database connection used across async tasks.
#[derive(Debug, Clone)]
pub struct SharedDatabase {
    conn: Arc<Mutex<Connection>>,
    init_once: Arc<OnceCell<()>>,
}

#[async_trait]
impl CachedSettingSource for SharedDatabase {
    async fn set_alert_channel(
        &self,
        guild_id: GuildId,
        channel_id: ChannelId,
    ) -> Result<(), CachedSourceError> {
        let guild_id_u64: u64 = guild_id.into();
        let channel_id_u64: u64 = channel_id.into();

        let db = self.conn.lock().await;

        db.execute(
            "INSERT OR REPLACE INTO guild_settings
            (guild_id, alert_channel_id) VALUES (?1, ?2)",
            [guild_id_u64, channel_id_u64],
        )?;
        Ok(())
    }

    async fn get_alert_channel(
        &self,
        guild_id: GuildId,
    ) -> Result<Option<ChannelId>, CachedSourceError> {
        let guild_id_u64: u64 = guild_id.into();

        let db = self.conn.lock().await;

        let maybe_channel_id_u64: Option<u64> = db
            .query_row(
                "SELECT alert_channel_id FROM guild_settings WHERE guild_id = ?",
                [guild_id_u64],
                |row| row.get(0),
            )
            .optional()?;

        Ok(maybe_channel_id_u64.map(Into::into))
    }

    async fn set_queue_alert_enabled(
        &self,
        guild_id: GuildId,
        queue_type: &dyn QueueKind,
        enabled: bool,
    ) -> Result<(), CachedSourceError> {
        let guild_id_u64: u64 = guild_id.into();
        let enabled_i64: i64 = if enabled { 1 } else { 0 };

        let db = self.conn.lock().await;

        db.execute(
            "INSERT OR REPLACE INTO queue_alert_settings (guild_id, queue_type, enabled) VALUES (?1, ?2, ?3)",
            params![guild_id_u64, queue_type.to_string(), enabled_i64],
        )?;
        Ok(())
    }

    async fn is_queue_alert_enabled(
        &self,
        guild_id: GuildId,
        queue_type: &dyn QueueKind,
    ) -> Result<bool, CachedSourceError> {
        let guild_id_u64: u64 = guild_id.into();

        let db = self.conn.lock().await;

        let maybe_enabled: Option<i64> = db
            .query_row(
                "SELECT enabled FROM queue_alert_settings WHERE guild_id = ?1 AND queue_type = ?2",
                params![guild_id_u64, queue_type.to_string()],
                |row| row.get(0),
            )
            .optional()?;

        Ok(maybe_enabled.map(|v| v != 0).unwrap_or(true))
    }
}

#[async_trait]
impl CachedAccountSource for SharedDatabase {
    async fn insert_account(
        &self,
        account: Account,
        guild_id: GuildId,
    ) -> Result<(), CachedSourceError> {
        let guild_id_u64: u64 = guild_id.into();

        let mut db = self.conn.lock().await;

        let tx = db.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO guild_settings (guild_id) VALUES (?1)",
            [guild_id_u64],
        )?;

        tx.execute(
            "INSERT INTO accounts (puuid, puuid_tft, game_name, tag_line, region, last_match_id)\n                VALUES (?1, ?2, ?3, ?4, ?5, '')\n                ON CONFLICT(puuid) DO UPDATE SET\n                    puuid_tft = excluded.puuid_tft,\n                    game_name = excluded.game_name,\n                    tag_line = excluded.tag_line,\n                    region = excluded.region",
            [
                account.puuid.clone(),
                account.puuid_tft.clone(),
                account.game_name,
                account.tag_line,
                account.region.into(),
            ],
        )?;

        tx.execute(
            "INSERT OR IGNORE INTO account_guilds (puuid, guild_id) VALUES (?1, ?2)",
            params![account.puuid, guild_id_u64],
        )?;

        tx.commit().map_err(|e| e.into())
    }

    async fn remove_account(
        &self,
        puuid: String,
        guild_id: GuildId,
    ) -> Result<(), CachedSourceError> {
        let guild_id_u64: u64 = guild_id.into();

        let db = self.conn.lock().await;

        db.execute(
            "DELETE FROM account_guilds WHERE puuid = ?1 AND guild_id = ?2",
            params![puuid, guild_id_u64],
        )?;

        let remaining: i64 = db.query_row(
            "SELECT COUNT(*) FROM account_guilds WHERE puuid = ?1",
            [puuid.clone()],
            |row| row.get(0),
        )?;

        if remaining == 0 {
            db.execute("DELETE FROM leagues WHERE puuid = ?1", [puuid.clone()])?;
            db.execute("DELETE FROM accounts WHERE puuid = ?1", [puuid])?;
        }

        Ok(())
    }

    async fn set_last_match_id(
        &self,
        puuid: String,
        match_id: String,
    ) -> Result<(), CachedSourceError> {
        let db = self.conn.lock().await;

        db.execute(
            "UPDATE accounts SET last_match_id = ?1 WHERE puuid = ?2",
            [match_id, puuid],
        )?;
        Ok(())
    }

    /// Get all accounts from the cache.
    async fn get_all_accounts(&self) -> Result<Vec<Account>, CachedSourceError> {
        let db = self.conn.lock().await;

        let mut stmt = db.prepare(
            "SELECT puuid, puuid_tft, game_name, tag_line, region, last_match_id FROM accounts",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(Account {
                puuid: row.get(0)?,
                puuid_tft: row.get(1)?,
                game_name: row.get(2)?,
                tag_line: row.get(3)?,
                region: {
                    let s: String = row.get(4)?;
                    s.try_into().unwrap()
                },
                last_match_id: row.get(5)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }
}

#[async_trait]
impl CachedAccountGuildSource for SharedDatabase {
    async fn get_guilds_for(
        &self,
        puuid: String,
    ) -> Result<HashMap<GuildId, Option<ChannelId>>, CachedSourceError> {
        let db = self.conn.lock().await;

        let mut stmt = db.prepare(
            "SELECT gs.guild_id, gs.alert_channel_id
            FROM account_guilds ag
            LEFT JOIN guild_settings gs ON ag.guild_id = gs.guild_id
            WHERE ag.puuid = ?",
        )?;

        let rows = stmt.query_map([puuid], |row| {
            let guild_id: u64 = row.get(0)?;
            let alert_channel_id: Option<u64> = row.get(1)?;
            Ok((guild_id, alert_channel_id))
        })?;

        let mut result = HashMap::new();
        for row in rows {
            let (guild_id, alert_channel) = row?;
            result.insert(guild_id.into(), alert_channel.map(Into::into));
        }

        Ok(result)
    }

    async fn get_accounts_for(&self, guild_id: GuildId) -> Result<Vec<Account>, CachedSourceError> {
        let guild_id_str = guild_id.to_string();

        let db = self.conn.lock().await;

        let mut stmt = db.prepare(
            "SELECT a.puuid, a.puuid_tft, a.game_name, a.tag_line, a.region, a.last_match_id
            FROM accounts a
            INNER JOIN account_guilds ag ON a.puuid = ag.puuid
            WHERE ag.guild_id = ?",
        )?;

        let rows = stmt.query_map(params![guild_id_str], |row| {
            Ok(Account {
                puuid: row.get(0)?,
                puuid_tft: row.get(1)?,
                game_name: row.get(2)?,
                tag_line: row.get(3)?,
                region: {
                    let s: String = row.get(4)?;
                    s.try_into().unwrap()
                },
                last_match_id: row.get(5)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }
}

#[async_trait]
impl CachedLeagueSource for SharedDatabase {
    async fn get_league_for(
        &self,
        puuid: String,
        queue_type: &dyn QueueKind,
    ) -> Result<Option<League>, Box<dyn Error + Send + Sync>> {
        let db = self.conn.lock().await;

        db.query_row(
            "SELECT points, rank, tier, wins, losses, queue_type FROM leagues WHERE puuid = ?1 AND queue_type = ?2",
            params![puuid, queue_type.to_string()],
            |row| {
                let rank: Option<String> = row.get(1)?;
                let tier: Option<String> = row.get(2)?;
                Ok(League {
                    league_points: row.get(0)?,
                    rank: rank.unwrap_or_default(),
                    tier: tier.unwrap_or_default(),
                    wins: row.get(3)?,
                    losses: row.get(4)?,
                    queue_type: row.get(5)?
                })
            },
        )
        .optional()
        .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }

    async fn set_league_for(
        &self,
        puuid: String,
        league: League,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let db = self.conn.lock().await;

        db.execute(
            "INSERT OR REPLACE INTO leagues (puuid, queue_type, points, wins, losses, rank, tier) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![puuid, league.queue_type.as_str(), league.league_points, league.wins, league.losses, league.rank, league.tier],
        )?;
        Ok(())
    }
}

impl CacheFull for SharedDatabase {}

impl SharedDatabase {
    /// Create a new database at the given path.
    pub fn new(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        Ok(SharedDatabase::from_connection(conn))
    }

    /// Create a new database from the given connection and initialize schema.
    pub fn from_connection(conn: Connection) -> Self {
        info!("opening SQLite connection");
        Self {
            conn: Arc::new(Mutex::new(conn)),
            init_once: Arc::new(OnceCell::new()),
        }
    }

    /// Create a new database using the `DB_PATH` environment variable.
    pub fn new_from_env() -> rusqlite::Result<Self> {
        let db_dir = env::var("DB_PATH").unwrap_or_else(|_| "./".to_string());

        // Expand '~' to the user's home directory
        let db_dir = if db_dir == "~" || db_dir.starts_with("~/") {
            if let Ok(home) = env::var("HOME") {
                format!("{}{}", home, &db_dir[1..])
            } else {
                db_dir
            }
        } else {
            db_dir
        };

        let mut db_path = std::path::PathBuf::from(db_dir);
        db_path.push("database.db3");
        Self::new(db_path)
    }

    /// Initialize the schemas of the database.
    pub async fn init(&self) {
        let _ = self
            .init_once
            .get_or_init(|| async {
                info!("initializing schema");

                let db = self.conn.lock().await;

                db.execute(
                    "CREATE TABLE IF NOT EXISTS guild_settings (
                        guild_id INTEGER PRIMARY KEY,
                        alert_channel_id INTEGER
                    )",
                    [],
                )
                .unwrap();

                db.execute(
                    "CREATE TABLE IF NOT EXISTS accounts (
                        puuid TEXT PRIMARY KEY,
                        puuid_tft TEXT NOT NULL,
                        game_name TEXT NOT NULL,
                        tag_line TEXT NOT NULL,
                        region TEXT NOT NULL,
                        last_match_id TEXT NOT NULL
                    )",
                    [],
                )
                .unwrap();

                db.execute(
                    "CREATE TABLE IF NOT EXISTS account_guilds (
                        puuid TEXT,
                        guild_id INTEGER,
                        PRIMARY KEY (puuid, guild_id),
                        FOREIGN KEY (puuid) REFERENCES accounts(puuid),
                        FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
                    )",
                    [],
                )
                .unwrap();

                debug!("running migrations");
                migrations::V2::do_migration(&db);
                migrations::V3::do_migration(&db);
                migrations::V4::do_migration(&db);
                migrations::V5::do_migration(&db);
                migrations::V6::do_migration(&db);

                info!("database ready");
            })
            .await;
    }
}
