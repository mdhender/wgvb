//! Basin influence: deterministic, bounded, and never a flood fill.
//!
//! See `DESIGN.md` sections 11.1, 15, 17, 25.2, 25.3, and 33.5.
//!
//! # What a basin influence is
//!
//! One scalar in `[-1, +1]` saying how enclosed the low ground at a coordinate
//! is. `+1` is the middle of a closed depression and `-1` is the crown of a
//! rise that sheds in every direction. It is a *tendency*, exactly as the
//! region parameters of section 11.1 are: it does not say a tile is under
//! water, it says what kind of place this is.
//!
//! Section 17 asks for it at broad, regional, and local scales blended with the
//! phase 3 region basin bias, and that is literally what [`influence`]
//! computes:
//!
//! ```text
//! basin =
//!     broad_basin_field
//!   + region_basin_bias
//!   + regional_basin_field
//!   + local_basin_field
//! ```
//!
//! A weighted average of four values in `[-1, +1]`, divided by the total
//! weight, so the range holds by construction rather than by a clamp — the same
//! shape the elevation and climate composites have. Terms accumulate coarsest
//! first, per section 25.3: the broad field at 600 hexes, the region bias
//! blended from anchors 512 and 128 hexes apart, then 150 hexes, then 40.
//!
//! # Bounded local sampling, and nothing else
//!
//! Section 15 and section 17 both say it: basins come from bounded local
//! sampling, **never** from global connectivity or a flood fill. Every term
//! above is a pure function of one coordinate, so this module cannot express a
//! traversal even by accident. There is no queue, no visited set, and no
//! neighbor loop here at all.
//!
//! # Basin influence does not move elevation
//!
//! Nothing here feeds [`crate::elevation`]. That is a deliberate boundary and
//! not an oversight: elevation is the primary field, its composition is pinned
//! by the golden tables, and a basin term inside it would move every tile in
//! every world. Basin influence is an input to *terrain*, which is where
//! section 17 puts it, and the elevation and relief columns of
//! `tests/golden.rs` are bit-for-bit what they were before this module existed.
//!
//! # Why there is no inland water
//!
//! Read the decision record in [`crate::terrain`]. In short: a coherent lake
//! needs one surface elevation shared by every tile of one basin, and knowing
//! which tiles those are is connectivity. Section 17 anticipates that and says
//! to omit inland water rather than emit incoherent per-tile water, so this
//! version keeps the geography and omits the lakes.

use crate::field::{Fields, normalize};
use crate::region;
use crate::{Config, Coord, Seed, Vec2};

/// The raw inputs to one tile's basin influence, sampled once.
///
/// A struct for the same reason [`crate::elevation::Inputs`] and
/// [`crate::climate::Inputs`] are: the generation path and the diagnostic view
/// must see the same numbers, and the cheapest guarantee of that is one place
/// that produces them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Inputs {
    /// The broad basin field.
    pub(crate) broad: f64,
    /// The regional basin field.
    pub(crate) regional: f64,
    /// The local basin field.
    pub(crate) local: f64,
    /// The blended region basin bias of section 11.1.
    pub(crate) bias: f64,

    /// The volcanic belt field.
    ///
    /// Carried here rather than in a struct of its own because the region
    /// anchors are walked once for both families: basin and volcanic are the
    /// two region parameters terrain reads, and two walks would cost the anchor
    /// loop twice to arrive at the same numbers.
    pub(crate) volcanic_field: f64,
    /// The blended region volcanic bias.
    pub(crate) volcanic_bias: f64,
}

/// Samples every field the basin and volcanic composites read, at one
/// coordinate.
///
/// `world` is the caller's already-computed canonical world position, for the
/// reason [`crate::climate::inputs`] takes one: a tile converts its coordinate
/// once rather than once per composite.
pub(crate) fn inputs(
    fields: &Fields,
    seed: Seed,
    config: &Config,
    coord: Coord,
    world: Vec2,
) -> Inputs {
    let region = region::terrain_inputs(seed, config, coord);
    Inputs {
        broad: fields.basin_broad.sample(world.x, world.y),
        regional: fields.basin_regional.sample(world.x, world.y),
        local: fields.basin_local.sample(world.x, world.y),
        bias: region.basin_bias,
        volcanic_field: fields.volcanic.sample(world.x, world.y),
        volcanic_bias: region.volcanic,
    }
}

/// The basin influence at one coordinate, in `[-1, +1]`.
///
/// A weighted average of four values already in range, so it needs no clamp.
/// Terms accumulate coarsest first, per section 25.3.
pub(crate) fn influence(config: &Config, inputs: &Inputs) -> f64 {
    let mut total = 0.0_f64;
    let mut weight = 0.0_f64;

    total += config.basin_broad_weight * inputs.broad;
    weight += config.basin_broad_weight;

    total += config.basin_region_weight * inputs.bias;
    weight += config.basin_region_weight;

    total += config.basin_regional_weight * inputs.regional;
    weight += config.basin_regional_weight;

    total += config.basin_local_weight * inputs.local;
    weight += config.basin_local_weight;

    normalize(total, weight)
}

