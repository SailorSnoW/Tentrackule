use rusqlite::Connection;
use tracing::info;

use super::DbMigration;

/// Create a table storing league details per queue.
pub struct V3;

impl DbMigration for V3 {
    fn do_migration(conn: &Connection) {
        info!("ensuring 'leagues' table exists");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS leagues (
            puuid TEXT NOT NULL,
            queue_type TEXT NOT NULL,
            points INTEGER,
            wins INTEGER,
            losses INTEGER,
            PRIMARY KEY (puuid, queue_type),
            FOREIGN KEY (puuid) REFERENCES accounts(puuid)
        )",
            [],
        )
        .unwrap();
    }
}
