//! The opening gates of `DESIGN.md` section 27.5, and section 30.11's rule
//! that every one of them is asserted on as an `OpenError` **variant**.
//!
//! A message string is not a contract. These tests match variants, and the one
//! place a message is read is where the variant carries the number that proves
//! the gate saw what it claimed to see.

mod support;

use rusqlite::Connection;
use support::{Scratch, bytes};
use wgvb::{ALGORITHM_VERSION, Config, Coord};
use wgvb_store::{
    APPLICATION_ID, OpenError, SCHEMA_VERSION, World, canonical_config_bytes, fingerprint_of_bytes,
};

/// WGVA's application id, one byte from WGVB's. Section 27.1 requires each
/// binary to reject the other's files at the first gate.
const WGVA_APPLICATION_ID: i32 = 0x5747_5641;

/// Makes a valid world and closes it.
fn make_world(path: &std::path::Path, seed: u64) {
    let world = World::open_or_create(path, seed, &Config::default()).expect("a fresh world");
    assert!(
        world.was_created(),
        "a fresh path creates rather than opens"
    );
}

/// Opens the file with raw SQL, for building a database the gates must refuse.
fn raw(path: &std::path::Path) -> Connection {
    Connection::open(path).expect("the file opens")
}

#[test]
fn a_wgva_file_is_rejected_at_the_first_gate_without_writing() {
    let scratch = Scratch::new("wgva");
    let path = scratch.file("world.wgva");
    {
        let connection = raw(&path);
        connection
            .pragma_update(None, "application_id", WGVA_APPLICATION_ID)
            .expect("the pragma is set");
        connection
            .execute_batch("CREATE TABLE tiles (q INTEGER, r INTEGER)")
            .expect("a table is created");
    }

    let before = bytes(&path);
    let error = World::open(&path).expect_err("a WGVA file is not a WGVB world");
    assert!(
        matches!(
            error,
            OpenError::WrongApplicationId { found, expected }
                if found == WGVA_APPLICATION_ID && expected == APPLICATION_ID
        ),
        "{error:?}"
    );
    assert_eq!(before, bytes(&path), "gate 1 wrote to the file");
}

#[test]
fn a_foreign_database_with_no_application_id_is_rejected_at_the_first_gate() {
    // An id of zero is the common case — every SQLite database that never set
    // one — so it must not be mistaken for an empty database to adopt.
    let scratch = Scratch::new("foreign");
    let path = scratch.file("addresses.db");
    raw(&path)
        .execute_batch("CREATE TABLE people (name TEXT)")
        .expect("a table is created");

    let before = bytes(&path);
    let error = World::open_or_create(&path, 1, &Config::default())
        .expect_err("a database with tables is not an empty one");
    assert!(
        matches!(error, OpenError::WrongApplicationId { found: 0, .. }),
        "{error:?}"
    );
    assert_eq!(before, bytes(&path), "gate 1 wrote to the file");
}

#[test]
fn a_newer_schema_is_rejected_at_the_second_gate_without_migrating() {
    let scratch = Scratch::new("schema");
    let path = scratch.file("world.wgvb");
    make_world(&path, 1);
    let ahead = SCHEMA_VERSION + 7;
    raw(&path)
        .pragma_update(None, "user_version", ahead)
        .expect("the pragma is set");

    let before = bytes(&path);
    let error = World::open(&path).expect_err("a schema from the future is refused");
    assert!(
        matches!(
            error,
            OpenError::SchemaTooNew { found, supported }
                if found == ahead && supported == SCHEMA_VERSION
        ),
        "{error:?}"
    );
    assert_eq!(before, bytes(&path), "gate 2 wrote to the file");
}

