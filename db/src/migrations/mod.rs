//! Database schema migrations.

use rusqlite::Connection;

mod v3;
pub use v3::V3;
mod v4;
pub use v4::V4;
mod v5;
pub use v5::V5;
mod v6;
pub use v6::V6;
mod v7;
pub use v7::V7;
mod v8;
pub use v8::V8;
mod v9;
pub use v9::V9;

pub trait DbMigration {
    fn do_migration(conn: &Connection);
}
