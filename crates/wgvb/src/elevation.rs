//! The elevation scalar and its classification.
//!
//! See `DESIGN.md` sections 10, 12, 14, 14.1, 15, 25.2, 25.3, and 33.5.
//!
//! Elevation is the primary physical field. Everything else in the generator
//! either feeds it or reads it, so this module is deliberately the one place
//! the composition of section 10 is written down:
//!
//! ```text
//! elevation =
//!     macro_continentalness
//!   + regional_uplift
//!   + ridge_structure
//!   + local_relief
//!   + fine_detail
//! ```
//!
//! # Globally stable, never sample-normalized
//!
//! Section 33.5 is the rule that shapes this module. Every term is a field with
//! a fixed range, the weights come from configuration, and the shaping is a
//! fixed polynomial — so the elevation of a tile does not depend on which other
//! tiles have been generated, in what order, or how many. There is no place
//! here where a minimum, a maximum, or a histogram of a finite sample could be
//! taken, and there must never be one: that would make exploration order change
//! the world.
//!
//! The same rule is why sea level and the band thresholds are configuration
//! values rather than quantiles. Tuning the land fraction means moving
//! [`crate::Config::sea_level`] and measuring again, which is what the
//! distribution tests in `tests/elevation.rs` do.
//!
//! # Ridge structure
//!
//! Section 10 lists ridge structure as a term of its own, and section 12 pins
//! how it must consume orientation: as a unit vector, never as an angle. The
//! construction here is
//!
//! 1. average the ridge field at three points spaced along the region's ridge
//!    line, which stretches its features along that line and leaves them sharp
//!    across it;
//! 2. take `1 - 2 * |value|`, so the crest of a ridge is where the averaged
//!    field crosses zero rather than where it peaks;
//! 3. scale by the region's roughness, so a smooth region has no ridges at all
//!    and a rough one has strong ones.
//!
//! Multiply, add, subtract, divide, and `abs` — nothing in it reaches for a
//! rotation matrix or a trigonometric function, which is the whole reason
//! orientation is stored as a vector.
//!
//! Averaging *along* the line rather than rotating the sample frame is not a
//! stylistic choice. Rotating the frame would multiply the tile's world
//! position, which is up to hundreds of thousands of miles from the origin, by
//! the orientation — so a hundredth of a degree of drift in the blended
//! orientation would move the sample point by miles and the field would
//! degenerate into noise far from the origin. The three taps move by at most
//! [`crate::Config::ridge_elongation_miles`] no matter where the tile is.

use crate::field::{Fields, normalize};
use crate::region::{self, UnitVec2};
use crate::{Config, Coord, Elevation, Seed, Vec2, axial_to_world};

/// Taps in the directional average along a ridge line.
///
/// Three: one behind, one at the tile, one ahead. A structural constant rather
/// than configuration — the spacing is what tunes the elongation, and a
/// different tap count would be a different stencil, not a different setting.
/// The offsets are symmetric, so this is a plain box average and no window
/// function is implied.
pub(crate) const RIDGE_TAPS: usize = 3;

/// The raw inputs to one elevation scalar, sampled once.
///
/// Carried as a struct because two callers want them: [`scalar`], which is the
/// generation path, and [`crate::Sample`], which is the diagnostic view. Both
/// must see the same numbers, and the cheapest way to guarantee that is for
/// there to be only one place they are produced.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Inputs {
    /// Where the coordinate sits in canonical world space.
    pub(crate) world: Vec2,

    pub(crate) continentalness: f64,
    pub(crate) regional: f64,
    pub(crate) local: f64,
    pub(crate) detail: f64,

    /// The ridged, elongated, roughness-independent structure term.
    pub(crate) ridge: f64,
    /// The blended region elevation bias.
    pub(crate) uplift: f64,
    /// The blended region roughness bias.
    pub(crate) roughness: f64,
}

/// Samples every field the elevation scalar composes, at one coordinate.
pub(crate) fn inputs(fields: &Fields, seed: Seed, config: &Config, coord: Coord) -> Inputs {
    let world = axial_to_world(coord);
    let region = region::elevation_inputs(seed, config, coord);
    Inputs {
        world,
        continentalness: fields.continentalness.sample(world.x, world.y),
        regional: fields.regional.sample(world.x, world.y),
        local: fields.local.sample(world.x, world.y),
        detail: fields.detail.sample(world.x, world.y),
        ridge: ridge_structure(
            &fields.ridge,
            world,
            region.ridge,
            config.ridge_elongation_miles,
        ),
        uplift: region.elevation_bias,
        roughness: region.roughness,
    }
}

