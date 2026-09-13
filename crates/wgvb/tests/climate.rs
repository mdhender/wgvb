//! Climate, over large samples.
//!
//! `DESIGN.md` sections 16, 16.1, 30.4, 30.7, 30.8, and 33.5.
//!
//! # What these tests are for
//!
//! The phase 5 exit condition is that climate maps form coherent broad zones
//! rather than tile-level speckle, and speckle is the failure these tests are
//! shaped around. A speckled world passes every bound on a mean and every
//! bound on a band share — that is precisely what makes those bounds
//! insufficient — so coherence is measured as agreement between *neighbors*,
//! which noise cannot fake.
//!
//! The distribution assertions are checked per seed rather than in aggregate,
//! for the reason section 30.8 gives: a threshold on a knife edge is plausible
//! for one world and absurd for the next. The bounds are wide enough that an
//! ordinary retune need not touch this file and tight enough that an empty
//! band, a world that is all one climate, or a ladder that has slid off the end
//! of its distribution fails.
//!
//! Nothing here derives a threshold from the sample. Section 33.5: the
//! configuration says where the bands are, and the sample only says what that
//! produces.

use wgvb::{Config, Coord, Generator, HeatBand, MoistureBand, Tile};

/// Seeds every distribution test runs over, so a bound that only one world
/// satisfies fails rather than passing on the lucky seed.
const SEEDS: [u64; 4] = [1, 0xfeed_face, 0x0123_4567_89ab_cdef, 42];

