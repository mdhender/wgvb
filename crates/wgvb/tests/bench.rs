//! The throughput measurements of `DESIGN.md` section 31.
//!
//! Section 31 states no performance target, deliberately. What it specifies is
//! how a number is produced and what has to be held fixed for two numbers to be
//! comparable — against WGVA, against another target, or against this workspace
//! before a change. This file is the harness.
//!
//! ```sh
//! cargo test --release -p wgvb --test bench -- --ignored --nocapture
//! ```
//!
//! Section 27.6 is the decision that first needed it: do not build a tile cache
//! in the first implementation, and **measure first**, because a cache that is
//! never faster than regeneration is pure liability — a fingerprint to validate
//! and a staleness bug to hit.
//!
//! The rows are chosen so that the differences between them mean something.
//! [`bench_tile`] against [`bench_chunk`] and [`bench_region_radius_32`] is the
//! batching question, which a pure function of its own coordinate should
//! answer "nothing to amortize". [`bench_relief`] against [`bench_tile`] splits
//! one tile into its seven elevation evaluations and everything classification
//! adds on top.
//!
//! These are `#[ignore]`d timing loops rather than `#[bench]` functions because
//! `#[bench]` is a nightly feature and this workspace is pinned to stable, and
//! `criterion` is a dependency the design did not take.
//!
//! **A debug build is ten to thirty times slower.** They print throughput
//! rather than asserting on it: a timing assertion on shared CI hardware fails
//! for reasons that have nothing to do with the code. The one assertion in this
//! file, [`generation_has_not_acquired_an_accidental_quadratic`], is set three
//! orders of magnitude below a release build and is a correctness tripwire
//! rather than a performance bar.

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
fn generation_has_not_acquired_an_accidental_quadratic() {
    // Not a performance target. Section 31 deliberately states none: it says
    // how throughput is measured and leaves what the number should be to
    // whoever is comparing two of them.
    //
    // This is a smoke alarm, and it runs in every `cargo test` including a
    // debug build, so the bar is three orders of magnitude below what a release
    // build actually does. What trips it is an accidental quadratic or an
    // allocation per tile — a defect in the generation path wearing a
    // stopwatch, not a regression in how fast the arithmetic is.
    //
    // Real figures come from `bench_tile` and the rest of this file, per
    // section 31.1.
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
