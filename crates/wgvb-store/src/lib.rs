//! Single-world SQLite persistence for WGVB.
//!
//! One database contains exactly one world and everything needed to reproduce
//! that world's generated baseline. See `DESIGN.md` section 27.

/// `PRAGMA application_id` for every WGVB database: ASCII `"WGVB"`.
///
/// Reject a non-empty database carrying any other application id before
/// applying migrations or performing application writes.
pub const APPLICATION_ID: i32 = 0x5747_5642;

/// Highest `PRAGMA user_version` this binary can operate on. A database
/// reporting a higher schema version is rejected before migration.
pub const SCHEMA_VERSION: u32 = 0;
