//! Golden coordinates for algorithm version 2.
//!
//! `DESIGN.md` sections 25.7, 27, 30.9, and the issue for phase 4.
//!
//! # What this file is
//!
//! A recorded fingerprint of the generation path, stored as raw `f64` bit
//! patterns so the comparison is exact rather than "close enough". Unlike the
//! rest of the suite, these values are *not* derived independently — that is
//! what a golden is. Their job is to fail when output changes, so that a change
//! is either a deliberate algorithm compatibility decision recorded in the
//! commit message, or a bug.
//!
//! # Compatibility decision
//!
//! **This table was re-recorded for `ALGORITHM_VERSION = 2`, and the version was
//! bumped in the same commit.**
//!
//! Phase 4 added the region uplift term, the ridge structure term, the constant
//! elevation offset, and the contrast shaping — all of which are new values that
//! could have been added without disturbing anything, exactly as phase 3 added
//! region influence under version 1. What forced the bump is that the same
//! tuning pass lowered `local_weight` and `detail_weight`, and those two feed
//! the four-field composite that version 1 had already fixed as
//! `Sample::elevation_raw`. The old coastlines dissolved into a wide band of
//! speckle because the two shortest scales carried enough amplitude to cross
//! sea level on their own; that is a defect worth a version, and the version is
//! what makes it a decision instead of a surprise.
//!
//! **After this point, a change that moves any value here changes every world.**
//! Do not re-record the table to make a test pass. Either bump
//! `ALGORITHM_VERSION` and re-record deliberately, saying so in the commit
//! message, or fix the change that moved the values.
//!
//! # The saturated relief values
//!
//! Several coordinates on the edges of the canonical hexagon record a relief of
//! exactly `1.0`. That is not a rounding artifact: a tile on an edge has a
//! neighbor across the wrap, its neighbor sits hundreds of thousands of miles
//! away in canonical world space, and the elevation difference to it is
//! therefore unrelated to anything local. This is the accepted world-warp seam
//! of section 7.1, measured in `tests/wrap_seam.rs`; those rows still pin
//! elevation and every other scalar, and the saturation itself is worth pinning
//! because a change to the wrap would move it.
//!
//! # Running on a second target
//!
//! Section 30.9: goldens must run on more than one target, because a single
//! target proves only self-consistency, and the determinism rules of section 25
//! exist precisely to survive a change of target. On an Apple silicon host the
//! second target is available without extra hardware:
//!
//! ```sh
//! rustup target add x86_64-apple-darwin
//! cargo test --workspace --target x86_64-apple-darwin
//! ```
//!
//! Never build a target under comparison with `-C target-cpu=native` or any
//! flag that relaxes floating-point semantics (section 25.7).

use wgvb::{Coord, Generator};

/// The seed the table was recorded with. Changing it invalidates the table.
const GOLDEN_SEED: u64 = 0x0123_4567_89ab_cdef;

/// The scalar names, in the order the golden table stores them.
///
/// Every scalar the public API exposes at one coordinate, so a new field that
/// is not added here is visibly missing rather than quietly unpinned.
const NAMES: [&str; 10] = [
    "continentalness",
    "regional",
    "local",
    "detail",
    "ridge",
    "elevation_raw",
    "regional_uplift",
    "roughness",
    "elevation",
    "relief",
];

