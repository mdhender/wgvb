//! Sparse coordinate-keyed player overlays: round trip, ordering, and the
//! range scan a viewport load is.

mod support;

use support::Scratch;
use wgvb::{Config, Coord};
use wgvb_store::{Bounds, Settlement, World};

/// A world on a scratch path, kept alive by the returned scratch directory.
fn world(label: &str) -> (Scratch, World) {
    let scratch = Scratch::new(label);
    let path = scratch.file("world.wgvb");
    let world = World::open_or_create(&path, 1, &Config::default()).expect("a fresh world");
    (scratch, world)
}

#[test]
fn discoveries_round_trip() {
    let (_scratch, world) = world("discover");
    let seen: Vec<Coord> = [(0, 0), (3, -1), (-7, 12)]
        .into_iter()
        .map(|(q, r)| Coord::new(q, r))
        .collect();

    assert_eq!(world.discover(&seen).expect("the write runs"), 3);
    let mut expected = seen.clone();
    expected.sort_unstable();
    assert_eq!(world.discoveries().expect("the read runs"), expected);
}

#[test]
fn discovering_a_tile_twice_changes_nothing() {
    let (_scratch, world) = world("twice");
    let seen = [Coord::new(4, 4)];

    assert_eq!(world.discover(&seen).expect("the write runs"), 1);
    assert_eq!(
        world.discover(&seen).expect("the second write runs"),
        0,
        "a tile seen twice was written twice"
    );
    assert_eq!(world.discoveries().expect("the read runs"), seen);
}

#[test]
fn settlements_round_trip_and_a_second_founding_renames() {
    let (_scratch, world) = world("settle");
    let coord = Coord::new(-11, 5);

    world.settle(coord, "Ashford").expect("the write runs");
    assert_eq!(
        world.settlements().expect("the read runs"),
        vec![Settlement {
            coord,
            name: "Ashford".to_owned(),
        }]
    );

    world
        .settle(coord, "Ashford Keep")
        .expect("the rename runs");
    assert_eq!(
        world.settlements().expect("the read runs"),
        vec![Settlement {
            coord,
            name: "Ashford Keep".to_owned(),
        }],
        "renaming a settlement left two rows at one tile"
    );
}

#[test]
fn overlays_survive_a_close_and_reopen() {
    let scratch = Scratch::new("persist");
    let path = scratch.file("world.wgvb");
    let seen: Vec<Coord> = [(1, 1), (-2, 30), (0, -9)]
        .into_iter()
        .map(|(q, r)| Coord::new(q, r))
        .collect();

    {
        let world = World::open_or_create(&path, 7, &Config::default()).expect("a fresh world");
        world.discover(&seen).expect("the write runs");
        world
            .settle(Coord::new(1, 1), "Harbor")
            .expect("the write runs");
    }

    let world = World::open(&path).expect("the world reopens");
    let mut expected = seen;
    expected.sort_unstable();
    assert_eq!(world.discoveries().expect("the read runs"), expected);
    assert_eq!(world.settlements().expect("the read runs").len(), 1);
}

#[test]
fn a_viewport_range_scan_returns_overlays_in_coordinate_order() {
    let (_scratch, world) = world("scan");

    // Written in an order that is neither insertion order nor sorted order, so
    // a query that forgot to order would have to be lucky to look right.
    let scattered: Vec<Coord> = [
        (5, 5),
        (-40, 0),
        (2, -3),
        (2, 9),
        (2, -30),
        (0, 0),
        (60, 60),
        (-1, 1),
    ]
    .into_iter()
    .map(|(q, r)| Coord::new(q, r))
    .collect();
    world.discover(&scattered).expect("the write runs");

    // The box a viewport would ask for: the smallest one holding its tiles.
    let window: Vec<Coord> = [(0, 0), (2, 9), (2, -3), (-1, 1)]
        .into_iter()
        .map(|(q, r)| Coord::new(q, r))
        .collect();
    let bounds = Bounds::containing(window.iter().copied()).expect("a non-empty window");

    let found = world.discoveries_in(&bounds).expect("the scan runs");

    // Ordered ascending by q then r — the primary key's own order, which is
    // what makes a `WITHOUT ROWID` viewport load one sequential walk.
    let mut sorted = found.clone();
    sorted.sort_unstable();
    assert_eq!(found, sorted, "the range scan came back unordered");

    // Everything inside the box, and nothing outside it. `(2, -30)` shares a
    // `q` with three tiles in the window and is outside on `r`, which is the
    // case a scan that filtered on `q` alone would get wrong.
    let expected: Vec<Coord> = [(-1, 1), (0, 0), (2, -3), (2, 9)]
        .into_iter()
        .map(|(q, r)| Coord::new(q, r))
        .collect();
    assert_eq!(found, expected);
}

