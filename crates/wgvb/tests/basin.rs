//! Basin influence, over large samples.
//!
//! `DESIGN.md` sections 11.1, 15, 17, 30.2, 30.3, 30.4, 30.7, 30.8, 30.10, and
//! 33.5.
//!
//! # What these tests are for
//!
//! Basin influence is a *tendency*, so most of what can be said about it is
//! what can be said about any deterministic field: it stays in range, it moves
//! smoothly where a player walks and freely across the world, it does not
//! change with the sign of a coordinate, and a wrapped coordinate produces
//! bit-identical values on all six edges.
//!
//! Two claims are specific to this phase and are the reason the file exists.
//!
//! The first is **bounded**. Section 15 and section 17 forbid global
//! connectivity and flood fill, and the observable consequence is that a
//! coordinate's basin influence cannot depend on anything but that coordinate:
//! generating one tile, a thousand tiles around it, or the same tile after a
//! walk across the world must give the same number. That is tested here as
//! order independence and concurrent determinism, which is the strongest thing
//! a test can say about the absence of a traversal.
//!
//! The second is that basin influence **does not reach elevation**. The whole
//! reason phase 6 could be added without moving a golden value is that the
//! arrow runs one way: basins read the world, and the world does not read
//! basins. A test compares the elevation of a strongly basined tile against
//! the elevation the same seed produced before, through the golden table's own
//! promise, and a second one asserts the two fields are not the same picture.

use wgvb::{Config, Coord, Generator, Tile};

/// Seeds every distribution test runs over, so a bound that only one world
/// satisfies fails rather than passing on the lucky seed.
const SEEDS: [u64; 4] = [1, 0xfeed_face, 0x0123_4567_89ab_cdef, 42];

/// A wide, decorrelated spread of coordinates.
///
/// The strides are well above the longest basin feature — six hundred hexes —
/// so consecutive samples are independent draws rather than a walk across one
/// depression. Negative on both axes.
fn spread() -> Vec<Coord> {
    let mut out = Vec::new();
    for i in -60..60_i64 {
        for j in -60..60_i64 {
            out.push(Coord::new(i * 701 + 11, j * 809 - 7));
        }
    }
    out
}

/// A contiguous block of tiles, for the tests that care about neighbors.
fn block(origin: (i64, i64), edge: i64) -> Vec<Coord> {
    let mut out = Vec::new();
    for q in 0..edge {
        for r in 0..edge {
            out.push(Coord::new(origin.0 + q, origin.1 + r));
        }
    }
    out
}

/// Standard deviation of a sample.
fn standard_deviation(values: &[f64]) -> f64 {
    let count = f64::from(u32::try_from(values.len()).expect("a test sample fits in u32"));
    let mean = values.iter().sum::<f64>() / count;
    let variance = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / count;
    variance.sqrt()
}

/// Pearson correlation of two equally long samples.
fn correlation(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len());
    let count = f64::from(u32::try_from(a.len()).expect("a test sample fits in u32"));
    let mean_a = a.iter().sum::<f64>() / count;
    let mean_b = b.iter().sum::<f64>() / count;
    let mut covariance = 0.0_f64;
    let mut variance_a = 0.0_f64;
    let mut variance_b = 0.0_f64;
    for (x, y) in a.iter().zip(b) {
        covariance += (x - mean_a) * (y - mean_b);
        variance_a += (x - mean_a) * (x - mean_a);
        variance_b += (y - mean_b) * (y - mean_b);
    }
    covariance / (variance_a.sqrt() * variance_b.sqrt())
}

#[test]
fn both_scalars_stay_in_range_and_use_a_useful_part_of_it() {
    // Section 30.8. Both composites are weighted averages of fields already in
    // `[-1, +1]`, so the range holds by construction — but a world that never
    // left the middle tenth of it would make every threshold read against it
    // meaningless, and that is not something construction guarantees.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let mut basin = (1.0_f64, -1.0_f64);
        let mut volcanic = (1.0_f64, -1.0_f64);
        for coord in spread() {
            let sample = generator.sample(coord);
            for (name, value) in [
                ("basin", sample.basin_influence),
                ("volcanic", sample.volcanic),
            ] {
                assert!(
                    value.is_finite() && (-1.0..=1.0).contains(&value),
                    "seed {seed:#x} at {coord:?}: {name} is {value}"
                );
            }
            basin = (
                basin.0.min(sample.basin_influence),
                basin.1.max(sample.basin_influence),
            );
            volcanic = (
                volcanic.0.min(sample.volcanic),
                volcanic.1.max(sample.volcanic),
            );
        }
        assert!(
            basin.0 < -0.3 && basin.1 > 0.3,
            "seed {seed:#x}: basin only reached {basin:?}"
        );
        assert!(
            volcanic.0 < -0.3 && volcanic.1 > 0.3,
            "seed {seed:#x}: volcanic only reached {volcanic:?}"
        );
    }
}

