//! Generator configuration and its validation.
//!
//! See `DESIGN.md` sections 19.1, 21, 21.1, and 21.2.
//!
//! Configuration is immutable after construction and is authoritative world
//! data: the complete effective configuration, including every value that came
//! from a default, is written when a world is created. Reopening a world never
//! substitutes current program defaults for missing stored values.

use crate::{DEFAULT_CHUNK_SIZE_HEXES, DEFAULT_MACRO_REGION_SIZE_HEXES, DEFAULT_REGION_SIZE_HEXES};

/// Why a configuration was rejected.
///
/// One variant per failure class, so tests assert on variants instead of
/// matching message strings. See `DESIGN.md` section 19.1.
#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ConfigError {
    #[error("{field} must be finite, got {value}")]
    NotFinite { field: &'static str, value: f64 },
    #[error("{field} must be positive, got {value}")]
    NotPositive { field: &'static str, value: f64 },
    #[error("{field} must be in [{lo}, {hi}], got {value}")]
    OutOfRange {
        field: &'static str,
        value: f64,
        lo: f64,
        hi: f64,
    },
    #[error("fbm octaves must be in 1..=16, got {0}")]
    OctaveCount(u8),
}

/// Largest accepted fbm octave count. More octaves than this cannot add detail
/// at any wavelength the world can express, and each one costs a sample.
pub const MAX_FBM_OCTAVES: u8 = 16;

/// Immutable generator configuration.
///
/// Every value that can alter generated output lives here rather than as a
/// constant scattered through the implementation, so a world file records the
/// whole of what produced it. Scale fields name their unit in the identifier.
///
/// `deny_unknown_fields` is required, not stylistic: an older binary reading a
/// newer world file must reject it rather than silently ignore the fields it
/// does not understand. For the same reason no field carries
/// `#[serde(default)]` — a defaulted missing field would be a changed world with
/// an unchanged version number, which is precisely what the versioning scheme
/// exists to prevent. Adding a field here is an algorithm version change.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Elevation threshold separating ocean water from potential land, on the
    /// normalized `[-1, +1]` elevation scale.
    pub sea_level: f64,

    /// Wavelength of the broad continental land/ocean field.
    pub continental_wavelength_miles: f64,
    /// Wavelength of regional uplift.
    pub regional_wavelength_miles: f64,
    /// Wavelength of hill-scale relief.
    pub local_wavelength_miles: f64,
    /// Wavelength of the finest terrain detail.
    pub detail_wavelength_miles: f64,

    /// Wavelength of the domain-warp offset field.
    pub warp_wavelength_miles: f64,
    /// Magnitude of the domain-warp offset. To disable warping, compose a field
    /// graph without a warp stage rather than setting this to zero.
    pub warp_strength_miles: f64,

    /// Wavelength of the weaker, higher-frequency warp that breaks up local
    /// relief. `DESIGN.md` section 13 calls for low-frequency warps for large
    /// geography and weaker high-frequency warps for local irregularity, which
    /// is two warp scales, not one.
    pub detail_warp_wavelength_miles: f64,
    /// Magnitude of the high-frequency warp offset.
    pub detail_warp_strength_miles: f64,

    /// Octave count for fbm composition, in `1..=16`.
    pub fbm_octaves: u8,
    /// Frequency multiplier between successive fbm octaves.
    pub fbm_lacunarity: f64,
    /// Amplitude multiplier between successive fbm octaves.
    pub fbm_gain: f64,

    /// Weight of continentalness in the multi-scale composite of `DESIGN.md`
    /// section 10. The composite divides by the total weight, so these are
    /// relative amplitudes rather than absolute ones.
    pub continental_weight: f64,
    /// Weight of regional uplift in the multi-scale composite.
    pub regional_weight: f64,
    /// Weight of hill-scale relief in the multi-scale composite.
    pub local_weight: f64,
    /// Weight of the finest terrain detail in the multi-scale composite.
    pub detail_weight: f64,

    /// Macro-region edge, in hexes.
    pub macro_region_size_hexes: u32,
    /// Region edge, in hexes.
    pub region_size_hexes: u32,
    /// Chunk edge, in hexes. Chunks serve callers and caches; they do not
    /// define geography.
    pub chunk_size_hexes: u32,
}