/// The volcanic tendency at one coordinate, in `[-1, +1]`.
///
/// `+1` is the most restless crust the world has. Two terms: the continuous
/// belt field and the region's own volcanic bias, averaged the same way
/// everything else in the generator is.
///
/// This is a tendency and not a volcano. Section 17 and section 33.1: a cone
/// appears only where this is high *and* the ground is high *and* the tile is a
/// local peak, and that conjunction is where rarity comes from. A threshold on
/// this value alone would be a per-tile lottery with a smooth field in front of
/// it.
pub(crate) fn volcanic(config: &Config, inputs: &Inputs) -> f64 {
    let mut total = 0.0_f64;
    let mut weight = 0.0_f64;

    // The region bias is blended from anchors 512 and 128 hexes apart, so it
    // is the coarser of the two — but the belt field is what decides whether
    // there is a belt here at all, and it is written first because the
    // accumulation order has to be written down somewhere and this is a
    // two-term average rather than a scale ladder. Section 25.3.
    total += config.volcanic_field_weight * inputs.volcanic_field;
    weight += config.volcanic_field_weight;

    total += config.volcanic_region_weight * inputs.volcanic_bias;
    weight += config.volcanic_region_weight;

    normalize(total, weight)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inputs with every field at zero, so a test can move one at a time.
    const FLAT: Inputs = Inputs {
        broad: 0.0,
        regional: 0.0,
        local: 0.0,
        bias: 0.0,
        volcanic_field: 0.0,
        volcanic_bias: 0.0,
    };

    #[test]
    fn a_world_with_no_basin_anywhere_has_no_basin_influence() {
        assert_eq!(influence(&Config::default(), &FLAT), 0.0);
        assert_eq!(volcanic(&Config::default(), &FLAT), 0.0);
    }

    #[test]
    fn the_composite_is_a_weighted_average_and_stays_in_range() {
        // Every term at an extreme puts the result at that extreme, and
        // nothing in between can leave the range. Derived from the definition
        // rather than recorded.
        let config = Config::default();
        for extreme in [-1.0, 1.0] {
            let inputs = Inputs {
                broad: extreme,
                regional: extreme,
                local: extreme,
                bias: extreme,
                ..FLAT
            };
            assert!((influence(&config, &inputs) - extreme).abs() < 1.0e-12);
        }

        for broad in [-1.0, -0.3, 0.0, 0.7, 1.0] {
            for bias in [-1.0, 0.0, 1.0] {
                for local in [-1.0, 0.25, 1.0] {
                    let inputs = Inputs {
                        broad,
                        regional: -local,
                        local,
                        bias,
                        ..FLAT
                    };
                    let value = influence(&config, &inputs);
                    assert!(value.is_finite() && (-1.0..=1.0).contains(&value));
                }
            }
        }
    }

    #[test]
    fn each_term_moves_the_composite_in_its_own_direction() {
        // A term dropped from the sum would leave the composite unmoved, which
        // is the failure this catches and a weight test would not.
        let config = Config::default();
        let base = influence(&config, &FLAT);
        for (name, inputs) in [
            ("broad", Inputs { broad: 1.0, ..FLAT }),
            (
                "regional",
                Inputs {
                    regional: 1.0,
                    ..FLAT
                },
            ),
            ("local", Inputs { local: 1.0, ..FLAT }),
            ("bias", Inputs { bias: 1.0, ..FLAT }),
        ] {
            assert!(
                influence(&config, &inputs) > base,
                "{name} does not raise the composite"
            );
        }
    }

    #[test]
    fn the_volcanic_composite_reads_both_of_its_terms() {
        let config = Config::default();
        assert!(
            volcanic(
                &config,
                &Inputs {
                    volcanic_field: 1.0,
                    ..FLAT
                }
            ) > 0.0,
            "the belt field does not reach the composite"
        );
        assert!(
            volcanic(
                &config,
                &Inputs {
                    volcanic_bias: 1.0,
                    ..FLAT
                }
            ) > 0.0,
            "the region bias does not reach the composite"
        );
        // Both at an extreme put the composite there, which is what a weighted
        // average of two values in range must do.
        for extreme in [-1.0, 1.0] {
            let value = volcanic(
                &config,
                &Inputs {
                    volcanic_field: extreme,
                    volcanic_bias: extreme,
                    ..FLAT
                },
            );
            assert!((value - extreme).abs() < 1.0e-12);
        }
    }

    #[test]
    fn the_broad_term_outweighs_the_local_one() {
        // A basin is mostly a broad fact about a place. If the local field
        // could outvote the broad one, the layer would be texture rather than
        // geography and a marsh would sit next to a desert.
        let config = Config::default();
        let broad = influence(
            &config,
            &Inputs {
                broad: 1.0,
                local: -1.0,
                ..FLAT
            },
        );
        assert!(broad > 0.0, "the local field outvoted the broad one");
    }
}
