use rusqlite::Connection;
use tracing::info;

use super::DbMigration;

/// Use account `id` as primary identifier and foreign key across related tables.
pub struct V8;

impl DbMigration for V8 {
    fn do_migration(conn: &Connection) {
        // Update accounts table to use id as primary key if not already
        let mut stmt = conn.prepare("PRAGMA table_info(accounts)").unwrap();
        let cols: Vec<(String, i64)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, i64>(5)?))
            })
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let mut id_pk = false;
        for (name, pk) in &cols {
            if name == "id" {
                id_pk = *pk != 0;
            }
        }
        if !id_pk {
            info!("migrating accounts primary key to id");
            conn.execute_batch(
                "PRAGMA foreign_keys = OFF;
                 CREATE TABLE accounts_new (
                     id TEXT PRIMARY KEY,
                     puuid TEXT UNIQUE,
                     puuid_tft TEXT,
                     game_name TEXT NOT NULL,
                     tag_line TEXT NOT NULL,
                     region TEXT NOT NULL,
                     last_match_id TEXT NOT NULL
                 );
                 INSERT INTO accounts_new (id, puuid, puuid_tft, game_name, tag_line, region, last_match_id)
                     SELECT id, puuid, puuid_tft, game_name, tag_line, region, last_match_id FROM accounts;
                 DROP TABLE accounts;
                 ALTER TABLE accounts_new RENAME TO accounts;
                 PRAGMA foreign_keys = ON;"
            ).unwrap();
        }

        // account_guilds
        let mut stmt = conn.prepare("PRAGMA table_info(account_guilds)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let recreate_account_guilds = !columns.contains(&"account_id".to_string());
        if recreate_account_guilds {
            info!("migrating 'account_guilds' to use account_id");
            conn.execute_batch(
                "PRAGMA foreign_keys = OFF;
                 CREATE TABLE account_guilds_new (
                     account_id TEXT NOT NULL,
                     guild_id INTEGER NOT NULL,
                     PRIMARY KEY (account_id, guild_id),
                     FOREIGN KEY (account_id) REFERENCES accounts(id),
                     FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
                 );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO account_guilds_new (account_id, guild_id)
                 SELECT accounts.id, guild_id FROM account_guilds
                 JOIN accounts ON accounts.puuid = account_guilds.puuid",
                [],
            )
            .unwrap();
            conn.execute_batch(
                "DROP TABLE account_guilds;
                 ALTER TABLE account_guilds_new RENAME TO account_guilds;
                 PRAGMA foreign_keys = ON;",
            )
            .unwrap();
        }

        // leagues
        let mut stmt = conn.prepare("PRAGMA table_info(leagues)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let recreate_leagues = !columns.contains(&"account_id".to_string());
        if recreate_leagues {
            info!("migrating 'leagues' to use account_id");
            conn.execute_batch(
                "PRAGMA foreign_keys = OFF;
                 CREATE TABLE leagues_new (
                     account_id TEXT NOT NULL,
                     queue_type TEXT NOT NULL,
                     points INTEGER,
                     wins INTEGER,
                     losses INTEGER,
                     rank TEXT,
                     tier TEXT,
                     PRIMARY KEY (account_id, queue_type),
                     FOREIGN KEY (account_id) REFERENCES accounts(id)
                 );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO leagues_new (account_id, queue_type, points, wins, losses, rank, tier)
                 SELECT accounts.id, queue_type, points, wins, losses, rank, tier
                 FROM leagues JOIN accounts ON accounts.puuid = leagues.puuid",
                [],
            )
            .unwrap();
            conn.execute_batch(
                "DROP TABLE leagues;
                 ALTER TABLE leagues_new RENAME TO leagues;
                 PRAGMA foreign_keys = ON;",
            )
            .unwrap();
        }

        // league_points
        let mut stmt = conn.prepare("PRAGMA table_info(league_points)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let recreate_league_points = !columns.contains(&"account_id".to_string());
        if recreate_league_points {
            info!("migrating 'league_points' to use account_id");
            conn.execute_batch(
                "PRAGMA foreign_keys = OFF;
                 CREATE TABLE league_points_new (
                     account_id TEXT NOT NULL,
                     queue_type TEXT NOT NULL,
                     league_points INTEGER,
                     PRIMARY KEY (account_id, queue_type),
                     FOREIGN KEY (account_id) REFERENCES accounts(id)
                 );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO league_points_new (account_id, queue_type, league_points)
                 SELECT accounts.id, queue_type, league_points
                 FROM league_points JOIN accounts ON accounts.puuid = league_points.puuid",
                [],
            )
            .unwrap();
            conn.execute_batch(
                "DROP TABLE league_points;
                 ALTER TABLE league_points_new RENAME TO league_points;
                 PRAGMA foreign_keys = ON;",
            )
            .unwrap();
        }
    }
}
