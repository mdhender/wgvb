//! The configuration fingerprint of `DESIGN.md` section 21.2.
//!
//! ```text
//! fingerprint = SHA-256( algorithm_version_le_bytes || canonical_config_bytes )
//! ```

use wgvb::{ALGORITHM_VERSION, Config};
use wgvb_store::{canonical_config_bytes, fingerprint, fingerprint_of_bytes};

/// The fingerprint of the algorithm version 5 defaults.
///
/// The *formula* is derived independently below, with `sha2` driven directly
/// rather than through the crate under test. This constant is the other half of
/// the job the issue asks for: fingerprint stability across processes and
/// targets. A value computed fresh in each run proves only that the code agrees
/// with itself today; a written-down one fails on the day the encoding, the
/// dependency, or a default moves, which is exactly when a compatibility
/// decision is owed.
///
/// Updating it is that decision, and it belongs in a commit message.
const DEFAULT_FINGERPRINT: &str =
    "2e3f10ef6c94ef7424ddf491f272f92d81c3de7eb579dccf522816d43266a9fe";

/// Lowercase hex, for a readable golden.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn the_fingerprint_is_sha256_of_the_version_and_the_canonical_bytes() {
    use sha2::{Digest, Sha256};

    let config = Config::default();
    let bytes = canonical_config_bytes(&config).expect("the defaults encode");

    // The formula, spelled out, with no help from the crate being tested.
    let mut hasher = Sha256::new();
    hasher.update(ALGORITHM_VERSION.to_le_bytes());
    hasher.update(&bytes);
    let expected: [u8; 32] = hasher.finalize().into();

    assert_eq!(fingerprint(ALGORITHM_VERSION, &config).unwrap(), expected);
    assert_eq!(fingerprint_of_bytes(ALGORITHM_VERSION, &bytes), expected);
    assert_eq!(hex(&expected), DEFAULT_FINGERPRINT);
}

#[test]
fn the_algorithm_version_is_part_of_the_fingerprint() {
    // Two versions that accept the same configuration do not generate the same
    // world, so they must not fingerprint the same.
    let config = Config::default();
    assert_ne!(
        fingerprint(ALGORITHM_VERSION, &config).unwrap(),
        fingerprint(ALGORITHM_VERSION + 1, &config).unwrap()
    );
}

#[test]
fn negative_zero_fingerprints_as_positive_zero() {
    // `-0.0 == 0.0`, and the generator cannot tell them apart, so the
    // fingerprint must not either. Without normalization their CBOR differs in
    // exactly one bit, which is the sign.
    let positive = Config {
        sea_level: 0.0,
        ..Config::default()
    };
    let negative = Config {
        sea_level: -0.0,
        ..Config::default()
    };

    assert!(
        negative.sea_level.is_sign_negative(),
        "the test lost its negative zero before it was used"
    );
    assert_eq!(
        canonical_config_bytes(&positive).unwrap(),
        canonical_config_bytes(&negative).unwrap(),
    );
    assert_eq!(
        fingerprint(ALGORITHM_VERSION, &positive).unwrap(),
        fingerprint(ALGORITHM_VERSION, &negative).unwrap(),
    );
}

#[test]
fn one_bit_of_one_field_changes_the_fingerprint() {
    let base = Config::default();
    // The next representable f64 above the default sea level: the smallest
    // change the type can express, and the fingerprint must see it.
    let nudged = Config {
        sea_level: f64::from_bits(base.sea_level.to_bits() + 1),
        ..Config::default()
    };

    assert_ne!(base.sea_level, nudged.sea_level);
    assert_ne!(
        fingerprint(ALGORITHM_VERSION, &base).unwrap(),
        fingerprint(ALGORITHM_VERSION, &nudged).unwrap(),
    );
}

#[test]
fn canonical_bytes_are_stable_across_repeated_encodings() {
    // Field order is fixed by the struct, not by a map iteration order, so the
    // encoding must not vary between calls within a process either.
    let config = Config::default();
    let first = canonical_config_bytes(&config).unwrap();
    for _ in 0..8 {
        assert_eq!(canonical_config_bytes(&config).unwrap(), first);
    }
}

#[test]
fn a_configuration_round_trips_through_its_canonical_bytes() {
    // The stored bytes are the world. If they do not decode to the
    // configuration they were made from, reopening a world silently generates a
    // different one.
    let config = Config {
        sea_level: 0.125,
        continental_wavelength_miles: 1_234.5,
        ..Config::default()
    };

    let bytes = canonical_config_bytes(&config).unwrap();
    let decoded: Config = ciborium::from_reader(bytes.as_slice()).expect("the bytes decode");
    assert_eq!(decoded, config);
}

#[test]
fn the_encoding_shrinks_floats_and_still_round_trips_bit_for_bit() {
    // `ciborium` writes a float in the shortest CBOR form that represents it
    // exactly, so a value that fits an `f32` or an `f16` is encoded narrow.
    // That is lossless — the encoder shrinks only on an exact round trip — and
    // it is still binary rather than the formatted string section 21.2 warns
    // about, so the rule that `f64` is hashed as its bits is kept.
    //
    // It does mean the encoding is a dependency's policy rather than this
    // crate's. `DEFAULT_FINGERPRINT` is the tripwire: a `ciborium` release that
    // changed the policy would fail that assertion loudly instead of quietly
    // invalidating every world file.
    //
    // What must hold regardless is that a stored world decodes to the
    // configuration it was written from, bit for bit.
    for value in [0.5_f64, -0.25, 1.0 / 3.0, 0.1, 1e-300, f64::MIN_POSITIVE] {
        let config = Config {
            sea_level: value,
            ..Config::default()
        };
        let bytes = canonical_config_bytes(&config).expect("the configuration encodes");
        let decoded: Config = ciborium::from_reader(bytes.as_slice()).expect("the bytes decode");
        assert_eq!(
            decoded.sea_level.to_bits(),
            value.to_bits(),
            "{value} did not survive the encoding"
        );
    }
}

#[test]
fn a_not_a_number_is_rejected_by_validation_and_never_reaches_a_fingerprint() {
    // Section 21.2 puts the `NaN` gate in validation rather than in the
    // encoder, and this is why that is sufficient: a world is validated before
    // it is stored, so the encoder never sees one. Both halves are asserted
    // here, because only the pair of them is the argument.
    for field in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let config = Config {
            sea_level: field,
            ..Config::default()
        };
        assert!(
            config.validate().is_err(),
            "{field} passed validation and could have been stored"
        );
    }

    // And if one somehow did: two `NaN`s with different bit patterns are the
    // same non-number, and would fingerprint as two different worlds. The
    // encoder cannot fix that, which is the reason the gate is upstream of it.
    let quiet = Config {
        sea_level: f64::NAN,
        ..Config::default()
    };
    let other = Config {
        sea_level: f64::from_bits(f64::NAN.to_bits() | 1),
        ..Config::default()
    };
    assert!(other.sea_level.is_nan(), "the second value is still a NaN");
    assert_ne!(
        canonical_config_bytes(&quiet).unwrap(),
        canonical_config_bytes(&other).unwrap(),
        "two NaNs encoded identically, which would have made this gate optional"
    );
}