/// A wide, decorrelated spread of coordinates.
///
/// The strides are well above the longest climate feature, so consecutive
/// samples are independent draws rather than a walk across one zone. Negative
/// on both axes.
fn spread() -> Vec<Coord> {
    let mut out = Vec::new();
    for i in -60..60_i64 {
        for j in -60..60_i64 {
            out.push(Coord::new(i * 37 + 11, j * 41 - 7));
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

/// Band occupancy as a fraction of the sample, indexed by discriminant.
///
/// Through `u32` rather than an `as` cast: the workspace denies a lossy numeric
/// cast, and a test that opts out of the rule it is checking the crate against
/// is a test nobody should trust.
fn shares(counts: [u32; 5], total: usize) -> [f64; 5] {
    let total = f64::from(u32::try_from(total).expect("a test sample fits in u32"));
    counts.map(|count| f64::from(count) / total)
}

fn heat_shares(tiles: &[Tile]) -> [f64; 5] {
    let mut counts = [0_u32; 5];
    for tile in tiles {
        counts[tile.climate.heat as usize] += 1;
    }
    shares(counts, tiles.len())
}

fn moisture_shares(tiles: &[Tile]) -> [f64; 5] {
    let mut counts = [0_u32; 5];
    for tile in tiles {
        counts[tile.climate.moisture as usize] += 1;
    }
    shares(counts, tiles.len())
}

#[test]
fn every_heat_band_is_occupied_and_none_of_them_holds_the_world() {
    // A band no tile ever lands in is a threshold that has drifted off the end
    // of the distribution: the classifier would compile and be wrong. A band
    // holding most of the world is the same defect from the other side.
    let names = ["polar", "cold", "temperate", "warm", "hot"];
    for seed in SEEDS {
        let tiles = Generator::with_defaults(seed).tiles(&spread());
        for (name, share) in names.into_iter().zip(heat_shares(&tiles)) {
            assert!(
                (0.02..=0.55).contains(&share),
                "seed {seed:#x}: {name} holds {share:.4} of the world"
            );
        }
    }
}

#[test]
fn every_moisture_band_is_occupied_and_none_of_them_holds_the_world() {
    let names = ["arid", "dry", "moderate", "humid", "saturated"];
    for seed in SEEDS {
        let tiles = Generator::with_defaults(seed).tiles(&spread());
        for (name, share) in names.into_iter().zip(moisture_shares(&tiles)) {
            assert!(
                (0.02..=0.55).contains(&share),
                "seed {seed:#x}: {name} holds {share:.4} of the world"
            );
        }
    }
}

#[test]
fn the_middle_of_each_ladder_is_the_commonest_part_of_it() {
    // Both composites are weighted averages of zero-mean fields, so the
    // distribution is single-peaked near the middle of the scale. A ladder
    // whose extremes were commoner than its middle would mean the thresholds
    // had slid to one side, which is the failure the per-band bounds above are
    // too loose to catch on their own.
    for seed in SEEDS {
        let tiles = Generator::with_defaults(seed).tiles(&spread());
        for (axis, shares) in [
            ("heat", heat_shares(&tiles)),
            ("moisture", moisture_shares(&tiles)),
        ] {
            assert!(
                shares[2] > shares[0] && shares[2] > shares[4],
                "seed {seed:#x}: the {axis} ladder is not centered: {shares:?}"
            );
        }
    }
}

#[test]
fn both_scales_span_a_useful_part_of_their_range() {
    // Section 30.8 for the climate scales. A world that never leaves the middle
    // third of either wastes the scale and gives the renderer nothing to draw.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let mut heat = (1.0_f64, -1.0_f64);
        let mut moisture = (1.0_f64, -1.0_f64);
        for coord in spread() {
            let tile = generator.tile(coord);
            for (name, value) in [("heat", tile.heat_value), ("moisture", tile.moisture_value)] {
                assert!(
                    value.is_finite() && (-1.0..=1.0).contains(&value),
                    "seed {seed:#x} at {coord:?}: {name} is {value}"
                );
            }
            heat = (heat.0.min(tile.heat_value), heat.1.max(tile.heat_value));
            moisture = (
                moisture.0.min(tile.moisture_value),
                moisture.1.max(tile.moisture_value),
            );
        }
        assert!(
            heat.0 < -0.4 && heat.1 > 0.4,
            "seed {seed:#x}: heat only reached {heat:?}"
        );
        assert!(
            moisture.0 < -0.4 && moisture.1 > 0.4,
            "seed {seed:#x}: moisture only reached {moisture:?}"
        );
    }
}

#[test]
fn adjacent_tiles_almost_never_jump_more_than_one_band() {
    // The exit condition, stated as the thing speckle cannot do. Neighbors are
    // six miles apart; the shortest field either composite reads is a hundred
    // hexes long, so a two-band jump between neighbors would mean a field had
    // collapsed into per-tile noise.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let mut same = (0_u32, 0_u32);
        let mut jumped = (0_u32, 0_u32);
        let mut total = 0_u32;
        for coord in block((11_000, -4_500), 48) {
            let here = generator.tile(coord);
            for direction in 0..6 {
                let there = generator.tile(coord.neighbor(direction));
                let heat = i32::from(here.climate.heat as u8) - i32::from(there.climate.heat as u8);
                let moisture = i32::from(here.climate.moisture as u8)
                    - i32::from(there.climate.moisture as u8);
                same.0 += u32::from(heat == 0);
                same.1 += u32::from(moisture == 0);
                jumped.0 += u32::from(heat.abs() > 1);
                jumped.1 += u32::from(moisture.abs() > 1);
                total += 1;
            }
        }
        for (axis, agreeing, jumped) in [("heat", same.0, jumped.0), ("moisture", same.1, jumped.1)]
        {
            let agreement = f64::from(agreeing) / f64::from(total);
            assert!(
                agreement > 0.9,
                "seed {seed:#x}: only {agreement:.3} of neighbor pairs share a {axis} band"
            );
            assert_eq!(
                jumped, 0,
                "seed {seed:#x}: {jumped} neighbor pairs jumped more than one {axis} band"
            );
        }
    }
}

#[test]
fn climate_is_smooth_at_tile_scale_and_varied_at_world_scale() {
    // Agreement between neighbors is necessary and not sufficient: a field that
    // barely varied at all would pass that too. The exit condition is two
    // claims at once — smooth where a player walks, different where a player
    // travels — so both are measured, and against each other.
    //
    // Deliberately *not* a correlation against a distant block, which is the
    // shape `tests/elevation.rs` uses for relief. It does not transfer: these
    // fields are thousands of miles long, so over a 48-hex block either sample
    // is very nearly a linear ramp, and two linear ramps correlate strongly for
    // a reason that has nothing to do with either being coherent.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let near = block((-7_000, 2_200), 48);
        let far = spread();
        for (axis, of) in [
            ("heat", (|t: &Tile| t.heat_value) as fn(&Tile) -> f64),
            ("moisture", |t: &Tile| t.moisture_value),
        ] {
            let mut step_total = 0.0_f64;
            let mut steps = 0_u32;
            for coord in &near {
                let here = of(&generator.tile(*coord));
                for direction in 0..6 {
                    step_total += (of(&generator.tile(coord.neighbor(direction))) - here).abs();
                    steps += 1;
                }
            }
            let step = step_total / f64::from(steps);

            let values: Vec<f64> = far.iter().map(|c| of(&generator.tile(*c))).collect();
            let spread = standard_deviation(&values);

            assert!(step > 0.0, "seed {seed:#x}: {axis} is constant");
            assert!(
                step < 0.02,
                "seed {seed:#x}: {axis} moves {step:.5} between neighbors"
            );
            assert!(
                spread > 0.12,
                "seed {seed:#x}: {axis} varies only {spread:.4} across the world"
            );
            assert!(
                spread > step * 15.0,
                "seed {seed:#x}: {axis} varies {spread:.4} across the world against \
                 {step:.5} between neighbors, which is speckle rather than zones"
            );
        }
    }
}

