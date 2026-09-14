//! The WGVB configuration as a thing on disk, and as a thing on a form.
//!
//! [`wgvb::Config`] says what a configuration *is* and which configurations are
//! valid. This crate says what one weighs: the canonical CBOR of section 21.2
//! and the fingerprint over it, and a TOML file a person can read, edit, keep
//! notes in, and hand back.
//!
//! # Two serializations, on purpose
//!
//! They answer different questions and neither can do the other's job.
//!
//! - **CBOR** is the fingerprint's. It is binary, fixed in field order, and
//!   never seen by a human, which is exactly what an identity needs. See
//!   [`canonical_config_bytes`].
//! - **TOML** is the person's. It takes comments, it sorts its keys so a field
//!   can be found by eye, and it round-trips an `f64` exactly. See [`to_toml`].
//!
//! The fingerprint is taken over the CBOR in both cases, so the TOML file's
//! spelling — key order, whitespace, the comments in the header — cannot change
//! a world's identity.
//!
//! # Editing one field
//!
//! [`fields`] and [`with_field`] are what a form is built from: the complete
//! effective configuration as name-and-value pairs, and one field changed by
//! name. Both go through the same [`toml::Table`] the file does, so the list of
//! editable fields *is* the struct rather than a table beside it that somebody
//! has to remember to update.

mod fingerprint;

pub use fingerprint::{
    CanonicalError, FINGERPRINT_LEN, Fingerprint, canonical_config_bytes, fingerprint,
    fingerprint_of_bytes,
};

use std::fmt::Write as _;

use toml::Value;
use wgvb::{ALGORITHM_VERSION, Config};

/// Why a configuration file, or one field of one, was refused.
///
/// One variant per failure class, so a caller can answer each in the way it
/// deserves — an unknown field names itself in a 400, an invalid value shows
/// the range it missed — rather than matching on message text.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    /// The text is not TOML at all.
    #[error("this is not a TOML configuration file: {0}")]
    Syntax(String),
    /// The text is TOML, but not a configuration: a missing field, a field of
    /// the wrong type, or a field this binary does not know.
    ///
    /// The last of those is `deny_unknown_fields` doing its job. A file written
    /// by a newer binary must be refused rather than half applied, because a
    /// field silently ignored is a different world under an unchanged version.
    #[error("this is not a configuration for algorithm version {ALGORITHM_VERSION}: {0}")]
    Shape(String),
    /// No field has this name.
    #[error("no configuration field is called {name:?}")]
    UnknownField { name: String },
    /// A field's value is not a number of the kind that field takes.
    #[error("{name} takes {wanted}, and {value:?} is not one")]
    NotANumber {
        name: String,
        wanted: &'static str,
        value: String,
    },
    /// The configuration is well formed and would not produce a world.
    #[error(transparent)]
    Invalid(#[from] wgvb::ConfigError),
    /// The configuration cannot be written out.
    #[error(transparent)]
    Canonical(#[from] CanonicalError),
}

/// One editable configuration field, as a form needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The field's name, which is its name in the struct and in the file.
    pub name: String,
    /// Its current value, formatted the way [`with_field`] will read it back.
    pub value: String,
    /// Its value in [`Config::default`], for a form that marks what has moved.
    pub default: String,
    /// Whether this field takes a whole number. Octave counts and sizes do;
    /// every threshold, weight, and wavelength does not.
    pub whole: bool,
}

impl Field {
    /// Whether this field has been moved off the value this binary ships.
    #[must_use]
    pub fn is_changed(&self) -> bool {
        self.value != self.default
    }
}

/// The complete effective configuration as editable fields, in file order.
///
/// Every field, including the ones that still hold their default: `DESIGN.md`
/// section 21 calls the complete effective configuration authoritative, and a
/// form that showed only what had changed would be a form that could not change
/// anything else.
///
/// # Errors
///
/// [`FileError::Canonical`] if the configuration cannot be serialized, which a
/// validated configuration cannot fail.
pub fn fields(config: &Config) -> Result<Vec<Field>, FileError> {
    let current = table(config)?;
    let defaults = table(&Config::default())?;
    Ok(current
        .iter()
        .map(|(name, value)| Field {
            name: name.clone(),
            value: text_of(value),
            default: defaults.get(name).map_or_else(String::new, text_of),
            whole: value.is_integer(),
        })
        .collect())
}

