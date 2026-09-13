//! Golden coordinates for algorithm version 5.
//!
//! `DESIGN.md` sections 25.7, 27, 30.9, and the issues for phases 4 to 6.
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
//! # Compatibility decision — version 5
//!
//! Phase 6 added two columns, `basin_influence` and `volcanic`, and a
//! `terrain` column, and **moved nothing**. Every value the table already held
//! is bit-for-bit what version 4 recorded, for the same structural reason
//! version 4 could say it of version 3: basin influence and volcanic tendency
//! are new nodes in the field graph and new terms of their own composites, and
//! terrain is a classification that reads elevation, relief, climate, and
//! those two — nothing in this phase feeds anything the earlier phases
//! computed.
//!
//! That is a decision and not a coincidence. A basin term inside the elevation
//! composite would be the obvious way to make a depression *be* lower ground,
//! and it would have moved every tile in every world; section 17 puts basin
//! influence in terrain, and this table is what holds the implementation to
//! that.
//!
//! `ALGORITHM_VERSION` was bumped all the same, because [`wgvb::Config`]
//! gained the basin wavelengths, weights, octave counts, volcanic settings,
//! and the terrain thresholds, and a world file written under version 4 does
//! not carry them.
//!
//! # Compatibility decision — version 4
//!
//! Phase 5 added two columns, `heat` and `moisture`, and **moved nothing**.
//! Every value the table already held is bit-for-bit what version 3 recorded:
//! climate reads the elevation scalar but does not feed it, and the two climate
//! fields are new nodes in the field graph rather than changes to existing ones.
//! `ALGORITHM_VERSION` was bumped all the same, because [`wgvb::Config`] gained
//! the climate settings and a world file written under version 3 does not carry
//! them — see the history note on that constant.
//!
//! # Compatibility decision — version 3
//!
//! **This table was re-recorded for `ALGORITHM_VERSION = 3`, and the version was
//! bumped in the same commit.**
//!
//! Two changes to the noise composition, landed together because either one
//! alone would have forced the bump and re-recording twice would have said
//! nothing extra:
//!
//! 1. The fbm ladder gained a per-field octave count, bounded by the Nyquist
//!    wavelength of the tile grid. The hill and detail fields ran five octaves
//!    each, down to 7.5 and 2.25 miles, against a grid whose tiles are six
//!    miles apart — those octaves could not be represented and aliased into
//!    per-tile noise instead, with relief the visible casualty.
//! 2. Every field graph gained a seed-derived sampling offset. The world origin
//!    was a lattice point of every scale at once, so `continentalness`,
//!    `regional`, and `local` were all *exactly* zero there — the previous
//!    version of the first row of this table recorded three zeroes and is the
//!    clearest surviving evidence of the defect.
//!
//! Neither is a tuning change, and the second one moves every coordinate in the
//! world rather than a band of them. The distributions barely moved: the land
//! fraction, the band shares, and the mean neighbor step all stayed inside the
//! bounds `tests/elevation.rs` was already asserting, so no threshold or weight
//! was retuned along with them.
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

use wgvb::{Coord, Generator, HeatBand, MoistureBand, Terrain};

/// The seed the table was recorded with. Changing it invalidates the table.
const GOLDEN_SEED: u64 = 0x0123_4567_89ab_cdef;

/// The scalar names, in the order the golden table stores them.
///
/// Every scalar the public API exposes at one coordinate, so a new field that
/// is not added here is visibly missing rather than quietly unpinned.
/// `basin_influence` and `volcanic` are the two phase 6 added; the twelve
/// before them are exactly the values, in exactly the order, that version 4
/// recorded.
const NAMES: [&str; 14] = [
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
    "heat",
    "moisture",
    "basin_influence",
    "volcanic",
];

