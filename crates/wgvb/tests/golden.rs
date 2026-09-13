//! Golden coordinates for algorithm version 1.
//!
//! `DESIGN.md` sections 25.7, 27, 30.9, and the issue for phase 2.
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
//! These goldens were recorded when phase 2 first gave `ALGORITHM_VERSION = 1`
//! any generation semantics at all. Version 1 had produced no field values
//! before that commit — phase 1 was coordinates, hashing, and configuration —
//! so no world existed to be invalidated and the version was not bumped.
//!
//! **After this point, a change that moves any value here changes every world.**
//! Do not re-record the table to make a test pass. Either bump
//! `ALGORITHM_VERSION` and re-record deliberately, saying so in the commit
//! message, or fix the change that moved the values.
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

/// `(q, r)` paired with the bit patterns of `continentalness`, `regional`,
/// `local`, `detail`, and `elevation_raw`, in that order.
///
/// The coordinates cover the origin, all six unit directions, negatives at
/// several magnitudes, and one coordinate on each of the six edges of the
/// canonical hexagon, because a target-dependent difference is most likely to
/// appear where the operands are largest.
#[rustfmt::skip]
const GOLDEN: [((i64, i64), [u64; 5]); 22] = [
    ((0, 0), [0x0000000000000000, 0x0000000000000000, 0x0000000000000000, 0x3fe199b086000000, 0x3fa2c611a0000000]),
    ((1, 0), [0x3f280c14c532e0cc, 0xbfadd1d3518c61be, 0xbfcffc225ea47721, 0x3fc819ed07d54848, 0xbfa2888d9063a2a9]),
    ((1, -1), [0x3f93037128345c18, 0xbfabf318c71e1e91, 0xbfd3927af39812ff, 0x3fd3339eb69674cd, 0xbf9a09c777870296]),
    ((0, -1), [0x3f92d5567a10f70a, 0x3f6b6a562fff05a9, 0xbfbd526d849ab0d6, 0x3fd848d34d9a844e, 0x3f95392d4ec1bb4b]),
    ((-1, 0), [0xbf280c14c532d628, 0x3fadd1d3518c61cb, 0x3fd1f49cd0fde9b5, 0x3fd6748ca158d9c3, 0x3fb383e76c9f60dd]),
    ((-1, 1), [0xbf93037128345c2e, 0x3fabf318c71e1e68, 0x3fd58db49a38cd9d, 0x3fd7a44e6d716744, 0x3fb2fdbec8e43cb2]),
    ((0, 1), [0xbf92d5567a10f724, 0xbf6b6a562fff0800, 0x3fc441fc70d5dc97, 0x3fd8856cc22cc602, 0x3fa2672824856e49]),
    ((-1, -1), [0x3f929f013cb7dd4c, 0x3faf19ab35963b49, 0x3fbcaf57c0d4942f, 0x3fc6ddec07589808, 0x3fab019564edabf2]),
    ((7, -3), [0x3fabcdc76ec9d6a2, 0xbfd1c76c0f8cb124, 0xbfd49000e8d9bfc0, 0xbfe46fa01bffa062, 0xbfc0b538e9bf6c8f]),
    ((-97, 411), [0xbfe21187a1d9a0e1, 0x3f856a1c1847f044, 0xbfc9a41f33972b15, 0xbfcafb3cce27abc3, 0xbfd5b41047d0cdee]),
    ((1000, 1000), [0xbfc3e24c6bad03c8, 0xbfcb2b98790eaef5, 0xbfbbaf256278f59a, 0xbfab567f5613156f, 0xbfc426c4f70ab49b]),
    ((-1000, -1000), [0xbfb278ff51429fec, 0x3fa1ce200e710257, 0xbfd94a5911bd7120, 0xbfe21b8ba6eb3cc8, 0xbfbe9fa88be5e4ee]),
    ((12345, -6789), [0xbfda02ae81379c97, 0xbfbf03912f308fb3, 0xbfba4b893d324b3a, 0x3fd7f57fd5130af7, 0xbfce702c0053acad]),
    ((-12345, 6789), [0xbfc0288cae5d50bb, 0xbfdb6f88989dd403, 0x3fda3c6a028e56f8, 0xbfd771a6dd0f598e, 0xbfc3613886ea90e1]),
    ((32767, 0), [0xbfbcabc087fb659f, 0xbfd60474dd0f9f3f, 0xbf8e29270868ecff, 0x3fdc996ee7517b42, 0xbfbfa70ef282c324]),
    ((0, 32767), [0x3fc8a64b3122861a, 0xbf5c70fb1ad49cf8, 0xbfc2a8f272cbb3a6, 0x3fbdddb6d86c2908, 0x3fb730811861a137]),
    ((-32767, 0), [0x3f6a52f84a1b7732, 0xbfbfc6266b7e9f99, 0x3fc7c83a0be5179d, 0x3fc56859f6d366db, 0x3f729733bda1a5f1]),
    ((0, -32767), [0xbf801e3961b41ccf, 0x3fda69ee5eb8dd05, 0x3fd7277464aa9228, 0x3fd1cab17474f4b7, 0x3fc618c4fbc4561f]),
    ((32767, -32767), [0x3fc04e2716db3e19, 0x3fa5c6b298a72adf, 0x3fdcc97ec824786f, 0xbfc3e7d5aae69d62, 0x3fc07f56738b7dde]),
    ((-32767, 32767), [0xbfd12f58d8c47999, 0x3fc85a822675aa8a, 0xbfcb9670185580d9, 0xbf7b766cbf9cd8ca, 0xbfbf24d012479440]),
    ((10922, -4681), [0xbf7b9f4f7b7f3ae3, 0x3fb6c851d0804c0a, 0xbfc35b27f78460c6, 0x3fc1f69e2ec1c223, 0x3f831acaaca40a9d]),
    ((-6553, 2978), [0x3fa02f66602174f4, 0x3fbcea1377d57265, 0x3fb8fd1d1c99ac00, 0xbfbe63f8dcce67a2, 0x3faaaa27909123f7]),
];

/// The scalar names, in the order the golden table stores them.
const NAMES: [&str; 5] = [
    "continentalness",
    "regional",
    "local",
    "detail",
    "elevation_raw",
];

#[test]
fn golden_coordinates_are_bit_exact() {
    let generator = Generator::with_defaults(GOLDEN_SEED);
    let mut failures = Vec::new();

    for ((q, r), expected) in GOLDEN {
        let sample = generator.sample(Coord::new(q, r));
        let actual = [
            sample.continentalness,
            sample.regional,
            sample.local,
            sample.detail,
            sample.elevation_raw,
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
        }
    }
}
