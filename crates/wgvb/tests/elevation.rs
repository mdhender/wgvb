//! Elevation, relief, and their classification, over large samples.
//!
//! `DESIGN.md` sections 14, 15, 18, 30.7, 30.8, and 33.5.
//!
//! # What these tests are for
//!
//! Section 30.8's real target is a threshold sitting on a knife edge: a world
//! where the land fraction happens to be plausible for one seed and is a
//! flooded plain for the next, or a band that one tuning pass away contains no
//! tiles at all. So every distribution assertion below is checked per seed as
//! well as in aggregate, and the bounds are wide enough that an ordinary
//! retune does not have to touch this file — while still being tight enough
//! that a world with no ocean, no mountains, or no coastline fails.
//!
//! Nothing here derives a threshold from the sample. Section 33.5: that is the
//! defect these tests exist to prevent, not a shortcut they may take. The
//! configuration says where sea level is; the sample only says what that
//! produces.

use wgvb::{Config, Coord, Elevation, Generator, Tile};

/// Seeds every distribution test runs over, so a bound that only one world
/// satisfies fails rather than passing on the lucky seed.
const SEEDS: [u64; 4] = [1, 0xfeed_face, 0x0123_4567_89ab_cdef, 42];

/// A wide, decorrelated spread of coordinates.
///
/// Strided by two coprime numbers well above the longest feature the composite
/// has at that scale, so consecutive samples are independent draws rather than
/// a walk across one continent. Includes negatives on both axes.
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

/// One count as a fraction of another.
///
/// Through `u32` rather than an `as` cast: the workspace denies a lossy numeric
/// cast, and a test that opts out of the rule it is checking the crate against
/// is a test nobody should trust.
fn share(count: usize, total: usize) -> f64 {
    let count = u32::try_from(count).expect("a test sample fits in u32");
    let total = u32::try_from(total).expect("a test sample fits in u32");
    f64::from(count) / f64::from(total)
}

/// Band occupancy as a fraction of the sample, indexed by discriminant.
fn band_shares(tiles: &[Tile]) -> [f64; 6] {
    let mut counts = [0_u32; 6];
    for tile in tiles {
        counts[tile.elevation as usize] += 1;
    }
    let total = f64::from(u32::try_from(tiles.len()).expect("a test sample fits in u32"));
    counts.map(|count| f64::from(count) / total)
}

#[test]
fn the_land_fraction_is_plausible_and_stable_across_seeds() {
    // Section 15 and 30.8. The defaults aim at roughly the terrestrial figure,
    // just under a third, and the bound is what separates "a world with
    // continents and oceans" from "a flooded plain" or "a desert planet".
    let coords = spread();
    for seed in SEEDS {
        let tiles = Generator::with_defaults(seed).tiles(&coords);
        let land = tiles.iter().filter(|t| t.elevation.is_land()).count();
        let fraction = share(land, tiles.len());
        assert!(
            (0.20..=0.45).contains(&fraction),
            "seed {seed:#x} put {fraction:.3} of the world above sea level"
        );
    }
}

#[test]
fn every_elevation_band_is_occupied_and_the_land_bands_fall_away_with_height() {
    // A band no tile ever lands in is a threshold that has drifted off the end
    // of the distribution — the classifier would compile and be wrong.
    for seed in SEEDS {
        let tiles = Generator::with_defaults(seed).tiles(&spread());
        let shares = band_shares(&tiles);
        let names = [
            "deep water",
            "shallow water",
            "lowland",
            "upland",
            "highland",
            "mountain",
        ];
        for (name, share) in names.into_iter().zip(shares) {
            assert!(
                share > 0.001,
                "seed {seed:#x}: {name} holds only {share:.5} of the world"
            );
        }

        // Higher ground is rarer than the ground below it. Not a law of
        // geography, but it is what the configured ladder claims, and a
        // retune that inverted it would mean the thresholds no longer say
        // what their names say.
        let land = &shares[Elevation::Lowland as usize..];
        for index in 1..land.len() {
            assert!(
                land[index] < land[index - 1],
                "seed {seed:#x}: land bands do not fall away with height: {land:?}"
            );
        }

        // Deep water dominates the ocean; the shelf is a fringe of it, which
        // is what the two water band names claim.
        let deep = shares[Elevation::DeepWater as usize];
        let shallow = shares[Elevation::ShallowWater as usize];
        assert!(
            deep > shallow * 2.0,
            "seed {seed:#x}: deep {deep:.3} against shallow {shallow:.3}"
        );
    }
}