#[test]
fn basins_are_smooth_at_tile_scale_and_varied_at_world_scale() {
    // The same shape of claim `tests/climate.rs` makes about climate, and for
    // the same reason: a field that only satisfied one half would be either
    // per-tile speckle or a constant. The shortest basin field is forty hexes,
    // so a six-mile step cannot move the composite far.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let near = block((-7_000, 2_200), 48);

        let mut step_total = 0.0_f64;
        let mut steps = 0_u32;
        for coord in &near {
            let here = generator.sample(*coord).basin_influence;
            for direction in 0..6 {
                step_total +=
                    (generator.sample(coord.neighbor(direction)).basin_influence - here).abs();
                steps += 1;
            }
        }
        let step = step_total / f64::from(steps);

        let values: Vec<f64> = spread()
            .iter()
            .map(|c| generator.sample(*c).basin_influence)
            .collect();
        let spread_of_world = standard_deviation(&values);

        assert!(step > 0.0, "seed {seed:#x}: basin influence is constant");
        assert!(
            step < 0.02,
            "seed {seed:#x}: basin influence moves {step:.5} between neighbors"
        );
        assert!(
            spread_of_world > 0.10,
            "seed {seed:#x}: basin influence varies only {spread_of_world:.4} across the world"
        );
        assert!(
            spread_of_world > step * 15.0,
            "seed {seed:#x}: basin influence varies {spread_of_world:.4} across the world \
             against {step:.5} between neighbors, which is speckle rather than geography"
        );
    }
}

#[test]
fn basin_influence_is_not_the_elevation_field_under_another_name() {
    // If the two correlated strongly, "basin" would be a second reading of the
    // ground's height and the layer would be telling nobody anything. They are
    // built from different domains at different wavelengths, so a weak
    // correlation is the expected result.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let coords = spread();
        let basin: Vec<f64> = coords
            .iter()
            .map(|c| generator.sample(*c).basin_influence)
            .collect();
        let elevation: Vec<f64> = coords.iter().map(|c| generator.elevation_at(*c)).collect();
        let r = correlation(&basin, &elevation);
        assert!(
            r.abs() < 0.35,
            "seed {seed:#x}: basin influence and elevation correlate {r:.3}"
        );
    }
}

#[test]
fn no_basin_setting_moves_elevation_relief_or_climate() {
    // The one-way arrow, stated as an experiment rather than as a comment.
    // Elevation is the primary field and its composition is pinned by
    // `tests/golden.rs`; a basin term inside it would move every tile in every
    // world. Two configurations of one seed, differing only in the basin and
    // volcanic settings, must produce the same elevation, relief, heat, and
    // moisture at every coordinate, bit for bit.
    let baseline = Generator::with_defaults(0x8ac1_0000);
    let altered = Generator::new(
        0x8ac1_0000,
        Config {
            basin_broad_wavelength_miles: 1_800.0,
            basin_regional_wavelength_miles: 420.0,
            basin_local_wavelength_miles: 96.0,
            basin_broad_weight: 0.2,
            basin_region_weight: 2.0,
            basin_regional_weight: 1.5,
            basin_local_weight: 0.9,
            basin_broad_octaves: 2,
            volcanic_wavelength_miles: 3_000.0,
            volcanic_field_weight: 0.1,
            volcanic_region_weight: 2.5,
            ..Config::default()
        },
    )
    .expect("configuration is valid");

    let mut basin_moved = 0_u32;
    for coord in spread() {
        let a = baseline.tile(coord);
        let b = altered.tile(coord);
        assert_eq!(
            a.elevation_value.to_bits(),
            b.elevation_value.to_bits(),
            "the basin settings reached elevation at {coord:?}"
        );
        assert_eq!(
            a.relief_value.to_bits(),
            b.relief_value.to_bits(),
            "the basin settings reached relief at {coord:?}"
        );
        assert_eq!(
            a.heat_value.to_bits(),
            b.heat_value.to_bits(),
            "the basin settings reached heat at {coord:?}"
        );
        assert_eq!(
            a.moisture_value.to_bits(),
            b.moisture_value.to_bits(),
            "the basin settings reached moisture at {coord:?}"
        );
        assert_eq!(a.elevation, b.elevation, "{coord:?}");
        assert_eq!(a.climate, b.climate, "{coord:?}");

        if baseline.sample(coord).basin_influence.to_bits()
            != altered.sample(coord).basin_influence.to_bits()
        {
            basin_moved += 1;
        }
    }
    // And the settings do something, or the test above is vacuous.
    assert!(
        basin_moved > 1_000,
        "the altered basin settings moved only {basin_moved} tiles"
    );
}

