//! Database API definition and SQLite based storage layer used by the bot.
//!
//! This crate defines the [`Database`] type and helpers to interact with it
//! asynchronously.

use std::{collections::HashMap, env, path::Path, sync::Arc};

use async_trait::async_trait;
use migrations::DbMigration;
use poise::serenity_prelude::{ChannelId, GuildId};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;
use tracing::{debug, info};
use types::{Account, League};

use tentrackule_riot_api::{
    api::types::AccountDto,
    types::{LeaguePoints, QueueType, Region},
};

pub mod types;

mod migrations;

/// Thread-safe wrapper around [`Database`] used across async tasks.
pub type SharedDatabase = Arc<Mutex<Database>>;

/// Convenience trait to run blocking database operations on a [`SharedDatabase`].
#[async_trait]
pub trait DatabaseExt {
    async fn run<F, T, E>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&Database) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: Send + 'static;

    async fn run_mut<F, T, E>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&mut Database) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: Send + 'static;
}

#[async_trait]
impl DatabaseExt for SharedDatabase {
    async fn run<F, T, E>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&Database) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
    {
        let db = self.clone();
        tokio::task::spawn_blocking(move || {
            let guard = db.blocking_lock();
            f(&guard)
        })
        .await
        .expect("DB task panicked")
    }

    async fn run_mut<F, T, E>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&mut Database) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
    {
        let db = self.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = db.blocking_lock();
            f(&mut guard)
        })
        .await
        .expect("DB task panicked")
    }
}

