//! Player frames: the three scalars round-tripping, the rung that stores them,
//! and the refusal that is not a `rem_euclid`.

mod support;

use rusqlite::Connection;
use support::Scratch;
use wgvb::{Config, Coord};
use wgvb_store::{Player, PlayerError, SCHEMA_VERSION, World};

/// A world on a scratch path, kept alive by the returned scratch directory.
fn world(label: &str) -> (Scratch, World) {
    let scratch = Scratch::new(label);
    let path = scratch.file("world.wgvb");
    let world = World::open_or_create(&path, 1, &Config::default()).expect("a fresh world");
    (scratch, world)
}

#[test]
fn a_frame_round_trips() {
    let (_scratch, world) = world("frame");
    world
        .add_player("ashe", Coord::new(100, -40), 1)
        .expect("the write runs");

    let stored = world.player("ashe").expect("the read runs");
    assert_eq!(
        stored,
        Some(Player {
            name: "ashe".to_owned(),
            origin: Coord::new(100, -40),
            rotation: 1,
        })
    );
    assert_eq!(world.player("nobody").expect("the read runs"), None);
}

#[test]
fn every_rotation_and_a_negative_origin_survive_the_round_trip() {
    let (_scratch, world) = world("rotations");
    for rotation in 0..6_u8 {
        let origin = Coord::new(-1_000 - i64::from(rotation), i64::from(rotation) - 30_000);
        let name = format!("player-{rotation}");
        world
            .add_player(&name, origin, rotation)
            .expect("the write runs");
        let stored = world
            .player(&name)
            .expect("the read runs")
            .expect("a player");
        assert_eq!(stored.origin, origin, "rotation {rotation}");
        assert_eq!(stored.rotation, rotation, "rotation {rotation}");
    }
    assert_eq!(world.players().expect("the read runs").len(), 6);
}

#[test]
fn players_come_back_in_name_order() {
    let (_scratch, world) = world("order");
    for name in ["cai", "ashe", "bram"] {
        world
            .add_player(name, Coord::new(1, 1), 0)
            .expect("the write runs");
    }
    let names: Vec<String> = world
        .players()
        .expect("the read runs")
        .into_iter()
        .map(|player| player.name)
        .collect();
    assert_eq!(names, ["ashe", "bram", "cai"]);
}

#[test]
fn a_frame_survives_a_close_and_reopen() {
    let scratch = Scratch::new("persist");
    let path = scratch.file("world.wgvb");
    {
        let world = World::open_or_create(&path, 7, &Config::default()).expect("a fresh world");
        world
            .add_player("bram", Coord::new(-7, 900), 4)
            .expect("the write runs");
    }

    let world = World::open(&path).expect("the world reopens");
    let stored = world
        .player("bram")
        .expect("the read runs")
        .expect("a player");
    assert_eq!(stored.origin, Coord::new(-7, 900));
    assert_eq!(stored.rotation, 4);
}

#[test]
fn a_frame_is_not_generated_data_and_outlives_discarding_every_cache() {
    // Section 27.6: generated tiles, chunks, and images are reproducible caches
    // and may be discarded at any time. Stand one up as a table this database
    // does not have, fill it, drop it — the thing a cache sweep does — and the
    // frame is untouched, because no foreign key and no derivation connects
    // them (section 27.3).
    let scratch = Scratch::new("cache");
    let path = scratch.file("world.wgvb");
    let world = World::open_or_create(&path, 3, &Config::default()).expect("a fresh world");
    world
        .add_player("ashe", Coord::new(12_345, -30_000), 5)
        .expect("the write runs");

    let raw = Connection::open(&path).expect("the file opens");
    raw.execute_batch(
        "CREATE TABLE cache_tile (q INTEGER NOT NULL, r INTEGER NOT NULL, \
         terrain INTEGER NOT NULL, PRIMARY KEY (q, r)) WITHOUT ROWID; \
         INSERT INTO cache_tile VALUES (12345, -30000, 4); \
         DROP TABLE cache_tile;",
    )
    .expect("a cache can be built and thrown away");
    drop(raw);

    let stored = world
        .player("ashe")
        .expect("the read runs")
        .expect("a player");
    assert_eq!(stored.origin, Coord::new(12_345, -30_000));
    assert_eq!(stored.rotation, 5);
}

