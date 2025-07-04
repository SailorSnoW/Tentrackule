use rusqlite::Connection;
use tracing::info;

use super::DbMigration;

/// Add `last_match_id_tft` column to the accounts table.
pub struct V9;

impl DbMigration for V9 {
    fn do_migration(conn: &Connection) {
        let mut stmt = conn.prepare("PRAGMA table_info(accounts)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        if !columns.contains(&"last_match_id_tft".to_string()) {
            info!("adding column 'last_match_id_tft' to 'accounts'");
            conn.execute(
                "ALTER TABLE accounts ADD COLUMN last_match_id_tft TEXT NOT NULL DEFAULT ''",
                [],
            )
            .unwrap();
        }
    }
}