/// Standard deviation of a sample.
fn standard_deviation(values: &[f64]) -> f64 {
    let count = f64::from(u32::try_from(values.len()).expect("a test sample fits in u32"));
    let mean = values.iter().sum::<f64>() / count;
    let variance = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / count;
    variance.sqrt()
}

/// Pearson correlation of two equally long samples.
///
/// Written out rather than pulled in: the core crate depends on `serde` and
/// `thiserror` and nothing else, and a test that reaches for a statistics crate
/// to compute a covariance is a dependency nobody needed.
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
fn high_ground_is_colder_than_the_lowland_beside_it() {
    // Elevation cooling, measured where it has to hold rather than on average
    // over the world: the same tile's neighborhood, so the broad heat field and
    // the region bias are all but identical and the only thing left to explain
    // a difference is height.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let mut pairs = 0_u32;
        let mut colder = 0_u32;
        for coord in block((2_500, 9_100), 80) {
            let here = generator.tile(coord);
            // Both tiles have to be land: cooling is zero at and below sea
            // level, so a pair with a water tile in it says nothing about
            // height and everything about the broad field.
            if here.elevation.is_water() {
                continue;
            }
            for direction in 0..6 {
                let there = generator.tile(coord.neighbor(direction));
                if there.elevation.is_water() {
                    continue;
                }
                // Only pairs with a real height difference say anything.
                if (here.elevation_value - there.elevation_value).abs() < 0.01 {
                    continue;
                }
                pairs += 1;
                let (high, low) = if here.elevation_value > there.elevation_value {
                    (here, there)
                } else {
                    (there, here)
                };
                colder += u32::from(high.heat_value <= low.heat_value);
            }
        }
        assert!(
            pairs > 1_000,
            "seed {seed:#x}: too few sloped pairs: {pairs}"
        );
        let share = f64::from(colder) / f64::from(pairs);
        assert!(
            share > 0.99,
            "seed {seed:#x}: only {share:.3} of sloped pairs are colder uphill"
        );
    }
}

