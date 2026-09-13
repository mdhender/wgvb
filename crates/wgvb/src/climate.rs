//! Temperature, moisture, and the two-axis climate classification.
//!
//! See `DESIGN.md` sections 11.1, 16, 16.1, 25.2, 25.3, and 33.5.
//!
//! Section 16 gives the two composites directly:
//!
//! ```text
//! temperature = broad_heat_field + regional_heat_bias - elevation_cooling
//! moisture    = broad_moisture_field + regional_moisture_bias + local_variation
//! ```
//!
//! Each is a weighted average of fields in `[-1, +1]`, divided by the total
//! weight in the same way [`crate::elevation`] divides its composite, so both
//! stay in the normalized range by construction rather than by a clamp. The one
//! exception is elevation cooling, which is subtracted *after* the average and
//! therefore does need a clamp; see [`heat`].
//!
//! # There is no equator
//!
//! The world is a wrapped hexagon. It has no poles, no axis, and no latitude,
//! so `r == 0` is not an equator and nothing here may treat it as one — a
//! latitude term would put a band of ice along a line that wraps around to meet
//! a band of desert. Broad heat is a procedural field at a wavelength longer
//! than a continent, which gives zones a player can cross without giving them a
//! direction a player can navigate by.
//!
//! If a latitude rule is ever adopted as an explicit world rule, its falloff is
//! a polynomial and not a cosine. Section 25.2 bars `sin` and `cos` from the
//! generation path outright, and this module holds to the same operation set as
//! the rest of the generator: `+`, `-`, `*`, `/`, `min`, `max`, `abs`, and
//! comparisons, accumulated in one fixed order.
//!
//! # Independent axes
//!
//! Heat and moisture are computed separately, classified separately, and
//! reported separately. Section 16.1 is explicit that they are not one scale:
//! cold and arid are not the same thing and neither implies the other, so a
//! tundra and a cold rainforest have to be expressible. The only place the two
//! meet is [`crate::Terrain`], in phase 6.
//!
//! # Why moisture has a local variation term and heat does not
//!
//! That asymmetry is section 16's, taken literally. Rainfall genuinely does
//! vary over a few dozen miles — a marsh sits next to a dry slope — while
//! temperature at one elevation does not. Giving heat a matching short-scale
//! term would be a plausible-looking way to reintroduce exactly the tile-level
//! speckle this phase's exit condition rules out.
//!
//! # Globally stable
//!
//! Section 33.5 applies here word for word. Every term is a field with a fixed
//! range, the weights and the band thresholds are configuration, and nothing
//! below takes a minimum, a maximum, or a quantile of a generated sample. Two
//! tiles a world apart are classified against the same ladder whether or not
//! anything between them has ever been generated.

use crate::field::{Fields, normalize};
use crate::region;
use crate::{Climate, Config, Coord, HeatBand, MoistureBand, Seed, Vec2};

/// The raw inputs to one tile's climate, sampled once.
///
/// Carried as a struct for the same reason [`crate::elevation::Inputs`] is: the
/// generation path and the diagnostic view must see the same numbers, and the
/// cheapest guarantee of that is one place that produces them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Inputs {
    /// The broad heat field, before any region bias or cooling.
    pub(crate) broad_heat: f64,
    /// The broad moisture field.
    pub(crate) broad_moisture: f64,
    /// The short-wavelength moisture variation term.
    pub(crate) variation: f64,
    /// The blended region heat bias.
    pub(crate) heat_bias: f64,
    /// The blended region moisture bias.
    pub(crate) moisture_bias: f64,
}

/// Samples every field the climate composites read, at one coordinate.
///
/// `world` is the caller's already-computed canonical world position, so a tile
/// converts its coordinate once rather than once per composite. The conversion
/// is a pure function either way; passing it makes that a property of the code
/// rather than of two call sites agreeing.
pub(crate) fn inputs(
    fields: &Fields,
    seed: Seed,
    config: &Config,
    coord: Coord,
    world: Vec2,
) -> Inputs {
    let region = region::climate_inputs(seed, config, coord);
    Inputs {
        broad_heat: fields.heat.sample(world.x, world.y),
        broad_moisture: fields.moisture.sample(world.x, world.y),
        variation: fields.moisture_variation.sample(world.x, world.y),
        heat_bias: region.heat_bias,
        moisture_bias: region.moisture_bias,
    }
}

