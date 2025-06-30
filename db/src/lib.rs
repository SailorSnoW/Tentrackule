//! Database API definition and SQLite based storage layer used by the bot.
//!
//! This crate defines the [`Database`] type and helpers to interact with it
//! asynchronously.

use std::{collections::HashMap, env, error::Error, path::Path, sync::Arc};

use async_trait::async_trait;
use migrations::DbMigration;
use poise::serenity_prelude::{ChannelId, GuildId};
use rusqlite::{params, Connection, OptionalExtension};
use tentrackule_types::{
    traits::{
        CachedAccountGuildSource, CachedAccountSource, CachedLeagueSource, CachedSettingSource,
        CachedSourceError,
    },
    Account, CachedLeague, QueueType,
};
use tokio::sync::Mutex;
use tracing::{debug, info};

mod migrations;

/// Thread-safe wrapper around a SQLite database connection used across async tasks.
#[derive(Debug, Clone)]
pub struct SharedDatabase(Arc<Mutex<Connection>>);

#[async_trait]
impl CachedSettingSource for SharedDatabase {
    async fn set_alert_channel(
        &self,
        guild_id: GuildId,
        channel_id: ChannelId,
    ) -> Result<(), CachedSourceError> {
        let guild_id_u64: u64 = guild_id.into();
        let channel_id_u64: u64 = channel_id.into();

        let db = self.0.lock().await;

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

        let db = self.0.lock().await;

        let maybe_channel_id_u64: Option<u64> = db
            .query_row(
                "SELECT alert_channel_id FROM guild_settings WHERE guild_id = ?",
                [guild_id_u64],
                |row| row.get(0),
            )
            .optional()?;

        Ok(maybe_channel_id_u64.map(Into::into))
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

        let mut db = self.0.lock().await;

        let tx = db.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO guild_settings (guild_id) VALUES (?1)",
            [guild_id_u64],
        )?;

        tx.execute(
            "INSERT INTO accounts (puuid, game_name, tag_line, region, last_match_id)\n                VALUES (?1, ?2, ?3, ?4, '')\n                ON CONFLICT(puuid) DO UPDATE SET\n                    game_name = excluded.game_name,\n                    tag_line = excluded.tag_line,\n                    region = excluded.region",
            [
                account.puuid.clone(),
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

        let db = self.0.lock().await;

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
            db.execute(
                "DELETE FROM league_points WHERE puuid = ?1",
                [puuid.clone()],
            )?;
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
        let db = self.0.lock().await;

        db.execute(
            "UPDATE accounts SET last_match_id = ?1 WHERE puuid = ?2",
            [match_id, puuid],
        )?;
        Ok(())
    }

    /// Get all accounts from the cache.
    async fn get_all_accounts(&self) -> Result<Vec<Account>, CachedSourceError> {
        let db = self.0.lock().await;

        let mut stmt =
            db.prepare("SELECT puuid, game_name, tag_line, region, last_match_id FROM accounts")?;

        let rows = stmt.query_map([], |row| {
            Ok(Account {
                puuid: row.get(0)?,
                game_name: row.get(1)?,
                tag_line: row.get(2)?,
                region: {
                    let s: String = row.get(3)?;
                    s.try_into().unwrap()
                },
                last_match_id: row.get(4)?,
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
        let db = self.0.lock().await;

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

        let db = self.0.lock().await;

        let mut stmt = db.prepare(
            "SELECT a.puuid, a.game_name, a.tag_line, a.region, a.last_match_id
            FROM accounts a
            INNER JOIN account_guilds ag ON a.puuid = ag.puuid
            WHERE ag.guild_id = ?",
        )?;

        let rows = stmt.query_map(params![guild_id_str], |row| {
            Ok(Account {
                puuid: row.get(0)?,
                game_name: row.get(1)?,
                tag_line: row.get(2)?,
                region: {
                    let s: String = row.get(3)?;
                    s.try_into().unwrap()
                },
                last_match_id: row.get(4)?,
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
        queue_type: QueueType,
    ) -> Result<Option<CachedLeague>, Box<dyn Error + Send + Sync>> {
        let db = self.0.lock().await;

        db.query_row(
            "SELECT points, wins, losses FROM leagues WHERE puuid = ?1 AND queue_type = ?2",
            params![puuid, queue_type.as_str()],
            |row| {
                Ok(CachedLeague {
                    points: row.get(0)?,
                    wins: row.get(1)?,
                    losses: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)
    }

    async fn set_league_for(
        &self,
        puuid: String,
        queue_type: QueueType,
        league: CachedLeague,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let db = self.0.lock().await;

        db.execute(
            "INSERT OR REPLACE INTO leagues (puuid, queue_type, points, wins, losses) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![puuid, queue_type.as_str(), league.points, league.wins, league.losses],
        )?;
        Ok(())
    }
}

impl SharedDatabase {
    /// Create a new database at the given path.
    pub fn new(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        Ok(SharedDatabase::from_connection(conn))
    }

    /// Create a new database from the given connection and initialize schema.
    pub fn from_connection(conn: Connection) -> Self {
        info!("opening SQLite connection");
        SharedDatabase(Arc::new(Mutex::new(conn)))
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
        info!("initializing schema");

        let db = self.0.lock().await;

        // Create tables only if they not exists
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
            FOREIGN KEY (puuid) REFERENCES accounts(puuid)
            FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
        )",
            [],
        )
        .unwrap();

        // Run Migrations
        debug!("running migrations");
        migrations::V1::do_migration(&db);
        migrations::V2::do_migration(&db);
        migrations::V3::do_migration(&db);

        info!("database ready");
    }
}
