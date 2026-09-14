//! What a window holds, counted, and the properties that make the count worth
//! reading.

use wgvb::{Coord, Generator, Terrain};
use wgvb_render::{Distribution, Viewport};

const SEED: u64 = 0x0123_4567_89ab_cdef;

fn window() -> Viewport {
    Viewport::centered_on(Coord::new(-87, 6543), 41, 31, 4.0).expect("a valid window")
}

#[test]
fn every_tile_is_counted_exactly_once_in_each_histogram() {
    // Four histograms over one set of tiles: each has to add up to the window,
    // or something is being double-counted or dropped.
    let generator = Generator::with_defaults(SEED);
    let viewport = window();
    let counted = Distribution::of(&generator, viewport.coords());

    let (cols, rows) = viewport.tile_counts();
    let tiles = u64::from(cols) * u64::from(rows);
    assert_eq!(counted.tiles(), tiles);

    assert_eq!(
        counted.terrain().map(|(_, count)| count).sum::<u64>(),
        tiles
    );
    assert_eq!(
        counted.elevation().map(|(_, count)| count).sum::<u64>(),
        tiles
    );
    assert_eq!(counted.heat().map(|(_, count)| count).sum::<u64>(), tiles);
    assert_eq!(
        counted.moisture().map(|(_, count)| count).sum::<u64>(),
        tiles
    );
}

#[test]
fn the_counts_are_what_counting_by_hand_gives() {
    // Derived independently of the type under test, per the verification rule:
    // ask the generator directly, tile by tile, and compare.
    let generator = Generator::with_defaults(SEED);
    let viewport = window();
    let counted = Distribution::of(&generator, viewport.coords());

    for terrain in Terrain::ALL {
        let by_hand = viewport
            .coords()
            .filter(|coord| generator.tile(*coord).terrain == terrain)
            .count() as u64;
        let reported = counted
            .terrain()
            .find(|(each, _)| *each == terrain)
            .map(|(_, count)| count)
            .expect("every terrain is listed");
        assert_eq!(reported, by_hand, "{}", terrain.name());
    }
}

#[test]
fn the_count_does_not_depend_on_the_order_the_tiles_arrive_in() {
    // Integer counts are order-independent, which is what makes this safe to
    // compute in a front end at all. Asserted rather than assumed, because the
    // moment somebody makes it a floating-point average it stops being true.
    let generator = Generator::with_defaults(SEED);
    let viewport = window();

    let forwards: Vec<Coord> = viewport.coords().collect();
    let backwards: Vec<Coord> = forwards.iter().copied().rev().collect();
    let mut sorted = forwards.clone();
    sorted.sort_unstable();

    let first = Distribution::of(&generator, forwards);
    assert_eq!(first, Distribution::of(&generator, backwards));
    assert_eq!(first, Distribution::of(&generator, sorted));
}

#[test]
fn every_terrain_is_listed_even_where_none_is_present() {
    // A row reading zero is the useful row: "no wetland anywhere in this
    // window" is exactly what somebody moving a wetland threshold needs to
    // see, and a list of only what is present would hide it.
    let generator = Generator::with_defaults(SEED);
    let counted = Distribution::of(&generator, window().coords());

    assert_eq!(counted.terrain().count(), Terrain::ALL.len());
    assert!(
        counted.terrain().any(|(_, count)| count == 0),
        "this window contains every terrain, so pick another for this test"
    );
}

#[test]
fn the_shares_add_up_to_the_whole_window() {
    let generator = Generator::with_defaults(SEED);
    let counted = Distribution::of(&generator, window().coords());
    let total: f64 = counted
        .terrain()
        .map(|(_, count)| counted.share(count))
        .sum();
    assert!(
        (total - 100.0).abs() < 1e-9,
        "the terrain shares add up to {total}, not 100"
    );
}

#[test]
fn a_turn_moves_the_ground_under_the_window_rather_than_relabeling_it() {
    // Worth pinning because the obvious guess is the other one. A turn rotates
    // the sampled *region* about the center cell, and a rectangle is not
    // invariant under a sixth of a turn — only the center tile is — so a turned
    // window covers different ground and the mix moves with it. The tile count
    // does not, because it is the same rectangle of cells either way.
    //
    // Anyone reading a distribution beside a turned map needs this to be the
    // stated behaviour rather than a surprise: the numbers describe what is on
    // the screen, and turning changes what is on the screen.
    let generator = Generator::with_defaults(SEED);
    let plain = Distribution::of(&generator, window().coords());

    let mut moved = 0;
    for turn in 1..6 {
        let turned = window().turned(turn).expect("a turn of a sixth");
        let counted = Distribution::of(&generator, turned.coords());
        assert_eq!(
            counted.tiles(),
            plain.tiles(),
            "turn {turn} changed how many cells the window has"
        );
        if counted != plain {
            moved += 1;
        }
    }
    assert_eq!(
        moved, 5,
        "a turn left the window over identical ground, which a rotation of a \
         rectangle cannot do"
    );
}
