//! Golden region parameters for algorithm version 1.
//!
//! `DESIGN.md` sections 11, 12, 25.7, 30.9, and the issue for phase 3. Read the
//! header of `golden.rs` first: everything it says about what a golden is, why
//! these values are not derived independently, and why re-recording one is a
//! compatibility decision rather than a test fix applies here unchanged.
//!
//! # Compatibility decision
//!
//! Phase 3 recorded this table when region parameters first existed. It added
//! output rather than moving any: `golden.rs` covers the four continuous fields
//! and the elevation composite, and phase 3 left every one of those values
//! exactly where phase 2 put it — the region bias is carried alongside them in
//! [`wgvb::Sample::regional_uplift`] and is not folded into `elevation_raw`
//! until phase 4 decides what an elevation bias is worth. No world existed that
//! this could invalidate, so `ALGORITHM_VERSION` was not bumped.
//!
//! **After this point, a change that moves any value here changes every world.**
//!
//! # Running on a second target
//!
//! Section 30.9, and the reason to bother: one target proves self-consistency,
//! and section 25 exists to survive a change of target. On an Apple silicon host
//! the second target needs no extra hardware.
//!
//! ```sh
//! rustup target add x86_64-apple-darwin
//! cargo test --workspace --target x86_64-apple-darwin
//! ```
//!
//! The ridge orientation is the value most worth having here. It is the one
//! parameter produced by `sqrt` and rejection sampling rather than by a hash
//! conversion alone, so it is where a target that contracted an expression into
//! a fused multiply-add would show itself first.

use wgvb::{Coord, Generator};

/// The seed the table was recorded with. Changing it invalidates the table.
const GOLDEN_SEED: u64 = 0x0123_4567_89ab_cdef;

