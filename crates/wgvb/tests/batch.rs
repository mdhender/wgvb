//! The batch API, order independence, and concurrent determinism.
//!
//! `DESIGN.md` sections 20, 30.1, 30.2, and 30.3.
//!
//! Section 20's rule is what makes all of this hold, and it is structural
//! rather than statistical: every tile is a pure function of its own coordinate
//! written to its own slot, and no batch operation accumulates across tiles. A
//! test that passes here while an accumulation exists is passing by luck, so
//! these tests are written to fail loudly rather than to sample: they compare
//! whole batches bit for bit, not summaries of them.

use rayon::prelude::*;
use wgvb::{Coord, Generator, Tile};

const SEED: u64 = 0x0bad_c0de_dead_beef;

/// A spread of coordinates: the origin, both signs, cell corners, coordinates
/// far from the origin, and one on each edge of the canonical hexagon.
fn coords() -> Vec<Coord> {
    let n = i64::from(i16::MAX);
    let mut out = Vec::new();
    for q in -20..=20_i64 {
        for r in -20..=20_i64 {
            out.push(Coord::new(q * 61, r * 67));
        }
    }
    for (q, r) in [
        (0, 0),
        (1, -1),
        (-1, 1),
        (127, 128),
        (-129, -128),
        (511, -512),
        (12_345, -6_789),
        (n, 0),
        (0, n),
        (-n, 0),
        (0, -n),
        (n, -n),
        (-n, n),
    ] {
        out.push(Coord::new(q, r));
    }
    out
}

/// Bit-exact comparison, because `Tile`'s `PartialEq` is a float comparison and
/// two different bit patterns can compare equal — `-0.0 == 0.0`, and a
/// not-a-number equals nothing including itself.
fn assert_bit_identical(left: &[Tile], right: &[Tile], what: &str) {
    assert_eq!(left.len(), right.len(), "{what}: different lengths");
    for (index, (a, b)) in left.iter().zip(right).enumerate() {
        assert_eq!(a.coord, b.coord, "{what} at {index}");
        for (name, x, y) in [
            ("elevation_value", a.elevation_value, b.elevation_value),
            ("heat_value", a.heat_value, b.heat_value),
            ("moisture_value", a.moisture_value, b.moisture_value),
            ("relief_value", a.relief_value, b.relief_value),
        ] {
            assert_eq!(
                x.to_bits(),
                y.to_bits(),
                "{what} at {index} ({:?}): {name} {x} against {y}",
                a.coord
            );
        }
        assert_eq!(a.elevation, b.elevation, "{what} at {index}");
        assert_eq!(a.climate, b.climate, "{what} at {index}");
        assert_eq!(a.terrain, b.terrain, "{what} at {index}");
    }
}

#[test]
fn a_batch_fill_matches_tile_by_tile_generation() {
    // Section 20: batch generation is an optimization only.
    let generator = Generator::with_defaults(SEED);
    let coords = coords();
    let single: Vec<Tile> = coords.iter().map(|c| generator.tile(*c)).collect();

    assert_bit_identical(&generator.tiles(&coords), &single, "tiles");

    let mut buffer = vec![single[0]; coords.len()];
    generator.tiles_into(&coords, &mut buffer);
    assert_bit_identical(&buffer, &single, "tiles_into");
}

#[test]
fn a_reused_buffer_is_fully_overwritten() {
    // `tiles_into` exists so a caller can keep one buffer across frames. A slot
    // left carrying the previous frame's tile would be a subtle, intermittent
    // rendering bug rather than a crash.
    let generator = Generator::with_defaults(SEED);
    let first: Vec<Coord> = coords().into_iter().take(64).collect();
    let second: Vec<Coord> = coords().into_iter().rev().take(64).collect();

    let mut buffer = generator.tiles(&first);
    generator.tiles_into(&second, &mut buffer);
    assert_bit_identical(&buffer, &generator.tiles(&second), "reused buffer");
}

#[test]
#[should_panic(expected = "one output slot per coordinate")]
fn a_mismatched_output_length_panics_rather_than_filling_part_of_it() {
    let generator = Generator::with_defaults(SEED);
    let coords = coords();
    let mut buffer = vec![generator.tile(Coord::ORIGIN); coords.len() - 1];
    generator.tiles_into(&coords, &mut buffer);
}

#[test]
fn generation_order_does_not_affect_any_value() {
    // Section 30.2. Forwards, backwards, and interleaved.
    let generator = Generator::with_defaults(SEED);
    let coords = coords();
    let forward = generator.tiles(&coords);

    let mut backward: Vec<Tile> = coords.iter().rev().map(|c| generator.tile(*c)).collect();
    backward.reverse();
    assert_bit_identical(&backward, &forward, "reversed");

    let mut interleaved = vec![forward[0]; coords.len()];
    for index in (0..coords.len()).step_by(2) {
        interleaved[index] = generator.tile(coords[index]);
    }
    for index in (1..coords.len()).step_by(2) {
        interleaved[index] = generator.tile(coords[index]);
    }
    assert_bit_identical(&interleaved, &forward, "interleaved");
}