/// Wrapper around a SQLite connection holding all persistent data.
#[derive(Debug)]
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Create a new database at the given path.
    pub fn new(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        info!("opening SQLite connection");
        let conn = Connection::open(path)?;
        Ok(Self::from_connection(conn))
    }

    /// Create a new database from the given connection and initialize schema.
    pub fn from_connection(conn: Connection) -> Self {
        let db = Self { conn };
        db.init();
        db
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

    /// Create a thread-safe shared database using the `DB_PATH` environment variable.
    pub fn new_shared_from_env() -> SharedDatabase {
        Arc::new(Mutex::new(
            Self::new_from_env().expect("Database open successfully."),
        ))
    }

    /// Initialize the schemas of the database.
    fn init(&self) {
        info!("initializing schema");

        // Create tables only if they not exists
        self.conn
            .execute(
                "CREATE TABLE IF NOT EXISTS guild_settings (
            guild_id INTEGER PRIMARY KEY,
            alert_channel_id INTEGER
        )",
                [],
            )
            .unwrap();
        self.conn
            .execute(
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
        self.conn
            .execute(
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
        migrations::V1::do_migration(&self.conn);
        migrations::V2::do_migration(&self.conn);
        migrations::V3::do_migration(&self.conn);

        info!("database ready");
    }

    pub fn track_new_account(
        &mut self,
        account_data: AccountDto,
        region: Region,
        guild_id: GuildId,
    ) -> rusqlite::Result<()> {
        let guild_id_u64: u64 = guild_id.into();

        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO guild_settings (guild_id) VALUES (?1)",
            [guild_id_u64],
        )?;

        tx.execute(
            "INSERT INTO accounts (puuid, game_name, tag_line, region, last_match_id)\n                VALUES (?1, ?2, ?3, ?4, '')\n                ON CONFLICT(puuid) DO UPDATE SET\n                    game_name = excluded.game_name,\n                    tag_line = excluded.tag_line,\n                    region = excluded.region",
            [
                account_data.puuid.clone(),
                account_data.game_name.unwrap(),
                account_data.tag_line.unwrap(),
                region.into(),
            ],
        )?;

        tx.execute(
            "INSERT OR IGNORE INTO account_guilds (puuid, guild_id) VALUES (?1, ?2)",
            params![account_data.puuid, guild_id_u64],
        )?;

        tx.commit()
    }

    pub fn untrack_account(&self, puuid: String, guild_id: GuildId) -> rusqlite::Result<()> {
        let guild_id_u64: u64 = guild_id.into();

        self.conn.execute(
            "DELETE FROM account_guilds WHERE puuid = ?1 AND guild_id = ?2",
            params![puuid, guild_id_u64],
        )?;

        let remaining: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM account_guilds WHERE puuid = ?1",
            [puuid.clone()],
            |row| row.get(0),
        )?;

        if remaining == 0 {
            self.conn.execute(
                "DELETE FROM league_points WHERE puuid = ?1",
                [puuid.clone()],
            )?;
            self.conn
                .execute("DELETE FROM leagues WHERE puuid = ?1", [puuid.clone()])?;
            self.conn
                .execute("DELETE FROM accounts WHERE puuid = ?1", [puuid])?;
        }

        Ok(())
    }

    pub fn set_alert_channel(
        &self,
        guild_id: GuildId,
        channel_id: ChannelId,
    ) -> rusqlite::Result<()> {
        let guild_id_u64: u64 = guild_id.into();
        let channel_id_u64: u64 = channel_id.into();

        self.conn.execute(
            "INSERT OR REPLACE INTO guild_settings
            (guild_id, alert_channel_id) VALUES (?1, ?2)",
            [guild_id_u64, channel_id_u64],
        )?;
        Ok(())
    }

    pub fn get_alert_channel(&self, guild_id: GuildId) -> rusqlite::Result<Option<ChannelId>> {
        let guild_id_u64: u64 = guild_id.into();

        let maybe_channel_id_u64: Option<u64> = self
            .conn
            .query_row(
                "SELECT alert_channel_id FROM guild_settings WHERE guild_id = ?",
                [guild_id_u64],
                |row| row.get(0),
            )
            .optional()?;

        Ok(maybe_channel_id_u64.map(Into::into))
    }

    pub fn set_last_match_id(&self, puuid: String, match_id: String) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE accounts SET last_match_id = ?1 WHERE puuid = ?2",
            [match_id, puuid],
        )?;
        Ok(())
    }

    pub fn get_league_points(
        &self,
        puuid: String,
        queue_type: QueueType,
    ) -> rusqlite::Result<Option<LeaguePoints>> {
        self.conn
            .query_row(
                "SELECT league_points FROM league_points WHERE puuid = ?1 AND queue_type = ?2",
                params![puuid, queue_type.as_str()],
                |row| row.get(0),
            )
            .optional()
    }

    pub fn update_league(
        &self,
        puuid: String,
        queue_type: QueueType,
        league: League,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO leagues (puuid, queue_type, points, wins, losses) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![puuid, queue_type.as_str(), league.points, league.wins, league.losses],
        )?;
        Ok(())
    }

    pub fn get_league(
        &self,
        puuid: String,
        queue_type: QueueType,
    ) -> rusqlite::Result<Option<League>> {
        self.conn
            .query_row(
                "SELECT points, wins, losses FROM leagues WHERE puuid = ?1 AND queue_type = ?2",
                params![puuid, queue_type.as_str()],
                |row| {
                    Ok(League {
                        points: row.get(0)?,
                        wins: row.get(1)?,
                        losses: row.get(2)?,
                    })
                },
            )
            .optional()
    }

    pub fn update_league_points(
        &self,
        puuid: String,
        queue_type: QueueType,
        league_points: LeaguePoints,
    ) -> rusqlite::Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO league_points (puuid, queue_type, league_points) VALUES (?1, ?2, ?3)",
            params![puuid, queue_type.as_str(), league_points])?;
        Ok(())
    }

    pub fn get_all_accounts(&self) -> rusqlite::Result<Vec<Account>> {
        let mut stmt = self
            .conn
            .prepare("SELECT puuid, game_name, tag_line, region, last_match_id FROM accounts")?;

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

        rows.collect()
    }

    pub fn get_guild_accounts(&self, guild_id: GuildId) -> rusqlite::Result<Vec<Account>> {
        let guild_id_str = guild_id.to_string();

        let mut stmt = self.conn.prepare(
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

        rows.collect()
    }

    pub fn get_guilds_for_puuid(
        &self,
        puuid: String,
    ) -> rusqlite::Result<HashMap<GuildId, Option<ChannelId>>> {
        let mut stmt = self.conn.prepare(
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use poise::serenity_prelude::{ChannelId, GuildId};
    use tentrackule_riot_api::types::Region;
    use types::League;

    fn setup_db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        Database::from_connection(conn)
    }

    fn sample_account() -> AccountDto {
        AccountDto {
            puuid: "puuid".into(),
            game_name: Some("player".into()),
            tag_line: Some("tag".into()),
        }
    }

    #[test]
    fn track_and_fetch_account() {
        let mut db = setup_db();
        db.track_new_account(sample_account(), Region::Euw, GuildId::new(1))
            .unwrap();

        let accounts = db.get_all_accounts().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].game_name, "player");
    }

    #[test]
    fn set_and_get_alert_channel() {
        let db = setup_db();
        db.set_alert_channel(GuildId::new(1), ChannelId::new(2))
            .unwrap();
        let res = db.get_alert_channel(GuildId::new(1)).unwrap();
        assert_eq!(res, Some(ChannelId::new(2)));
    }

    #[test]
    fn get_guilds_for_account() {
        let mut db = setup_db();
        db.track_new_account(sample_account(), Region::Euw, GuildId::new(1))
            .unwrap();
        db.set_alert_channel(GuildId::new(1), ChannelId::new(2))
            .unwrap();
        let map = db.get_guilds_for_puuid("puuid".into()).unwrap();
        assert_eq!(map.get(&GuildId::new(1)), Some(&Some(ChannelId::new(2))));
    }

    #[test]
    fn untrack_account_removes_entries() {
        let mut db = setup_db();
        db.track_new_account(sample_account(), Region::Euw, GuildId::new(1))
            .unwrap();

        db.untrack_account("puuid".into(), GuildId::new(1)).unwrap();

        let accounts = db.get_all_accounts().unwrap();
        assert_eq!(accounts.len(), 0);
    }

    #[test]
    fn retracking_keeps_last_match_id() {
        let mut db = setup_db();
        db.track_new_account(sample_account(), Region::Euw, GuildId::new(1))
            .unwrap();

        db.set_last_match_id("puuid".into(), "match1".into())
            .unwrap();

        let mut updated = sample_account();
        updated.game_name = Some("player2".into());

        db.track_new_account(updated, Region::Eune, GuildId::new(1))
            .unwrap();

        let accounts = db.get_all_accounts().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].last_match_id, "match1");
        assert_eq!(accounts[0].game_name, "player2");
        assert_eq!(accounts[0].region, Region::Eune);
    }

    #[test]
    fn store_and_get_league() {
        let mut db = setup_db();
        db.track_new_account(sample_account(), Region::Euw, GuildId::new(1))
            .unwrap();

        let league = League {
            points: 120,
            wins: 10,
            losses: 5,
        };
        db.update_league("puuid".into(), QueueType::SoloDuo, league.clone())
            .unwrap();

        let fetched = db
            .get_league("puuid".into(), QueueType::SoloDuo)
            .unwrap()
            .unwrap();

        assert_eq!(fetched.points, league.points);
        assert_eq!(fetched.wins, league.wins);
        assert_eq!(fetched.losses, league.losses);
    }
}