/// `(q, r)` paired with the bit patterns of the scalars named in [`NAMES`] and
/// with the terrain classified from them.
///
/// Terrain is a variant rather than a bit pattern because it is a
/// classification rather than a number, and it belongs in the same table
/// rather than in one of its own: the whole point of pinning it is that it is
/// the classification *of these scalars*, and a second table keyed by the same
/// coordinates could drift from this one without failing.
///
/// The coordinates cover the origin, all six unit directions, negatives at
/// several magnitudes, and one coordinate on each of the six edges of the
/// canonical hexagon, because a target-dependent difference is most likely to
/// appear where the operands are largest.
#[rustfmt::skip]
const GOLDEN: [((i64, i64), [u64; NAMES.len()], Terrain); 22] = [
    ((0, 0), [0x3fa5b0402bbc41d9, 0xbfc7b08f83c88d02, 0x3fd73a1e9eba1a86, 0x3fc1d8b06b63dd13, 0x3fe8fa01501d59f5, 0x3f7e3e7ecfc993d3, 0xbfd1f732738c4ab9, 0xbfd8d1b0fbe4ccb8, 0xbfd50203836cc654, 0x3fce7e5ccbf059ad, 0x3fd55a0d969b91f7, 0xbfd6b1df9dc2760e, 0xbf8d7c70f2e147aa, 0xbfd5d250b263a661], Terrain::DeepOcean),
    ((1, 0), [0x3fa93a45c5bbf814, 0xbfc4c43a31c1c7e9, 0x3fd2bf426d80f84f, 0xbf9500d175803ec0, 0x3fe8ea865b13cf28, 0x3f7a043b920ecd39, 0xbfd1f713b20b3b3f, 0xbfd8d1d5e749eb40, 0xbfd4e9c08c42c2e0, 0x3fd6b7e5f883cc54, 0x3fd53df5a0c09aa7, 0xbfd71993e3340a19, 0x3f3002789bd7fcf5, 0xbfd6027a4ffb6cd9], Terrain::DeepOcean),
    ((1, -1), [0x3fa4e9852de7fb36, 0xbfc65bde3bf43e94, 0x3fd2e5fbb13c01da, 0xbfb71bb978827e3d, 0x3feb1940d5c94160, 0xbf723f73ffba6085, 0xbfd1f6f283b2d7b2, 0xbfd8d1895e986625, 0xbfd598bdf135d2ee, 0x3fd4abed6238a536, 0x3fd5487055061025, 0xbfd6b7824b085640, 0xbf834e709fbf8eb6, 0xbfd5076ba8dd026d], Terrain::DeepOcean),
    ((0, -1), [0x3fa168c3bdb3cfab, 0xbfc8e3a7a13cf33f, 0x3fd16096eab4a4b1, 0x3fbd615323786aed, 0x3feadfd6715cb4fa, 0xbf82b1368dd36e87, 0xbfd1f6ed6401dd9f, 0xbfd8d14cdccb6d5f, 0xbfd601341748bab6, 0x3fd05c68ad61cb29, 0x3fd564bb26f6eb4d, 0xbfd65e0c09cbc230, 0xbf9463d3667874b0, 0xbfd4c9637eaec794], Terrain::DeepOcean),
    ((-1, 0), [0x3fa24583f148cfe3, 0xbfc9e2d65786c742, 0x3fd95692bb04f105, 0x3fc6b5fe0c45ea8d, 0x3fe8c8b8b25a1400, 0x3f660aeaeb0ae69e, 0xbfd1f6c3eb9c9dad, 0xbfd8d19735a749b8, 0xbfd593ddc3adec1e, 0x3fcea05be9faccac, 0x3fd57653a46bad5f, 0xbfd65124c17ff047, 0xbf9a0950c08e5d97, 0xbfd58eebfdc78c32], Terrain::DeepOcean),
    ((-1, 1), [0x3fa6892b15f6ca7d, 0xbfc8ad2cbb82bb47, 0x3fe201f54ebb4612, 0x3fc09e8c2d8153c5, 0x3fe782551f73c9e4, 0x3f97d90a7b4042dc, 0xbfd1f6c923a4247f, 0xbfd8d16ed0ed2e48, 0xbfd44e75ae447f09, 0x3fd504acd4c3b3a3, 0x3fd56b961e0f4feb, 0xbfd6a834fad677d0, 0xbf97469939f74309, 0xbfd6970a37980321], Terrain::DeepOcean),
    ((0, 1), [0x3fa9fc5ef75254b7, 0xbfc60812439b8d42, 0x3fd91f12b0666192, 0x3fc1a1e45787c0ad, 0x3fe73f4a77967606, 0x3f931086b8b1b6bf, 0xbfd1f6fa4f53094d, 0xbfd8d18dd31ba471, 0xbfd44cd61d034191, 0x3fd166e197384286, 0x3fd54f6c1753321a, 0xbfd711b47d415891, 0xbf822b6006b1178b, 0xbfd6cb74905dffe0], Terrain::DeepOcean),
    ((-1, -1), [0x3f9c074cd91c6f8e, 0xbfca8548e22ff9f7, 0x3fd2f1cab22ef647, 0x3fcec699ae0ad57f, 0x3fe9fd1fc8b60965, 0xbf84d60c08c501bf, 0xbfd1f67c21f38a99, 0xbfd8d1311737c6d0, 0xbfd65005af14aebe, 0x3fc9586852e2e6d0, 0x3fd58130fe222928, 0xbfd6109c659315e2, 0xbf9b53d3a275c009, 0xbfd471ca3f527d2b], Terrain::DeepOcean),
    ((7, -3), [0x3fb1bbfe7f347c80, 0xbfa2893ea64cb1eb, 0xbfc67378c5a103cf, 0xbfe344624ce2c52f, 0x3fee520aa663544e, 0xbf7afa39d61df4e0, 0xbfd1f2a10a009463, 0xbfd8d2b34a0ac867, 0xbfd3eaceac8a22d5, 0x3fe53659bf843cd4, 0x3fd4b7d3bce61569, 0xbfd89c58a245f438, 0x3fa39670975daf0a, 0xbfd4bbad13b89186], Terrain::DeepOcean),
    ((-97, 411), [0x3fd787723cb7bf9b, 0xbfc0ddde5a524848, 0x3fdd4d3856550f54, 0xbfdac8e7c156eddc, 0x3fe90606144f27d5, 0x3fc9d93fb66adae8, 0x3fda48de8edf864d, 0xbfda2797ebd9c9e0, 0x3fcc8b9ce4633fbe, 0x3fd0b50301b485a8, 0x3f865cb06eb337c0, 0xbfc98f0b03df8d5e, 0x3f98d37c48f035ab, 0xbfcbeadcfe6edfbb], Terrain::Grassland),
    ((1000, 1000), [0xbfca7a38fe986ae3, 0xbfd4bbba576d4ee0, 0x3fe0f7edfc3a66dc, 0xbfd52b9b41a7c30d, 0x3fe495ad394c9020, 0xbfc723f1db6ae8f1, 0xbfceba195163d17d, 0xbfebdc7ee482d6ac, 0xbfe40fbca79f0d71, 0x3fd49bf28aea9895, 0xbfc8d1334ded4eff, 0xbfb794ff094ec0c2, 0x3fcb4d4b45d55357, 0x3fd0e4bcdb55cde0], Terrain::DeepOcean),
    ((-1000, -1000), [0x3fd5db19bd98f9ca, 0x3fde6c143f964405, 0x3fde661d164a5c73, 0xbfe11d1e33a6328d, 0x3fe653b9d449166a, 0x3fd724663f6ccb76, 0x3fe390eedf4c6313, 0xbfd2bdda9c0f2455, 0x3fe0c95c61ccea35, 0x3fd54b199261560d, 0xbfd3527abf4f366e, 0x3fd872ca2fce6415, 0xbfdaf0e6649ab709, 0xbfc82438d46184dd], Terrain::Alpine),
    ((12345, -6789), [0xbfc24e9b7e7bdb72, 0xbfd89649decc499b, 0x3fd2712b48f40e53, 0xbfcd028e0c0bbf70, 0x3fefa0cc29a45b29, 0xbfc6dde1d6569d9d, 0xbfb673119ae3bf01, 0xbfd4fc0d59b29503, 0xbfe0002ff4fde014, 0x3fdbd3cf5b2f4ff1, 0x3fd8000a5c82ca9d, 0xbfc05c08e2b39771, 0xbfcc8109f8ea23a1, 0x3fcb7df9d2bc862c], Terrain::DeepOcean),
    ((-12345, 6789), [0x3fe1528705057ed6, 0x3fe0259fd04953da, 0x3fd8f0564e7e9bf6, 0xbfc8986eff84a2ac, 0x3fefc321a08366ec, 0x3fdf7578c3f3b9c1, 0x3fe38f9c4d84c69a, 0xbfe0f881d1a6c30b, 0x3fe5a60ec5143dbe, 0x3fc31c8ae27e68b8, 0xbfe85cc27a848ec0, 0xbfcfb4a633a27458, 0xbfc16293b2dabd65, 0xbfcb7f17137aa42e], Terrain::GlacialIce),
    ((32767, 0), [0x3fdcc7d2f1de9aad, 0xbfa6b5215fdd73d1, 0x3fc504440e49071f, 0xbfa053964393042b, 0x3fe8dfcc49b36a55, 0x3fd0d9d88925cfc7, 0x3febf209376f9dc0, 0x3fc4c1cc515ec6cb, 0x3fe12c41870b3b42, 0x3ff0000000000000, 0xbfe2c471f0549bcd, 0xbfd0ea68d23a6546, 0x3fbebff2da58ab12, 0x3fc079281f1c580f], Terrain::Alpine),
    ((0, 32767), [0xbfd4ae8650418a43, 0xbfb8fe482eb3405b, 0xbf90d6822e288f6f, 0x3fda916e37964f88, 0x3fef6d79574e60e1, 0xbfca29d19e4e295b, 0x3fe3047690bc3e99, 0xbfb5aa77db75285d, 0xbfd1090c182d3362, 0x3ff0000000000000, 0xbfb5ea45a6830cc8, 0xbfb70b3f45b4d92f, 0xbfae317d1fb4f3a1, 0x3fc5005b284f267f], Terrain::CoastalWater),
    ((-32767, 0), [0x3fd6ba2d32e74a64, 0x3fe632e57a7bf94c, 0xbfc2592576966f0b, 0x3f67ca851f07ed55, 0x3feae127900a795a, 0x3fd9796d92b4e167, 0x3fcd4a597c628ec5, 0xbfe718af44354139, 0x3fdcb0330c2c41ea, 0x3ff0000000000000, 0xbfd7964350588dd9, 0x3f9f063d7215325e, 0xbfd710fed5bb1bb2, 0x3f43fdd04b6536a0], Terrain::Mountain),
    ((0, -32767), [0xbfbb9ee06efda31f, 0x3fadbdea0f7b8529, 0xbfde3dd88c01860d, 0x3fda79af6ecb4eb3, 0x3fe4013c05fd60a8, 0xbfb2b30da694ad4c, 0xbfba70aefe47cd9b, 0x3fe750a6249095c0, 0xbfd257afee14b95b, 0x3ff0000000000000, 0xbf846cea2ab5e315, 0xbfbf9c83e6fbd749, 0xbfb48b7e3854de22, 0xbfb8f95616659511], Terrain::CoastalWater),
    ((32767, -32767), [0xbfddad1618474cfa, 0x3fe09f867c6a590b, 0xbfdf07700cc59342, 0xbfc1429088bcb85c, 0x3fe35047bdc0450e, 0xbfc550e1a0e3d94d, 0x3fd272d8382eae36, 0xbfe05f2837ea0c51, 0xbfd7cd8f01b76e8e, 0x3ff0000000000000, 0x3fcd9f638ce5eb8d, 0x3f9f1b7dd8e97f16, 0xbfaed172a6ac3cc3, 0x3fbc4b794c477a34], Terrain::CoastalWater),
    ((-32767, 32767), [0xbfbdb368be176591, 0xbfcb708d13506c56, 0x3fd9613040524719, 0x3fe78c8fe9af2681, 0x3fea6d5cb4c55212, 0xbfb1e66b62ab4f83, 0xbfb5c8f1c3380bd8, 0x3fde7d17bedda511, 0xbfd1294d7c368b3e, 0x3ff0000000000000, 0x3fd2fb5042ddf560, 0x3fc2f684bc5d94c7, 0x3fb4f1d85acb5f2f, 0xbfd380c2786cd8e3], Terrain::CoastalWater),
    ((10922, -4681), [0x3fd43cc03c6cde54, 0x3fd105e058588140, 0x3fa3d16cbafa0193, 0xbfb2fc6dc8a24c75, 0x3fedd06b05cf7771, 0x3fd0dce7f41a1d0d, 0xbfdb1afc96a3b248, 0xbfe5c4ac6e8f703d, 0x3f834c68159dfb4a, 0x3fc78d9da75014da, 0x3fca804730017c93, 0xbfd1fdb6011c60de, 0xbfcd3a065a5e609e, 0xbfc6871d32b18999], Terrain::Coast),
    ((-6553, 2978), [0xbfb1b86c12a42872, 0x3fc03fdb0c7fdadc, 0xbfbbd9acb236b31d, 0x3fbf3de740abb02b, 0x3fe51cc76c60c5a4, 0xbf81a7f833ce70da, 0xbfcb6aaf58588594, 0xbfd6e6eeaa960a75, 0xbfd42467b3d9187f, 0x3fd00a74597498d4, 0xbfb5c9e9993ec55d, 0x3fd24a9b8fef68b7, 0x3fcfbac7f73821d3, 0x3f9440a1560ee8d2], Terrain::DeepOcean),
];