/// `(q, r)` paired with the bit patterns of `elevation_bias`, `moisture_bias`,
/// `heat_bias`, `roughness`, `basin_bias`, `volcanic`, `variation`, `ridge.x`,
/// and `ridge.y`, in that order.
///
/// The coordinates cover the origin, all six unit directions, both sides of a
/// region anchor line and of a macro-region anchor line, negatives at several
/// magnitudes, and one coordinate on each of the six edges of the canonical
/// hexagon.
#[rustfmt::skip]
const GOLDEN: [((i64, i64), [u64; 9]); 20] = [
    ((0, 0), [0xbfd1f732738c4ab9, 0x3fdc6e3a8e178574, 0x3fe1740a73e7086c, 0xbfd8d1b0fbe4ccb8, 0x3fd22bdfabd9886b, 0xbfd677eb1037d575, 0x3fcc6f162ebda24d, 0x3fea0307741ab03d, 0xbfe2a364b7ab8b1f]),
    ((1, 0), [0xbfd1f713b20b3b3f, 0x3fdc6e89e5096290, 0x3fe1740fbdb2a86f, 0xbfd8d1d5e749eb40, 0x3fd22bd0a84c9b72, 0xbfd677d6b22bae37, 0x3fcc6e90e73f4a09, 0x3fea03282a439056, 0xbfe2a337103c335b]),
    ((1, -1), [0xbfd1f6f283b2d7b2, 0x3fdc6ea87340cb3c, 0x3fe173e94d7ab2b3, 0xbfd8d1895e986625, 0x3fd22bd84df13a29, 0xbfd67819ea58fcad, 0x3fcc6f8db3a9819b, 0x3fea030e02a042e7, 0xbfe2a35b91161aac]),
    ((0, -1), [0xbfd1f6ed6401dd9f, 0x3fdc6e6dad30fbe0, 0x3fe173e606dbc8d4, 0xbfd8d14cdccb6d5f, 0x3fd22bbacc0798fd, 0xbfd6785928b73723, 0x3fcc6f7f027b9315, 0x3fea03037869cf7e, 0xbfe2a36a46c2e6d3]),
    ((-1, 0), [0xbfd1f6c3eb9c9dad, 0x3fdc6e2551ac4f60, 0x3fe173d8d0e9f291, 0xbfd8d19735a749b8, 0x3fd22bdfaf24cb70, 0xbfd677fb40c1906d, 0x3fcc6f573287eeb3, 0x3fea03096d80950c, 0xbfe2a361f653e657]),
    ((-1, 1), [0xbfd1f6c923a4247f, 0x3fdc6e1f1134fe55, 0x3fe173e2202dec08, 0xbfd8d16ed0ed2e48, 0x3fd22bfe39945f33, 0xbfd678209d255ca4, 0x3fcc6e5e75297853, 0x3fea030d5ea69de8, 0xbfe2a35c75ef5fc0]),
    ((0, 1), [0xbfd1f6fa4f53094d, 0x3fdc6eaff9ca99a8, 0x3fe173f323c84a17, 0xbfd8d18dd31ba471, 0x3fd22bb863265764, 0xbfd67816b79ee5f5, 0x3fcc6f6f95ba485d, 0x3fea0316ca7f5984, 0xbfe2a34f4fdd0ae2]),
    ((127, 63), [0xbfbffcf98df6100f, 0x3fe8c65cbbb6aed5, 0x3fe06c1b6b93ded9, 0xbfdae3d75193db3b, 0x3fd092e6332720dd, 0xbfcf04deb98e58d8, 0x3fc1b8864af398c1, 0x3fee67a14084099a, 0xbfd3f4699da30b32]),
    ((128, 64), [0xbfbf9ad92620891c, 0x3fe8d154e69d87f8, 0x3fe052538109eb2a, 0xbfda87fb2fc80351, 0x3fd0b4f114fed4bc, 0xbfcedf52d018ebd7, 0x3fc2d97998efaee0, 0x3fee50380820b6a9, 0xbfd480f3014cb6d2]),
    ((-129, -1), [0x3fc2384e7a8444b9, 0x3fd5f0198b96161b, 0x3fc192abf027d8cd, 0xbfd2ca974fe5ce6b, 0x3fd2556cf73a3d9d, 0xbfd98e11a13840f8, 0x3fd62d39f25f515d, 0x3feab342d5364d28, 0xbfe1a350a848a306]),
    ((511, -511), [0xbfc91851c1e8878c, 0xbfdd7d8e60a899b0, 0x3fdd01c4a39bd7fb, 0x3fc6fd96c3ee64ed, 0xbfe8296fe542b72d, 0xbfd7431aa37e15a5, 0x3fea046f9ad91f4d, 0x3fe9e5c7cb7504ce, 0xbfe2cbf387ad556c]),
    ((512, -512), [0xbfc91876138f864d, 0xbfdd7dd9125551ac, 0x3fdd01faf031f14b, 0x3fc6fd877499f1bb, 0xbfe82976ffe914d5, 0xbfd74336a7f26de5, 0x3fea047c56195130, 0x3fe9e5dd31364944, 0xbfe2cbd60c6e913e]),
    ((12345, -6789), [0xbfb673119ae3bf01, 0x3fd65844f0c44581, 0x3fe3d6220c63a27f, 0xbfd4fc0d59b29503, 0xbfc75deb46281589, 0x3fe0b525d5b68475, 0xbfcf5bbe20d39093, 0x3fed689c01fd78d1, 0xbfd93ac34ad02725]),
    ((-12345, 6789), [0x3fe38f9c4d84c69a, 0xbfe8cbe61573e558, 0xbfcf8abd4966a595, 0xbfe0f881d1a6c30b, 0xbfd28e2b35588fef, 0xbfdf7d2e48161305, 0x3fb321ba5cee0998, 0x3fe0cadbab5fe046, 0x3feb3d6d4b6a846b]),
    ((32767, 0), [0x3febf209376f9dc0, 0xbfd6b254738b768c, 0xbfc962fc0f66f407, 0x3fc4c1cc515ec6cb, 0x3fa0b8ccbc078705, 0x3fce0e0bccdc8a40, 0xbfdbcda7d690a8b8, 0x3fdf006607dc5fb7, 0x3febfebf28bf7a86]),
    ((0, 32767), [0x3fe3047690bc3e99, 0xbfb158a78db272eb, 0x3fccc1c3560c7058, 0xbfb5aa77db75285d, 0x3fdc137d2b98f641, 0x3f917c06c0e31440, 0xbfcc3c9535d9e9e5, 0x3fdbfee07e2bdb53, 0x3fecc6ab9f586438]),
    ((-32767, 0), [0x3fcd4a597c628ec5, 0x3fab1d7a04773c9b, 0x3fe1b79918a3aab9, 0xbfe718af44354139, 0xbfd8bf4aba915d65, 0x3fbcd572f4ba49a0, 0x3fa62d4e2387531b, 0x3fda25a944cb93b1, 0xbfed35234381c4a5]),
    ((0, -32767), [0xbfba70aefe47cd9b, 0xbfdf98526cabfb44, 0xbfdfeebef6fb8a08, 0x3fe750a6249095c0, 0x3fcf1bcff1af3795, 0xbfe3c8d5e4fa1707, 0x3fd3695b36e69d0d, 0x3fdb52d71decc8b2, 0x3fecefe541f0754c]),
    ((32767, -32767), [0x3fd272d8382eae36, 0xbfa11d7eb5cc7c63, 0x3fd14094d4fde4db, 0xbfe05f2837ea0c51, 0xbfd198ea3c52313e, 0x3fe889302ff06169, 0x3fc670f98fc7e99d, 0x3fef93d4fd98c7ec, 0x3fc4bb63d68f83f6]),
    ((-32767, 32767), [0xbfb5c8f1c3380bd8, 0x3fd2132737761c12, 0x3fd931125a5fd527, 0x3fde7d17bedda511, 0xbfdef1cbb94ec04d, 0xbfe8df6700d3c180, 0x3fbe05df9a396ef9, 0x3fdd49b3b007c42e, 0x3fec73e28ea80e9a]),
];