/// This configuration with one field set from text a person typed.
///
/// The field's kind comes from the field itself rather than from a table
/// written beside it: whatever [`Config`] declares is what the value is parsed
/// as, so a field added to the struct is editable the day it is added and no
/// list can fall behind.
///
/// The result is validated before it is returned, so a caller that gets a
/// [`Config`] back has one that will build a generator.
///
/// # Errors
///
/// [`FileError::UnknownField`] for a name no field has,
/// [`FileError::NotANumber`] for text that is not the kind of number that field
/// takes, and [`FileError::Invalid`] for a value the generator refuses — which
/// names the field and the range it missed.
pub fn with_field(config: &Config, name: &str, text: &str) -> Result<Config, FileError> {
    let mut fields = table(config)?;
    let current = fields.get(name).ok_or_else(|| FileError::UnknownField {
        name: name.to_string(),
    })?;

    let text = text.trim();
    let value = if current.is_integer() {
        Value::Integer(text.parse::<i64>().map_err(|_| FileError::NotANumber {
            name: name.to_string(),
            wanted: "a whole number",
            value: text.to_string(),
        })?)
    } else {
        let number = text.parse::<f64>().map_err(|_| FileError::NotANumber {
            name: name.to_string(),
            wanted: "a number",
            value: text.to_string(),
        })?;
        // A non-finite value would be refused by `Config::validate` anyway, but
        // it would be refused as a field rather than as a spelling, and
        // "sea_level must be finite, got NaN" is a worse answer to `?` than
        // "sea_level takes a number".
        if !number.is_finite() {
            return Err(FileError::NotANumber {
                name: name.to_string(),
                wanted: "a finite number",
                value: text.to_string(),
            });
        }
        Value::Float(number)
    };

    fields.insert(name.to_string(), value);
    from_table(fields)
}

/// The configuration as a TOML file.
///
/// The header names the algorithm version and the fingerprint the file was
/// written from. Both are comments, so reading the file back ignores them —
/// they are for the person who opens it in six weeks and wants to know what it
/// came out of, not for the parser.
///
/// # Errors
///
/// [`FileError::Canonical`] if the configuration cannot be serialized.
pub fn to_toml(config: &Config) -> Result<String, FileError> {
    let fingerprint = fingerprint(ALGORITHM_VERSION, config)?;
    let mut out = String::with_capacity(4 * 1024);
    out.push_str("# WGVB configuration.\n");
    let _ = writeln!(out, "# algorithm version {ALGORITHM_VERSION}");
    out.push_str("# fingerprint ");
    for byte in fingerprint {
        let _ = write!(out, "{byte:02x}");
    }
    out.push_str(
        "\n#\n\
         # Every field of the complete effective configuration is here, including the\n\
         # ones still at their default: a missing field is a refusal, not a default.\n\
         # Comments and key order do not affect the fingerprint, which is taken over\n\
         # the canonical CBOR of the values.\n\n",
    );
    let body = toml::to_string(config).map_err(|error| FileError::Shape(error.to_string()))?;
    out.push_str(&body);
    Ok(out)
}

/// A configuration read back from a TOML file.
///
/// Validated before it is returned, so a file that parses but could not build a
/// world is refused here rather than at the next render.
///
/// # Errors
///
/// [`FileError::Syntax`] for text that is not TOML, [`FileError::Shape`] for a
/// missing, mistyped, or unknown field, and [`FileError::Invalid`] for a
/// configuration the generator refuses.
pub fn from_toml(text: &str) -> Result<Config, FileError> {
    let table: toml::Table = text
        .parse()
        .map_err(|error: toml::de::Error| FileError::Syntax(error.to_string()))?;
    from_table(table)
}

/// The configuration as a sorted table of scalars.
fn table(config: &Config) -> Result<toml::Table, FileError> {
    Value::try_from(config)
        .map_err(|error| FileError::Shape(error.to_string()))?
        .as_table()
        .cloned()
        .ok_or_else(|| FileError::Shape("a configuration is a table of values".to_string()))
}

/// A validated configuration from a table of scalars.
fn from_table(table: toml::Table) -> Result<Config, FileError> {
    let config: Config = Value::Table(table)
        .try_into()
        .map_err(|error: toml::de::Error| FileError::Shape(error.to_string()))?;
    config.validate()?;
    Ok(config)
}

/// One value as the text a form shows and [`with_field`] reads back.
fn text_of(value: &Value) -> String {
    match value {
        Value::Integer(number) => number.to_string(),
        // `{}` on an `f64` is the shortest text that parses back to exactly
        // this value, which is the whole reason a form can round-trip one.
        Value::Float(number) => number.to_string(),
        other => other.to_string(),
    }
}