#[test]
fn the_region_basin_bias_is_visible_in_the_basins_it_biases() {
    // Section 11.1's tendency toward enclosed low ground has to reach the
    // composite named after it. Split the world by the blended bias and
    // require the two halves to differ in the right direction; a bias that had
    // been dropped from the sum would leave them identical.
    let generator = Generator::with_defaults(0x2468_1357);
    let mut high = (0.0_f64, 0_u32);
    let mut low = (0.0_f64, 0_u32);
    let mut restless = (0.0_f64, 0_u32);
    let mut quiet = (0.0_f64, 0_u32);
    for coord in spread() {
        let params = generator.region_params(coord);
        let sample = generator.sample(coord);
        if params.basin_bias > 0.3 {
            high.0 += sample.basin_influence;
            high.1 += 1;
        } else if params.basin_bias < -0.3 {
            low.0 += sample.basin_influence;
            low.1 += 1;
        }
        if params.volcanic > 0.3 {
            restless.0 += sample.volcanic;
            restless.1 += 1;
        } else if params.volcanic < -0.3 {
            quiet.0 += sample.volcanic;
            quiet.1 += 1;
        }
    }
    assert!(
        high.1 > 100 && low.1 > 100 && restless.1 > 100 && quiet.1 > 100,
        "too few tiles either side of a bias"
    );
    assert!(
        high.0 / f64::from(high.1) > low.0 / f64::from(low.1),
        "the region basin bias does not deepen anything"
    );
    assert!(
        restless.0 / f64::from(restless.1) > quiet.0 / f64::from(quiet.1),
        "the region volcanic bias does not stir anything"
    );
}

#[test]
fn basins_are_deterministic_bit_for_bit_and_order_independent() {
    // Sections 2.1 and 30.2, and the observable form of "no flood fill": if a
    // traversal had crept in, the answer would depend on what had been
    // generated before it.
    let generator = Generator::with_defaults(0xba51_0000);
    let coords = spread();

    let forward: Vec<f64> = coords
        .iter()
        .map(|c| generator.sample(*c).basin_influence)
        .collect();
    let mut backward: Vec<f64> = coords
        .iter()
        .rev()
        .map(|c| generator.sample(*c).basin_influence)
        .collect();
    backward.reverse();
    for ((coord, a), b) in coords.iter().zip(&forward).zip(&backward) {
        assert_eq!(a.to_bits(), b.to_bits(), "{coord:?}");
    }

    // And one coordinate generated alone is the same as the same coordinate
    // generated after the whole spread, which is the claim a cache or a
    // traversal would break.
    let alone = Generator::with_defaults(0xba51_0000);
    for (coord, expected) in coords.iter().zip(&forward) {
        assert_eq!(
            alone.sample(*coord).basin_influence.to_bits(),
            expected.to_bits(),
            "{coord:?}"
        );
    }
}

#[test]
fn concurrent_generation_produces_identical_basins() {
    // Section 30.3. Compiles only if `Generator: Sync`, and passes only if no
    // thread schedule can reach a value.
    let generator = Generator::with_defaults(0xba51_0001);
    let g = &generator;
    let coords = spread();
    let expected: Vec<(f64, f64)> = coords
        .iter()
        .map(|c| {
            let s = g.sample(*c);
            (s.basin_influence, s.volcanic)
        })
        .collect();

    std::thread::scope(|scope| {
        for worker in 0..8_usize {
            let coords = &coords;
            let expected = &expected;
            scope.spawn(move || {
                for step in 0..coords.len() {
                    let index = (step + worker * 137) % coords.len();
                    let sample = g.sample(coords[index]);
                    assert_eq!(
                        sample.basin_influence.to_bits(),
                        expected[index].0.to_bits()
                    );
                    assert_eq!(sample.volcanic.to_bits(), expected[index].1.to_bits());
                }
            });
        }
    });
}

