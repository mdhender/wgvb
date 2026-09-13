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
    #[error("elevation contrast passes must be in 0..=4, got {0}")]
    PassCount(u8),
    #[error("{field} ({value}) must be greater than {below} ({limit})")]
    NotAscending {
        field: &'static str,
        value: f64,
        below: &'static str,
        limit: f64,
    },
}

/// Largest accepted fbm octave count. More octaves than this cannot add detail
/// at any wavelength the world can express, and each one costs a sample.
pub const MAX_FBM_OCTAVES: u8 = 16;

/// Largest accepted elevation shaping pass count.
///
/// Four passes at full contrast steepen the middle of the scale by more than
/// five, which already leaves almost every tile at one extreme or the other.
/// More would be a step function with extra arithmetic.
pub const MAX_CONTRAST_PASSES: u8 = 4;

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
    ///
    /// One of the six [`crate::Elevation`] band thresholds, which must ascend
    /// strictly: `deep_water_level < sea_level < upland_level <
    /// highland_level < mountain_level`. See `DESIGN.md` sections 14.1 and 15.
    ///
    /// Sea level is a *threshold on a globally stable field*, never a quantile
    /// of a generated sample. Section 33.5: computing it from the explored area
    /// would make exploration order change the world. If a target land fraction
    /// is wanted, move this value and re-measure, which is what the
    /// distribution tests in `tests/elevation.rs` are for.
    pub sea_level: f64,

    /// Below this elevation a water tile is [`crate::Elevation::DeepWater`].
    pub deep_water_level: f64,
    /// Above [`Config::sea_level`] and up to this, land is
    /// [`crate::Elevation::Lowland`].
    pub upland_level: f64,
    /// Up to this, land is [`crate::Elevation::Upland`].
    pub highland_level: f64,
    /// Up to this, land is [`crate::Elevation::Highland`]; above it,
    /// [`crate::Elevation::Mountain`].
    pub mountain_level: f64,

    /// Wavelength of the broad continental land/ocean field.
    pub continental_wavelength_miles: f64,
    /// Wavelength of regional uplift.
    pub regional_wavelength_miles: f64,
    /// Wavelength of hill-scale relief.
    pub local_wavelength_miles: f64,
    /// Wavelength of the finest terrain detail.
    pub detail_wavelength_miles: f64,

    /// Wavelength of the ridge structure field of `DESIGN.md` section 10.
    ///
    /// The *crests* this produces are the zero crossings of that field, so they
    /// are spaced about half a wavelength apart rather than one.
    pub ridge_wavelength_miles: f64,
    /// How far along a ridge line the structure field is averaged, in miles.
    ///
    /// Ridges are elongated by averaging the field at three points spaced this
    /// far apart along the region's ridge orientation, which stretches features
    /// along the ridge and leaves them sharp across it. Zero would leave the
    /// field isotropic and the ridge orientation unused.
    pub ridge_elongation_miles: f64,

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
    /// Weight of the blended regional elevation bias in the elevation scalar.
    ///
    /// This is the `regional_uplift` term of `DESIGN.md` section 10, and it is
    /// the only term that comes from the region hierarchy rather than from a
    /// continuous noise field.
    pub uplift_weight: f64,
    /// Weight of the ridge structure term in the elevation scalar.
    pub ridge_weight: f64,

    /// Constant added to the elevation composite before it is shaped.
    ///
    /// This is the land-fraction knob, and it is why [`Config::sea_level`] can
    /// stay at zero where `DESIGN.md` section 14's scale says sea level is.
    /// A weighted average of zero-mean fields puts about half the world above
    /// zero; a negative offset sinks the world until the fraction above sea
    /// level is the intended one.
    ///
    /// Section 15 allows the land fraction to be tuned by "the continentalness
    /// distribution and the sea-level threshold", and section 33.5 forbids
    /// deriving either from a generated sample. This is the first of those two:
    /// a fixed shift of the distribution, measured once against the
    /// distribution tests and then stored, never recomputed at run time.
    pub elevation_offset: f64,

    /// How hard one shaping pass pushes the elevation composite away from sea
    /// level, in `[0, 1]`.
    ///
    /// A weighted average of fields that are each in `[-1, +1]` clusters near
    /// zero: with the alpha defaults, ninety per cent of the composite lies
    /// within a quarter of the scale, which would leave a world with no deep
    /// ocean and no mountains. One pass is the odd polynomial
    /// `x + contrast * (x - x^3) / 2`, which is monotone on `[-1, +1]` for any
    /// contrast in `[0, 1]`, fixes both ends exactly, and steepens the middle
    /// by `1 + contrast / 2`. A polynomial rather than an exponent because
    /// section 25.2 bars `powf`.
    ///
    /// Zero disables the shaping. One is the steepest a single pass can be and
    /// stay monotone — above it the map would fold two elevations onto one — so
    /// validation rejects more, and [`Config::elevation_contrast_passes`] is
    /// how the shaping is made stronger than that.
    pub elevation_contrast: f64,

    /// How many times the shaping pass is applied, in `0..=4`.
    ///
    /// A monotone map composed with itself is monotone, and each of these
    /// passes fixes `-1`, `0`, and `+1`, so the composition does too. Two
    /// passes at full contrast steepen the middle by `2.25`, which is what the
    /// alpha defaults need for the top and bottom of the scale to be reachable
    /// at all.
    ///
    /// Paired with [`Config::elevation_contrast`] rather than replacing it: the
    /// pass count chooses the order of magnitude and the contrast trims within
    /// it, which together cover the range continuously. Zero of either disables
    /// the shaping.
    pub elevation_contrast_passes: u8,

    /// The per-hex elevation step that reads as maximum local relief.
    ///
    /// [`crate::Generator::relief`] averages the absolute elevation difference
    /// to the six neighbors and divides by this, so a smaller value makes more
    /// of the world read as steep. Named for its unit: elevation units per one
    /// hex of center-to-center distance.
    pub relief_reference_delta_per_hex: f64,

    /// Macro-region anchor spacing, in hexes.
    pub macro_region_size_hexes: u32,
    /// Region anchor spacing, in hexes.
    pub region_size_hexes: u32,
    /// Chunk edge, in hexes. Chunks serve callers and caches; they do not
    /// define geography, and they are deliberately not an anchored level of the
    /// region hierarchy.
    pub chunk_size_hexes: u32,

    /// Weight of the macro-region level in the combined region parameters of
    /// `DESIGN.md` section 11. Relative to [`Config::region_influence`], not
    /// absolute: the combination divides by the total.
    ///
    /// Named `influence` rather than `weight` because
    /// [`Config::regional_weight`] already names something else entirely — the
    /// share of the *noise* field at regional wavelength in the elevation
    /// composite. The two are independent knobs and a reader must not have to
    /// guess which is which.
    pub macro_region_influence: f64,
    /// Weight of the region level in the combined region parameters.
    pub region_influence: f64,
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

            // Measured against `tests/elevation.rs` rather than guessed. Sea
            // level stays at zero, where section 14's scale says it is, and
            // `elevation_offset` is what puts 29 per cent of the world above
            // it. The remaining thresholds then give, as a share of the world:
            // 54 per cent deep water, 17 shallow, 16 lowland, 9 upland, 3
            // highland, and 1 mountain — a shelf that is a fringe of the ocean
            // rather than half of it, and land bands that fall away with
            // height.
            deep_water_level: -0.15,
            upland_level: 0.18,
            highland_level: 0.35,
            mountain_level: 0.48,

            // 1,000 hexes.
            continental_wavelength_miles: 6_000.0,
            // 300 hexes.
            regional_wavelength_miles: 1_800.0,
            // 20 hexes.
            local_wavelength_miles: 120.0,
            // 6 hexes.
            detail_wavelength_miles: 36.0,

            // 80 hexes, so crests land roughly 40 hexes apart. That sits inside
            // the 40-200 hex "regional relief" row of the section 10 table.
            ridge_wavelength_miles: 480.0,
            // 40 hexes of directional averaging along the ridge line.
            ridge_elongation_miles: 240.0,

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
            local_weight: 0.15,
            detail_weight: 0.06,

            // Uplift sits between the two coarse noise scales, because that is
            // where the anchor lattice sits: 512 and 128 hexes are 3,072 and
            // 768 miles. Ridges are weaker again, so a mountain belt shapes a
            // continent rather than inventing one.
            uplift_weight: 0.4,
            ridge_weight: 0.3,

            // Measured, not guessed: the unshaped composite has its 71st
            // percentile at about +0.13, so sinking the world by that much puts
            // a bit under a third of it above sea level. See
            // `tests/elevation.rs`.
            elevation_offset: -0.13,
            elevation_contrast: 1.0,
            elevation_contrast_passes: 2,

            // Measured: the mean absolute step between neighbors is about
            // 0.012 and the steepest ground reaches 0.045, so a reference of
            // 0.04 puts typical ground around a third of the way up the relief
            // scale and leaves the top of it for genuinely steep places.
            relief_reference_delta_per_hex: 0.04,

            macro_region_size_hexes: DEFAULT_MACRO_REGION_SIZE_HEXES,
            region_size_hexes: DEFAULT_REGION_SIZE_HEXES,
            chunk_size_hexes: DEFAULT_CHUNK_SIZE_HEXES,

            // The same halving ladder as the composite weights, and for the
            // same reason: the coarser level sets the character of a continent
            // and the finer one varies it, rather than the two competing.
            macro_region_influence: 1.0,
            region_influence: 0.5,
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
        // The band ladder, in ascending order. Each threshold is checked for
        // range before it is checked against the one below it, so a not-a-number
        // is reported as `NotFinite` rather than silently failing a comparison.
        in_range("deep_water_level", self.deep_water_level, -1.0, 1.0)?;
        in_range("sea_level", self.sea_level, -1.0, 1.0)?;
        in_range("upland_level", self.upland_level, -1.0, 1.0)?;
        in_range("highland_level", self.highland_level, -1.0, 1.0)?;
        in_range("mountain_level", self.mountain_level, -1.0, 1.0)?;
        let ladder = [
            ("deep_water_level", self.deep_water_level),
            ("sea_level", self.sea_level),
            ("upland_level", self.upland_level),
            ("highland_level", self.highland_level),
            ("mountain_level", self.mountain_level),
        ];
        for pair in ladder.windows(2) {
            let (below, limit) = pair[0];
            let (field, value) = pair[1];
            if value <= limit {
                return Err(ConfigError::NotAscending {
                    field,
                    value,
                    below,
                    limit,
                });
            }
        }

        positive(
            "continental_wavelength_miles",
            self.continental_wavelength_miles,
        )?;
        positive("regional_wavelength_miles", self.regional_wavelength_miles)?;
        positive("local_wavelength_miles", self.local_wavelength_miles)?;
        positive("detail_wavelength_miles", self.detail_wavelength_miles)?;
        positive("ridge_wavelength_miles", self.ridge_wavelength_miles)?;
        positive("ridge_elongation_miles", self.ridge_elongation_miles)?;

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
        positive("uplift_weight", self.uplift_weight)?;
        positive("ridge_weight", self.ridge_weight)?;

        in_range("elevation_offset", self.elevation_offset, -1.0, 1.0)?;
        in_range("elevation_contrast", self.elevation_contrast, 0.0, 1.0)?;
        if self.elevation_contrast_passes > MAX_CONTRAST_PASSES {
            return Err(ConfigError::PassCount(self.elevation_contrast_passes));
        }
        positive(
            "relief_reference_delta_per_hex",
            self.relief_reference_delta_per_hex,
        )?;

        positive_size("macro_region_size_hexes", self.macro_region_size_hexes)?;
        positive_size("region_size_hexes", self.region_size_hexes)?;
        positive_size("chunk_size_hexes", self.chunk_size_hexes)?;

        positive("macro_region_influence", self.macro_region_influence)?;
        positive("region_influence", self.region_influence)?;

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
            ("ridge", c.ridge_wavelength_miles),
            ("ridge elongation", c.ridge_elongation_miles),
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
            ("deep_water_level", |c, v| c.deep_water_level = v),
            ("upland_level", |c, v| c.upland_level = v),
            ("highland_level", |c, v| c.highland_level = v),
            ("mountain_level", |c, v| c.mountain_level = v),
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
            ("ridge_wavelength_miles", |c, v| {
                c.ridge_wavelength_miles = v
            }),
            ("ridge_elongation_miles", |c, v| {
                c.ridge_elongation_miles = v
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
            ("uplift_weight", |c, v| c.uplift_weight = v),
            ("ridge_weight", |c, v| c.ridge_weight = v),
            ("elevation_offset", |c, v| c.elevation_offset = v),
            ("elevation_contrast", |c, v| c.elevation_contrast = v),
            ("relief_reference_delta_per_hex", |c, v| {
                c.relief_reference_delta_per_hex = v
            }),
            ("fbm_lacunarity", |c, v| c.fbm_lacunarity = v),
            ("fbm_gain", |c, v| c.fbm_gain = v),
            ("macro_region_influence", |c, v| {
                c.macro_region_influence = v
            }),
            ("region_influence", |c, v| c.region_influence = v),
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
            "ridge_wavelength_miles",
            "ridge_elongation_miles",
            "warp_wavelength_miles",
            "warp_strength_miles",
            "detail_warp_wavelength_miles",
            "detail_warp_strength_miles",
            "continental_weight",
            "regional_weight",
            "local_weight",
            "detail_weight",
            "uplift_weight",
            "ridge_weight",
            "relief_reference_delta_per_hex",
            "fbm_gain",
            "macro_region_influence",
            "region_influence",
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
    fn a_band_threshold_outside_the_normalized_range_is_rejected() {
        for field in [
            "deep_water_level",
            "sea_level",
            "upland_level",
            "highland_level",
            "mountain_level",
        ] {
            for bad in [1.000_001, -1.000_001, 5.0, -5.0] {
                let mut config = Config::default();
                let (_, set) = float_fields()
                    .into_iter()
                    .find(|(name, _)| *name == field)
                    .unwrap();
                set(&mut config, bad);
                let error = config
                    .validate()
                    .expect_err("an out-of-range band threshold was accepted");
                assert!(
                    matches!(
                        error,
                        ConfigError::OutOfRange {
                            field: f,
                            lo: -1.0,
                            hi: 1.0,
                            ..
                        } if f == field
                    ),
                    "{field} with {bad} produced {error:?}"
                );
            }
        }
    }

    #[test]
    fn the_band_ladder_must_ascend_strictly() {
        // Section 14.1's bands are a ladder: an out-of-order threshold would
        // make one band unreachable, and a classifier that can never return a
        // variant is a silently broken world rather than a compile error.
        let ordered = [
            "deep_water_level",
            "sea_level",
            "upland_level",
            "highland_level",
            "mountain_level",
        ];
        let baseline = Config::default();
        let values = [
            baseline.deep_water_level,
            baseline.sea_level,
            baseline.upland_level,
            baseline.highland_level,
            baseline.mountain_level,
        ];
        for pair in values.windows(2) {
            assert!(pair[0] < pair[1], "the defaults are not ascending");
        }

        // Push each threshold down onto the one below it, and then past it.
        for index in 1..ordered.len() {
            for offset in [0.0, 0.1] {
                let mut config = Config::default();
                let (_, set) = float_fields()
                    .into_iter()
                    .find(|(name, _)| *name == ordered[index])
                    .unwrap();
                set(&mut config, values[index - 1] - offset);
                let error = config
                    .validate()
                    .expect_err("a non-ascending ladder was accepted");
                assert!(
                    matches!(
                        error,
                        ConfigError::NotAscending { field, below, .. }
                            if field == ordered[index] && below == ordered[index - 1]
                    ),
                    "{} at {} produced {error:?}",
                    ordered[index],
                    values[index - 1] - offset
                );
            }
        }
    }

    #[test]
    fn a_sea_level_that_keeps_the_ladder_ordered_is_accepted() {
        // Sea level is the tuning knob for land fraction, so moving it within
        // the ladder must not need any other edit.
        for good in [-0.1, -0.05, 0.0, 0.1, 0.17] {
            let config = Config {
                sea_level: good,
                ..Config::default()
            };
            assert_eq!(config.validate(), Ok(()), "{good}");
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
