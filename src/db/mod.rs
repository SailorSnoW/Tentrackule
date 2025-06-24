use std::{collections::HashMap, env};

use migrations::DbMigration;
use poise::serenity_prelude::{ChannelId, GuildId};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info};
use types::Account;

use crate::riot::{
    api::types::AccountDto,
    types::{LeaguePoints, QueueType, Region},
};

pub mod types;

mod migrations;

pub type DatabaseTx = mpsc::Sender<DbRequest>;
pub type DatabaseRx = mpsc::Receiver<DbRequest>;

pub struct DatabaseHandler {
    connection: Connection,
    sender: DatabaseTx,
    receiver: DatabaseRx,
}

impl DatabaseHandler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);

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

        Self {
            connection,
            receiver: rx,
            sender: tx,
        }
    }

    pub fn sender_cloned(&self) -> DatabaseTx {
        self.sender.clone()
    }

    pub fn start(self) -> tokio::task::JoinHandle<()> {
        self.init_db();

        tokio::spawn(async move {
            self.run().await;
        })
    }

    async fn run(mut self) {
        while let Some(request) = self.receiver.recv().await {
            match request {
                DbRequest::TrackNewAccount {
                    account_data,
                    guild_id,
                    region,
                    respond_to,
                } => {
                    let _ = respond_to.send(track_new_account(
                        &self.connection,
                        account_data,
                        region,
                        guild_id,
                    ));
                }
                DbRequest::GetAllAccounts { respond_to } => {
                    let _ = respond_to.send(get_all_accounts(&self.connection));
                }
                DbRequest::GetAllAccountsForGuild {
                    guild_id,
                    respond_to,
                } => {
                    let _ = respond_to.send(get_guild_accounts(&self.connection, guild_id));
                }
                DbRequest::GetAlertChannel {
                    guild_id,
                    respond_to,
                } => {
                    let _ = respond_to.send(get_alert_channel(&self.connection, guild_id));
                }
                DbRequest::SetAlertChannel {
                    guild_id,
                    channel_id,
                    respond_to,
                } => {
                    let _ =
                        respond_to.send(set_alert_channel(&self.connection, guild_id, channel_id));
                }
                DbRequest::SetLastMatchId {
                    puuid,
                    match_id,
                    respond_to,
                } => {
                    let _ = respond_to.send(set_last_match_id(&self.connection, puuid, match_id));
                }
                DbRequest::UpdateLeaguePoints {
                    puuid,
                    queue_type,
                    league_points,
                    respond_to,
                } => {
                    let _ = respond_to.send(update_league_points(
                        &self.connection,
                        puuid,
                        queue_type,
                        league_points,
                    ));
                }
                DbRequest::GetGuildsForAccount { puuid, respond_to } => {
                    let _ = respond_to.send(get_guilds_for_puuid(&self.connection, puuid));
                }
                DbRequest::UntrackAccount {
                    puuid,
                    guild_id,
                    respond_to,
                } => {
                    let _ = respond_to.send(untrack_account(&self.connection, puuid, guild_id));
                }
            }
        }
    }

    /// Initialize the schemas of the database.
    fn init_db(&self) {
        info!("📐 [DB] initializing schema");

        // Create tables only if they not exists
        self.connection
            .execute(
                "CREATE TABLE IF NOT EXISTS guild_settings (
            guild_id INTEGER PRIMARY KEY,
            alert_channel_id INTEGER
        )",
                [],
            )
            .unwrap();
        self.connection
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
        self.connection
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
        migrations::V1::do_migration(&self.connection);
        migrations::V2::do_migration(&self.connection);

        info!("✅ [DB] database ready");
    }
}