#[test]
fn negative_coordinates_are_no_different_from_positive_ones() {
    // Section 30.4. The addressing arithmetic is `div_euclid` and
    // `rem_euclid`, and the classic defect is a distribution that changes sign
    // with the coordinate because a bare `/` mirrored the lattice about the
    // origin.
    let generator = Generator::with_defaults(0x5a5a_5a5a);
    let mut means = Vec::new();
    for quadrant in [(1_i64, 1_i64), (-1, 1), (1, -1), (-1, -1)] {
        let mut basin = 0.0_f64;
        let mut count = 0_u32;
        for i in 0..80_i64 {
            for j in 0..80_i64 {
                let sample = generator.sample(Coord::new(
                    quadrant.0 * (i * 53 + 17),
                    quadrant.1 * (j * 59 + 23),
                ));
                basin += sample.basin_influence;
                count += 1;
            }
        }
        means.push(basin / f64::from(count));
    }
    let lowest = means.iter().copied().fold(f64::INFINITY, f64::min);
    let highest = means.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!(
        highest - lowest < 0.35,
        "the quadrants disagree about basin influence: {means:?}"
    );
}

#[test]
fn a_wrapped_coordinate_produces_exactly_the_same_basin() {
    // Sections 7.1 and 30.10, on all six edges rather than on average. A
    // coordinate and its wrapped image name one tile, so every value on it
    // must be bit-identical.
    let generator = Generator::with_defaults(0xba51_0002);
    let n = i64::from(i16::MAX);
    let mirrors = [
        (2 * n + 1, -n),
        (n + 1, -(2 * n + 1)),
        (-n, -(n + 1)),
        (-(2 * n + 1), n),
        (-(n + 1), 2 * n + 1),
        (n, n + 1),
    ];
    for (q, r) in [
        (0_i64, 0_i64),
        (5, -3),
        (-11, 400),
        (n, -n),
        (-n, n),
        (0, n),
        (n, 0),
        (0, -n),
    ] {
        let canonical = Coord::new(q, r);
        let here = generator.sample(canonical);
        for (mq, mr) in mirrors {
            let wrapped = Coord::new(q + mq, r + mr);
            assert_eq!(wrapped, canonical, "({q}, {r}) + ({mq}, {mr})");
            let there = generator.sample(wrapped);
            assert_eq!(
                there.basin_influence.to_bits(),
                here.basin_influence.to_bits(),
                "({q}, {r}) across ({mq}, {mr})"
            );
            assert_eq!(
                there.volcanic.to_bits(),
                here.volcanic.to_bits(),
                "({q}, {r}) across ({mq}, {mr})"
            );
        }
    }
}

#[test]
fn a_basin_is_wide_enough_to_stand_in() {
    // Geography rather than texture. Count the tiles above a high threshold
    // and require most of them to have neighbors above it too: a field of
    // isolated high tiles would be noise wearing a basin's name, and the
    // wetlands that read it would be one-tile puddles.
    // Four widely separated blocks, pooled: a basin is six hundred hexes
    // across at its coarsest, so a single 96-hex block can easily sit entirely
    // outside one and say nothing.
    let origins = [
        (2_500_i64, 9_100_i64),
        (-7_000, 2_200),
        (11_000, -4_500),
        (-600, -450),
    ];
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let mut deep = 0_u32;
        let mut deep_with_company = 0_u32;
        for origin in origins {
            for coord in block(origin, 96) {
                if generator.sample(coord).basin_influence < 0.2 {
                    continue;
                }
                deep += 1;
                let mut neighbors = 0_u32;
                for direction in 0..6 {
                    if generator.sample(coord.neighbor(direction)).basin_influence >= 0.2 {
                        neighbors += 1;
                    }
                }
                deep_with_company += u32::from(neighbors >= 4);
            }
        }
        assert!(deep > 100, "seed {seed:#x}: only {deep} tiles in a basin");
        let share = f64::from(deep_with_company) / f64::from(deep);
        assert!(
            share > 0.9,
            "seed {seed:#x}: only {share:.3} of basin tiles sit inside a basin"
        );
    }
}

#[test]
fn a_tile_generated_with_its_neighborhood_is_the_tile_generated_alone() {
    // The strongest available statement that nothing here is a flood fill: a
    // whole region generated at once, and the same coordinates generated one
    // at a time from a fresh generator, must agree on every field of every
    // tile.
    let generator = Generator::with_defaults(0xba51_0003);
    let center = Coord::new(-4_321, 8_765);
    let region: Vec<Tile> = generator.region(center, 12);
    let fresh = Generator::with_defaults(0xba51_0003);
    for tile in region {
        assert_eq!(fresh.tile(tile.coord), tile, "{:?}", tile.coord);
    }
}