impl Default for Config {
    /// The alpha defaults.
    ///
    /// Wavelengths follow the multi-scale table in `DESIGN.md` section 10, where
    /// one hex of wavelength is six miles of center-to-center distance. These are
    /// starting values for tuning, but changing one changes every world, so a
    /// change here is an algorithm version change like any other.
    fn default() -> Config {
        Config {
            sea_level: 0.0,

            // 1,000 hexes.
            continental_wavelength_miles: 6_000.0,
            // 300 hexes.
            regional_wavelength_miles: 1_800.0,
            // 20 hexes.
            local_wavelength_miles: 120.0,
            // 6 hexes.
            detail_wavelength_miles: 36.0,

            // 200 hexes, warping by up to 15 hexes.
            warp_wavelength_miles: 1_200.0,
            warp_strength_miles: 90.0,

            // 20 hexes, warping by up to 3 hexes.
            detail_warp_wavelength_miles: 120.0,
            detail_warp_strength_miles: 18.0,

            fbm_octaves: 5,
            fbm_lacunarity: 2.0,
            fbm_gain: 0.5,

            // A halving ladder: each finer scale contributes half of the one
            // above it, so broad structure dominates and detail textures it.
            continental_weight: 1.0,
            regional_weight: 0.5,
            local_weight: 0.25,
            detail_weight: 0.125,

            macro_region_size_hexes: DEFAULT_MACRO_REGION_SIZE_HEXES,
            region_size_hexes: DEFAULT_REGION_SIZE_HEXES,
            chunk_size_hexes: DEFAULT_CHUNK_SIZE_HEXES,
        }
    }
}

impl Config {
    /// Rejects any configuration that cannot produce a well-defined world.
    ///
    /// Checks run in a fixed field order so the reported failure is
    /// deterministic. `NaN` and infinities are rejected here, which is what
    /// keeps them out of the configuration fingerprint.
    pub fn validate(&self) -> Result<(), ConfigError> {
        in_range("sea_level", self.sea_level, -1.0, 1.0)?;

        positive(
            "continental_wavelength_miles",
            self.continental_wavelength_miles,
        )?;
        positive("regional_wavelength_miles", self.regional_wavelength_miles)?;
        positive("local_wavelength_miles", self.local_wavelength_miles)?;
        positive("detail_wavelength_miles", self.detail_wavelength_miles)?;

        positive("warp_wavelength_miles", self.warp_wavelength_miles)?;
        positive("warp_strength_miles", self.warp_strength_miles)?;
        positive(
            "detail_warp_wavelength_miles",
            self.detail_warp_wavelength_miles,
        )?;
        positive(
            "detail_warp_strength_miles",
            self.detail_warp_strength_miles,
        )?;

        if self.fbm_octaves == 0 || self.fbm_octaves > MAX_FBM_OCTAVES {
            return Err(ConfigError::OctaveCount(self.fbm_octaves));
        }
        in_range("fbm_lacunarity", self.fbm_lacunarity, 1.0, 4.0)?;
        positive("fbm_gain", self.fbm_gain)?;
        in_range("fbm_gain", self.fbm_gain, 0.0, 1.0)?;

        positive("continental_weight", self.continental_weight)?;
        positive("regional_weight", self.regional_weight)?;
        positive("local_weight", self.local_weight)?;
        positive("detail_weight", self.detail_weight)?;

        positive_size("macro_region_size_hexes", self.macro_region_size_hexes)?;
        positive_size("region_size_hexes", self.region_size_hexes)?;
        positive_size("chunk_size_hexes", self.chunk_size_hexes)?;

        Ok(())
    }
}

