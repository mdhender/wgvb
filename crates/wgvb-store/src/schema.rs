//! The owned `user_version` migration ladder. See `DESIGN.md` section 27.4.
//!
//! There is no `sqlitemigration` equivalent in Rust and none is needed: the
//! ladder is forty lines and it is part of the file format. Owning it is the
//! same decision as owning the noise (section 9.2) — the file format must not
//! move because a dependency did.

use rusqlite::Connection;

use crate::OpenError;

/// Each entry migrates from its index to its index plus one, and is applied in
/// one transaction whose final statement sets `PRAGMA user_version`.
///
/// **Never edit a released migration.** Add one.
const MIGRATIONS: &[&str] = &[
    // schema version 0 -> 1
    include_str!("../migrations/0001_initial.sql"),
];

/// Highest `PRAGMA user_version` this binary can operate on. A database
/// reporting a higher schema version is rejected before migration.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a migration ladder with more than four billion rungs is not a \
              thing that can exist; this is a const-evaluated slice length"
)]
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// Applies every migration from `current` up to [`SCHEMA_VERSION`].
///
/// Gate 3. Idempotent on a current database: the loop body does not run.
/// Applying nothing is why reopening an up-to-date world performs no write at
/// all, which is what the "no write before a gate passes" rule needs from a
/// gate that is allowed to write.
pub(crate) fn migrate(connection: &Connection, current: u32) -> Result<(), OpenError> {
    for step in current..SCHEMA_VERSION {
        let index = usize::try_from(step).expect("a u32 step fits a usize on every target");
        let transaction = connection.unchecked_transaction()?;
        transaction.execute_batch(MIGRATIONS[index])?;
        transaction.commit()?;

        // A migration that forgets its trailing `PRAGMA user_version` would
        // otherwise be applied again on the next open, on top of itself. Check
        // rather than trust: the statement lives in a `.sql` file that nothing
        // else validates.
        let reached = user_version(connection)?;
        let expected = step + 1;
        assert_eq!(
            reached, expected,
            "migration {expected} left user_version at {reached}; \
             its final statement must be `PRAGMA user_version = {expected}`"
        );
    }
    Ok(())
}

/// Reads `PRAGMA user_version`.
pub(crate) fn user_version(connection: &Connection) -> Result<u32, rusqlite::Error> {
    connection.pragma_query_value(None, "user_version", |row| row.get(0))
}

/// Reads `PRAGMA application_id`.
pub(crate) fn application_id(connection: &Connection) -> Result<i32, rusqlite::Error> {
    connection.pragma_query_value(None, "application_id", |row| row.get(0))
}
