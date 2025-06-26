use rusqlite::Connection;
use tracing::info;

use super::DbMigration;

/// Create a dedicated table to store league points per queue type.
pub struct V2;

impl DbMigration for V2 {
    fn do_migration(conn: &Connection) {
        info!("ensuring 'league_points' table exists");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS league_points (
            puuid TEXT NOT NULL,
            queue_type TEXT NOT NULL,
            league_points INTEGER,
            PRIMARY KEY (puuid, queue_type),
            FOREIGN KEY (puuid) REFERENCES accounts(puuid)
        )",
            [],
        )
        .unwrap();
    }
}
