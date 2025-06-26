use rusqlite::Connection;
use tracing::info;

use super::DbMigration;

/// This migration adds a lol_solo_duo_lps column to handle computing LPs loss/win between the new
/// match and the old one.
pub struct V1;

impl DbMigration for V1 {
    fn do_migration(conn: &Connection) {
        let mut stmt = conn.prepare("PRAGMA table_info(accounts)").unwrap();

        let column_exists = stmt
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .any(|col| col.unwrap() == "lol_solo_duo_lps");

        if !column_exists {
            info!("adding column 'lol_solo_duo_lps' to 'accounts'");
            conn.execute(
                "ALTER TABLE accounts ADD COLUMN lol_solo_duo_lps INTEGER",
                [],
            )
            .unwrap();
        }
    }
}
