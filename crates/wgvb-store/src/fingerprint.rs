//! Canonical configuration bytes and the world fingerprint.
//!
//! See `DESIGN.md` section 21.2:
//!
//! ```text
//! fingerprint = SHA-256( algorithm_version_le_bytes || canonical_config_bytes )
//! ```
//!
//! # Why this lives in the store and not in `wgvb`
//!
//! Appendix B holds the core crate to exactly `serde` and `thiserror`, and a
//! fingerprint is a property of a *stored* world rather than of a generated
//! tile: nothing in the generation path reads one. The core crate defines what
//! a configuration is and which configurations are valid; this crate defines
//! what one weighs on disk.

use ciborium::Value;
use sha2::{Digest, Sha256};
use wgvb::Config;

/// Length of a configuration fingerprint, in bytes.
pub const FINGERPRINT_LEN: usize = 32;

/// A configuration fingerprint: SHA-256 over the algorithm version and the
/// canonical configuration bytes.
pub type Fingerprint = [u8; FINGERPRINT_LEN];

/// Why a configuration could not be reduced to canonical bytes.
///
/// In practice this cannot happen for a [`Config`] that passed
/// [`Config::validate`] — the type is eighty-odd numeric fields and `NaN` is
/// rejected before it gets here — but it is an error rather than a panic
/// because the failure would be in a dependency's serializer, and a library
/// should not abort a caller's process over that.
#[derive(Debug, thiserror::Error)]
#[error("the configuration cannot be serialized canonically: {0}")]
pub struct CanonicalError(String);

/// The canonical CBOR encoding of a complete effective configuration.
///
/// CBOR via `ciborium` rather than JSON: JSON invites whitespace,
/// float-formatting, and key-ordering variance, while CBOR from a struct is
/// binary and fixed in field order.
///
/// `-0.0` is normalized to `0.0` before encoding. The two compare equal and
/// generate identical worlds, so they must not fingerprint differently; `NaN`
/// cannot reach here at all because [`Config::validate`] rejects it.
///
/// # Errors
///
/// Returns [`CanonicalError`] if the configuration cannot be serialized.
pub fn canonical_config_bytes(config: &Config) -> Result<Vec<u8>, CanonicalError> {
    let mut value = Value::serialized(config).map_err(|error| CanonicalError(error.to_string()))?;
    normalize_negative_zero(&mut value);

    let mut bytes = Vec::new();
    ciborium::ser::into_writer(&value, &mut bytes)
        .map_err(|error| CanonicalError(error.to_string()))?;
    Ok(bytes)
}

/// The fingerprint of one algorithm version and one configuration.
///
/// The version is hashed as little-endian bytes *before* the configuration, so
/// two algorithm versions that happen to accept the same configuration still
/// fingerprint differently — which is the whole point, since they do not
/// generate the same world.
///
/// # Errors
///
/// Returns [`CanonicalError`] if the configuration cannot be serialized.
pub fn fingerprint(algorithm_version: u32, config: &Config) -> Result<Fingerprint, CanonicalError> {
    Ok(fingerprint_of_bytes(
        algorithm_version,
        &canonical_config_bytes(config)?,
    ))
}

/// The fingerprint of an algorithm version and already-canonical bytes.
///
/// Reading a world hashes the bytes that are *stored* rather than bytes
/// re-encoded from the decoded configuration. Those differ precisely when
/// somebody has edited the blob in place, and catching that is what the
/// fingerprint gate is for.
#[must_use]
pub fn fingerprint_of_bytes(algorithm_version: u32, config_bytes: &[u8]) -> Fingerprint {
    let mut hasher = Sha256::new();
    hasher.update(algorithm_version.to_le_bytes());
    hasher.update(config_bytes);
    hasher.finalize().into()
}

/// Rewrites every `-0.0` in a CBOR value tree as `0.0`.
///
/// A tree walk rather than eighty-four field assignments: the normalization is
/// a property of the encoding, so it belongs at the encoding, where adding a
/// configuration field cannot forget it.
fn normalize_negative_zero(value: &mut Value) {
    match value {
        // `-0.0 == 0.0` is true, so this branch catches both and the
        // assignment leaves only the positive one.
        Value::Float(float) => {
            if *float == 0.0 {
                *float = 0.0;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalize_negative_zero),
        Value::Map(entries) => {
            for (key, entry) in entries {
                normalize_negative_zero(key);
                normalize_negative_zero(entry);
            }
        }
        Value::Tag(_, inner) => normalize_negative_zero(inner),
        _ => {}
    }
}