/// `(q, r)` paired with the bit patterns of the scalars named in [`NAMES`].
///
/// The coordinates cover the origin, all six unit directions, negatives at
/// several magnitudes, and one coordinate on each of the six edges of the
/// canonical hexagon, because a target-dependent difference is most likely to
/// appear where the operands are largest.
#[rustfmt::skip]
const GOLDEN: [((i64, i64), [u64; NAMES.len()]); 22] = [
    ((0, 0), [0x0000000000000000, 0x0000000000000000, 0x0000000000000000, 0x3fe199b086000000, 0x3fe1feb70b2be6c2, 0x3f93c3051435e50d, 0xbfd1f732738c4ab9, 0xbfd8d1b0fbe4ccb8, 0xbfd3efa83894c0af, 0x3fef8a84807c8b13]),
    ((1, 0), [0x3f280c14c532e0cc, 0xbfadd1d3518c61be, 0xbfcffc225ea47721, 0x3fc819ed07d54848, 0x3fdb3313a9217ec0, 0xbfa0812004e1f78f, 0xbfd1f713b20b3b3f, 0xbfd8d1d5e749eb40, 0xbfd8c138fd69803a, 0x3ff0000000000000]),
    ((1, -1), [0x3f93037128345c18, 0xbfabf318c71e1e91, 0xbfd3927af39812ff, 0x3fd3339eb69674cd, 0x3fe0b7cf1abae034, 0xbf95ea6aa1a55e47, 0xbfd1f6f283b2d7b2, 0xbfd8d1895e986625, 0xbfd71b64104ee0b0, 0x3ff0000000000000]),
    ((0, -1), [0x3f92d5567a10f70a, 0x3f6b6a562fff05a9, 0xbfbd526d849ab0d6, 0x3fd848d34d9a844e, 0x3fe5d3309bc8be90, 0x3f8eb8a7895c3193, 0xbfd1f6ed6401dd9f, 0xbfd8d14cdccb6d5f, 0xbfd365bb47f3d874, 0x3feaa3bda03d26f5]),
    ((-1, 0), [0xbf280c14c532d628, 0x3fadd1d3518c61cb, 0x3fd1f49cd0fde9b5, 0x3fd6748ca158d9c3, 0x3fe72fda8ad7bb62, 0x3fab915ef59151c7, 0xbfd1f6c3eb9c9dad, 0xbfd8d19735a749b8, 0xbfd093c61cf7abde, 0x3fe6524fa134f2ef]),
    ((-1, 1), [0xbf93037128345c2e, 0x3fabf318c71e1e68, 0x3fd58db49a38cd9d, 0x3fd7a44e6d716744, 0x3fe375149639f2c8, 0x3fa85fdf372abc27, 0xbfd1f6c923a4247f, 0xbfd8d16ed0ed2e48, 0xbfd1f5bb416cf4b8, 0x3fe6b9808e51d158]),
    ((0, 1), [0xbf92d5567a10f724, 0xbf6b6a562fff0800, 0x3fc441fc70d5dc97, 0x3fd8856cc22cc602, 0x3fde2c56206a9afe, 0x3f8feee2c0aef607, 0xbfd1f6fa4f53094d, 0xbfd8d18dd31ba471, 0xbfd53664512dcb85, 0x3fee82b17d6a8540]),
    ((-1, -1), [0x3f929f013cb7dd4c, 0x3faf19ab35963b49, 0x3fbcaf57c0d4942f, 0x3fc6ddec07589808, 0x3fea540b431191c5, 0x3fa6c7be44489059, 0xbfd1f67c21f38a99, 0xbfd8d1311737c6d0, 0xbfd0795f5d29bc15, 0x3fe4b67c46334373]),
    ((7, -3), [0x3fabcdc76ec9d6a2, 0xbfd1c76c0f8cb124, 0xbfd49000e8d9bfc0, 0xbfe46fa01bffa062, 0x3fd078b337e990f6, 0xbfb99d9fbea0dfc9, 0xbfd1f2a10a009463, 0xbfd8d2b34a0ac867, 0xbfdf62f9fe6840af, 0x3fcea65d926b85b6]),
    ((-97, 411), [0xbfe21187a1d9a0e1, 0x3f856a1c1847f044, 0xbfc9a41f33972b15, 0xbfcafb3cce27abc3, 0x3fe031b674fa9a14, 0xbfd688f6b51ba941, 0x3fda48de8edf864d, 0xbfda2797ebd9c9e0, 0xbfe3032c2b25ae83, 0x3fc344b2732ec60c]),
    ((1000, 1000), [0xbfc3e24c6bad03c8, 0xbfcb2b98790eaef5, 0xbfbbaf256278f59a, 0xbfab567f5613156f, 0x3fc2137f461d120c, 0xbfc506d00d9d5a87, 0xbfceba195163d17d, 0xbfebdc7ee482d6ac, 0xbfe2a7a8b3c4674e, 0x3fc6f74517a5ba72]),
    ((-1000, -1000), [0xbfb278ff51429fec, 0x3fa1ce200e710257, 0xbfd94a5911bd7120, 0xbfe21b8ba6eb3cc8, 0x3fe768d0482a416a, 0xbfb628027d6f86f6, 0x3fe390eedf4c6313, 0xbfd2bdda9c0f2455, 0xbfbc98579a3063fc, 0x3fd7bea15b08baa4]),
    ((12345, -6789), [0xbfda02ae81379c97, 0xbfbf03912f308fb3, 0xbfba4b893d324b3a, 0x3fd7f57fd5130af7, 0x3feaf8e7e7ba8982, 0xbfd136c126d28a86, 0xbfb673119ae3bf01, 0xbfd4fc0d59b29503, 0xbfe38410a4dc0c26, 0x3fdf742e3c55fa14]),
    ((-12345, 6789), [0xbfc0288cae5d50bb, 0xbfdb6f88989dd403, 0x3fda3c6a028e56f8, 0xbfd771a6dd0f598e, 0x3fe688cbd161f582, 0xbfc68936f5349c3c, 0x3fe38f9c4d84c69a, 0xbfe0f881d1a6c30b, 0xbfd4181989aa5783, 0x3fd2b8513a04d1e3]),
    ((32767, 0), [0xbfbcabc087fb659f, 0xbfd60474dd0f9f3f, 0xbf8e29270868ecff, 0x3fdc996ee7517b42, 0x3fe573a157699ac8, 0xbfc36ad887ba09f3, 0x3febf209376f9dc0, 0x3fc4c1cc515ec6cb, 0xbfb94bcdf82fb6cd, 0x3ff0000000000000]),
    ((0, 32767), [0x3fc8a64b3122861a, 0xbf5c70fb1ad49cf8, 0xbfc2a8f272cbb3a6, 0x3fbdddb6d86c2908, 0x3fec32147458d332, 0x3fba7967d2d5980f, 0x3fe3047690bc3e99, 0xbfb5aa77db75285d, 0x3fcb00e32458e8a4, 0x3ff0000000000000]),
    ((-32767, 0), [0x3f6a52f84a1b7732, 0xbfbfc6266b7e9f99, 0x3fc7c83a0be5179d, 0x3fc56859f6d366db, 0x3feb6f4c8a21fa06, 0xbf8914743768f88e, 0x3fcd4a597c628ec5, 0xbfe718af44354139, 0xbfc9ede12bfdbb0b, 0x3ff0000000000000]),
    ((0, -32767), [0xbf801e3961b41ccf, 0x3fda69ee5eb8dd05, 0x3fd7277464aa9228, 0x3fd1cab17474f4b7, 0x3fdc8d7b6090cb30, 0x3fc42b10b91204d6, 0xbfba70aefe47cd9b, 0x3fe750a6249095c0, 0x3f99a2953927e428, 0x3ff0000000000000]),
    ((32767, -32767), [0x3fc04e2716db3e19, 0x3fa5c6b298a72adf, 0x3fdcc97ec824786f, 0xbfc3e7d5aae69d62, 0x3fda1f327f41ced2, 0x3fbef53fe48bbef4, 0x3fd272d8382eae36, 0xbfe05f2837ea0c51, 0x3f893b703f8fbdd8, 0x3ff0000000000000]),
    ((-32767, 32767), [0xbfd12f58d8c47999, 0x3fc85a822675aa8a, 0xbfcb9670185580d9, 0xbf7b766cbf9cd8ca, 0x3fe577cdb81be576, 0xbfbedb6697aa85b0, 0xbfb5c8f1c3380bd8, 0x3fde7d17bedda511, 0xbfd73c7742562f63, 0x3ff0000000000000]),
    ((10922, -4681), [0xbf7b9f4f7b7f3ae3, 0x3fb6c851d0804c0a, 0xbfc35b27f78460c6, 0x3fc1f69e2ec1c223, 0x3fec49e56c1a0933, 0x3f8c224b182d2a81, 0xbfdb1afc96a3b248, 0xbfe5c4ac6e8f703d, 0xbfd7990adaa0c88c, 0x3fc6f60b1f8a57a3]),
    ((-6553, 2978), [0x3fa02f66602174f4, 0x3fbcea1377d57265, 0x3fb8fd1d1c99ac00, 0xbfbe63f8dcce67a2, 0x3fe5b0a09eef4162, 0x3faca013f90ea42e, 0xbfcb6aaf58588594, 0xbfd6e6eeaa960a75, 0xbfccba4c9e927628, 0x3fbef5fa739617cd]),
];

