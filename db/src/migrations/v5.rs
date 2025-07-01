use rusqlite::Connection;
use tracing::info;

use super::DbMigration;

/// Create a table storing queue alert settings per guild.
pub struct V5;

impl DbMigration for V5 {
    fn do_migration(conn: &Connection) {
        info!("ensuring 'queue_alert_settings' table exists");
        conn.execute(
            "CREATE TABLE IF NOT EXISTS queue_alert_settings (
                guild_id INTEGER NOT NULL,
                queue_type TEXT NOT NULL,
                enabled INTEGER NOT NULL,
                PRIMARY KEY (guild_id, queue_type),
                FOREIGN KEY (guild_id) REFERENCES guild_settings(guild_id)
            )",
            [],
        )
        .unwrap();
    }
}
