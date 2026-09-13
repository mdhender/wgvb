//! The world origin is an ordinary tile.
//!
//! `DESIGN.md` sections 8, 9, 11.2, 33.4, and the issue for the origin
//! anomaly.
//!
//! # What this file is for
//!
//! Every noise lattice has its own origin at the world origin, so `(0, 0, 0)`
//! used to be a lattice point of every scale at once: `continentalness`,
//! `regional`, and `local` were all *exactly* zero there. Simplex noise has its
//! steepest gradient at a lattice point, and all of the scales put theirs in
//! the same place, so the ground around the origin was measurably steeper than
//! the rest of the world — relief at the origin ran to three and a half times
//! the world mean, was still half again as steep six hexes out, and did not
//! reach parity until roughly fifty.
//!
//! That is not an arbitrary tile to spoil. It is the reference every player
//! frame is expressed against, the coordinate every worked example and
//! diagnostic render defaults to, and the first place anyone looks at a new
//! world. It also violates the homogeneity the design asks for everywhere else:
//! section 11.2 and section 33.4 both refuse to let an addressing decision be
//! visible in the terrain, and this was the same defect arriving through the
//! noise composition.
//!
//! # Why the tests are shaped the way they are
//!
//! A single rendered window does not show it: at one seed the origin is a
//! bright dot among a handful of others, easily mistaken for a genuine ridge
//! crest. Only pooling separates it from terrain, because ordinary terrain is
//! uncorrelated between worlds and cancels, leaving whatever is a function of
//! position relative to the centre. So the measurement below pools many seeds,
//! and it compares *rings* rather than the origin tile alone: the defect was a
//! halo, and a single-tile assertion would pass as soon as the peak moved one
//! hex.

use wgvb::{Config, Coord, Generator};

/// Seeds every pooled measurement runs over.
///
/// Spread by the golden-ratio odd constant rather than taken as `1..=N`: small
/// consecutive seeds differ in a handful of bits, and the mixer is what this
/// file is *not* testing.
fn seeds(count: u64) -> Vec<u64> {
    (1..=count)
        .map(|i| i.wrapping_mul(0x9e37_79b9_7f4a_7c15))
        .collect()
}

/// Arbitrary centres to compare the origin against.
///
/// Deliberately unremarkable coordinates at several magnitudes, on both signs
/// of both axes, and not multiples of any region or chunk size.
const CONTROL_CENTRES: [(i64, i64); 12] = [
    (10_980, -4_485),
    (-6_553, 2_978),
    (777, -1_234),
    (-15_000, 9_001),
    (4_321, 8_765),
    (-2_501, -3_499),
    (21_003, -11_000),
    (-9_119, -7_331),
    (313, 20_011),
    (-18_777, 4_099),
    (6_007, -6_006),
    (-1_357, 2_468),
];

/// The coordinates exactly `distance` steps from `centre`, in hex distance.
fn ring(centre: (i64, i64), distance: i64) -> Vec<Coord> {
    if distance == 0 {
        return vec![Coord::new(centre.0, centre.1)];
    }
    let mut out = Vec::new();
    for dq in -distance..=distance {
        for dr in -distance..=distance {
            let ds = -dq - dr;
            if dq.abs().max(dr.abs()).max(ds.abs()) == distance {
                out.push(Coord::new(centre.0 + dq, centre.1 + dr));
            }
        }
    }
    out
}

/// A sample count as an `f64`.
///
/// Through `u32` rather than an `as` cast: the workspace denies a lossy numeric
/// cast, and a test that opts out of the rule it holds the crate to is a test
/// nobody should trust.
fn count_of(values: usize) -> f64 {
    f64::from(u32::try_from(values).expect("a test sample fits in u32"))
}

/// The mean of a sample.
fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / count_of(values.len())
}

/// Mean relief over one ring, pooled across generators.
fn ring_relief(generators: &[Generator], centre: (i64, i64), distance: i64) -> f64 {
    let mut total = 0.0_f64;
    let mut count = 0_u32;
    for generator in generators {
        for coord in ring(centre, distance) {
            total += generator.relief(coord);
            count += 1;
        }
    }
    total / f64::from(count)
}

#[test]
fn no_continuous_field_is_zero_at_the_world_origin() {
    // The one-line statement of the defect. Every scale, every seed: the origin
    // is an interior point of the lattice, not a vertex of it.
    for seed in seeds(24) {
        let sample = Generator::with_defaults(seed).sample(Coord::new(0, 0));
        for (name, value) in [
            ("continentalness", sample.continentalness),
            ("regional", sample.regional),
            ("local", sample.local),
            ("detail", sample.detail),
        ] {
            assert_ne!(
                value, 0.0,
                "{name} is zero at the origin for seed {seed:#x}"
            );
        }
    }
}

#[test]
fn the_origin_is_not_a_ridge_crest_by_construction() {
    // The ridged transform is `1 - 2 * |field|`, which is at its maximum where
    // the field is zero — so while the ridge field was exactly zero at the
    // origin, the ridge term was pinned at exactly `+1.0` there for every seed
    // and every configuration. Ordinary behaviour along any crest; guaranteed
    // rather than incidental at one tile, and it stacked with everything else.
    let mut crests = 0_u32;
    let seeds = seeds(24);
    for seed in &seeds {
        let sample = Generator::with_defaults(*seed).sample(Coord::new(0, 0));
        assert!(
            sample.ridge < 1.0,
            "seed {seed:#x} pins the ridge term at one"
        );
        crests += u32::from(sample.ridge > 0.9);
    }
    let share = f64::from(crests) / count_of(seeds.len());
    assert!(
        share < 0.5,
        "the origin is on a crest in {share:.2} of worlds"
    );
}