/// The parameter names, in the order the golden table stores them.
const NAMES: [&str; 9] = [
    "elevation_bias",
    "moisture_bias",
    "heat_bias",
    "roughness",
    "basin_bias",
    "volcanic",
    "variation",
    "ridge.x",
    "ridge.y",
];

/// The nine values of one coordinate's parameters, in the table's order.
fn recorded(generator: &Generator, q: i64, r: i64) -> [f64; 9] {
    let p = generator.region_params(Coord::new(q, r));
    [
        p.elevation_bias,
        p.moisture_bias,
        p.heat_bias,
        p.roughness,
        p.basin_bias,
        p.volcanic,
        p.variation,
        p.ridge.x,
        p.ridge.y,
    ]
}

#[test]
fn golden_region_parameters_are_bit_exact() {
    let generator = Generator::with_defaults(GOLDEN_SEED);
    let mut failures = Vec::new();

    for ((q, r), expected) in GOLDEN {
        for ((name, want), got) in NAMES
            .into_iter()
            .zip(expected)
            .zip(recorded(&generator, q, r))
        {
            if want != got.to_bits() {
                failures.push(format!(
                    "({q}, {r}) {name}: expected 0x{want:016x} ({}), got 0x{:016x} ({got})",
                    f64::from_bits(want),
                    got.to_bits()
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} golden region value(s) moved. This is an algorithm compatibility \
         change, not a test to re-record:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn the_recorded_elevation_bias_is_the_one_a_sample_carries() {
    // The table golden-compares `region_params`; `Sample::regional_uplift` is a
    // second path to the same number and must not drift from it.
    let generator = Generator::with_defaults(GOLDEN_SEED);
    for ((q, r), expected) in GOLDEN {
        let sample = generator.sample(Coord::new(q, r));
        assert_eq!(
            sample.regional_uplift.to_bits(),
            expected[0],
            "regional_uplift at ({q}, {r})"
        );
    }
}

#[test]
fn the_golden_table_covers_what_it_claims_to() {
    let n = 32_767_i64;
    let coords: Vec<(i64, i64)> = GOLDEN.iter().map(|(c, _)| *c).collect();

    assert!(coords.contains(&(0, 0)), "the origin is missing");
    for (dq, dr) in [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)] {
        assert!(
            coords.contains(&(dq, dr)),
            "unit direction ({dq}, {dr}) is missing"
        );
    }
    for edge in [(n, 0), (0, n), (-n, 0), (0, -n), (n, -n), (-n, n)] {
        assert!(
            coords.contains(&edge),
            "edge coordinate {edge:?} is missing"
        );
    }
    // Both sides of a region anchor line and of a macro-region anchor line, at
    // the default spacings of 128 and 512 hexes.
    for pair in [((127, 63), (128, 64)), ((511, -511), (512, -512))] {
        assert!(
            coords.contains(&pair.0) && coords.contains(&pair.1),
            "{pair:?}"
        );
    }
    assert!(
        coords.iter().filter(|(q, r)| *q < 0 || *r < 0).count() >= 8,
        "too few negative coordinates"
    );

    let mut unique = coords.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), coords.len(), "the table repeats a coordinate");
}

#[test]
fn every_golden_value_is_a_normalized_finite_number() {
    for ((q, r), expected) in GOLDEN {
        for (name, bits) in NAMES.into_iter().zip(expected) {
            let value = f64::from_bits(bits);
            assert!(value.is_finite(), "({q}, {r}) {name} is {value}");
            assert!(
                (-1.0..=1.0).contains(&value),
                "({q}, {r}) {name} is {value}"
            );
        }
    }
}

#[test]
fn every_golden_ridge_is_a_unit_direction() {
    // A recorded pair that was not on the unit circle would mean the half-angle
    // recovery had gone wrong at recording time and the table had preserved it.
    for ((q, r), expected) in GOLDEN {
        let x = f64::from_bits(expected[7]);
        let y = f64::from_bits(expected[8]);
        assert!(x >= 0.0, "({q}, {r}) ridge.x is {x}");
        assert!(
            (x * x + y * y - 1.0).abs() < 1.0e-12,
            "({q}, {r}) ridge ({x}, {y}) is not a unit vector"
        );
    }
}

#[test]
fn the_table_records_places_that_actually_differ() {
    // Neighboring tiles inside one region share a character by design, so the
    // table is full of near-duplicates on purpose. Distant entries must not be.
    let far: Vec<[f64; 9]> = [
        (12_345_i64, -6_789_i64),
        (-12_345, 6_789),
        (32_767, 0),
        (0, -32_767),
    ]
    .into_iter()
    .map(|(q, r)| {
        let (_, bits) = GOLDEN
            .iter()
            .find(|((cq, cr), _)| (*cq, *cr) == (q, r))
            .expect("a recorded coordinate");
        bits.map(f64::from_bits)
    })
    .collect();

    for i in 0..far.len() {
        for j in (i + 1)..far.len() {
            let differing = (0..7)
                .filter(|k| (far[i][*k] - far[j][*k]).abs() > 0.1)
                .count();
            assert!(differing >= 4, "entries {i} and {j} are too similar");
        }
    }
}