#[test]
fn a_second_player_of_the_same_name_is_refused_rather_than_reanchored() {
    let (_scratch, world) = world("exists");
    world
        .add_player("ashe", Coord::new(3, -1), 2)
        .expect("the write runs");

    let error = world
        .add_player("ashe", Coord::new(-500, 40), 5)
        .expect_err("a frame is assigned once");
    assert!(
        matches!(&error, PlayerError::Exists(name) if name == "ashe"),
        "{error:?}"
    );

    // And the refusal wrote nothing: the original frame is intact.
    let stored = world
        .player("ashe")
        .expect("the read runs")
        .expect("a player");
    assert_eq!(stored.origin, Coord::new(3, -1));
    assert_eq!(stored.rotation, 2);
}

#[test]
fn an_out_of_range_rotation_is_refused_on_write() {
    let (_scratch, world) = world("write-rotation");
    for rotation in [6_u8, 7, 255] {
        let error = world
            .add_player("ashe", Coord::ORIGIN, rotation)
            .expect_err("a rotation is a direction index");
        assert!(
            matches!(
                &error,
                PlayerError::RotationOutOfRange { player, found }
                    if player == "ashe" && *found == i64::from(rotation)
            ),
            "rotation {rotation}: {error:?}"
        );
        assert_eq!(
            world.player("ashe").expect("the read runs"),
            None,
            "rotation {rotation} was written anyway"
        );
    }
}

#[test]
fn an_out_of_range_stored_rotation_is_an_error_and_not_a_coerced_value() {
    let scratch = Scratch::new("read-rotation");
    let path = scratch.file("world.wgvb");
    let world = World::open_or_create(&path, 1, &Config::default()).expect("a fresh world");
    world
        .add_player("ashe", Coord::new(3, -1), 1)
        .expect("the write runs");

    // Past the CHECK constraint, the way an outside tool or a corrupted file
    // would arrive: the point of validating on read is that the column
    // constraint is not the only thing that can write this row.
    let raw = Connection::open(&path).expect("the file opens");
    raw.pragma_update(None, "ignore_check_constraints", "ON")
        .expect("the pragma is set");
    raw.execute("UPDATE player SET rotation = 7 WHERE name = 'ashe'", [])
        .expect("the row is edited");
    drop(raw);

    let error = world
        .player("ashe")
        .expect_err("a stored 7 is not a rotation");
    assert!(
        matches!(
            &error,
            PlayerError::RotationOutOfRange { player, found } if player == "ashe" && *found == 7
        ),
        "{error:?}"
    );
    // Not 1, which is what `7 % 6` would have handed back — a world silently
    // rotated a sixth of a turn from the one this player has been walking.
    let error = world.players().expect_err("the scan refuses it too");
    assert!(
        matches!(&error, PlayerError::RotationOutOfRange { found, .. } if *found == 7),
        "{error:?}"
    );
}

#[test]
fn the_player_table_arrives_by_migration_on_a_world_that_predates_it() {
    // The rung, exercised: a database at schema 1 is migrated up rather than
    // rejected, and the world it describes is untouched by the trip.
    let scratch = Scratch::new("rung");
    let path = scratch.file("world.wgvb");
    let seed = 0xfeed_face_u64;
    {
        let world = World::open_or_create(&path, seed, &Config::default()).expect("a fresh world");
        assert!(world.was_created());
    }

    // Wind the file back to what schema 1 left behind.
    let raw = Connection::open(&path).expect("the file opens");
    raw.execute_batch("DROP TABLE player;")
        .expect("the table drops");
    raw.pragma_update(None, "user_version", 1_u32)
        .expect("the pragma is set");
    drop(raw);

    let world = World::open(&path).expect("an older schema migrates rather than failing");
    assert_eq!(world.seed(), seed);
    assert!(!world.was_created());
    assert_eq!(world.players().expect("the read runs"), Vec::new());
    world
        .add_player("ashe", Coord::new(1, 2), 3)
        .expect("the write runs");

    let raw = Connection::open(&path).expect("the file opens");
    let reached: u32 = raw
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("the pragma reads");
    assert_eq!(reached, SCHEMA_VERSION);
}