#[test]
fn relief_near_the_origin_agrees_with_relief_anywhere_else() {
    // The halo, measured the way it has to be measured: pooled over seeds, as a
    // ring comparison, out to the distance the defect used to reach.
    //
    // The bound is wide on purpose. Rings this small are one draw of terrain
    // per seed and they sit inside a single region, so the pooled mean carries
    // real scatter — the same rings around arbitrary centres vary by a few per
    // cent between distances. What is being excluded is a factor of two or
    // three, which is what the defect was, not a few per cent.
    let generators: Vec<Generator> = seeds(24)
        .into_iter()
        .map(Generator::with_defaults)
        .collect();

    for distance in [0_i64, 1, 2, 3, 4, 5, 6, 10, 20, 30] {
        // A large ring is already hundreds of tiles per seed, so it needs
        // fewer centres to pool than a ring of six does. The cost of this test
        // is otherwise cubic in nothing useful.
        let centres = if distance > 6 {
            &CONTROL_CENTRES[..4]
        } else {
            &CONTROL_CENTRES[..]
        };
        let origin = ring_relief(&generators, (0, 0), distance);
        let mut control = 0.0_f64;
        for centre in centres {
            control += ring_relief(&generators, *centre, distance);
        }
        control /= count_of(centres.len());

        let ratio = origin / control;
        assert!(
            (0.7..=1.35).contains(&ratio),
            "at distance {distance} the origin reads {origin:.3} against {control:.3} elsewhere, \
             a ratio of {ratio:.2}"
        );
    }
}

/// A scalar of [`wgvb::Sample`], named and paired with its accessor.
type Scalar = (&'static str, fn(&wgvb::Sample) -> f64);

#[test]
fn the_world_origin_is_not_special_in_any_scalar() {
    // Relief is where the anomaly showed, but it was a property of the fields,
    // so the fields themselves must be unremarkable there too. Each scalar at
    // the origin, pooled over seeds, against the same scalar over a wide spread
    // of ordinary coordinates.
    let generators: Vec<Generator> = seeds(24)
        .into_iter()
        .map(Generator::with_defaults)
        .collect();

    let mut elsewhere = Vec::new();
    for generator in &generators {
        for i in -6..=6_i64 {
            for j in -6..=6_i64 {
                elsewhere.push(generator.sample(Coord::new(i * 911 + 37, j * 733 - 53)));
            }
        }
    }
    let at_origin: Vec<_> = generators
        .iter()
        .map(|g| g.sample(Coord::new(0, 0)))
        .collect();

    let scalars: [Scalar; 5] = [
        ("continentalness", |s| s.continentalness),
        ("regional", |s| s.regional),
        ("local", |s| s.local),
        ("detail", |s| s.detail),
        ("ridge", |s| s.ridge),
    ];
    for (name, get) in scalars {
        let origin_mean = mean(&at_origin.iter().map(get).collect::<Vec<f64>>());
        let other_mean = mean(&elsewhere.iter().map(get).collect::<Vec<f64>>());
        // Every field is zero-mean by construction, so this compares two means
        // that should both sit near zero rather than two magnitudes.
        assert!(
            (origin_mean - other_mean).abs() < 0.25,
            "{name} averages {origin_mean:+.3} at the origin against {other_mean:+.3} elsewhere"
        );
    }
}

#[test]
fn two_worlds_do_not_share_a_sampling_offset() {
    // The offset is derived from the seed so that whatever residue is left does
    // not sit in the same place in every world. Checked through the public API
    // rather than on the constant: the same coordinate must not produce the
    // same field values in two worlds, and the built graphs must differ.
    let a = Generator::with_defaults(1);
    let b = Generator::with_defaults(2);
    assert_ne!(a, b);
    for coord in [
        Coord::new(0, 0),
        Coord::new(37, -11),
        Coord::new(-9_000, 500),
    ] {
        assert_ne!(
            a.sample(coord).continentalness,
            b.sample(coord).continentalness
        );
    }
}

#[test]
fn the_offset_does_not_disturb_tile_identity_at_the_origin() {
    // A translation of the sample position must not become a translation of the
    // *world*: the origin still has to be the origin, wrapped images still have
    // to agree with it, and the configuration still has to be what decides.
    let generator = Generator::new(0x0f0f, Config::default()).expect("valid configuration");
    let origin = Coord::new(0, 0);
    let n = wgvb::WORLD_RADIUS;
    for (mq, mr) in [(2 * n + 1, -n), (n + 1, -(2 * n + 1)), (-(2 * n + 1), n)] {
        let wrapped = Coord::new(mq, mr);
        assert_eq!(wrapped, origin);
        assert_eq!(
            generator.elevation_at(wrapped).to_bits(),
            generator.elevation_at(origin).to_bits()
        );
    }
    assert_eq!(generator.sample(origin).world, wgvb::axial_to_world(origin));
    assert_eq!(generator.sample(origin).world.x, 0.0);
    assert_eq!(generator.sample(origin).world.y, 0.0);
}