#[test]
fn golden_coordinates_are_bit_exact() {
    let generator = Generator::with_defaults(GOLDEN_SEED);
    let mut failures = Vec::new();

    for ((q, r), expected) in GOLDEN {
        let coord = Coord::new(q, r);
        let sample = generator.sample(coord);
        let actual = [
            sample.continentalness,
            sample.regional,
            sample.local,
            sample.detail,
            sample.ridge,
            sample.elevation_raw,
            sample.regional_uplift,
            sample.roughness,
            sample.elevation,
            generator.relief(coord),
        ];
        for ((name, want), got) in NAMES.into_iter().zip(expected).zip(actual) {
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
        "{} golden value(s) moved. This is an algorithm compatibility change, \
         not a test to re-record:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn the_tile_api_reports_the_same_numbers_as_the_golden_table() {
    // `sample`, `elevation_at`, `relief`, and `tile` are four public routes to
    // the same two numbers. Pinning only one of them would let the other three
    // drift, and section 20 requires the batch forms to be bit-identical too.
    let generator = Generator::with_defaults(GOLDEN_SEED);
    let elevation_index = NAMES.iter().position(|n| *n == "elevation").unwrap();
    let relief_index = NAMES.iter().position(|n| *n == "relief").unwrap();

    let coords: Vec<Coord> = GOLDEN
        .iter()
        .map(|((q, r), _)| Coord::new(*q, *r))
        .collect();
    let batch = generator.tiles(&coords);

    for ((((q, r), expected), coord), batched) in GOLDEN.into_iter().zip(coords).zip(batch) {
        let elevation = f64::from_bits(expected[elevation_index]);
        let relief = f64::from_bits(expected[relief_index]);
        let tile = generator.tile(coord);

        assert_eq!(
            tile.elevation_value.to_bits(),
            elevation.to_bits(),
            "({q}, {r})"
        );
        assert_eq!(tile.relief_value.to_bits(), relief.to_bits(), "({q}, {r})");
        assert_eq!(
            generator.elevation_at(coord).to_bits(),
            elevation.to_bits(),
            "({q}, {r})"
        );
        assert_eq!(
            generator.relief(coord).to_bits(),
            relief.to_bits(),
            "({q}, {r})"
        );
        assert_eq!(batched, tile, "({q}, {r})");
    }
}

#[test]
fn the_golden_table_covers_what_it_claims_to() {
    // A golden table that quietly lost its extreme coordinates would still
    // pass. Check the coverage the module comment promises.
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
    // A table of quiet not-a-numbers would compare equal to itself forever.
    for ((q, r), expected) in GOLDEN {
        for (name, bits) in NAMES.into_iter().zip(expected) {
            let value = f64::from_bits(bits);
            assert!(value.is_finite(), "({q}, {r}) {name} is {value}");
            assert!(
                (-1.0..=1.0).contains(&value),
                "({q}, {r}) {name} is {value}"
            );
            if name == "relief" {
                assert!((0.0..=1.0).contains(&value), "({q}, {r}) relief is {value}");
            }
        }
    }
}
