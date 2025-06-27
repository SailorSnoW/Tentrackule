//! Database schema migrations.

use rusqlite::Connection;

mod v1;
pub use v1::V1;
mod v2;
pub use v2::V2;
mod v3;
pub use v3::V3;

pub trait DbMigration {
    fn do_migration(conn: &Connection);
}
