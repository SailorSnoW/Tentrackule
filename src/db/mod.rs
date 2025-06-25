use std::{collections::HashMap, env, sync::Arc};

use migrations::DbMigration;
use poise::serenity_prelude::{ChannelId, GuildId};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::Mutex;
use tracing::{debug, info};
use types::Account;

use crate::riot::{
    api::types::AccountDto,
    types::{LeaguePoints, QueueType, Region},
};

pub mod types;

mod migrations;

pub type SharedDatabase = Arc<Mutex<Database>>;

pub trait DatabaseExt {
    async fn run<F, T, E>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&Database) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: Send + 'static;
}

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
}

#[derive(Debug)]
pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new() -> Self {
        info!("💾 [DB] opening SQLite connection");
        let db_dir = env::var("DB_PATH").unwrap_or("./".to_string());

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

        // Handle trailing path separators gracefully
        let mut db_path = std::path::PathBuf::from(db_dir);
        db_path.push("database.db3");

        let connection = Connection::open(db_path).expect("Database open successfully.");

        let db = Self { conn: connection };
        db.init();
        db
    }

    pub fn new_shared() -> SharedDatabase {
        Arc::new(Mutex::new(Self::new()))
    }

    /// Initialize the schemas of the database.
    fn init(&self) {
        info!("📐 [DB] initializing schema");

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
        debug!("⬆️ [DB] running migrations");
        migrations::V1::do_migration(&self.conn);
        migrations::V2::do_migration(&self.conn);

        info!("✅ [DB] database ready");
    }

    pub fn track_new_account(
        &self,
        account_data: AccountDto,
        region: Region,
        guild_id: GuildId,
    ) -> rusqlite::Result<()> {
        let guild_id_u64: u64 = guild_id.into();

        self.conn.execute("BEGIN", [])?;
        let res: rusqlite::Result<()> = (|| {
            self.conn.execute(
                "INSERT OR IGNORE INTO guild_settings (guild_id) VALUES (?1)",
                [guild_id_u64],
            )?;

            self.conn.execute(
                "INSERT INTO accounts (puuid, game_name, tag_line, region, last_match_id)
                VALUES (?1, ?2, ?3, ?4, '')
                ON CONFLICT(puuid) DO UPDATE SET
                    game_name = excluded.game_name,
                    tag_line = excluded.tag_line,
                    region = excluded.region",
                [
                    account_data.puuid.clone(),
                    account_data.game_name.unwrap(),
                    account_data.tag_line.unwrap(),
                    region.into(),
                ],
            )?;

            self.conn.execute(
                "INSERT OR IGNORE INTO account_guilds (puuid, guild_id) VALUES (?1, ?2)",
                params![account_data.puuid, guild_id_u64],
            )?;

            Ok(())
        })();

        match res {
            Ok(_) => {
                self.conn.execute("COMMIT", [])?;
                Ok(())
            }
            Err(e) => {
                self.conn.execute("ROLLBACK", [])?;
                Err(e)
            }
        }
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
        let mut stmt = self.conn.prepare(
            "SELECT a.puuid, a.game_name, a.tag_line, a.region, a.last_match_id, lp.league_points
            FROM accounts a
            LEFT JOIN league_points lp ON a.puuid = lp.puuid AND lp.queue_type = 'RANKED_SOLO_5x5'",
        )?;

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
                cached_league_points: row.get(5)?,
            })
        })?;

        rows.collect()
    }

    pub fn get_guild_accounts(&self, guild_id: GuildId) -> rusqlite::Result<Vec<Account>> {
        let guild_id_str = guild_id.to_string();

        let mut stmt = self.conn.prepare(
            "SELECT a.puuid, a.game_name, a.tag_line, a.region, a.last_match_id, lp.league_points
            FROM accounts a
            INNER JOIN account_guilds ag ON a.puuid = ag.puuid
            LEFT JOIN league_points lp ON a.puuid = lp.puuid AND lp.queue_type = 'RANKED_SOLO_5x5'
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
                cached_league_points: row.get(5)?,
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
    use crate::riot::types::Region;
    use poise::serenity_prelude::{ChannelId, GuildId};

    fn setup_db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        let db = Database { conn };
        db.init();
        db
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
        let db = setup_db();
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
        let db = setup_db();
        db.track_new_account(sample_account(), Region::Euw, GuildId::new(1))
            .unwrap();
        db.set_alert_channel(GuildId::new(1), ChannelId::new(2))
            .unwrap();
        let map = db.get_guilds_for_puuid("puuid".into()).unwrap();
        assert_eq!(map.get(&GuildId::new(1)), Some(&Some(ChannelId::new(2))));
    }

    #[test]
    fn untrack_account_removes_entries() {
        let db = setup_db();
        db.track_new_account(sample_account(), Region::Euw, GuildId::new(1))
            .unwrap();

        db.untrack_account("puuid".into(), GuildId::new(1)).unwrap();

        let accounts = db.get_all_accounts().unwrap();
        assert_eq!(accounts.len(), 0);
    }

    #[test]
    fn retracking_keeps_last_match_id() {
        let db = setup_db();
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
}