#[test]
fn cooling_is_what_makes_the_mountains_cold() {
    // The attribution test for the previous one. With cooling disabled the two
    // populations are the same world sampled twice and must agree; with it on,
    // the high ground has to move down the scale. Two configurations of one
    // seed, so the fields are identical and the only difference is the term
    // under test.
    let coords = spread();
    let cool = Generator::with_defaults(7);
    let flat = Generator::new(
        7,
        Config {
            elevation_cooling: 0.0,
            ..Config::default()
        },
    )
    .expect("configuration is valid");

    let mut high = (0.0_f64, 0.0_f64, 0_u32);
    let mut low = (0.0_f64, 0.0_f64, 0_u32);
    for coord in &coords {
        let cooled = cool.tile(*coord);
        let uncooled = flat.tile(*coord);
        assert_eq!(
            cooled.elevation_value.to_bits(),
            uncooled.elevation_value.to_bits(),
            "cooling reached elevation at {coord:?}"
        );
        assert_eq!(
            cooled.moisture_value.to_bits(),
            uncooled.moisture_value.to_bits(),
            "cooling reached moisture at {coord:?}"
        );

        let bucket = if cooled.elevation_value > 0.4 {
            &mut high
        } else if cooled.elevation.is_water() {
            &mut low
        } else {
            continue;
        };
        bucket.0 += cooled.heat_value;
        bucket.1 += uncooled.heat_value;
        bucket.2 += 1;
    }

    assert!(
        high.2 > 50 && low.2 > 500,
        "too few tiles either side: {} high, {} low",
        high.2,
        low.2
    );
    let high_cooled = high.0 / f64::from(high.2);
    let high_uncooled = high.1 / f64::from(high.2);
    let low_cooled = low.0 / f64::from(low.2);
    let low_uncooled = low.1 / f64::from(low.2);

    // Water is not cooled at all, so those two means are the same number.
    assert!(
        (low_cooled - low_uncooled).abs() < 1.0e-12,
        "water was cooled: {low_cooled} against {low_uncooled}"
    );
    // High ground is, by enough to be a band or more.
    assert!(
        high_uncooled - high_cooled > 0.15,
        "cooling moved high ground by only {}",
        high_uncooled - high_cooled
    );
}

#[test]
fn the_two_axes_are_not_the_same_field_twice() {
    // Section 16.1's independence, measured. Heat and moisture read different
    // domains, so a correlation near zero is the expected result; a strong one
    // would mean a domain had been shared or a bias crossed over, and the
    // two-axis model would be a fiction.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let tiles = generator.tiles(&spread());
        let heat: Vec<f64> = tiles.iter().map(|t| t.heat_value).collect();
        let moisture: Vec<f64> = tiles.iter().map(|t| t.moisture_value).collect();
        let r = correlation(&heat, &moisture);
        assert!(
            r.abs() < 0.35,
            "seed {seed:#x}: heat and moisture correlate {r:.3}"
        );

        // And every pair of bands has to be reachable somewhere in the world,
        // which one combined scale could not do.
        let mut seen = Vec::new();
        for tile in &tiles {
            let pair = (tile.climate.heat as u8, tile.climate.moisture as u8);
            if !seen.contains(&pair) {
                seen.push(pair);
            }
        }
        assert_eq!(
            seen.len(),
            25,
            "seed {seed:#x}: only {} of the 25 band pairs occur",
            seen.len()
        );
    }
}

#[test]
fn the_region_bias_is_visible_in_the_climate_it_biases() {
    // Section 11.1's wet/dry and warm/cool tendencies have to reach the
    // composites they are named for. Split the world by the blended region bias
    // and require the two halves to differ in the right direction; a bias that
    // had been dropped from a composite would leave them identical.
    let generator = Generator::with_defaults(0x2468);
    let mut warm = (0.0_f64, 0_u32);
    let mut cool = (0.0_f64, 0_u32);
    let mut wet = (0.0_f64, 0_u32);
    let mut dry = (0.0_f64, 0_u32);
    for coord in spread() {
        let params = generator.region_params(coord);
        let tile = generator.tile(coord);
        if params.heat_bias > 0.3 {
            warm.0 += tile.heat_value;
            warm.1 += 1;
        } else if params.heat_bias < -0.3 {
            cool.0 += tile.heat_value;
            cool.1 += 1;
        }
        if params.moisture_bias > 0.3 {
            wet.0 += tile.moisture_value;
            wet.1 += 1;
        } else if params.moisture_bias < -0.3 {
            dry.0 += tile.moisture_value;
            dry.1 += 1;
        }
    }
    assert!(
        warm.1 > 100 && cool.1 > 100 && wet.1 > 100 && dry.1 > 100,
        "too few tiles either side of a bias"
    );
    assert!(
        warm.0 / f64::from(warm.1) > cool.0 / f64::from(cool.1),
        "the region heat bias does not warm anything"
    );
    assert!(
        wet.0 / f64::from(wet.1) > dry.0 / f64::from(dry.1),
        "the region moisture bias does not wet anything"
    );
}