#[test]
fn a_rayon_fill_agrees_with_a_sequential_one_bit_for_bit() {
    // Section 30.3, and the reason section 20 permits the parallel fill at all.
    // The chunk sizes are deliberately awkward so the work-stealing split
    // cannot line up with anything.
    let generator = Generator::with_defaults(SEED);
    let coords = coords();
    let sequential = generator.tiles(&coords);

    let mapped: Vec<Tile> = coords.par_iter().map(|c| generator.tile(*c)).collect();
    assert_bit_identical(&mapped, &sequential, "par_iter map");

    for chunk in [1_usize, 3, 7, 64, 1_000] {
        let mut buffer = vec![sequential[0]; coords.len()];
        buffer
            .par_chunks_mut(chunk)
            .zip(coords.par_chunks(chunk))
            .for_each(|(out, part)| generator.tiles_into(part, out));
        assert_bit_identical(&buffer, &sequential, &format!("par chunks of {chunk}"));
    }
}

#[test]
fn several_threads_generating_at_once_see_identical_tiles() {
    // Section 30.3 without rayon in the way: this compiles only if
    // `Generator: Sync`, and it passes only if thread scheduling cannot reach
    // any value. Each worker walks the list from a different offset, so no two
    // threads visit in the same order.
    let generator = Generator::with_defaults(SEED);
    let coords = coords();
    let expected = generator.tiles(&coords);

    std::thread::scope(|scope| {
        for worker in 0..8_usize {
            let generator = &generator;
            let coords = &coords;
            let expected = &expected;
            scope.spawn(move || {
                for step in 0..coords.len() {
                    let index = (step + worker * 13) % coords.len();
                    let tile = generator.tile(coords[index]);
                    assert_eq!(
                        tile.elevation_value.to_bits(),
                        expected[index].elevation_value.to_bits()
                    );
                    assert_eq!(
                        tile.relief_value.to_bits(),
                        expected[index].relief_value.to_bits()
                    );
                    assert_eq!(tile.elevation, expected[index].elevation);
                }
            });
        }
    });
}

#[test]
fn a_region_is_a_hexagon_of_the_documented_size_and_order() {
    // Derived from the closed form rather than from what the code produced:
    // a hexagon of radius `n` holds `1 + 3n(n+1)` tiles.
    let generator = Generator::with_defaults(SEED);
    for radius in [0_u32, 1, 2, 5, 17] {
        let tiles = generator.region(Coord::new(-4_321, 987), radius);
        let n = u64::from(radius);
        assert_eq!(tiles.len() as u64, 1 + 3 * n * (n + 1), "radius {radius}");

        // Every tile is within `radius` steps of the center, and the ordering
        // is the documented one: ascending offset, `dq` then `dr`.
        let center = Coord::new(-4_321, 987);
        let mut expected = Vec::new();
        let radius = i64::from(radius);
        for dq in -radius..=radius {
            for dr in (-radius).max(-dq - radius)..=radius.min(-dq + radius) {
                assert!(
                    (-dq - dr).abs() <= radius,
                    "({dq}, {dr}) is outside the hexagon"
                );
                expected.push(Coord::new(
                    i64::from(center.q()) + dq,
                    i64::from(center.r()) + dr,
                ));
            }
        }
        let actual: Vec<Coord> = tiles.iter().map(|t| t.coord).collect();
        assert_eq!(actual, expected, "radius {radius}");
    }
}

#[test]
fn a_region_matches_tile_by_tile_generation() {
    let generator = Generator::with_defaults(SEED);
    let region = generator.region(Coord::new(19, -23), 6);
    let single: Vec<Tile> = region.iter().map(|t| generator.tile(t.coord)).collect();
    assert_bit_identical(&region, &single, "region");
}

#[test]
fn a_region_straddling_a_wrapped_edge_returns_canonical_tiles() {
    // Section 7.1: a region centered on an edge runs off it, and every tile it
    // names must come back as the canonical representative rather than as an
    // out-of-range coordinate or a panic.
    let generator = Generator::with_defaults(SEED);
    let n = i64::from(i16::MAX);
    for center in [(n, 0), (0, n), (-n, 0), (0, -n), (n, -n), (-n, n)] {
        let center = Coord::new(center.0, center.1);
        let region = generator.region(center, 3);
        assert_eq!(region.len(), 1 + 3 * 3 * 4);
        for tile in &region {
            assert_eq!(generator.tile(tile.coord), *tile);
        }
        assert!(region.iter().any(|t| t.coord == center));
    }
}

#[test]
#[should_panic(expected = "WORLD_RADIUS")]
fn a_region_larger_than_the_world_is_rejected() {
    let generator = Generator::with_defaults(SEED);
    let radius = u32::try_from(wgvb::WORLD_RADIUS).expect("the world radius fits in u32") + 1;
    let _ = generator.region(Coord::ORIGIN, radius);
}
