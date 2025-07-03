use rusqlite::Connection;
use tracing::info;

use super::DbMigration;

/// Add `puuid_tft` column to the accounts table.
pub struct V6;

impl DbMigration for V6 {
    fn do_migration(conn: &Connection) {
        let mut stmt = conn.prepare("PRAGMA table_info(accounts)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        if !columns.contains(&"puuid_tft".to_string()) {
            info!("adding column 'puuid_tft' to 'accounts'");
            conn.execute(
                "ALTER TABLE accounts ADD COLUMN puuid_tft TEXT NOT NULL DEFAULT ''",
                [],
            )
            .unwrap();
        }
    }
}