#[test]
fn a_settlement_scan_is_ordered_and_bounded_the_same_way() {
    let (_scratch, world) = world("settlescan");
    for (q, r, name) in [
        (10, 10, "Far"),
        (1, 2, "Second"),
        (1, -8, "Outside"),
        (1, 0, "First"),
        (3, 4, "Third"),
    ] {
        world
            .settle(Coord::new(q, r), name)
            .expect("the write runs");
    }

    let bounds = Bounds::containing([Coord::new(1, 0), Coord::new(3, 4)]).expect("a window");
    let names: Vec<String> = world
        .settlements_in(&bounds)
        .expect("the scan runs")
        .into_iter()
        .map(|settlement| settlement.name)
        .collect();
    assert_eq!(names, ["First", "Second", "Third"]);
}

#[test]
fn bounds_of_nothing_is_nothing() {
    assert_eq!(Bounds::containing(std::iter::empty()), None);
}

#[test]
fn everywhere_holds_the_whole_canonical_world() {
    let (_scratch, world) = world("everywhere");
    let corners: Vec<Coord> = [
        (0, 0),
        (-32_767, 0),
        (32_767, -32_767),
        (0, 32_767),
        (-32_767, 32_767),
    ]
    .into_iter()
    .map(|(q, r)| Coord::new(q, r))
    .collect();
    world.discover(&corners).expect("the write runs");

    assert_eq!(
        world
            .discoveries_in(&Bounds::everywhere())
            .expect("the scan runs")
            .len(),
        corners.len(),
        "a corner of the world fell outside `everywhere`"
    );
}

#[test]
fn an_overlay_is_keyed_by_the_canonical_tile() {
    // Two coordinates that normalize to the same tile are the same tile, and
    // the primary key has to agree. Section 24 and the `Coord` invariant.
    let (_scratch, world) = world("canonical");
    let inside = Coord::new(3, 4);
    let wrapped = Coord::new(3 + 65_535, 4 - 32_767);
    assert_eq!(
        inside, wrapped,
        "the test's two coordinates are not one tile"
    );

    world.discover(&[inside, wrapped]).expect("the write runs");
    assert_eq!(
        world.discoveries().expect("the read runs"),
        vec![inside],
        "one tile was stored twice"
    );
}

#[test]
fn two_connections_to_one_world_can_read_and_write_at_the_same_time() {
    // The viewer holds a read connection per worker while `wgvb-map --db
    // --discover` writes to the same file. SQLite's default is to give up on a
    // locked database immediately, which would turn an ordinary overlapping
    // write into a failed page load, so every connection sets a busy timeout.
    let scratch = Scratch::new("concurrent");
    let path = scratch.file("world.wgvb");
    let writer = World::open_or_create(&path, 1, &Config::default()).expect("a fresh world");
    let reader = World::open(&path).expect("a second connection");

    writer
        .discover(&[Coord::new(2, 3)])
        .expect("the write runs while another connection is open");
    assert_eq!(
        reader.discoveries().expect("the read runs"),
        vec![Coord::new(2, 3)],
        "the second connection did not see the write"
    );

    writer
        .settle(Coord::new(2, 3), "Ashford")
        .expect("the write runs");
    assert_eq!(
        reader.settlements().expect("the read runs").len(),
        1,
        "the second connection did not see the settlement"
    );
}
