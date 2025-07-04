use rusqlite::Connection;
use tracing::info;
use uuid::Uuid;

use super::DbMigration;

/// Add `id` column to the accounts table and populate it.
pub struct V7;

impl DbMigration for V7 {
    fn do_migration(conn: &Connection) {
        let mut stmt = conn.prepare("PRAGMA table_info(accounts)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        if !columns.contains(&"id".to_string()) {
            info!("adding column 'id' to 'accounts'");
            conn.execute("ALTER TABLE accounts ADD COLUMN id TEXT", [])
                .unwrap();

            let mut stmt = conn.prepare("SELECT puuid FROM accounts").unwrap();
            let puuids: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect();

            for puuid in puuids {
                let uuid = Uuid::new_v4().to_string();
                conn.execute(
                    "UPDATE accounts SET id = ?1 WHERE puuid = ?2",
                    [&uuid, &puuid],
                )
                .unwrap();
            }

            conn.execute(
                "CREATE UNIQUE INDEX IF NOT EXISTS accounts_id_uindex ON accounts(id)",
                [],
            )
            .unwrap();
        }
    }
}