/// The unshaped four-field noise composite of `DESIGN.md` section 10.
///
/// This is what [`crate::Sample::elevation_raw`] reports: the continuous fields
/// alone, with no region influence, no ridges, and no shaping. It exists so the
/// tuning renderer can separate "the noise is wrong" from "the composition is
/// wrong", and its exact value is pinned by `tests/golden.rs`.
pub(crate) fn raw_composite(config: &Config, inputs: &Inputs) -> f64 {
    let mut total = 0.0_f64;
    let mut weight = 0.0_f64;
    total += config.continental_weight * inputs.continentalness;
    weight += config.continental_weight;
    total += config.regional_weight * inputs.regional;
    weight += config.regional_weight;
    total += config.local_weight * inputs.local;
    weight += config.local_weight;
    total += config.detail_weight * inputs.detail;
    weight += config.detail_weight;
    normalize(total, weight)
}

/// The elevation scalar at one coordinate, in `[-1, +1]`.
///
/// `-1.0` is deep ocean, `0.0` is the middle of the scale, `+1.0` is extreme
/// highland. Sea level is [`Config::sea_level`] and is not required to be zero.
///
/// Terms accumulate in one fixed order, coarsest scale first, per section 25.3.
/// With the default configuration that order is 6,000 miles of continentalness,
/// 3,072-to-768 miles of region uplift, 1,800 miles of regional relief, 480
/// miles of ridge structure, 120 miles of hills, and 36 miles of detail.
///
/// Roughness scales two of the terms rather than their weights. Scaling a
/// weight would change the divisor and so change what every *other* term is
/// worth at that tile, which is a coupling nobody tuning the composite would
/// expect; scaling the value leaves the composite a weighted average of numbers
/// in `[-1, +1]` and so keeps the documented range without a clamp.
pub(crate) fn scalar(config: &Config, inputs: &Inputs) -> f64 {
    let roughness_gain = gain(inputs.roughness);

    let mut total = 0.0_f64;
    let mut weight = 0.0_f64;

    total += config.continental_weight * inputs.continentalness;
    weight += config.continental_weight;

    total += config.uplift_weight * inputs.uplift;
    weight += config.uplift_weight;

    total += config.regional_weight * inputs.regional;
    weight += config.regional_weight;

    // Ridges exist only where the region is rough. A perfectly smooth region
    // contributes nothing here rather than a weak ridge.
    total += config.ridge_weight * (roughness_gain * inputs.ridge);
    weight += config.ridge_weight;

    // Hill-scale relief never disappears entirely — a smooth region is gentle,
    // not billiard-flat — so its gain runs over the upper half of the range.
    total += config.local_weight * ((0.5 + 0.5 * roughness_gain) * inputs.local);
    weight += config.local_weight;

    total += config.detail_weight * inputs.detail;
    weight += config.detail_weight;

    // The offset is what puts sea level at zero rather than at the median of
    // the composite, and the clamp is a contract guard for the shaping below,
    // which is only in range for an input in range.
    let composite = (normalize(total, weight) + config.elevation_offset).clamp(-1.0, 1.0);
    shape(
        config.elevation_contrast,
        config.elevation_contrast_passes,
        composite,
    )
}

/// Applies the shaping pass `passes` times, in `[-1, +1]`.
///
/// Each pass is monotone and fixes `-1`, `0`, and `+1`, so the composition is
/// monotone and fixes them too — which is what makes "how many passes" an
/// ordinary tuning knob rather than a change of meaning. A `u8` count and an
/// integer loop, so the number of arithmetic operations is a property of the
/// configuration and not of the value being shaped.
#[inline]
fn shape(strength: f64, passes: u8, x: f64) -> f64 {
    let mut shaped = x;
    for _ in 0..passes {
        shaped = contrast(strength, shaped);
    }
    shaped
}

/// A region roughness bias in `[-1, +1]` as a gain in `[0, 1]`.
#[inline]
fn gain(roughness: f64) -> f64 {
    (roughness + 1.0) * 0.5
}