#[test]
fn climate_is_deterministic_bit_for_bit_and_order_independent() {
    // Sections 2.1 and 30.2. Forwards, backwards, and interleaved must agree
    // exactly, not approximately.
    let generator = Generator::with_defaults(0xc11a_7e00);
    let coords = spread();

    let forward: Vec<Tile> = coords.iter().map(|c| generator.tile(*c)).collect();
    let mut backward: Vec<Tile> = coords.iter().rev().map(|c| generator.tile(*c)).collect();
    backward.reverse();
    for ((coord, a), b) in coords.iter().zip(&forward).zip(&backward) {
        assert_eq!(a.heat_value.to_bits(), b.heat_value.to_bits(), "{coord:?}");
        assert_eq!(
            a.moisture_value.to_bits(),
            b.moisture_value.to_bits(),
            "{coord:?}"
        );
        assert_eq!(a.climate, b.climate, "{coord:?}");
    }

    let mut interleaved = vec![forward[0]; coords.len()];
    for index in (0..coords.len()).step_by(2) {
        interleaved[index] = generator.tile(coords[index]);
    }
    for index in (1..coords.len()).step_by(2) {
        interleaved[index] = generator.tile(coords[index]);
    }
    assert_eq!(interleaved, forward);
}

#[test]
fn concurrent_generation_produces_identical_climate() {
    // Section 30.3. This compiles only if `Generator: Sync` and passes only if
    // no thread schedule can reach a value.
    let generator = Generator::with_defaults(0x3691);
    let g = &generator;
    let coords = spread();
    let expected: Vec<Tile> = coords.iter().map(|c| g.tile(*c)).collect();

    std::thread::scope(|scope| {
        for worker in 0..8_usize {
            let coords = &coords;
            let expected = &expected;
            scope.spawn(move || {
                // Each worker walks the whole list from a different offset, so
                // no two threads generate in the same order.
                for step in 0..coords.len() {
                    let index = (step + worker * 137) % coords.len();
                    let tile = g.tile(coords[index]);
                    assert_eq!(
                        tile.heat_value.to_bits(),
                        expected[index].heat_value.to_bits()
                    );
                    assert_eq!(
                        tile.moisture_value.to_bits(),
                        expected[index].moisture_value.to_bits()
                    );
                    assert_eq!(tile.climate, expected[index].climate);
                }
            });
        }
    });
}

#[test]
fn negative_coordinates_are_no_different_from_positive_ones() {
    // Section 30.4. The addressing arithmetic is `div_euclid` and `rem_euclid`,
    // and the classic defect is a distribution that changes sign with the
    // coordinate because a bare `/` mirrored the lattice about the origin.
    let generator = Generator::with_defaults(0x5555);
    let mut means = Vec::new();
    for quadrant in [(1_i64, 1_i64), (-1, 1), (1, -1), (-1, -1)] {
        let mut heat = 0.0_f64;
        let mut moisture = 0.0_f64;
        let mut count = 0_u32;
        for i in 0..80_i64 {
            for j in 0..80_i64 {
                let tile = generator.tile(Coord::new(
                    quadrant.0 * (i * 53 + 17),
                    quadrant.1 * (j * 59 + 23),
                ));
                heat += tile.heat_value;
                moisture += tile.moisture_value;
                count += 1;
            }
        }
        means.push((heat / f64::from(count), moisture / f64::from(count)));
    }
    for (axis, of) in [
        ("heat", (|m: &(f64, f64)| m.0) as fn(&(f64, f64)) -> f64),
        ("moisture", |m: &(f64, f64)| m.1),
    ] {
        let lowest = means.iter().map(of).fold(f64::INFINITY, f64::min);
        let highest = means.iter().map(of).fold(f64::NEG_INFINITY, f64::max);
        assert!(
            highest - lowest < 0.35,
            "the quadrants disagree about {axis}: {means:?}"
        );
    }
}

