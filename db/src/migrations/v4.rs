use rusqlite::Connection;
use tracing::info;

use super::DbMigration;

/// Add `rank` and `tier` columns to the leagues table.
pub struct V4;

impl DbMigration for V4 {
    fn do_migration(conn: &Connection) {
        let mut stmt = conn.prepare("PRAGMA table_info(leagues)").unwrap();
        let columns: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        if !columns.contains(&"rank".to_string()) {
            info!("adding column 'rank' to 'leagues'");
            conn.execute("ALTER TABLE leagues ADD COLUMN rank TEXT", [])
                .unwrap();
        }
        if !columns.contains(&"tier".to_string()) {
            info!("adding column 'tier' to 'leagues'");
            conn.execute("ALTER TABLE leagues ADD COLUMN tier TEXT", [])
                .unwrap();
        }
    }
}