/// Pushes a composite away from the middle of the scale, in `[-1, +1]`.
///
/// `x + contrast * (x - x^3) / 2`. Written as explicit multiplications rather
/// than `powi`, because section 25.2 notes that `powi`'s association order is
/// compiler-defined and this value is compared against goldens.
///
/// For `contrast` in `[0, 1]` the derivative `1 + contrast * (1 - 3x^2) / 2` is
/// non-negative on `[-1, +1]`, so the map is monotone and cannot fold two
/// elevations onto one; `f(±1) = ±1`, so it does not need a clamp to stay in
/// range. Validation rejects a contrast above one for exactly that reason.
#[inline]
fn contrast(contrast: f64, x: f64) -> f64 {
    x + contrast * ((x - x * x * x) * 0.5)
}

/// The ridge structure term at one point, in `[-1, +1]`.
///
/// See the module comment for why the field is averaged along the ridge line
/// rather than sampled in a rotated frame.
///
/// The two outer taps are summed *before* the center is added. That is not
/// incidental: floating-point addition is commutative even though it is not
/// associative, so grouping the pair keeps the result identical when the
/// orientation is replaced by its opposite. A ridge orientation is a line, not
/// an arrow — section 12 — and this is what makes that true of the value and
/// not only of the documentation.
fn ridge_structure(
    field: &crate::Field,
    world: Vec2,
    ridge: UnitVec2,
    elongation_miles: f64,
) -> f64 {
    let dx = ridge.x * elongation_miles;
    let dy = ridge.y * elongation_miles;

    let behind = field.sample(world.x - dx, world.y - dy);
    let ahead = field.sample(world.x + dx, world.y + dy);
    let here = field.sample(world.x, world.y);

    #[expect(
        clippy::cast_precision_loss,
        reason = "RIDGE_TAPS is 3; the conversion is exact"
    )]
    let taps = RIDGE_TAPS as f64;
    let smeared = (here + (behind + ahead)) / taps;

    // A crest sits where the smeared field crosses zero, which is a line rather
    // than the isolated point a peak would be. This is the classic ridged
    // transform and it uses only `abs`, a multiply, and a subtract.
    1.0 - 2.0 * smeared.abs()
}