#[test]
fn the_elevation_scale_reaches_both_ends_without_piling_up_at_either() {
    // Section 14 puts deep ocean at -1 and extreme highland at +1. A world that
    // never leaves the middle third wastes the scale and gives the palette
    // nothing to work with; one that saturates has lost the shaping's
    // monotonicity or its clamp.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let mut lowest = 1.0_f64;
        let mut highest = -1.0_f64;
        let mut pinned = 0_u32;
        let coords = spread();
        for coord in &coords {
            let elevation = generator.elevation_at(*coord);
            assert!(
                (-1.0..=1.0).contains(&elevation),
                "seed {seed:#x} at {coord:?}: {elevation}"
            );
            lowest = lowest.min(elevation);
            highest = highest.max(elevation);
            pinned += u32::from(elevation.abs() >= 1.0);
        }
        assert!(lowest < -0.7, "seed {seed:#x} never got deep: {lowest}");
        assert!(highest > 0.5, "seed {seed:#x} never got high: {highest}");
        let pinned = share(
            usize::try_from(pinned).expect("a count fits in usize"),
            coords.len(),
        );
        assert!(
            pinned < 0.001,
            "seed {seed:#x} clamped {pinned:.4} of tiles"
        );
    }
}

#[test]
fn moving_sea_level_moves_the_land_fraction_the_way_it_should() {
    // The knob section 15 names has to actually be the knob, and it has to be
    // monotone: a higher sea level drowns more of the world, always.
    let coords = spread();
    let mut previous = 1.0_f64;
    for sea_level in [-0.10, -0.05, 0.0, 0.05, 0.10] {
        let config = Config {
            sea_level,
            ..Config::default()
        };
        let generator = Generator::new(7, config).expect("configuration is valid");
        let tiles = generator.tiles(&coords);
        let land = tiles.iter().filter(|t| t.elevation.is_land()).count();
        let fraction = share(land, tiles.len());
        assert!(
            fraction < previous,
            "sea level {sea_level} did not drown anything: {fraction} against {previous}"
        );
        previous = fraction;
    }
}

#[test]
fn neighboring_elevations_stay_close_together() {
    // Section 30.7, on the elevation scalar rather than on the raw composite.
    // Adjacent tiles are six miles apart and the shortest wavelength in the
    // composite is thirty-six, so a neighbor pair cannot jump a large fraction
    // of the range. The bound is on the distribution, not on any one pair: a
    // handful of steep pairs is a mountain, a fat tail is per-tile noise.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let mut worst = 0.0_f64;
        let mut total = 0.0_f64;
        let mut count = 0_u32;
        let mut large = 0_u32;
        for coord in block((-3_000, 1_500), 60) {
            let here = generator.elevation_at(coord);
            for direction in 0..6 {
                let delta = (generator.elevation_at(coord.neighbor(direction)) - here).abs();
                worst = worst.max(delta);
                total += delta;
                count += 1;
                large += u32::from(delta > 0.1);
            }
        }
        let mean = total / f64::from(count);
        assert!(mean > 0.0, "seed {seed:#x}: the field is constant");
        assert!(mean < 0.03, "seed {seed:#x}: mean neighbor step {mean}");
        assert!(worst < 0.25, "seed {seed:#x}: worst neighbor step {worst}");
        let tail = f64::from(large) / f64::from(count);
        assert!(
            tail < 0.01,
            "seed {seed:#x}: {tail:.4} of steps exceeded 0.1"
        );
    }
}

#[test]
fn land_and_water_form_regions_rather_than_speckle() {
    // The phase 4 exit condition asks for coherent oceans and coastlines, and
    // the thing that would quietly break it is per-tile classification noise —
    // section 33.1's failure mode arriving through the back door of a
    // too-strong fine scale. A coherent map has most tiles agreeing with most
    // of their neighbors about whether they are wet.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let mut agreeing = 0_u32;
        let mut total = 0_u32;
        for coord in block((11_000, -4_500), 60) {
            let wet = generator.tile(coord).elevation.is_water();
            for direction in 0..6 {
                let neighbor = generator.tile(coord.neighbor(direction));
                agreeing += u32::from(neighbor.elevation.is_water() == wet);
                total += 1;
            }
        }
        let share = f64::from(agreeing) / f64::from(total);
        assert!(
            share > 0.9,
            "seed {seed:#x}: only {share:.3} of neighbor pairs agree about water"
        );
    }
}

#[test]
fn relief_spans_its_range_without_saturating() {
    // Section 30.8 for the relief scale. If almost everything reads as maximum
    // relief the reference slope is too small and relief carries no
    // information; if nothing does, it is too large.
    for seed in SEEDS {
        let generator = Generator::with_defaults(seed);
        let coords = spread();
        let mut total = 0.0_f64;
        let mut saturated = 0_u32;
        let mut flat = 0_u32;
        let mut highest = 0.0_f64;
        for coord in &coords {
            let relief = generator.relief(*coord);
            assert!(
                (0.0..=1.0).contains(&relief),
                "seed {seed:#x} at {coord:?}: {relief}"
            );
            total += relief;
            highest = highest.max(relief);
            saturated += u32::from(relief >= 1.0);
            flat += u32::from(relief < 0.02);
        }
        let count = f64::from(u32::try_from(coords.len()).expect("a test sample fits in u32"));
        let mean = total / count;
        assert!(
            (0.15..=0.5).contains(&mean),
            "seed {seed:#x}: mean relief {mean}"
        );
        assert!(highest > 0.8, "seed {seed:#x}: steepest relief {highest}");
        assert!(
            f64::from(saturated) / count < 0.02,
            "seed {seed:#x}: too much relief is pinned at one"
        );
        assert!(
            f64::from(flat) / count < 0.02,
            "seed {seed:#x}: too much relief is pinned at zero"
        );
    }
}