/// Rejects a non-finite or non-positive scale.
fn positive(field: &'static str, value: f64) -> Result<(), ConfigError> {
    if !value.is_finite() {
        return Err(ConfigError::NotFinite { field, value });
    }
    if value <= 0.0 {
        return Err(ConfigError::NotPositive { field, value });
    }
    Ok(())
}

/// Rejects a non-finite value or one outside an inclusive range.
fn in_range(field: &'static str, value: f64, lo: f64, hi: f64) -> Result<(), ConfigError> {
    debug_assert!(lo <= hi);
    if !value.is_finite() {
        return Err(ConfigError::NotFinite { field, value });
    }
    if value < lo || value > hi {
        return Err(ConfigError::OutOfRange {
            field,
            value,
            lo,
            hi,
        });
    }
    Ok(())
}

/// Rejects a zero cell size. A size is the divisor in `div_euclid`, and the
/// equivalence of Euclidean and floor division holds only for a positive
/// divisor, which is why sizes are unsigned and zero is an error.
fn positive_size(field: &'static str, value: u32) -> Result<(), ConfigError> {
    if value == 0 {
        return Err(ConfigError::NotPositive {
            field,
            value: f64::from(value),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_configuration_is_valid() {
        assert_eq!(Config::default().validate(), Ok(()));
    }

    #[test]
    fn default_wavelengths_are_ordered_coarse_to_fine() {
        // The multi-scale sum is meaningless if two scales cross over.
        let c = Config::default();
        assert!(c.continental_wavelength_miles > c.regional_wavelength_miles);
        assert!(c.regional_wavelength_miles > c.local_wavelength_miles);
        assert!(c.local_wavelength_miles > c.detail_wavelength_miles);
    }

    #[test]
    fn default_composite_weights_fall_coarse_to_fine() {
        // The composite of section 10 is dominated by broad structure; a finer
        // scale outweighing a coarser one produces noise, not geography.
        let c = Config::default();
        assert!(c.continental_weight > c.regional_weight);
        assert!(c.regional_weight > c.local_weight);
        assert!(c.local_weight > c.detail_weight);
    }

    #[test]
    fn the_detail_warp_is_shorter_and_weaker_than_the_geographic_warp() {
        // Section 13: low-frequency warps for large geography, weaker
        // high-frequency warps for local irregularity.
        let c = Config::default();
        assert!(c.detail_warp_wavelength_miles < c.warp_wavelength_miles);
        assert!(c.detail_warp_strength_miles < c.warp_strength_miles);
    }

    #[test]
    fn default_scales_are_whole_numbers_of_hexes() {
        // One hex of wavelength is 2 * APOTHEM_MILES of center-to-center
        // distance. DESIGN.md section 10.
        let hex = 2.0 * crate::APOTHEM_MILES;
        let c = Config::default();
        for (field, value) in [
            ("continental", c.continental_wavelength_miles),
            ("regional", c.regional_wavelength_miles),
            ("local", c.local_wavelength_miles),
            ("detail", c.detail_wavelength_miles),
            ("warp", c.warp_wavelength_miles),
            ("detail warp", c.detail_warp_wavelength_miles),
        ] {
            assert_eq!(
                value % hex,
                0.0,
                "{field} wavelength {value} is not a whole hex count"
            );
        }
    }

    /// A field name paired with a setter for it.
    type FloatSetter = (&'static str, fn(&mut Config, f64));

    /// Every float field, so a new one cannot skip validation unnoticed.
    fn float_fields() -> Vec<FloatSetter> {
        vec![
            ("sea_level", |c, v| c.sea_level = v),
            ("continental_wavelength_miles", |c, v| {
                c.continental_wavelength_miles = v
            }),
            ("regional_wavelength_miles", |c, v| {
                c.regional_wavelength_miles = v
            }),
            ("local_wavelength_miles", |c, v| {
                c.local_wavelength_miles = v
            }),
            ("detail_wavelength_miles", |c, v| {
                c.detail_wavelength_miles = v
            }),
            ("warp_wavelength_miles", |c, v| c.warp_wavelength_miles = v),
            ("warp_strength_miles", |c, v| c.warp_strength_miles = v),
            ("detail_warp_wavelength_miles", |c, v| {
                c.detail_warp_wavelength_miles = v
            }),
            ("detail_warp_strength_miles", |c, v| {
                c.detail_warp_strength_miles = v
            }),
            ("continental_weight", |c, v| c.continental_weight = v),
            ("regional_weight", |c, v| c.regional_weight = v),
            ("local_weight", |c, v| c.local_weight = v),
            ("detail_weight", |c, v| c.detail_weight = v),
            ("fbm_lacunarity", |c, v| c.fbm_lacunarity = v),
            ("fbm_gain", |c, v| c.fbm_gain = v),
        ]
    }

    #[test]
    fn nan_and_infinities_are_rejected_in_every_float_field() {
        for (field, set) in float_fields() {
            for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut c = Config::default();
                set(&mut c, bad);
                let error = c.validate().expect_err("a non-finite value was accepted");
                assert!(
                    matches!(error, ConfigError::NotFinite { field: f, .. } if f == field),
                    "{field} with {bad} produced {error:?}"
                );
            }
        }
    }

    #[test]
    fn non_positive_scales_are_rejected() {
        for field in [
            "continental_wavelength_miles",
            "regional_wavelength_miles",
            "local_wavelength_miles",
            "detail_wavelength_miles",
            "warp_wavelength_miles",
            "warp_strength_miles",
            "detail_warp_wavelength_miles",
            "detail_warp_strength_miles",
            "continental_weight",
            "regional_weight",
            "local_weight",
            "detail_weight",
            "fbm_gain",
        ] {
            for bad in [0.0, -0.0, -1.0] {
                let mut c = Config::default();
                let (_, set) = float_fields()
                    .into_iter()
                    .find(|(f, _)| *f == field)
                    .unwrap();
                set(&mut c, bad);
                let error = c.validate().expect_err("accepted a non-positive scale");
                assert!(
                    matches!(error, ConfigError::NotPositive { field: f, .. } if f == field),
                    "{field} with {bad} produced {error:?}"
                );
            }
        }
    }

    #[test]
    fn sea_level_outside_the_normalized_range_is_rejected() {
        for bad in [1.000_001, -1.000_001, 5.0, -5.0] {
            let config = Config {
                sea_level: bad,
                ..Config::default()
            };
            let error = config
                .validate()
                .expect_err("an out-of-range sea level was accepted");
            assert!(
                matches!(
                    error,
                    ConfigError::OutOfRange {
                        field: "sea_level",
                        lo: -1.0,
                        hi: 1.0,
                        ..
                    }
                ),
                "{bad} produced {error:?}"
            );
        }
        // The endpoints themselves are legal.
        for good in [-1.0, 0.0, 1.0] {
            assert_eq!(
                Config {
                    sea_level: good,
                    ..Config::default()
                }
                .validate(),
                Ok(()),
                "{good}"
            );
        }
    }

    #[test]
    fn out_of_range_fbm_shaping_is_rejected() {
        for bad in [0.5, 4.5] {
            let config = Config {
                fbm_lacunarity: bad,
                ..Config::default()
            };
            assert!(
                matches!(
                    config.validate(),
                    Err(ConfigError::OutOfRange {
                        field: "fbm_lacunarity",
                        ..
                    })
                ),
                "lacunarity {bad}"
            );
        }
        for good in [1.0, 2.0, 4.0] {
            let config = Config {
                fbm_lacunarity: good,
                ..Config::default()
            };
            assert_eq!(config.validate(), Ok(()), "lacunarity {good}");
        }

        let config = Config {
            fbm_gain: 1.5,
            ..Config::default()
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::OutOfRange {
                field: "fbm_gain",
                ..
            })
        ));
        assert_eq!(
            Config {
                fbm_gain: 1.0,
                ..Config::default()
            }
            .validate(),
            Ok(())
        );
    }

    #[test]
    fn the_octave_count_is_rejected_outside_one_through_sixteen() {
        for bad in [0, MAX_FBM_OCTAVES + 1, u8::MAX] {
            let config = Config {
                fbm_octaves: bad,
                ..Config::default()
            };
            assert_eq!(
                config.validate(),
                Err(ConfigError::OctaveCount(bad)),
                "{bad} octaves"
            );
        }
        for good in [1, 8, MAX_FBM_OCTAVES] {
            let config = Config {
                fbm_octaves: good,
                ..Config::default()
            };
            assert_eq!(config.validate(), Ok(()), "{good} octaves");
        }
    }

    #[test]
    fn a_zero_cell_size_is_rejected() {
        let cases: [(&str, Config); 3] = [
            (
                "macro_region_size_hexes",
                Config {
                    macro_region_size_hexes: 0,
                    ..Config::default()
                },
            ),
            (
                "region_size_hexes",
                Config {
                    region_size_hexes: 0,
                    ..Config::default()
                },
            ),
            (
                "chunk_size_hexes",
                Config {
                    chunk_size_hexes: 0,
                    ..Config::default()
                },
            ),
        ];
        for (field, config) in cases {
            let error = config
                .validate()
                .expect_err("a zero cell size was accepted");
            assert!(
                matches!(error, ConfigError::NotPositive { field: f, .. } if f == field),
                "{field} produced {error:?}"
            );
        }
    }

    #[test]
    fn an_unknown_field_is_rejected_rather_than_ignored() {
        // Section 21.1: an older binary reading a newer world file must reject
        // it. CBOR here because that is the canonical configuration format.
        let mut bytes = Vec::new();
        ciborium::into_writer(&Config::default(), &mut bytes).unwrap();
        let mut value: ciborium::Value = ciborium::from_reader(bytes.as_slice()).unwrap();
        value.as_map_mut().unwrap().push((
            ciborium::Value::Text("future_field_from_a_newer_version".to_string()),
            ciborium::Value::Float(1.0),
        ));

        let mut extended = Vec::new();
        ciborium::into_writer(&value, &mut extended).unwrap();
        let result: Result<Config, _> = ciborium::from_reader(extended.as_slice());
        assert!(result.is_err(), "an unknown field was silently accepted");
    }

    #[test]
    fn a_missing_field_is_rejected_rather_than_defaulted() {
        // Section 21.1: no field affecting generation may carry
        // `#[serde(default)]`, so dropping one must fail to deserialize.
        let mut bytes = Vec::new();
        ciborium::into_writer(&Config::default(), &mut bytes).unwrap();
        let value: ciborium::Value = ciborium::from_reader(bytes.as_slice()).unwrap();
        let entries = value.as_map().unwrap().clone();
        assert!(!entries.is_empty());

        for index in 0..entries.len() {
            let mut reduced = entries.clone();
            let (removed, _) = reduced.remove(index);
            let mut truncated = Vec::new();
            ciborium::into_writer(&ciborium::Value::Map(reduced), &mut truncated).unwrap();
            let result: Result<Config, _> = ciborium::from_reader(truncated.as_slice());
            assert!(result.is_err(), "{removed:?} was silently defaulted");
        }
    }

    #[test]
    fn serialization_round_trips_and_is_byte_stable() {
        let config = Config::default();
        let mut first = Vec::new();
        ciborium::into_writer(&config, &mut first).unwrap();
        let mut second = Vec::new();
        ciborium::into_writer(&config, &mut second).unwrap();
        assert_eq!(first, second, "canonical bytes must not vary between calls");

        let decoded: Config = ciborium::from_reader(first.as_slice()).unwrap();
        assert_eq!(decoded, config);
    }
}