/// The elevation band an elevation scalar falls in.
///
/// Section 14.1's ladder, read from configuration. The comparisons use `<=` at
/// every step, so a value exactly on a threshold belongs to the lower band and
/// `elevation <= sea_level` is water — which is section 15's rule written
/// literally rather than approximated.
///
/// Validation guarantees the thresholds ascend strictly, so every band is
/// reachable and the `match` this replaces cannot fall through.
#[must_use]
pub(crate) fn classify(config: &Config, elevation: f64) -> Elevation {
    if elevation <= config.deep_water_level {
        Elevation::DeepWater
    } else if elevation <= config.sea_level {
        Elevation::ShallowWater
    } else if elevation <= config.upland_level {
        Elevation::Lowland
    } else if elevation <= config.highland_level {
        Elevation::Upland
    } else if elevation <= config.mountain_level {
        Elevation::Highland
    } else {
        Elevation::Mountain
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Field;
    use crate::hash::DOM_RIDGE_STRUCTURE;

    const SEED: Seed = 0x51de_0000_0000_0004;

    fn ridge_field() -> Field {
        Field::Simplex {
            seed: SEED,
            domain: DOM_RIDGE_STRUCTURE,
            wavelength_miles: 480.0,
        }
    }

    /// A spread of world positions, off the origin so no lattice point of any
    /// scale makes two fields agree for an irrelevant reason.
    fn positions() -> Vec<Vec2> {
        let mut out = Vec::new();
        for i in -12..=12_i32 {
            for j in -12..=12_i32 {
                out.push(Vec2 {
                    x: f64::from(i) * 173.0 + 19.75,
                    y: f64::from(j) * 241.0 - 37.5,
                });
            }
        }
        out
    }

    #[test]
    fn the_shaping_polynomial_fixes_the_ends_and_the_middle() {
        for c in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(contrast(c, 0.0), 0.0, "contrast {c}");
            assert_eq!(contrast(c, 1.0), 1.0, "contrast {c}");
            assert_eq!(contrast(c, -1.0), -1.0, "contrast {c}");
        }
    }

    #[test]
    fn the_shaping_polynomial_is_monotone_and_stays_in_range() {
        // Monotone is the property that makes the shaping a change of contrast
        // rather than a change of geography: two distinct elevations must not
        // be folded onto one another.
        for step in 0..=100_i32 {
            let c = f64::from(step) / 100.0;
            let mut previous = contrast(c, -1.0);
            for step in -1_000..=1_000_i32 {
                let x = f64::from(step) / 1_000.0;
                let y = contrast(c, x);
                assert!((-1.0..=1.0).contains(&y), "contrast {c} at {x} gave {y}");
                assert!(y >= previous, "contrast {c} fell at {x}");
                previous = y;
            }
        }
    }

    #[test]
    fn repeated_shaping_passes_stay_monotone_and_in_range() {
        // Composing a monotone map with itself is monotone, but only if each
        // pass really is: this is the test that would catch a contrast bound
        // being widened without the polynomial being changed.
        for passes in 0..=crate::MAX_CONTRAST_PASSES {
            for strength in [0.0, 0.5, 1.0] {
                assert_eq!(shape(strength, passes, 0.0), 0.0);
                assert_eq!(shape(strength, passes, 1.0), 1.0);
                assert_eq!(shape(strength, passes, -1.0), -1.0);

                let mut previous = shape(strength, passes, -1.0);
                for step in -1_000..=1_000_i32 {
                    let y = shape(strength, passes, f64::from(step) / 1_000.0);
                    assert!((-1.0..=1.0).contains(&y), "{passes}/{strength}: {y}");
                    assert!(y >= previous, "{passes}/{strength} fell at {step}");
                    previous = y;
                }
            }
        }
    }

    #[test]
    fn each_shaping_pass_multiplies_the_slope_at_sea_level() {
        // Two passes at full contrast must give 1.5 * 1.5, or the pass count is
        // not doing what its documentation says and the defaults were tuned
        // against something else.
        let x = 1.0e-6;
        for (passes, expected) in [(0_u8, 1.0), (1, 1.5), (2, 2.25), (3, 3.375)] {
            let ratio = shape(1.0, passes, x) / x;
            assert!(
                (ratio - expected).abs() < 1.0e-6,
                "{passes} passes gave slope {ratio}, expected {expected}"
            );
        }
    }

    #[test]
    fn the_shaping_polynomial_steepens_the_middle_and_zero_disables_it() {
        for step in -1_000..=1_000_i32 {
            let x = f64::from(step) / 1_000.0;
            assert_eq!(contrast(0.0, x), x, "zero contrast is not the identity");
        }
        // The derivative at the origin is 1 + contrast / 2, so a value near
        // zero must come back about 1.5 times larger at full contrast.
        let x = 1.0e-6;
        let ratio = contrast(1.0, x) / x;
        assert!((ratio - 1.5).abs() < 1.0e-9, "slope at zero was {ratio}");
    }

    #[test]
    fn a_ridge_orientation_and_its_opposite_give_the_same_structure() {
        // Section 12: a ridge orientation is a line, not an arrow. The blend in
        // `region.rs` always returns the representative with `x >= 0`, so this
        // never comes up in the generation path — which is precisely why it
        // would rot unnoticed if it were only a comment.
        let field = ridge_field();
        for world in positions() {
            for (x, y) in [(1.0, 0.0), (0.6, 0.8), (-0.6, 0.8), (0.0, 1.0)] {
                let forward = UnitVec2 { x, y };
                let backward = UnitVec2 { x: -x, y: -y };
                assert_eq!(
                    ridge_structure(&field, world, forward, 240.0).to_bits(),
                    ridge_structure(&field, world, backward, 240.0).to_bits(),
                    "{world:?} with ({x}, {y})"
                );
            }
        }
    }

    #[test]
    fn ridge_structure_stays_in_the_normalized_range() {
        let field = ridge_field();
        for world in positions() {
            let value = ridge_structure(&field, world, UnitVec2 { x: 0.8, y: 0.6 }, 240.0);
            assert!(value.is_finite(), "{world:?} gave {value}");
            assert!((-1.0..=1.0).contains(&value), "{world:?} gave {value}");
        }
    }

    #[test]
    fn ridge_structure_varies_more_across_the_ridge_than_along_it() {
        // The whole point of the directional average: features are elongated
        // along the orientation. Walk the same distance along and across and
        // compare how much the term moved.
        let field = ridge_field();
        let ridge = UnitVec2 { x: 0.6, y: 0.8 };
        let across = UnitVec2 { x: -0.8, y: 0.6 };
        let step = 60.0;

        let mut along_total = 0.0_f64;
        let mut across_total = 0.0_f64;
        for world in positions() {
            let here = ridge_structure(&field, world, ridge, 240.0);
            let moved_along = ridge_structure(
                &field,
                Vec2 {
                    x: world.x + ridge.x * step,
                    y: world.y + ridge.y * step,
                },
                ridge,
                240.0,
            );
            let moved_across = ridge_structure(
                &field,
                Vec2 {
                    x: world.x + across.x * step,
                    y: world.y + across.y * step,
                },
                ridge,
                240.0,
            );
            along_total += (moved_along - here).abs();
            across_total += (moved_across - here).abs();
        }
        assert!(
            across_total > along_total * 1.2,
            "ridges are not elongated: along {along_total}, across {across_total}"
        );
    }

    #[test]
    fn a_zero_elongation_leaves_the_ridge_term_isotropic() {
        // The degenerate case has to be well defined rather than merely
        // rejected by validation: all three taps land on the same point, so the
        // term collapses to the plain ridged transform of the field.
        //
        // Not bit-exact, and it must not be asserted as such: the average is
        // `(v + (v + v)) / 3`, and `3v` rounds before the division does, so the
        // result can sit a couple of units in the last place away from `v`.
        // Special-casing a zero elongation to avoid that would put a branch on
        // a configuration value in the middle of the generation path, which
        // buys nothing — validation already rejects it.
        let field = ridge_field();
        for world in positions() {
            let plain = 1.0 - 2.0 * field.sample(world.x, world.y).abs();
            let actual = ridge_structure(&field, world, UnitVec2::X, 0.0);
            assert!(
                (actual - plain).abs() <= 8.0 * f64::EPSILON,
                "{world:?}: {actual} against {plain}"
            );
        }
    }

    #[test]
    fn the_roughness_gain_spans_zero_to_one() {
        assert_eq!(gain(-1.0), 0.0);
        assert_eq!(gain(0.0), 0.5);
        assert_eq!(gain(1.0), 1.0);
        for step in -100..=100_i32 {
            let g = gain(f64::from(step) / 100.0);
            assert!((0.0..=1.0).contains(&g), "{g}");
        }
    }

    #[test]
    fn classification_agrees_with_the_configured_ladder() {
        let config = Config::default();
        let cases = [
            (-1.0, Elevation::DeepWater),
            (config.deep_water_level, Elevation::DeepWater),
            (config.deep_water_level + 1.0e-9, Elevation::ShallowWater),
            (config.sea_level, Elevation::ShallowWater),
            (config.sea_level + 1.0e-9, Elevation::Lowland),
            (config.upland_level, Elevation::Lowland),
            (config.upland_level + 1.0e-9, Elevation::Upland),
            (config.highland_level, Elevation::Upland),
            (config.highland_level + 1.0e-9, Elevation::Highland),
            (config.mountain_level, Elevation::Highland),
            (config.mountain_level + 1.0e-9, Elevation::Mountain),
            (1.0, Elevation::Mountain),
        ];
        for (elevation, expected) in cases {
            assert_eq!(classify(&config, elevation), expected, "{elevation}");
        }
    }

    #[test]
    fn classification_is_monotone_and_reaches_every_band() {
        // A ladder with an unreachable rung would be a silently broken world.
        let config = Config::default();
        let mut seen = Vec::new();
        let mut previous = classify(&config, -1.0) as u8;
        for step in -1_000..=1_000_i32 {
            let band = classify(&config, f64::from(step) / 1_000.0) as u8;
            assert!(band >= previous, "band fell at {step}");
            previous = band;
            if !seen.contains(&band) {
                seen.push(band);
            }
        }
        assert_eq!(seen, vec![0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn water_is_exactly_at_or_below_sea_level() {
        // Section 15's rule, checked at the boundary rather than in the middle
        // of a band where any threshold would pass.
        for sea_level in [-0.1, -0.05, 0.0, 0.1, 0.17] {
            let config = Config {
                sea_level,
                // Keep the rung below sea level under it; see the same move in
                // `config.rs`.
                ocean_level: (Config::default().deep_water_level + sea_level) * 0.5,
                ..Config::default()
            };
            assert_eq!(config.validate(), Ok(()));
            for step in -1_000..=1_000_i32 {
                let elevation = f64::from(step) / 1_000.0;
                assert_eq!(
                    classify(&config, elevation).is_water(),
                    elevation <= sea_level,
                    "sea level {sea_level} at {elevation}"
                );
            }
        }
    }
}
