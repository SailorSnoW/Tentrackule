use rusqlite::Connection;

mod v1;
pub use v1::V1;

pub trait DbMigration {
    fn do_migration(conn: &Connection);
}