#[derive(Debug)]
pub enum DbRequest {
    SetAlertChannel {
        guild_id: GuildId,
        channel_id: ChannelId,
        respond_to: oneshot::Sender<rusqlite::Result<()>>,
    },
    GetAlertChannel {
        guild_id: GuildId,
        respond_to: oneshot::Sender<Option<ChannelId>>,
    },
    GetAllAccountsForGuild {
        guild_id: GuildId,
        respond_to: oneshot::Sender<rusqlite::Result<Vec<Account>>>,
    },
    GetAllAccounts {
        respond_to: oneshot::Sender<rusqlite::Result<Vec<Account>>>,
    },
    SetLastMatchId {
        puuid: String,
        match_id: String,
        respond_to: oneshot::Sender<rusqlite::Result<()>>,
    },
    UpdateLeaguePoints {
        puuid: String,
        queue_type: QueueType,
        league_points: LeaguePoints,
        respond_to: oneshot::Sender<rusqlite::Result<()>>,
    },
    GetGuildsForAccount {
        puuid: String,
        respond_to: oneshot::Sender<rusqlite::Result<HashMap<GuildId, Option<ChannelId>>>>,
    },
    TrackNewAccount {
        account_data: AccountDto,
        region: Region,
        guild_id: GuildId,
        respond_to: oneshot::Sender<rusqlite::Result<()>>,
    },
    UntrackAccount {
        puuid: String,
        guild_id: GuildId,
        respond_to: oneshot::Sender<rusqlite::Result<()>>,
    },
}

fn track_new_account(
    conn: &Connection,
    account_data: AccountDto,
    region: Region,
    guild_id: GuildId,
) -> rusqlite::Result<()> {
    let guild_id_u64: u64 = guild_id.into();

    // Ensure the guild exists in guild_settings so the foreign key constraint
    // on account_guilds doesn't fail when tracking a new account before any
    // alert channel has been configured for that guild.
    conn.execute(
        "INSERT OR IGNORE INTO guild_settings (guild_id) VALUES (?1)",
        [guild_id_u64],
    )?;

    conn.execute(
        "INSERT OR REPLACE INTO accounts
        (puuid, game_name, tag_line, region, last_match_id) VALUES (?1, ?2, ?3, ?4, \"\")",
        [
            account_data.puuid.clone(),
            account_data.game_name.unwrap(),
            account_data.tag_line.unwrap(),
            region.into(),
        ],
    )
    .map(|_| ())?;
    conn.execute(
        "INSERT OR IGNORE INTO account_guilds (puuid, guild_id) VALUES (?1, ?2)",
        params![account_data.puuid, guild_id_u64],
    )
    .map(|_| ())
}

fn untrack_account(conn: &Connection, puuid: String, guild_id: GuildId) -> rusqlite::Result<()> {
    let guild_id_u64: u64 = guild_id.into();

    conn.execute(
        "DELETE FROM account_guilds WHERE puuid = ?1 AND guild_id = ?2",
        params![puuid, guild_id_u64],
    )?;

    let remaining: i64 = conn.query_row(
        "SELECT COUNT(*) FROM account_guilds WHERE puuid = ?1",
        [puuid.clone()],
        |row| row.get(0),
    )?;

    // If the player is no longer tracked in any guild, we remove extra informations
    if remaining == 0 {
        conn.execute(
            "DELETE FROM league_points WHERE puuid = ?1",
            [puuid.clone()],
        )?;
        conn.execute("DELETE FROM accounts WHERE puuid = ?1", [puuid])?;
    }

    Ok(())
}

fn set_alert_channel(
    conn: &Connection,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> rusqlite::Result<()> {
    let guild_id_u64: u64 = guild_id.into();
    let channel_id_u64: u64 = channel_id.into();

    conn.execute(
        "INSERT OR REPLACE INTO guild_settings
        (guild_id, alert_channel_id) VALUES (?1, ?2)",
        [guild_id_u64, channel_id_u64],
    )
    .map(|_| ())
}

fn get_alert_channel(conn: &Connection, guild_id: GuildId) -> Option<ChannelId> {
    let guild_id_u64: u64 = guild_id.into();

    let maybe_channel_id_u64: Option<u64> = conn
        .query_row(
            "SELECT alert_channel_id FROM guild_settings WHERE guild_id = ?",
            [guild_id_u64],
            |row| row.get(0),
        )
        .optional()
        .unwrap();

    maybe_channel_id_u64.map(|x| x.into())
}

fn set_last_match_id(conn: &Connection, puuid: String, match_id: String) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE accounts SET last_match_id = ?1 WHERE puuid = ?2",
        [match_id, puuid],
    )
    .map(|_| ())
}