/// The temperature scalar at one coordinate, in `[-1, +1]`.
///
/// `-1.0` is the coldest the world gets and `+1.0` the warmest. Terms
/// accumulate coarsest first, per section 25.3: the broad field has a
/// wavelength of two thousand hexes and the region bias is blended from anchors
/// five hundred and one hundred and twenty-eight hexes apart.
///
/// Cooling is subtracted from the average rather than being a term in it. That
/// is section 16's formula, and it is also the behavior worth having: a term in
/// a weighted average would make a mountain *nearer the regional mean*, which
/// is the opposite of what altitude does. The price is that the result can
/// leave the range — a cooling of two is allowed, and the scale is only two
/// wide — so it is clamped, which is the one place in this module a value is
/// not in range by construction.
pub(crate) fn heat(config: &Config, inputs: &Inputs, elevation: f64) -> f64 {
    let mut total = 0.0_f64;
    let mut weight = 0.0_f64;

    total += config.heat_field_weight * inputs.broad_heat;
    weight += config.heat_field_weight;

    total += config.heat_region_weight * inputs.heat_bias;
    weight += config.heat_region_weight;

    (normalize(total, weight) - cooling(config, elevation)).clamp(-1.0, 1.0)
}

/// The moisture scalar at one coordinate, in `[-1, +1]`.
///
/// A weighted average of three fields in `[-1, +1]`, so it needs no clamp.
/// Terms accumulate coarsest first: the broad field at a thousand hexes, the
/// region bias, then the hundred-hex variation.
///
/// Moisture does not read elevation. A rain shadow is a directional effect that
/// needs to know which way the weather comes from, and this world has no
/// prevailing wind because it has no rotation axis; inventing one here would be
/// a global rule smuggled in through a local field. What moisture does have is
/// a region bias, which is section 11.1's wet/dry tendency and is how one part
/// of a continent ends up drier than another.
pub(crate) fn moisture(config: &Config, inputs: &Inputs) -> f64 {
    let mut total = 0.0_f64;
    let mut weight = 0.0_f64;

    total += config.moisture_field_weight * inputs.broad_moisture;
    weight += config.moisture_field_weight;

    total += config.moisture_region_weight * inputs.moisture_bias;
    weight += config.moisture_region_weight;

    total += config.moisture_variation_weight * inputs.variation;
    weight += config.moisture_variation_weight;

    normalize(total, weight)
}

/// How much temperature one elevation costs, in `[0, elevation_cooling]`.
///
/// Proportional to height above sea level, and zero everywhere at or below it:
/// the sea surface is as warm as the air over it regardless of how deep the
/// water is, and a cooling that followed elevation down would make a trench
/// polar for no reason a player could be told.
///
/// `max` rather than a branch, so the operation count does not depend on the
/// value. The division is by `1.0 - sea_level`, which validation guarantees is
/// positive: the band ladder ascends strictly and `mountain_level` is at most
/// one, so sea level is strictly below one.
fn cooling(config: &Config, elevation: f64) -> f64 {
    let above_sea_level = (elevation - config.sea_level).max(0.0);
    let span = 1.0 - config.sea_level;
    config.elevation_cooling * (above_sea_level / span)
}

/// The heat band a temperature falls in.
///
/// The comparisons use `<=` at every step, so a value exactly on a threshold
/// belongs to the lower band — the same convention as
/// [`crate::elevation::classify`], and worth keeping identical so that no
/// reader has to check which way a boundary falls twice.
///
/// Validation guarantees the thresholds ascend strictly, so every band is
/// reachable.
pub(crate) fn heat_band(config: &Config, heat: f64) -> HeatBand {
    if heat <= config.polar_level {
        HeatBand::Polar
    } else if heat <= config.cold_level {
        HeatBand::Cold
    } else if heat <= config.temperate_level {
        HeatBand::Temperate
    } else if heat <= config.warm_level {
        HeatBand::Warm
    } else {
        HeatBand::Hot
    }
}

/// The moisture band a moisture value falls in.
pub(crate) fn moisture_band(config: &Config, moisture: f64) -> MoistureBand {
    if moisture <= config.arid_level {
        MoistureBand::Arid
    } else if moisture <= config.dry_level {
        MoistureBand::Dry
    } else if moisture <= config.moderate_level {
        MoistureBand::Moderate
    } else if moisture <= config.humid_level {
        MoistureBand::Humid
    } else {
        MoistureBand::Saturated
    }
}