#[test]
fn a_tampered_configuration_fails_the_fingerprint_check() {
    let scratch = Scratch::new("tamper");
    let path = scratch.file("world.wgvb");
    make_world(&path, 1);

    {
        let connection = raw(&path);
        let mut config: Vec<u8> = connection
            .query_row("SELECT config FROM world WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("the stored configuration");
        // One byte, deep enough in to be a value rather than a length prefix.
        let target = config.len() / 2;
        config[target] ^= 0x01;
        connection
            .execute("UPDATE world SET config = ?1 WHERE id = 1", [config])
            .expect("the row is edited");
    }

    let before = bytes(&path);
    let error = World::open(&path).expect_err("edited configuration bytes are refused");
    assert!(matches!(error, OpenError::FingerprintMismatch), "{error:?}");
    assert_eq!(before, bytes(&path), "gate 4 wrote to the file");
}

#[test]
fn an_unsupported_generator_version_is_rejected_at_the_fifth_gate() {
    let scratch = Scratch::new("version");
    let path = scratch.file("world.wgvb");
    make_world(&path, 1);

    // Re-fingerprint for the impossible version, so gate 4 passes and gate 5 is
    // demonstrably the one that refuses. A mismatched fingerprint would prove
    // nothing about gate 5.
    let ahead = ALGORITHM_VERSION + 1;
    {
        let connection = raw(&path);
        let config: Vec<u8> = connection
            .query_row("SELECT config FROM world WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("the stored configuration");
        let fingerprint = fingerprint_of_bytes(ahead, &config);
        connection
            .execute(
                "UPDATE world SET algorithm_version = ?1, fingerprint = ?2 WHERE id = 1",
                rusqlite::params![i64::from(ahead), fingerprint.as_slice()],
            )
            .expect("the row is edited");
    }

    let before = bytes(&path);
    let error = World::open(&path).expect_err("an unreproducible world is refused");
    assert!(
        matches!(error, OpenError::UnsupportedGeneratorVersion(found) if found == ahead),
        "{error:?}"
    );
    assert_eq!(
        before,
        bytes(&path),
        "gate 5 rewrote the world rather than refusing it"
    );
}

#[test]
fn a_missing_world_row_is_malformed_metadata() {
    let scratch = Scratch::new("norow");
    let path = scratch.file("world.wgvb");
    make_world(&path, 1);
    raw(&path)
        .execute("DELETE FROM world", [])
        .expect("the row is deleted");

    let error = World::open(&path).expect_err("a world with no metadata is refused");
    assert!(
        matches!(error, OpenError::MalformedMetadata(_)),
        "{error:?}"
    );
}

#[test]
fn a_short_fingerprint_is_malformed_metadata() {
    let scratch = Scratch::new("shortprint");
    let path = scratch.file("world.wgvb");
    make_world(&path, 1);
    raw(&path)
        .execute(
            "UPDATE world SET fingerprint = ?1 WHERE id = 1",
            [vec![0_u8; 4]],
        )
        .expect("the row is edited");

    let error = World::open(&path).expect_err("a four-byte fingerprint is refused");
    assert!(
        matches!(error, OpenError::MalformedMetadata(_)),
        "{error:?}"
    );
}

#[test]
fn an_undecodable_configuration_is_malformed_metadata() {
    let scratch = Scratch::new("garbage");
    let path = scratch.file("world.wgvb");
    make_world(&path, 1);

    // Valid CBOR, correctly fingerprinted, and not a configuration. This is the
    // case gate 4's fingerprint check cannot catch, because nothing was
    // tampered with: the bytes and their hash agree.
    let mut garbage = Vec::new();
    ciborium::ser::into_writer(&"not a configuration", &mut garbage).expect("CBOR encodes");
    let fingerprint = fingerprint_of_bytes(ALGORITHM_VERSION, &garbage);
    raw(&path)
        .execute(
            "UPDATE world SET config = ?1, fingerprint = ?2 WHERE id = 1",
            rusqlite::params![garbage, fingerprint.as_slice()],
        )
        .expect("the row is edited");

    let error = World::open(&path).expect_err("a configuration that is not one is refused");
    assert!(
        matches!(error, OpenError::MalformedMetadata(_)),
        "{error:?}"
    );
}

#[test]
fn a_stored_configuration_that_is_invalid_is_rejected_as_a_config_error() {
    let scratch = Scratch::new("invalid");
    let path = scratch.file("world.wgvb");
    make_world(&path, 1);

    // Decodes cleanly, hashes correctly, and describes a world with sea level
    // above the top of the elevation scale.
    let config = Config {
        sea_level: 5.0,
        ..Config::default()
    };
    let encoded = canonical_config_bytes(&config).expect("an encodable configuration");
    let fingerprint = fingerprint_of_bytes(ALGORITHM_VERSION, &encoded);
    raw(&path)
        .execute(
            "UPDATE world SET config = ?1, fingerprint = ?2 WHERE id = 1",
            rusqlite::params![encoded, fingerprint.as_slice()],
        )
        .expect("the row is edited");

    let error = World::open(&path).expect_err("an out-of-range sea level is refused");
    assert!(matches!(error, OpenError::Config(_)), "{error:?}");
}

#[test]
fn creating_with_an_invalid_configuration_leaves_no_file_behind() {
    let scratch = Scratch::new("badcreate");
    let path = scratch.file("world.wgvb");
    let config = Config {
        mountain_level: -1.0,
        ..Config::default()
    };

    let error =
        World::open_or_create(&path, 1, &config).expect_err("an unordered ladder is refused");
    assert!(matches!(error, OpenError::Config(_)), "{error:?}");
    assert!(
        !path.exists(),
        "a rejected configuration created a database anyway"
    );
}

#[test]
fn opening_a_missing_file_never_creates_one() {
    let scratch = Scratch::new("missing");
    let path = scratch.file("absent.wgvb");

    let error = World::open(&path).expect_err("there is nothing to open");
    assert!(matches!(error, OpenError::Sqlite(_)), "{error:?}");
    assert!(!path.exists(), "`open` created a database");
}

#[test]
fn a_created_world_carries_the_wgvb_application_id_and_the_current_schema() {
    let scratch = Scratch::new("created");
    let path = scratch.file("world.wgvb");
    make_world(&path, 0x0123_4567_89ab_cdef);

    let connection = raw(&path);
    let application: i32 = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .expect("the pragma reads");
    let schema: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("the pragma reads");
    assert_eq!(application, APPLICATION_ID);
    assert_eq!(schema, SCHEMA_VERSION);
}

#[test]
fn reopening_uses_the_stored_world_and_ignores_the_arguments() {
    let scratch = Scratch::new("reopen");
    let path = scratch.file("world.wgvb");

    let stored = Config {
        sea_level: 0.125,
        ..Config::default()
    };
    let created = World::open_or_create(&path, 42, &stored).expect("a fresh world");
    assert!(created.was_created());
    drop(created);

    // Different seed, different configuration, same file: neither may win.
    // Both are *valid*, because `open_or_create` validates its argument before
    // it touches the filesystem and so before it can know the argument will be
    // ignored.
    let other = Config {
        sea_level: 0.0625,
        ..Config::default()
    };
    let reopened = World::open_or_create(&path, 99, &other).expect("the stored world reopens");
    assert!(!reopened.was_created(), "an existing world was recreated");
    assert_eq!(reopened.seed(), 42, "the argument overrode the stored seed");
    assert_eq!(
        reopened.config().sea_level,
        0.125,
        "the argument overrode the stored configuration"
    );
    assert_eq!(reopened.algorithm_version(), ALGORITHM_VERSION);
}

#[test]
fn tiles_from_a_reopened_database_are_bit_identical() {
    let scratch = Scratch::new("bitwise");
    let path = scratch.file("world.wgvb");

    let coords: Vec<Coord> = [
        (0, 0),
        (-1, -1),
        (137, -4_211),
        (-32_767, 32_767),
        (12_345, -6_789),
    ]
    .into_iter()
    .map(|(q, r)| Coord::new(q, r))
    .collect();

    let before = {
        let world =
            World::open_or_create(&path, 0xdead_beef, &Config::default()).expect("a fresh world");
        let generator = world
            .generator()
            .expect("the stored configuration is valid");
        generator.tiles(&coords)
    };

    let world = World::open(&path).expect("the world reopens");
    let generator = world
        .generator()
        .expect("the stored configuration is valid");
    assert_eq!(
        generator.tiles(&coords),
        before,
        "a reopened world does not reproduce its own tiles"
    );
}

#[test]
fn the_migration_ladder_applies_from_empty_and_is_idempotent() {
    let scratch = Scratch::new("ladder");
    let path = scratch.file("world.wgvb");
    make_world(&path, 1);

    let stamp = {
        let connection = raw(&path);
        schema_text(&connection)
    };

    // Reopening runs gate 3 again. An up-to-date database must come out of it
    // unchanged, which is what makes the ladder safe to walk on every open.
    let before = bytes(&path);
    let world = World::open(&path).expect("the world reopens");
    drop(world);
    assert_eq!(
        before,
        bytes(&path),
        "reopening a current database wrote to it"
    );
    assert_eq!(
        stamp,
        schema_text(&raw(&path)),
        "the schema changed on reopen"
    );
}

#[test]
fn every_coordinate_keyed_table_is_without_rowid() {
    let scratch = Scratch::new("rowid");
    let path = scratch.file("world.wgvb");
    make_world(&path, 1);
    let connection = raw(&path);

    // Section 27.2. A `WITHOUT ROWID` table *is* the coordinate B-tree; with a
    // rowid it is a heap plus an index, and a viewport load stops being one
    // ordered scan. The distinguishing fact SQLite exposes is that a rowid
    // table answers a query for `rowid` and a `WITHOUT ROWID` one does not.
    for table in ["overlay_discovery", "overlay_settlement"] {
        let query = format!("SELECT rowid FROM {table} LIMIT 1");
        assert!(
            connection.prepare(&query).is_err(),
            "{table} has a rowid, so it is not the coordinate B-tree section 27.2 requires"
        );
    }
}

#[test]
fn no_table_carries_a_world_id_column() {
    // One database, one world (section 27.6). A `world_id` column is the shape
    // a multi-world schema takes, and it must not appear by habit.
    let scratch = Scratch::new("oneworld");
    let path = scratch.file("world.wgvb");
    make_world(&path, 1);
    let connection = raw(&path);

    let schema = schema_text(&connection);
    assert!(
        !schema.contains("world_id"),
        "the schema names a world_id:\n{schema}"
    );
}

#[test]
fn foreign_keys_are_enabled_on_every_connection() {
    // Section 27.3: nothing points at generated data today, but a constraint
    // added later between two authoritative tables has to be enforced, and
    // SQLite leaves this off per connection by default.
    let scratch = Scratch::new("fk");
    let path = scratch.file("world.wgvb");
    let world = World::open_or_create(&path, 1, &Config::default()).expect("a fresh world");
    assert!(
        world.foreign_keys_enabled().expect("the pragma reads"),
        "a created world has them off"
    );
    drop(world);

    let world = World::open(&path).expect("the world reopens");
    assert!(
        world.foreign_keys_enabled().expect("the pragma reads"),
        "a reopened world has them off"
    );
}

/// The full `CREATE` text of every object in the schema, in a stable order.
fn schema_text(connection: &Connection) -> String {
    let mut statement = connection
        .prepare("SELECT sql FROM sqlite_master WHERE sql IS NOT NULL ORDER BY name")
        .expect("the schema query prepares");
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("the schema query runs");
    rows.map(|row| row.expect("a schema row"))
        .collect::<Vec<_>>()
        .join("\n")
}
