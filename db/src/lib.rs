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
    Account, League, Region,
    traits::{
        CacheFull, CachedAccountGuildSource, CachedAccountSource, CachedLeagueSource,
        CachedSettingSource, CachedSourceError, QueueKind,
    },
};
use tokio::sync::{Mutex, OnceCell};
use tracing::{debug, info, instrument};
use uuid::Uuid;

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
    #[instrument("🛢 ", skip_all, fields(op = "insert_account"))]
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
            "INSERT INTO accounts (id, puuid, puuid_tft, game_name, tag_line, region, last_match_id, last_match_id_tft)\n                VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', '')\n            ON CONFLICT(puuid) DO UPDATE SET\n                    puuid_tft = excluded.puuid_tft,\n                    game_name = excluded.game_name,\n                    tag_line = excluded.tag_line,\n                    region = excluded.region",
            [
                account.id.to_string(),
                account.puuid.clone().unwrap_or_default(),
                account.puuid_tft.clone().unwrap_or_default(),
                account.game_name,
                account.tag_line,
                String::from(account.region),
            ],
        )?;

        tx.execute(
            "INSERT OR IGNORE INTO account_guilds (account_id, guild_id) VALUES (?1, ?2)",
            params![account.id.to_string(), guild_id_u64],
        )?;

        tx.commit().map_err(|e| e.into())
    }

    #[instrument("🛢 ", skip_all, fields(op = "remove_account"))]
    async fn remove_account(&self, id: Uuid, guild_id: GuildId) -> Result<(), CachedSourceError> {
        let guild_id_u64: u64 = guild_id.into();

        let db = self.conn.lock().await;

        db.execute(
            "DELETE FROM account_guilds WHERE account_id = ?1 AND guild_id = ?2",
            params![id.to_string(), guild_id_u64],
        )?;

        let remaining: i64 = db.query_row(
            "SELECT COUNT(*) FROM account_guilds WHERE account_id = ?1",
            [id.to_string()],
            |row| row.get(0),
        )?;

        if remaining == 0 {
            db.execute(
                "DELETE FROM leagues WHERE account_id = ?1",
                [id.to_string()],
            )?;
            db.execute("DELETE FROM accounts WHERE id = ?1", [id.to_string()])?;
        }

        Ok(())
    }

    #[instrument("🛢 ", skip_all, fields(op = "set_last_match_id"))]
    async fn set_last_match_id(&self, id: Uuid, match_id: String) -> Result<(), CachedSourceError> {
        let db = self.conn.lock().await;

        db.execute(
            "UPDATE accounts SET last_match_id = ?1 WHERE id = ?2",
            [match_id, id.to_string()],
        )?;
        Ok(())
    }

    #[instrument("🛢 ", skip_all, fields(op = "set_last_match_id_tft"))]
    async fn set_last_match_id_tft(
        &self,
        id: Uuid,
        match_id: String,
    ) -> Result<(), CachedSourceError> {
        let db = self.conn.lock().await;

        db.execute(
            "UPDATE accounts SET last_match_id_tft = ?1 WHERE id = ?2",
            [match_id, id.to_string()],
        )?;
        Ok(())
    }

    /// Get all accounts from the cache.
    #[instrument("🛢 ", skip_all, fields(op = "get_all_accounts"))]
    async fn get_all_accounts(&self) -> Result<Vec<Account>, CachedSourceError> {
        let db = self.conn.lock().await;

        let mut stmt = db.prepare(
            "SELECT id, puuid, puuid_tft, game_name, tag_line, region, last_match_id, last_match_id_tft FROM accounts",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(Account {
                id: Uuid::parse_str(row.get::<_, String>(0)?.as_str()).unwrap(),
                puuid: row.get(1)?,
                puuid_tft: row.get(2)?,
                game_name: row.get(3)?,
                tag_line: row.get(4)?,
                region: {
                    let s: String = row.get(5)?;
                    s.try_into().unwrap()
                },
                last_match_id: row.get(6)?,
                last_match_id_tft: row.get(7)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }

    #[instrument("🛢 ", skip_all, fields(op = "get_account_id"))]
    async fn get_account_id(
        &self,
        game_name: String,
        tag_line: String,
        region: Region,
    ) -> Result<Option<Uuid>, CachedSourceError> {
        let db = self.conn.lock().await;

        let maybe_id: Option<String> = db
            .query_row(
                "SELECT id FROM accounts WHERE game_name = ?1 AND tag_line = ?2 AND region = ?3",
                params![game_name, tag_line, String::from(region)],
                |row| row.get(0),
            )
            .optional()?;

        Ok(maybe_id.and_then(|s| Uuid::parse_str(&s).ok()))
    }

    #[instrument("🛢 ", skip_all, fields(op = "get_account_by_puuid"))]
    async fn get_account_by_puuid(
        &self,
        puuid: String,
    ) -> Result<Option<Account>, CachedSourceError> {
        let db = self.conn.lock().await;

        let mut stmt = db.prepare(
            "SELECT id, puuid, puuid_tft, game_name, tag_line, region, last_match_id, last_match_id_tft FROM accounts WHERE puuid = ?1 OR puuid_tft = ?1",
        )?;

        let account = stmt
            .query_row([puuid], |row| {
                Ok(Account {
                    id: Uuid::parse_str(row.get::<_, String>(0)?.as_str()).unwrap(),
                    puuid: row.get(1)?,
                    puuid_tft: row.get(2)?,
                    game_name: row.get(3)?,
                    tag_line: row.get(4)?,
                    region: {
                        let s: String = row.get(5)?;
                        s.try_into().unwrap()
                    },
                    last_match_id: row.get(6)?,
                    last_match_id_tft: row.get(7)?,
                })
            })
            .optional()?;

        Ok(account)
    }
}

#[async_trait]
impl CachedAccountGuildSource for SharedDatabase {
    #[instrument("🛢 ", skip_all, fields(op = "get_guilds_for"))]
    async fn get_guilds_for(
        &self,
        id: Uuid,
    ) -> Result<HashMap<GuildId, Option<ChannelId>>, CachedSourceError> {
        let db = self.conn.lock().await;

        let mut stmt = db.prepare(
            "SELECT gs.guild_id, gs.alert_channel_id
            FROM account_guilds ag
            LEFT JOIN guild_settings gs ON ag.guild_id = gs.guild_id
            WHERE ag.account_id = ?",
        )?;

        let rows = stmt.query_map([id.to_string()], |row| {
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

    #[instrument("🛢 ", skip_all, fields(op = "get_accounts_for"))]
    async fn get_accounts_for(&self, guild_id: GuildId) -> Result<Vec<Account>, CachedSourceError> {
        let guild_id_str = guild_id.to_string();

        let db = self.conn.lock().await;

        let mut stmt = db.prepare(
            "SELECT a.id, a.puuid, a.puuid_tft, a.game_name, a.tag_line, a.region, a.last_match_id, a.last_match_id_tft
            FROM accounts a
            INNER JOIN account_guilds ag ON a.id = ag.account_id
            WHERE ag.guild_id = ?",
        )?;

        let rows = stmt.query_map(params![guild_id_str], |row| {
            Ok(Account {
                id: Uuid::parse_str(row.get::<_, String>(0)?.as_str()).unwrap(),
                puuid: row.get(1)?,
                puuid_tft: row.get(2)?,
                game_name: row.get(3)?,
                tag_line: row.get(4)?,
                region: {
                    let s: String = row.get(5)?;
                    s.try_into().unwrap()
                },
                last_match_id: row.get(6)?,
                last_match_id_tft: row.get(7)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }
}

#[async_trait]
impl CachedLeagueSource for SharedDatabase {
    #[instrument("🛢 ", skip_all, fields(op = "get_league_for"))]
    async fn get_league_for(
        &self,
        id: Uuid,
        queue_type: &dyn QueueKind,
    ) -> Result<Option<League>, Box<dyn Error + Send + Sync>> {
        let db = self.conn.lock().await;

        db.query_row(
            "SELECT points, rank, tier, wins, losses, queue_type FROM leagues WHERE account_id = ?1 AND queue_type = ?2",
            params![id.to_string(), queue_type.to_string()],
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

    #[instrument("🛢 ", skip_all, fields(op = "set_league_for"))]
    async fn set_league_for(
        &self,
        id: Uuid,
        league: League,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let db = self.conn.lock().await;

        db.execute(
            "INSERT OR REPLACE INTO leagues (account_id, queue_type, points, wins, losses, rank, tier) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id.to_string(), league.queue_type.as_str(), league.league_points, league.wins, league.losses, league.rank, league.tier],
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
    #[instrument("🛢 ", skip_all, fields(op = "open_connection"))]
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
    #[instrument("🛢 ", skip_all, fields(op = "initialization"))]
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
                        last_match_id TEXT NOT NULL,
                        last_match_id_tft TEXT NOT NULL
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
                migrations::V7::do_migration(&db);
                migrations::V8::do_migration(&db);
                migrations::V9::do_migration(&db);

                info!("database ready");
            })
            .await;
    }
}
