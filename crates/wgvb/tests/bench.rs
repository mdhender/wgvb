//! The section 31 measurements, and the question they exist to answer.
//!
//! `DESIGN.md` section 27.6 says not to build a tile cache in the first
//! implementation and to **measure first**, because a cache that is never
//! faster than regeneration is pure liability: a fingerprint to validate and a
//! staleness bug to hit. This file is that measurement.
//!
//! ```sh
//! cargo test --release -p wgvb --test bench -- --ignored --nocapture
//! ```
//!
//! They are `#[ignore]`d timing loops rather than `#[bench]` functions because
//! `#[bench]` is a nightly feature and this workspace is pinned to stable, and
//! `criterion` is a dependency the design did not take. Section 31 says a plain
//! `--release` timing loop is enough to answer the only question that matters
//! early, and it is: the answer is not close.
//!
//! **A debug build is ten to thirty times slower.** These print throughput
//! rather than asserting on it — a timing assertion on shared CI hardware fails
//! for reasons that have nothing to do with the code. The floor the design
//! names, tens of thousands of tiles per second, is a signal that something is
//! wrong if missed rather than a goal, and
//! [`the_floor_from_section_31_is_not_in_danger`] is the one assertion here.

use std::hint::black_box;
use std::time::{Duration, Instant};

use wgvb::{Coord, DEFAULT_CHUNK_SIZE_HEXES, Generator};

/// A tile count as an `f64`, for arithmetic that is about to be printed.
///
/// Exact for every count these loops can reach, and a lossy cast written out is
/// still a lossy cast: section 25.5 does not care that this one is in a test.
fn as_f64(tiles: usize) -> f64 {
    f64::from(u32::try_from(tiles).expect("a benchmark counts fewer than four billion tiles"))
}

/// Prints one measurement in the form the design's question needs.
fn report(name: &str, tiles: usize, elapsed: Duration) {
    let per_second = as_f64(tiles) / elapsed.as_secs_f64();
    let nanos_each = elapsed.as_secs_f64() * 1e9 / as_f64(tiles);
    println!(
        "{name:28} {tiles:>9} tiles in {:>8.3} ms  =  {per_second:>12.0} tiles/s  \
         ({nanos_each:.0} ns each)",
        elapsed.as_secs_f64() * 1e3,
    );
}

/// Every coordinate of a square window, in a fixed order.
fn window(origin: Coord, cols: i64, rows: i64) -> Vec<Coord> {
    let mut coords =
        Vec::with_capacity(usize::try_from(cols * rows).expect("a window of non-negative size"));
    for dq in 0..cols {
        for dr in 0..rows {
            coords.push(Coord::new(
                i64::from(origin.q()) + dq,
                i64::from(origin.r()) + dr,
            ));
        }
    }
    coords
}

#[test]
#[ignore = "a timing loop; run with --release --ignored --nocapture"]
fn bench_tile() {
    let generator = Generator::with_defaults(0x5747_5642);
    let coords = window(Coord::new(-4_000, 2_500), 256, 256);

    // One warm pass, so the measured pass is not paying for first-touch page
    // faults on the coordinate vector.
    for coord in &coords {
        black_box(generator.tile(*coord));
    }

    let start = Instant::now();
    for coord in &coords {
        black_box(generator.tile(*coord));
    }
    report("tile", coords.len(), start.elapsed());
}

#[test]
#[ignore = "a timing loop; run with --release --ignored --nocapture"]
fn bench_chunk() {
    // One chunk, the unit section 23 defines, repeated enough times to measure.
    let generator = Generator::with_defaults(0x5747_5642);
    let edge = i64::from(DEFAULT_CHUNK_SIZE_HEXES);
    let coords = window(Coord::new(1_024, -2_048), edge, edge);
    let mut tiles = vec![generator.tile(Coord::ORIGIN); coords.len()];

    let rounds = 200;
    let start = Instant::now();
    for _ in 0..rounds {
        generator.tiles_into(&coords, &mut tiles);
        black_box(&tiles);
    }
    report("chunk (32x32)", coords.len() * rounds, start.elapsed());
}

#[test]
#[ignore = "a timing loop; run with --release --ignored --nocapture"]
fn bench_region_radius_32() {
    let generator = Generator::with_defaults(0x5747_5642);
    let center = Coord::new(-17_000, 9_000);
    let count = generator.region(center, 32).len();

    let rounds = 200;
    let start = Instant::now();
    for _ in 0..rounds {
        black_box(generator.region(center, 32));
    }
    report("region (radius 32)", count * rounds, start.elapsed());
}

#[test]
#[ignore = "a timing loop; run with --release --ignored --nocapture"]
fn bench_relief() {
    // Relief costs seven elevation evaluations, so it is the expensive thing a
    // renderer can ask for and the one a cache would be most tempting for.
    let generator = Generator::with_defaults(0x5747_5642);
    let coords = window(Coord::new(600, -600), 128, 128);

    let start = Instant::now();
    for coord in &coords {
        black_box(generator.relief(*coord));
    }
    report("relief", coords.len(), start.elapsed());
}

#[test]
fn the_floor_from_section_31_is_not_in_danger() {
    // Section 31 names tens of thousands of tiles per second as a floor that
    // signals something is wrong if missed. This runs in every `cargo test`,
    // including a debug build, so the bar is set an order of magnitude below
    // that floor: it is a smoke alarm for an accidental quadratic or an
    // accidental allocation per tile, not a performance target.
    //
    // The real numbers live in `bench_tile` and are three orders of magnitude
    // above this.
    let generator = Generator::with_defaults(1);
    let coords = window(Coord::new(0, 0), 32, 32);

    let start = Instant::now();
    let tiles = generator.tiles(&coords);
    let elapsed = start.elapsed();

    assert_eq!(tiles.len(), coords.len());
    let per_second = as_f64(tiles.len()) / elapsed.as_secs_f64();
    assert!(
        per_second > 1_000.0,
        "generated {per_second:.0} tiles/s, which is below even the debug-build \
         smoke test; something is wrong with the generation path"
    );
}
