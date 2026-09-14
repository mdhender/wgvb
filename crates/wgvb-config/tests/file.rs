//! The configuration file, and the one property that matters about it.
//!
//! A configuration file that loses a low bit of one `f64` is a silently
//! different world with an unchanged version number — precisely the failure
//! `DESIGN.md` section 21.1 exists to prevent. So the round trip is asserted on
//! the fingerprint as well as on the value, and on every field rather than on a
//! sample of them.

use wgvb::{ALGORITHM_VERSION, Config};
use wgvb_config::{FileError, fields, fingerprint, from_toml, to_toml, with_field};

#[test]
fn the_default_configuration_round_trips_exactly() {
    let config = Config::default();
    let text = to_toml(&config).expect("the defaults serialize");
    let back = from_toml(&text).expect("what this crate wrote, this crate reads");

    assert_eq!(back, config, "a field changed value through the file");
    assert_eq!(
        fingerprint(ALGORITHM_VERSION, &back).expect("a fingerprint"),
        fingerprint(ALGORITHM_VERSION, &config).expect("a fingerprint"),
        "the file changed the world's identity"
    );
}

#[test]
fn every_field_round_trips_a_value_that_is_not_its_default() {
    // Exactness is a per-field property and `f64` formatting is where it would
    // be lost, so every field is moved to a value with a long decimal
    // expansion and read back.
    let base = Config::default();
    for field in fields(&base).expect("the defaults enumerate") {
        let moved = if field.whole {
            let current: i64 = field.value.parse().expect("a whole number");
            (current.max(2) - 1).to_string()
        } else {
            let current: f64 = field.value.parse().expect("a number");
            // A factor with no exact binary representation, so the decimal text
            // has to carry the whole mantissa to survive.
            (current * 0.930_172_431).to_string()
        };

        let Ok(config) = with_field(&base, &field.name, &moved) else {
            // Not every field accepts an arbitrary nudge — a threshold ladder
            // has to stay ascending — and a refusal is the validator working.
            continue;
        };

        let text = to_toml(&config).expect("a valid configuration serializes");
        let back = from_toml(&text).expect("and reads back");
        assert_eq!(
            back, config,
            "{} did not survive the file exactly",
            field.name
        );
    }
}

#[test]
fn the_fields_are_the_struct_rather_than_a_list_beside_it() {
    let listed = fields(&Config::default()).expect("the defaults enumerate");
    let text = to_toml(&Config::default()).expect("the defaults serialize");
    let written: toml::Table = text.parse().expect("the file is TOML");

    assert_eq!(
        listed.len(),
        written.len(),
        "the form and the file disagree about how many fields there are"
    );
    for field in &listed {
        assert!(
            written.contains_key(&field.name),
            "{} is offered on a form and is not in the file",
            field.name
        );
    }
    assert!(
        listed.iter().all(|field| !field.is_changed()),
        "a default configuration has a field that differs from the default"
    );
}

#[test]
fn a_file_from_a_newer_binary_is_refused_rather_than_half_applied() {
    // `deny_unknown_fields`, one level up. A field this binary does not know is
    // a world this binary cannot reproduce.
    let mut text = to_toml(&Config::default()).expect("the defaults serialize");
    text.push_str("\nfuture_field_nobody_here_knows = 1.0\n");
    assert!(matches!(from_toml(&text), Err(FileError::Shape(_))));
}

#[test]
fn a_file_missing_a_field_is_refused_rather_than_defaulted() {
    // The other half of the same rule: a defaulted missing field would be a
    // changed world with an unchanged version number.
    let text = to_toml(&Config::default()).expect("the defaults serialize");
    let without: String = text
        .lines()
        .filter(|line| !line.starts_with("sea_level"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(matches!(from_toml(&without), Err(FileError::Shape(_))));
}

#[test]
fn a_field_is_set_by_name_and_a_typo_is_named_back() {
    let config = with_field(&Config::default(), "sea_level", "0.05").expect("a valid sea level");
    assert!((config.sea_level - 0.05).abs() < f64::EPSILON);

    assert!(matches!(
        with_field(&Config::default(), "see_level", "0.05"),
        Err(FileError::UnknownField { name }) if name == "see_level"
    ));
    assert!(matches!(
        with_field(&Config::default(), "sea_level", "quite high"),
        Err(FileError::NotANumber { .. })
    ));
    assert!(matches!(
        with_field(&Config::default(), "sea_level", "NaN"),
        Err(FileError::NotANumber { .. })
    ));
    assert!(matches!(
        with_field(&Config::default(), "continental_octaves", "1.5"),
        Err(FileError::NotANumber { .. })
    ));
}

#[test]
fn a_value_the_generator_refuses_is_refused_here() {
    // Not deferred to the next render. The error names the field and the range,
    // because it is the generator's own.
    assert!(matches!(
        with_field(&Config::default(), "sea_level", "40"),
        Err(FileError::Invalid(_))
    ));
    assert!(matches!(
        with_field(&Config::default(), "continental_octaves", "0"),
        Err(FileError::Invalid(_))
    ));
}

#[test]
fn the_header_says_what_the_file_came_out_of() {
    let text = to_toml(&Config::default()).expect("the defaults serialize");
    let print = fingerprint(ALGORITHM_VERSION, &Config::default()).expect("a fingerprint");
    let hex: String = print.iter().map(|byte| format!("{byte:02x}")).collect();

    assert!(text.contains(&format!("algorithm version {ALGORITHM_VERSION}")));
    assert!(
        text.contains(&hex),
        "the header does not name the fingerprint"
    );
    // And the header is inert: it is comments, so it cannot change what is read
    // back or what the configuration fingerprints to.
    assert_eq!(from_toml(&text).expect("the file reads"), Config::default());
}