#[test]
fn a_wrapped_coordinate_produces_exactly_the_same_climate() {
    // Sections 7.1 and 30.4, on all six edges rather than on average. A
    // coordinate and its wrapped image name one tile, so every climate value on
    // it must be bit-identical.
    let generator = Generator::with_defaults(0xabc_def);
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
        let tile = generator.tile(canonical);
        for (mq, mr) in mirrors {
            let wrapped = Coord::new(q + mq, r + mr);
            assert_eq!(wrapped, canonical, "({q}, {r}) + ({mq}, {mr})");
            let other = generator.tile(wrapped);
            assert_eq!(
                other.heat_value.to_bits(),
                tile.heat_value.to_bits(),
                "({q}, {r}) across ({mq}, {mr})"
            );
            assert_eq!(
                other.moisture_value.to_bits(),
                tile.moisture_value.to_bits(),
                "({q}, {r}) across ({mq}, {mr})"
            );
            assert_eq!(other.climate, tile.climate, "({q}, {r})");
        }
    }
}

#[test]
fn the_tile_classification_agrees_with_its_own_scalars() {
    // The bands and the numbers must never disagree: a consumer that switches
    // on one and compares the other would see a polar tile at the top of the
    // heat scale.
    let config = Config::default();
    let generator = Generator::new(0x99, config.clone()).expect("configuration is valid");
    for coord in spread() {
        let tile = generator.tile(coord);
        let expected_heat = match tile.heat_value {
            h if h <= config.polar_level => HeatBand::Polar,
            h if h <= config.cold_level => HeatBand::Cold,
            h if h <= config.temperate_level => HeatBand::Temperate,
            h if h <= config.warm_level => HeatBand::Warm,
            _ => HeatBand::Hot,
        };
        let expected_moisture = match tile.moisture_value {
            m if m <= config.arid_level => MoistureBand::Arid,
            m if m <= config.dry_level => MoistureBand::Dry,
            m if m <= config.moderate_level => MoistureBand::Moderate,
            m if m <= config.humid_level => MoistureBand::Humid,
            _ => MoistureBand::Saturated,
        };
        assert_eq!(tile.climate.heat, expected_heat, "{coord:?}");
        assert_eq!(tile.climate.moisture, expected_moisture, "{coord:?}");
    }
}

#[test]
fn moving_a_threshold_moves_the_band_it_bounds_and_nothing_else() {
    // The band ladders are configuration, and that has to mean something: a
    // lower `polar_level` must freeze less of the world, and must not touch the
    // moisture axis at all.
    let coords = spread();
    let mut previous = 1.0_f64;
    for polar_level in [-0.5, -0.4, -0.3, -0.2] {
        let generator = Generator::new(
            3,
            Config {
                polar_level,
                ..Config::default()
            },
        )
        .expect("configuration is valid");
        let tiles = generator.tiles(&coords);
        let polar = heat_shares(&tiles)[HeatBand::Polar as usize];
        assert!(
            polar > previous || previous == 1.0,
            "raising polar_level to {polar_level} did not freeze more: {polar} against {previous}"
        );
        previous = polar;

        let baseline = Generator::with_defaults(3).tiles(&coords);
        assert_eq!(
            moisture_shares(&tiles),
            moisture_shares(&baseline),
            "polar_level {polar_level} reached the moisture axis"
        );
    }
}