#[test]
fn relief_is_higher_in_rough_places_than_in_smooth_ones() {
    // Relief has to be a measurement of the ground rather than a second noise
    // field: the tiles the ridge term is strongest on must be the steep ones.
    let generator = Generator::with_defaults(0x1234);
    let mut rough = (0.0_f64, 0_u32);
    let mut smooth = (0.0_f64, 0_u32);
    for coord in spread() {
        let sample = generator.sample(coord);
        let relief = generator.relief(coord);
        if sample.roughness > 0.3 {
            rough.0 += relief;
            rough.1 += 1;
        } else if sample.roughness < -0.3 {
            smooth.0 += relief;
            smooth.1 += 1;
        }
    }
    assert!(rough.1 > 100 && smooth.1 > 100, "too few tiles either side");
    let rough_mean = rough.0 / f64::from(rough.1);
    let smooth_mean = smooth.0 / f64::from(smooth.1);
    assert!(
        rough_mean > smooth_mean,
        "rough regions are not steeper: {rough_mean} against {smooth_mean}"
    );
}

#[test]
fn a_wrapped_coordinate_produces_exactly_the_same_tile() {
    // Section 7.1 and 30.4. A coordinate and its wrapped image name one tile,
    // so every value on it — elevation, relief, band — must be bit-identical,
    // and that has to hold on all six edges rather than on average.
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
    ] {
        let canonical = Coord::new(q, r);
        let tile = generator.tile(canonical);
        for (mq, mr) in mirrors {
            let wrapped = Coord::new(q + mq, r + mr);
            assert_eq!(wrapped, canonical, "({q}, {r}) + ({mq}, {mr})");
            assert_eq!(generator.tile(wrapped), tile);
            assert_eq!(
                generator.elevation_at(wrapped).to_bits(),
                tile.elevation_value.to_bits()
            );
            assert_eq!(
                generator.relief(wrapped).to_bits(),
                tile.relief_value.to_bits()
            );
        }
    }
}

#[test]
fn negative_coordinates_are_no_different_from_positive_ones() {
    // Section 30.4. The addressing arithmetic is `div_euclid` and `rem_euclid`,
    // and the classic defect is a distribution that changes sign with the
    // coordinate because a bare `/` mirrored the lattice about the origin.
    let generator = Generator::with_defaults(0x5555);
    let mut shares = Vec::new();
    for quadrant in [(1_i64, 1_i64), (-1, 1), (1, -1), (-1, -1)] {
        let mut tiles = Vec::new();
        for i in 0..80_i64 {
            for j in 0..80_i64 {
                tiles.push(generator.tile(Coord::new(
                    quadrant.0 * (i * 53 + 17),
                    quadrant.1 * (j * 59 + 23),
                )));
            }
        }
        let land = tiles.iter().filter(|t| t.elevation.is_land()).count();
        shares.push(share(land, tiles.len()));
    }
    let lowest = shares.iter().copied().fold(f64::INFINITY, f64::min);
    let highest = shares.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!(
        highest - lowest < 0.2,
        "the quadrants disagree about how much land there is: {shares:?}"
    );
}

#[test]
fn the_tile_classification_agrees_with_its_own_scalar() {
    // The band and the number must never disagree: a consumer that switches on
    // one and compares the other would see a tile that is water at an elevation
    // above sea level.
    let config = Config::default();
    let generator = Generator::new(0x99, config.clone()).expect("configuration is valid");
    for coord in spread() {
        let tile = generator.tile(coord);
        assert_eq!(
            tile.elevation.is_water(),
            tile.elevation_value <= config.sea_level,
            "{coord:?} at {}",
            tile.elevation_value
        );
        let expected = match tile.elevation_value {
            e if e <= config.deep_water_level => Elevation::DeepWater,
            e if e <= config.sea_level => Elevation::ShallowWater,
            e if e <= config.upland_level => Elevation::Lowland,
            e if e <= config.highland_level => Elevation::Upland,
            e if e <= config.mountain_level => Elevation::Highland,
            _ => Elevation::Mountain,
        };
        assert_eq!(tile.elevation, expected, "{coord:?}");
    }
}