/// Both bands of one tile.
///
/// Two independent classifications assembled into a pair, never one
/// classification of a combined scale. See section 16.1.
pub(crate) fn classify(config: &Config, heat: f64, moisture: f64) -> Climate {
    Climate {
        heat: heat_band(config, heat),
        moisture: moisture_band(config, moisture),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inputs with every field at zero, so a test can move one at a time.
    const FLAT: Inputs = Inputs {
        broad_heat: 0.0,
        broad_moisture: 0.0,
        variation: 0.0,
        heat_bias: 0.0,
        moisture_bias: 0.0,
    };

    #[test]
    fn water_is_never_cooled_by_its_own_depth() {
        // The rule the `max` is there for: below sea level the term is exactly
        // zero, not a small negative warming.
        let config = Config::default();
        for elevation in [-1.0, -0.5, -0.001, config.sea_level] {
            assert_eq!(cooling(&config, elevation), 0.0, "{elevation}");
        }
    }

    #[test]
    fn cooling_rises_to_its_configured_strength_at_the_top_of_the_scale() {
        // Derived independently: the term is linear in height above sea level
        // and reaches `elevation_cooling` at an elevation of one. With the
        // default sea level of zero, halfway up costs half.
        let config = Config::default();
        assert!((cooling(&config, 1.0) - config.elevation_cooling).abs() < 1.0e-12);
        assert!((cooling(&config, 0.5) - config.elevation_cooling * 0.5).abs() < 1.0e-12);

        // And with sea level moved, the scale follows it rather than staying
        // pinned to zero.
        let raised = Config {
            sea_level: 0.1,
            ..Config::default()
        };
        assert_eq!(raised.validate(), Ok(()));
        assert_eq!(cooling(&raised, 0.1), 0.0);
        assert!((cooling(&raised, 1.0) - raised.elevation_cooling).abs() < 1.0e-12);
        assert!((cooling(&raised, 0.55) - raised.elevation_cooling * 0.5).abs() < 1.0e-12);
    }

    #[test]
    fn cooling_is_monotone_and_never_negative() {
        let config = Config::default();
        let mut previous = cooling(&config, -1.0);
        for step in -1_000..=1_000_i32 {
            let value = cooling(&config, f64::from(step) / 1_000.0);
            assert!(value >= 0.0, "{step} gave {value}");
            assert!(value >= previous, "cooling fell at {step}");
            previous = value;
        }
    }

    #[test]
    fn a_zero_cooling_strength_leaves_temperature_to_the_fields() {
        // The configuration the tests use to attribute an effect to cooling.
        let config = Config {
            elevation_cooling: 0.0,
            ..Config::default()
        };
        assert_eq!(config.validate(), Ok(()));
        let inputs = Inputs {
            broad_heat: 0.4,
            ..FLAT
        };
        for elevation in [-1.0, 0.0, 0.5, 1.0] {
            assert_eq!(
                heat(&config, &inputs, elevation).to_bits(),
                heat(&config, &inputs, -1.0).to_bits(),
                "{elevation}"
            );
        }
    }

    #[test]
    fn higher_ground_is_never_warmer_than_lower_ground_under_the_same_fields() {
        let config = Config::default();
        let inputs = Inputs {
            broad_heat: 0.3,
            heat_bias: -0.2,
            ..FLAT
        };
        let mut previous = heat(&config, &inputs, -1.0);
        for step in -1_000..=1_000_i32 {
            let value = heat(&config, &inputs, f64::from(step) / 1_000.0);
            assert!(
                value <= previous,
                "temperature rose with elevation at {step}"
            );
            previous = value;
        }
    }

    #[test]
    fn both_composites_stay_in_the_normalized_range() {
        // Every corner of the input space, including the cooling extreme that
        // the clamp exists for.
        let config = Config {
            elevation_cooling: 2.0,
            ..Config::default()
        };
        assert_eq!(config.validate(), Ok(()));
        for broad in [-1.0, 0.0, 1.0] {
            for bias in [-1.0, 0.0, 1.0] {
                for variation in [-1.0, 0.0, 1.0] {
                    let inputs = Inputs {
                        broad_heat: broad,
                        broad_moisture: broad,
                        variation,
                        heat_bias: bias,
                        moisture_bias: bias,
                    };
                    for elevation in [-1.0, -0.25, 0.0, 0.5, 1.0] {
                        let h = heat(&config, &inputs, elevation);
                        let m = moisture(&config, &inputs);
                        assert!(h.is_finite() && (-1.0..=1.0).contains(&h), "heat {h}");
                        assert!(m.is_finite() && (-1.0..=1.0).contains(&m), "moisture {m}");
                    }
                }
            }
        }
    }

    #[test]
    fn each_composite_is_the_weighted_average_its_configuration_says_it_is() {
        // Derived independently rather than recorded: reproduce the documented
        // formula by hand and require a bit-exact match, so a reordered
        // accumulation or a missing divisor shows up here.
        let config = Config::default();
        let inputs = Inputs {
            broad_heat: 0.37,
            broad_moisture: -0.21,
            variation: 0.64,
            heat_bias: -0.5,
            moisture_bias: 0.125,
        };

        let expected_heat = (config.heat_field_weight * inputs.broad_heat
            + config.heat_region_weight * inputs.heat_bias)
            / (config.heat_field_weight + config.heat_region_weight);
        assert_eq!(
            heat(&config, &inputs, config.sea_level).to_bits(),
            expected_heat.to_bits()
        );

        let mut total = config.moisture_field_weight * inputs.broad_moisture;
        total += config.moisture_region_weight * inputs.moisture_bias;
        total += config.moisture_variation_weight * inputs.variation;
        let expected_moisture = total
            / (config.moisture_field_weight
                + config.moisture_region_weight
                + config.moisture_variation_weight);
        assert_eq!(
            moisture(&config, &inputs).to_bits(),
            expected_moisture.to_bits()
        );
    }

    #[test]
    fn the_region_bias_moves_each_composite_in_its_own_direction_only() {
        // Section 16.1's independence, at the level of the arithmetic: the heat
        // bias must not reach moisture and the moisture bias must not reach
        // heat.
        let config = Config::default();
        let warm = Inputs {
            heat_bias: 0.9,
            ..FLAT
        };
        let wet = Inputs {
            moisture_bias: 0.9,
            ..FLAT
        };
        assert!(heat(&config, &warm, config.sea_level) > heat(&config, &FLAT, config.sea_level));
        assert_eq!(moisture(&config, &warm), moisture(&config, &FLAT));
        assert!(moisture(&config, &wet) > moisture(&config, &FLAT));
        assert_eq!(
            heat(&config, &wet, config.sea_level).to_bits(),
            heat(&config, &FLAT, config.sea_level).to_bits()
        );
    }

    #[test]
    fn classification_agrees_with_the_configured_ladders() {
        let config = Config::default();
        let heat_cases = [
            (-1.0, HeatBand::Polar),
            (config.polar_level, HeatBand::Polar),
            (config.polar_level + 1.0e-9, HeatBand::Cold),
            (config.cold_level, HeatBand::Cold),
            (config.cold_level + 1.0e-9, HeatBand::Temperate),
            (config.temperate_level, HeatBand::Temperate),
            (config.temperate_level + 1.0e-9, HeatBand::Warm),
            (config.warm_level, HeatBand::Warm),
            (config.warm_level + 1.0e-9, HeatBand::Hot),
            (1.0, HeatBand::Hot),
        ];
        for (value, expected) in heat_cases {
            assert_eq!(heat_band(&config, value), expected, "{value}");
        }

        let moisture_cases = [
            (-1.0, MoistureBand::Arid),
            (config.arid_level, MoistureBand::Arid),
            (config.arid_level + 1.0e-9, MoistureBand::Dry),
            (config.dry_level, MoistureBand::Dry),
            (config.dry_level + 1.0e-9, MoistureBand::Moderate),
            (config.moderate_level, MoistureBand::Moderate),
            (config.moderate_level + 1.0e-9, MoistureBand::Humid),
            (config.humid_level, MoistureBand::Humid),
            (config.humid_level + 1.0e-9, MoistureBand::Saturated),
            (1.0, MoistureBand::Saturated),
        ];
        for (value, expected) in moisture_cases {
            assert_eq!(moisture_band(&config, value), expected, "{value}");
        }
    }

    #[test]
    fn both_ladders_are_monotone_and_reach_every_band() {
        // A ladder with an unreachable rung would be a silently broken world.
        let config = Config::default();
        let mut heat_seen = Vec::new();
        let mut moisture_seen = Vec::new();
        let mut previous_heat = heat_band(&config, -1.0) as u8;
        let mut previous_moisture = moisture_band(&config, -1.0) as u8;
        for step in -1_000..=1_000_i32 {
            let value = f64::from(step) / 1_000.0;

            let band = heat_band(&config, value) as u8;
            assert!(band >= previous_heat, "heat band fell at {step}");
            previous_heat = band;
            if !heat_seen.contains(&band) {
                heat_seen.push(band);
            }

            let band = moisture_band(&config, value) as u8;
            assert!(band >= previous_moisture, "moisture band fell at {step}");
            previous_moisture = band;
            if !moisture_seen.contains(&band) {
                moisture_seen.push(band);
            }
        }
        assert_eq!(heat_seen, vec![0, 1, 2, 3, 4]);
        assert_eq!(moisture_seen, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn the_two_axes_are_classified_independently() {
        // Every combination of the two ladders must be reachable. One enum
        // mixing the axes could not express a cold rainforest at all, which is
        // the mistake section 16.1 names.
        let config = Config::default();
        let values = [-0.5, -0.2, 0.0, 0.2, 0.5];
        let mut seen = Vec::new();
        for h in values {
            for m in values {
                let climate = classify(&config, h, m);
                assert_eq!(climate.heat, heat_band(&config, h));
                assert_eq!(climate.moisture, moisture_band(&config, m));
                seen.push((climate.heat as u8, climate.moisture as u8));
            }
        }
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 25, "not every pair of bands is reachable");
    }
}