#[test]
fn golden_coordinates_are_bit_exact() {
    let generator = Generator::with_defaults(GOLDEN_SEED);
    let mut failures = Vec::new();

    for ((q, r), expected, _) in GOLDEN {
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
            sample.heat,
            sample.moisture,
            sample.basin_influence,
            sample.volcanic,
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
        .map(|((q, r), _, _)| Coord::new(*q, *r))
        .collect();
    let batch = generator.tiles(&coords);

    for ((((q, r), expected, _), coord), batched) in GOLDEN.into_iter().zip(coords).zip(batch) {
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
    let coords: Vec<(i64, i64)> = GOLDEN.iter().map(|(c, _, _)| *c).collect();

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
    for ((q, r), expected, _) in GOLDEN {
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

#[test]
fn the_terrain_column_is_what_the_tile_api_reports() {
    // The classification, pinned. Unlike the scalars this is not a bit
    // pattern to compare, and unlike the climate bands it is not derivable
    // from the other columns by restating a ladder — terrain reads relief,
    // peak prominence, and coastal adjacency, none of which are in the table.
    // So this column is a golden in the strongest sense: the only thing that
    // can be said about it is that it has not moved.
    let generator = Generator::with_defaults(GOLDEN_SEED);
    let mut failures = Vec::new();
    for ((q, r), _, expected) in GOLDEN {
        let actual = generator.tile(Coord::new(q, r)).terrain;
        if actual != expected {
            failures.push(format!(
                "({q}, {r}): expected {}, got {}",
                expected.name(),
                actual.name()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} golden terrain(s) moved. This is an algorithm compatibility \
         change, not a test to re-record:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn the_terrain_column_covers_more_than_one_family_and_no_inland_water() {
    // A terrain column that had collapsed to one variant would pass the test
    // above forever. These coordinates are a spread of the world rather than
    // a spread of the vocabulary, so what can be asked of them is that they
    // are not all the same thing and that they include land as well as water.
    let terrains: Vec<Terrain> = GOLDEN.iter().map(|(_, _, t)| *t).collect();
    let mut distinct = terrains.clone();
    distinct.sort_unstable_by_key(|t| *t as u8);
    distinct.dedup();
    assert!(
        distinct.len() >= 5,
        "the table pins only {} distinct terrains",
        distinct.len()
    );
    assert!(terrains.iter().any(|t| t.is_water()));
    assert!(terrains.iter().any(|t| !t.is_water()));
    for terrain in terrains {
        assert!(
            !terrain.is_inland_water(),
            "the table records {}, which this version does not generate",
            terrain.name()
        );
    }
}

#[test]
fn the_climate_columns_are_classified_by_the_tile_api() {
    // The bands a tile reports must be the classification of the two numbers
    // this table pins, not of a second evaluation. Pinning the scalars without
    // this would leave the ladders free to drift under them.
    let generator = Generator::with_defaults(GOLDEN_SEED);
    let config = generator.config();
    let heat_index = NAMES.iter().position(|n| *n == "heat").unwrap();
    let moisture_index = NAMES.iter().position(|n| *n == "moisture").unwrap();

    for ((q, r), expected, _) in GOLDEN {
        let tile = generator.tile(Coord::new(q, r));
        let heat = f64::from_bits(expected[heat_index]);
        let moisture = f64::from_bits(expected[moisture_index]);
        assert_eq!(tile.heat_value.to_bits(), heat.to_bits(), "({q}, {r})");
        assert_eq!(
            tile.moisture_value.to_bits(),
            moisture.to_bits(),
            "({q}, {r})"
        );

        // The ladders, written out rather than called, so this is a check on
        // the classifier and not a restatement of it.
        let expected_heat = match heat {
            h if h <= config.polar_level => HeatBand::Polar,
            h if h <= config.cold_level => HeatBand::Cold,
            h if h <= config.temperate_level => HeatBand::Temperate,
            h if h <= config.warm_level => HeatBand::Warm,
            _ => HeatBand::Hot,
        };
        let expected_moisture = match moisture {
            m if m <= config.arid_level => MoistureBand::Arid,
            m if m <= config.dry_level => MoistureBand::Dry,
            m if m <= config.moderate_level => MoistureBand::Moderate,
            m if m <= config.humid_level => MoistureBand::Humid,
            _ => MoistureBand::Saturated,
        };
        assert_eq!(tile.climate.heat, expected_heat, "({q}, {r})");
        assert_eq!(tile.climate.moisture, expected_moisture, "({q}, {r})");
    }
}
