use std::{collections::HashMap, env};

use log::{debug, info};
use migrations::DbMigration;
use poise::serenity_prelude::{ChannelId, GuildId};
use rusqlite::{params, Connection, OptionalExtension};
use tokio::sync::{mpsc, oneshot};
use types::Account;

use crate::riot::types::{AccountDto, LeaguePoints, Region};

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

        debug!("📜 Opening database connection...");
        let path = env::var("DB_PATH").unwrap_or("./".to_string());

        let connection = Connection::open(format!("{}/database.db3", path))
            .expect("Database open successfully.");

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
                DbRequest::SetNewLolSoloDuoLps {
                    puuid,
                    league_points,
                    respond_to,
                } => {
                    let _ = respond_to.send(set_new_lol_solo_duo_lps(
                        &self.connection,
                        puuid,
                        league_points,
                    ));
                }
                DbRequest::GetGuildsForAccount { puuid, respond_to } => {
                    let _ = respond_to.send(get_guilds_for_puuid(&self.connection, puuid));
                }
            }
        }
    }

    /// Initialize the schemas of the database.
    fn init_db(&self) {
        info!("📜 Initializing Database...");

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
        debug!("📜 Running migrations...");
        migrations::V1::do_migration(&self.connection);

        info!("📜 Database initialized.");
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
    SetNewLolSoloDuoLps {
        puuid: String,
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
}

fn track_new_account(
    conn: &Connection,
    account_data: AccountDto,
    region: Region,
    guild_id: GuildId,
) -> rusqlite::Result<()> {
    let guild_id_u64: u64 = guild_id.into();

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

fn set_new_lol_solo_duo_lps(
    conn: &Connection,
    puuid: String,
    league_points: LeaguePoints,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE accounts SET lol_solo_duo_lps = ?1 WHERE puuid = ?2",
        params![league_points, puuid],
    )
    .map(|_| ())
}

fn get_all_accounts(conn: &Connection) -> rusqlite::Result<Vec<Account>> {
    let mut stmt = conn.prepare(
        "SELECT puuid, game_name, tag_line, region, last_match_id, lol_solo_duo_lps FROM accounts",
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
            cached_lol_solo_duo_lps: row.get(5)?,
        })
    })?;

    rows.collect()
}
fn get_guild_accounts(conn: &Connection, guild_id: GuildId) -> rusqlite::Result<Vec<Account>> {
    let guild_id_str = guild_id.to_string(); // Conversion u64 -> String

    let mut stmt = conn.prepare(
        "SELECT a.puuid, a.game_name, a.tag_line, a.region, a.last_match_id, a.lol_solo_duo_lps
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
                let str: String = row.get(3)?;
                str.try_into().unwrap()
            },
            last_match_id: row.get(4)?,
            cached_lol_solo_duo_lps: row.get(5)?,
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