fn update_league_points(
    conn: &Connection,
    puuid: String,
    queue_type: QueueType,
    league_points: LeaguePoints,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO league_points (puuid, queue_type, league_points) VALUES (?1, ?2, ?3)",
        params![puuid, queue_type.as_str(), league_points],
    )
    .map(|_| ())
}

fn get_all_accounts(conn: &Connection) -> rusqlite::Result<Vec<Account>> {
    let mut stmt = conn.prepare(
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
                let str: String = row.get(3)?;
                str.try_into().unwrap()
            },
            last_match_id: row.get(4)?,
            cached_league_points: row.get(5)?,
        })
    })?;

    rows.collect()
}
fn get_guild_accounts(conn: &Connection, guild_id: GuildId) -> rusqlite::Result<Vec<Account>> {
    let guild_id_str = guild_id.to_string(); // Conversion u64 -> String

    let mut stmt = conn.prepare(
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
                let str: String = row.get(3)?;
                str.try_into().unwrap()
            },
            last_match_id: row.get(4)?,
            cached_league_points: row.get(5)?,
        })
    })?;

    rows.collect()
}

pub fn get_guilds_for_puuid(
    conn: &Connection,
    puuid: String,
) -> rusqlite::Result<HashMap<GuildId, Option<ChannelId>>> {
    let mut stmt = conn.prepare(
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
        result.insert(guild_id.into(), alert_channel.map(|x| x.into()));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::riot::types::Region;
    use poise::serenity_prelude::{ChannelId, GuildId};

    fn setup_connection() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE guild_settings (guild_id INTEGER PRIMARY KEY, alert_channel_id INTEGER)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE accounts (puuid TEXT PRIMARY KEY, game_name TEXT NOT NULL, tag_line TEXT NOT NULL, region TEXT NOT NULL, last_match_id TEXT NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute(
            "CREATE TABLE account_guilds (puuid TEXT, guild_id INTEGER, PRIMARY KEY (puuid, guild_id), FOREIGN KEY (puuid) REFERENCES accounts(puuid), FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id))",
            [],
        )
        .unwrap();
        migrations::V1::do_migration(&conn);
        migrations::V2::do_migration(&conn);
        conn
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
        let conn = setup_connection();
        track_new_account(&conn, sample_account(), Region::Euw, GuildId::new(1)).unwrap();

        let accounts = get_all_accounts(&conn).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].game_name, "player");
    }

    #[test]
    fn set_and_get_alert_channel() {
        let conn = setup_connection();
        set_alert_channel(&conn, GuildId::new(1), ChannelId::new(2)).unwrap();
        let res = get_alert_channel(&conn, GuildId::new(1));
        assert_eq!(res, Some(ChannelId::new(2)));
    }

    #[test]
    fn get_guilds_for_account() {
        let conn = setup_connection();
        track_new_account(&conn, sample_account(), Region::Euw, GuildId::new(1)).unwrap();
        set_alert_channel(&conn, GuildId::new(1), ChannelId::new(2)).unwrap();
        let map = get_guilds_for_puuid(&conn, "puuid".into()).unwrap();
        assert_eq!(map.get(&GuildId::new(1)), Some(&Some(ChannelId::new(2))));
    }

    #[test]
    fn untrack_account_removes_entries() {
        let conn = setup_connection();
        track_new_account(&conn, sample_account(), Region::Euw, GuildId::new(1)).unwrap();

        untrack_account(&conn, "puuid".into(), GuildId::new(1)).unwrap();

        let accounts = get_all_accounts(&conn).unwrap();
        assert_eq!(accounts.len(), 0);
    }
}
