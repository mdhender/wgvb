//! Composable continuous scalar fields.
//!
//! See `DESIGN.md` sections 9.1, 10, 13, and 25.3.
//!
//! # Why an enum and not a trait object
//!
//! The Go design used a `Field2D` interface. Section 9.1 rejects the direct
//! translation: `Box<dyn Field2D>` costs a virtual call per octave per tile and
//! blocks inlining, while `impl Field2D` generics inline fully but cannot
//! express a composition chosen at runtime from configuration. The set of field
//! kinds is *closed* and comes from configuration, so an enum with `match`
//! dispatch is both the faster option and the better fit — and it serializes
//! directly, so a world file can record the exact graph that produced it rather
//! than a set of scattered constants to be reconstructed.
//!
//! # Range
//!
//! Every variant produces a value in `[-1, +1]`, documented per variant and
//! checked by tests. Composition preserves the range: [`Field::Fbm`] and
//! [`Field::Sum`] divide by the total weight, and [`Field::Warp`] and
//! [`Field::Offset`] only move the sample position.
//!
//! # Coordinates
//!
//! [`Field::sample`] takes canonical world space in miles — the output of
//! [`crate::axial_to_world`] — not axial coordinates and not pixels.
//!
//! # Wrapped edges
//!
//! Sampling is a pure function of the *canonical* coordinate, so a coordinate
//! and its wrapped image always produce the same value; that much is exact and
//! is what tile identity depends on. It is not the same thing as continuity
//! across a wrapped edge: two tiles that neighbor each other across the `+q`
//! edge sit roughly `393,204` miles apart in canonical world space, so their
//! field values are uncorrelated and the join is visible. Section 7.1 permits
//! this as an accepted world-warp seam for the first implementation, and
//! `tests/wrap_seam.rs` documents and measures it rather than leaving it to be
//! discovered. Making the fields exactly periodic under the mirror translations
//! requires a lattice whose spacing divides `2N+1 = 65535`, which constrains
//! every wavelength in [`crate::Config`]; that is a later decision and an
//! algorithm version change when it is made.

use crate::hash::{DOM_FIELD_OFFSET, hash_n, signed_pair};
use crate::noise::{simplex2, value2};

/// A composable continuous scalar field over canonical world space.
///
/// All wavelengths are in miles and must be finite and positive. Graphs built
/// from a validated [`crate::Config`] satisfy that by construction; a graph
/// deserialized from elsewhere does not, and a non-positive wavelength produces
/// a non-finite sample position rather than a panic.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Field {
    /// Value noise with quintic interpolation, in `[-1, +1)`.
    ///
    /// Cheaper than [`Field::Simplex`] and a good fit for fine texture, where
    /// its square lattice is smaller than a tile and cannot be seen. Prefer
    /// simplex at geographic wavelengths.
    Value {
        seed: u64,
        domain: u64,
        wavelength_miles: f64,
    },

    /// Simplex noise on a triangular lattice, in `[-1, +1]`.
    ///
    /// The default choice for geography: at continental wavelengths one lattice
    /// cell spans hundreds of tiles, and a square lattice leaves visible
    /// axis-aligned structure at that scale.
    Simplex {
        seed: u64,
        domain: u64,
        wavelength_miles: f64,
    },

    /// Fractional Brownian motion: `octaves` samples of `source` at rising
    /// frequency and falling amplitude, in `[-1, +1]`.
    ///
    /// Octaves accumulate coarsest to finest in fixed order, per section 25.3.
    /// The result is divided by the total amplitude, so it is a weighted average
    /// of values in `[-1, +1]` and cannot leave that range.
    Fbm {
        source: Box<Field>,
        octaves: u8,
        lacunarity: f64,
        gain: f64,
    },

    /// Domain warping: samples `source` at a position displaced by `wx` and
    /// `wy`, in `[-1, +1]`. See section 13.
    ///
    /// Straight noise reveals its mathematical origin; displacing the sample
    /// position produces irregular coastlines and mountain belts. Use a
    /// low-frequency warp for large geography and a weaker high-frequency warp
    /// for local irregularity.
    Warp {
        source: Box<Field>,
        wx: Box<Field>,
        wy: Box<Field>,
        strength_miles: f64,
    },

    /// Samples `source` at a position translated by a constant, in `[-1, +1]`.
    ///
    /// A constant [`Field::Warp`], and the home of the seed-derived sampling
    /// offset. Every noise lattice has its origin at the world origin, and the
    /// world origin is a lattice point of every scale at once, so without this
    /// node the fields are all *exactly* zero there: simplex noise has its
    /// steepest gradient at a lattice point, and the region around `(0, 0, 0)`
    /// is measurably steeper than the rest of the world for no reason a player
    /// could ever be told. Translating the sample position makes the origin an
    /// ordinary interior point.
    ///
    /// # Why it sits above the fbm rather than inside the leaf
    ///
    /// [`Field::Fbm`] multiplies the sample position by the octave frequency
    /// before the leaf divides by the wavelength. A translation *inside* the
    /// leaf would therefore be the same fraction of a lattice cell at every
    /// octave, so at the origin every octave would sample the same cell at the
    /// same offset with the same gradients — identical values and identical,
    /// constructively adding slopes. That trades an exactly-zero origin for a
    /// perfectly correlated one, which is the same defect with a smaller
    /// coefficient. Above the fbm, the translation is scaled by the frequency
    /// along with everything else, and the octaves land wherever they land.
    Offset {
        source: Box<Field>,
        dx_miles: f64,
        dy_miles: f64,
    },

    /// Weighted sum of fields, divided by the total absolute weight, in
    /// `[-1, +1]`. This is the multi-scale composition of section 10.
    ///
    /// Terms accumulate in vector order, which is fixed by construction and by
    /// serialization; section 25.3 forbids reducing over anything whose
    /// iteration order can vary.
    Sum(Vec<(f64, Field)>),
}

impl Field {
    /// Samples the field at a point in canonical world space, in miles.
    ///
    /// The result is in `[-1, +1]`.
    ///
    /// Dispatch is a `match`, so the leaf arms inline. The recursive arms
    /// cannot fully inline through `Box`, which is inherent to a
    /// configuration-driven composition tree and is still strictly cheaper than
    /// the virtual call per octave a trait object would cost.
    #[inline]
    #[must_use]
    pub fn sample(&self, x: f64, y: f64) -> f64 {
        match self {
            Field::Value {
                seed,
                domain,
                wavelength_miles,
            } => value2(*seed, *domain, x / *wavelength_miles, y / *wavelength_miles),

            Field::Simplex {
                seed,
                domain,
                wavelength_miles,
            } => simplex2(*seed, *domain, x / *wavelength_miles, y / *wavelength_miles),

            Field::Fbm {
                source,
                octaves,
                lacunarity,
                gain,
            } => {
                let mut frequency = 1.0_f64;
                let mut amplitude = 1.0_f64;
                let mut total = 0.0_f64;
                let mut weight = 0.0_f64;
                // Coarse to fine, always. Floating-point addition is not
                // associative, so this loop's direction is part of the result.
                for _ in 0..*octaves {
                    total += amplitude * source.sample(x * frequency, y * frequency);
                    weight += amplitude;
                    frequency *= *lacunarity;
                    amplitude *= *gain;
                }
                normalize(total, weight)
            }

            Field::Warp {
                source,
                wx,
                wy,
                strength_miles,
            } => {
                let offset_x = wx.sample(x, y) * *strength_miles;
                let offset_y = wy.sample(x, y) * *strength_miles;
                source.sample(x + offset_x, y + offset_y)
            }

            Field::Offset {
                source,
                dx_miles,
                dy_miles,
            } => source.sample(x + *dx_miles, y + *dy_miles),

            Field::Sum(terms) => {
                let mut total = 0.0_f64;
                let mut weight = 0.0_f64;
                for (term_weight, field) in terms {
                    total += *term_weight * field.sample(x, y);
                    weight += term_weight.abs();
                }
                normalize(total, weight)
            }
        }
    }

    /// Convenience constructor for a boxed clone, for building graphs.
    #[must_use]
    fn boxed(self) -> Box<Field> {
        Box::new(self)
    }
}

/// Divides an accumulated sum by its total weight, or returns zero.
///
/// A zero total weight means every term was weightless — an empty
/// [`Field::Sum`], or an [`Field::Fbm`] with a zero gain and one octave's
/// amplitude already exhausted — and zero is the only value that keeps the
/// documented range without introducing a not-a-number into the generation
/// path.
///
/// Shared with region blending, which divides a weighted level sum by the same
/// kind of total.
#[inline]
pub(crate) fn normalize(total: f64, weight: f64) -> f64 {
    if weight > 0.0 { total / weight } else { 0.0 }
}

/// Builds the multi-scale fields from a seed and configuration.
///
/// This is the one place the field graph of `DESIGN.md` section 10 is written
/// down. Every constant it uses comes from [`crate::Config`], so a world file
/// records the whole of what produced it.
///
/// Warping follows section 13: one low-frequency warp displaces the three
/// geographic scales together — sharing it keeps continents, their uplift, and
/// the mountain belts on them coherent rather than sliding past each other —
/// and a weaker, higher-frequency warp breaks up local relief. The finest scale
/// is left unwarped, because a warp shorter than its own wavelength only adds
/// noise to noise.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Fields {
    pub(crate) continentalness: Field,
    pub(crate) regional: Field,
    /// The base field for ridge structure, before the directional averaging and
    /// the ridged transform that `elevation.rs` applies to it.
    ///
    /// Warped with the same low-frequency warp as the two geographic scales, so
    /// mountain belts bend with the continents they sit on rather than cutting
    /// across them.
    pub(crate) ridge: Field,
    pub(crate) local: Field,
    pub(crate) detail: Field,

    /// The broad heat field of `DESIGN.md` section 16.
    ///
    /// Warped with the same low-frequency warp as the geographic scales, so a
    /// climate zone bends where the continent under it bends instead of cutting
    /// a straight edge across a coastline.
    pub(crate) heat: Field,
    /// The broad moisture field.
    pub(crate) moisture: Field,
    /// The local variation term of the moisture composite.
    ///
    /// Broken up by the same high-frequency warp as hill relief rather than by
    /// the geographic one: this is the scale at which a patch of marsh sits
    /// next to a dry slope, and a warp longer than the field itself would only
    /// translate the whole patch.
    pub(crate) moisture_variation: Field,
}

impl Fields {
    /// Composes the graph. Called once, from `Generator::new`.
    pub(crate) fn build(seed: crate::Seed, config: &crate::Config) -> Fields {
        use crate::hash::{
            DOM_CONTINENTALNESS, DOM_DETAIL_WARP_X, DOM_DETAIL_WARP_Y, DOM_MOISTURE,
            DOM_MOISTURE_VARIATION, DOM_REGIONAL_ELEVATION, DOM_RELIEF, DOM_RIDGE_STRUCTURE,
            DOM_TEMPERATURE, DOM_TERRAIN_DETAIL, DOM_WARP_X, DOM_WARP_Y,
        };

        // Every ladder of noise, in one shape: an fbm over one leaf, translated
        // by that leaf's own seed-derived offset. The offset wraps the fbm
        // rather than the leaf, for the reason on `Field::Offset`.
        let ladder = |leaf: Field, domain: u64, wavelength_miles: f64, octaves: u8| {
            offset(
                seed,
                domain,
                wavelength_miles,
                Field::Fbm {
                    source: leaf.boxed(),
                    octaves,
                    lacunarity: config.fbm_lacunarity,
                    gain: config.fbm_gain,
                },
            )
        };
        let simplex = |domain: u64, wavelength_miles: f64| Field::Simplex {
            seed,
            domain,
            wavelength_miles,
        };
        let simplex_ladder = |domain: u64, wavelength_miles: f64, octaves: u8| {
            ladder(
                simplex(domain, wavelength_miles),
                domain,
                wavelength_miles,
                octaves,
            )
        };
        // A warp component is a bare leaf, so its offset has no fbm between it
        // and the lattice — but it needs one all the same, or the displacement
        // is exactly zero at the origin and the warp leaves that one point
        // where the unwarped field put it.
        let warp_component = |domain: u64, wavelength_miles: f64| {
            offset(
                seed,
                domain,
                wavelength_miles,
                simplex(domain, wavelength_miles),
            )
        };

        let warp = |source: Field| Field::Warp {
            source: source.boxed(),
            wx: warp_component(DOM_WARP_X, config.warp_wavelength_miles).boxed(),
            wy: warp_component(DOM_WARP_Y, config.warp_wavelength_miles).boxed(),
            strength_miles: config.warp_strength_miles,
        };
        let detail_warp = |source: Field| Field::Warp {
            source: source.boxed(),
            wx: warp_component(DOM_DETAIL_WARP_X, config.detail_warp_wavelength_miles).boxed(),
            wy: warp_component(DOM_DETAIL_WARP_Y, config.detail_warp_wavelength_miles).boxed(),
            strength_miles: config.detail_warp_strength_miles,
        };

        Fields {
            continentalness: warp(simplex_ladder(
                DOM_CONTINENTALNESS,
                config.continental_wavelength_miles,
                config.continental_octaves,
            )),
            regional: warp(simplex_ladder(
                DOM_REGIONAL_ELEVATION,
                config.regional_wavelength_miles,
                config.regional_octaves,
            )),
            ridge: warp(simplex_ladder(
                DOM_RIDGE_STRUCTURE,
                config.ridge_wavelength_miles,
                config.ridge_octaves,
            )),
            local: detail_warp(simplex_ladder(
                DOM_RELIEF,
                config.local_wavelength_miles,
                config.local_octaves,
            )),
            detail: ladder(
                Field::Value {
                    seed,
                    domain: DOM_TERRAIN_DETAIL,
                    wavelength_miles: config.detail_wavelength_miles,
                },
                DOM_TERRAIN_DETAIL,
                config.detail_wavelength_miles,
                config.detail_octaves,
            ),
            heat: warp(simplex_ladder(
                DOM_TEMPERATURE,
                config.heat_wavelength_miles,
                config.heat_octaves,
            )),
            moisture: warp(simplex_ladder(
                DOM_MOISTURE,
                config.moisture_wavelength_miles,
                config.moisture_octaves,
            )),
            moisture_variation: detail_warp(simplex_ladder(
                DOM_MOISTURE_VARIATION,
                config.moisture_variation_wavelength_miles,
                config.moisture_variation_octaves,
            )),
        }
    }
}

/// Wraps a field in its seed-derived sampling offset.
///
/// See [`Field::Offset`] for why the offset exists and why it belongs above the
/// fbm. This function is the whole of "derived from the seed, applied per
/// domain": the displacement is a hash of the seed and the field's *own* domain
/// identifier, so two worlds do not share the anomaly's new location and the
/// five scales do not all land on their own lattice points at some other single
/// coordinate.
fn offset(seed: crate::Seed, domain: u64, wavelength_miles: f64, source: Field) -> Field {
    let (dx, dy) = offset_fraction(seed, domain);
    Field::Offset {
        source: source.boxed(),
        dx_miles: dx * wavelength_miles,
        dy_miles: dy * wavelength_miles,
    }
}

/// The sampling offset for one domain, as a fraction of its wavelength.
///
/// In `[0.25, 0.75)` on each axis rather than anywhere in the cell. A hash is
/// free to come back near zero, and a near-zero offset would leave the origin
/// near a lattice point again for that seed — a fix that works for most seeds
/// and not for the unlucky one is not a fix. Keeping the coarsest octave at
/// least a quarter wavelength clear costs nothing: the finer octaves multiply
/// the same displacement by the frequency, so they land wherever they land.
///
/// Scaled by the wavelength by the caller rather than being a literal distance,
/// so the offset is irrational-looking relative to *that field's* lattice
/// instead of being a round number of miles that some other wavelength divides.
fn offset_fraction(seed: crate::Seed, domain: u64) -> (f64, f64) {
    let (gx, gy) = signed_pair(hash_n(seed, DOM_FIELD_OFFSET, [domain.cast_signed()]));
    (0.5 + 0.25 * gx, 0.5 + 0.25 * gy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::{DOM_CONTINENTALNESS, DOM_TERRAIN_DETAIL, DOM_WARP_X, DOM_WARP_Y};
    use crate::{Config, Seed};

    const SEED: Seed = 0x5eed_0000_0000_0001;

    fn leaf() -> Field {
        Field::Simplex {
            seed: SEED,
            domain: DOM_CONTINENTALNESS,
            wavelength_miles: 600.0,
        }
    }

    /// Sample positions spread over a wide area, including negatives.
    ///
    /// Deliberately offset from the origin: simplex noise is exactly zero at
    /// every one of its lattice points, and the world origin is a lattice point
    /// of every scale at once, so a grid through `(0, 0)` would make several
    /// fields agree there for a reason that has nothing to do with the property
    /// under test.
    fn positions() -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        for i in -20..=20_i32 {
            for j in -20..=20_i32 {
                out.push((f64::from(i) * 137.0 + 23.5, f64::from(j) * 211.0 - 41.25));
            }
        }
        out
    }

    #[test]
    fn every_variant_stays_within_the_normalized_range() {
        let graph = Field::Sum(vec![
            (
                1.0,
                Field::Fbm {
                    source: leaf().boxed(),
                    octaves: 6,
                    lacunarity: 2.0,
                    gain: 0.5,
                },
            ),
            (
                -0.5,
                Field::Warp {
                    source: leaf().boxed(),
                    wx: Field::Simplex {
                        seed: SEED,
                        domain: DOM_WARP_X,
                        wavelength_miles: 1_200.0,
                    }
                    .boxed(),
                    wy: Field::Simplex {
                        seed: SEED,
                        domain: DOM_WARP_Y,
                        wavelength_miles: 1_200.0,
                    }
                    .boxed(),
                    strength_miles: 300.0,
                },
            ),
            (
                0.25,
                Field::Value {
                    seed: SEED,
                    domain: DOM_TERRAIN_DETAIL,
                    wavelength_miles: 36.0,
                },
            ),
        ]);
        for (x, y) in positions() {
            let v = graph.sample(x, y);
            assert!((-1.0..=1.0).contains(&v), "({x}, {y}) gave {v}");
        }
    }

    #[test]
    fn sampling_is_deterministic_and_bit_exact_when_repeated() {
        let fields = Fields::build(SEED, &Config::default());
        for (x, y) in positions() {
            let first = fields.continentalness.sample(x, y);
            let second = fields.continentalness.sample(x, y);
            assert_eq!(first.to_bits(), second.to_bits(), "({x}, {y})");
        }
    }

    #[test]
    fn a_rebuilt_graph_is_identical_and_samples_identically() {
        let config = Config::default();
        let a = Fields::build(SEED, &config);
        let b = Fields::build(SEED, &config);
        assert_eq!(a, b);
        for (x, y) in positions() {
            assert_eq!(
                a.detail.sample(x, y).to_bits(),
                b.detail.sample(x, y).to_bits()
            );
        }
    }

    /// The octave terms of a five-octave fbm at one position, coarsest first.
    fn octave_terms(source: &Field, x: f64, y: f64) -> (Vec<f64>, f64) {
        let mut terms = Vec::new();
        let mut weight = 0.0_f64;
        let mut frequency = 1.0_f64;
        let mut amplitude = 1.0_f64;
        for _ in 0..5 {
            terms.push(amplitude * source.sample(x * frequency, y * frequency));
            weight += amplitude;
            frequency *= 2.0;
            amplitude *= 0.5;
        }
        (terms, weight)
    }

    #[test]
    fn fbm_accumulates_coarse_to_fine_in_a_fixed_order() {
        // Reproduce the loop by hand, in the documented direction, and require
        // a bit-exact match.
        let source = leaf();
        let field = Field::Fbm {
            source: source.clone().boxed(),
            octaves: 5,
            lacunarity: 2.0,
            gain: 0.5,
        };
        for (x, y) in positions() {
            let (terms, weight) = octave_terms(&source, x, y);
            let mut forward = 0.0_f64;
            for term in &terms {
                forward += *term;
            }
            assert_eq!(
                field.sample(x, y).to_bits(),
                (forward / weight).to_bits(),
                "({x}, {y})"
            );
        }
    }

    #[test]
    fn reversing_the_octave_order_would_change_the_result() {
        // The reason section 25.3 fixes the direction: floating-point addition
        // is not associative, so a "harmless" reordering is a different world.
        // Not every position shows it — many octave sums happen to round the
        // same either way — so this asserts over the sample set rather than at
        // one lucky point.
        let source = leaf();
        let mut differing = 0_u32;
        for (x, y) in positions() {
            let (terms, _) = octave_terms(&source, x, y);
            let mut forward = 0.0_f64;
            for term in &terms {
                forward += *term;
            }
            let mut reversed = 0.0_f64;
            for term in terms.iter().rev() {
                reversed += *term;
            }
            differing += u32::from(forward.to_bits() != reversed.to_bits());
        }
        assert!(differing > 0, "no sampled position was order-sensitive");
    }

    #[test]
    fn one_octave_of_fbm_is_the_source_itself() {
        let field = Field::Fbm {
            source: leaf().boxed(),
            octaves: 1,
            lacunarity: 2.0,
            gain: 0.5,
        };
        for (x, y) in positions() {
            assert_eq!(field.sample(x, y).to_bits(), leaf().sample(x, y).to_bits());
        }
    }

    #[test]
    fn fbm_adds_detail_rather_than_only_rescaling() {
        // More octaves must change the field, or the composition is doing
        // nothing.
        let coarse = Field::Fbm {
            source: leaf().boxed(),
            octaves: 1,
            lacunarity: 2.0,
            gain: 0.5,
        };
        let fine = Field::Fbm {
            source: leaf().boxed(),
            octaves: 6,
            lacunarity: 2.0,
            gain: 0.5,
        };
        let differing = positions()
            .into_iter()
            .filter(|(x, y)| coarse.sample(*x, *y) != fine.sample(*x, *y))
            .count();
        assert_eq!(differing, positions().len());
    }

    #[test]
    fn a_zero_strength_warp_is_the_identity() {
        let warped = Field::Warp {
            source: leaf().boxed(),
            wx: Field::Simplex {
                seed: SEED,
                domain: DOM_WARP_X,
                wavelength_miles: 1_200.0,
            }
            .boxed(),
            wy: Field::Simplex {
                seed: SEED,
                domain: DOM_WARP_Y,
                wavelength_miles: 1_200.0,
            }
            .boxed(),
            strength_miles: 0.0,
        };
        for (x, y) in positions() {
            assert_eq!(warped.sample(x, y).to_bits(), leaf().sample(x, y).to_bits());
        }
    }

    #[test]
    fn a_warp_actually_displaces_the_source() {
        let fields = Fields::build(SEED, &Config::default());
        let Field::Warp { source, .. } = &fields.continentalness else {
            panic!("continentalness is expected to be a warp");
        };
        let moved = positions()
            .into_iter()
            .filter(|(x, y)| fields.continentalness.sample(*x, *y) != source.sample(*x, *y))
            .count();
        assert_eq!(moved, positions().len(), "the warp displaced nothing");
    }

    #[test]
    fn an_empty_or_weightless_composition_is_zero_rather_than_not_a_number() {
        assert_eq!(Field::Sum(vec![]).sample(1.0, 2.0), 0.0);
        assert_eq!(Field::Sum(vec![(0.0, leaf())]).sample(1.0, 2.0), 0.0);
        let field = Field::Fbm {
            source: leaf().boxed(),
            octaves: 0,
            lacunarity: 2.0,
            gain: 0.5,
        };
        assert_eq!(field.sample(1.0, 2.0), 0.0);
    }

    #[test]
    fn a_sum_is_a_weighted_average_in_the_given_order() {
        let a = leaf();
        let b = Field::Value {
            seed: SEED,
            domain: DOM_TERRAIN_DETAIL,
            wavelength_miles: 36.0,
        };
        let (x, y) = (91.5, -17.25);
        let terms = [(2.0_f64, a.clone()), (-1.0_f64, b.clone())];
        let sum = Field::Sum(terms.to_vec());

        let mut expected = 0.0_f64;
        let mut weight = 0.0_f64;
        for (term_weight, field) in &terms {
            expected += *term_weight * field.sample(x, y);
            weight += term_weight.abs();
        }
        assert_eq!(weight, 3.0);
        assert_eq!(sum.sample(x, y).to_bits(), (expected / weight).to_bits());
    }

    #[test]
    fn the_graph_serializes_and_round_trips() {
        // Section 9.1: the exact field graph that produced a world must be
        // storable, not reconstructed from scattered constants.
        let fields = Fields::build(SEED, &Config::default());
        let mut bytes = Vec::new();
        ciborium::into_writer(&fields.continentalness, &mut bytes).unwrap();
        let decoded: Field = ciborium::from_reader(bytes.as_slice()).unwrap();
        assert_eq!(decoded, fields.continentalness);
        for (x, y) in positions() {
            assert_eq!(
                decoded.sample(x, y).to_bits(),
                fields.continentalness.sample(x, y).to_bits()
            );
        }
    }

    #[test]
    fn the_built_graph_matches_the_documented_shape() {
        // A structural assertion, so a change to the composition is a
        // deliberate edit to this test rather than a silent world change.
        let config = Config::default();
        let fields = Fields::build(SEED, &config);

        let Field::Warp {
            source,
            strength_miles,
            ..
        } = &fields.continentalness
        else {
            panic!("continentalness must be warped");
        };
        assert_eq!(*strength_miles, config.warp_strength_miles);
        let Field::Offset {
            source,
            dx_miles,
            dy_miles,
        } = source.as_ref()
        else {
            panic!("continentalness must be offset under the warp");
        };
        // The offset sits *above* the fbm, so the octaves do not share a
        // fractional lattice position at the origin. See `Field::Offset`.
        let (fx, fy) = offset_fraction(SEED, DOM_CONTINENTALNESS);
        assert_eq!(*dx_miles, fx * config.continental_wavelength_miles);
        assert_eq!(*dy_miles, fy * config.continental_wavelength_miles);
        let Field::Fbm {
            source,
            octaves,
            lacunarity,
            gain,
        } = source.as_ref()
        else {
            panic!("continentalness must be fbm under the offset");
        };
        assert_eq!(*octaves, config.continental_octaves);
        assert_eq!(*lacunarity, config.fbm_lacunarity);
        assert_eq!(*gain, config.fbm_gain);
        assert_eq!(
            **source,
            Field::Simplex {
                seed: SEED,
                domain: DOM_CONTINENTALNESS,
                wavelength_miles: config.continental_wavelength_miles,
            }
        );

        // The finest scale is value noise and carries no warp, but it carries
        // the same offset every other scale does.
        let Field::Offset { source, .. } = &fields.detail else {
            panic!("detail must be offset");
        };
        let Field::Fbm {
            source, octaves, ..
        } = source.as_ref()
        else {
            panic!("detail must be fbm under the offset");
        };
        assert_eq!(*octaves, config.detail_octaves);
        assert!(matches!(**source, Field::Value { .. }));
    }

    #[test]
    fn the_sampling_offset_is_a_pure_function_of_the_seed_and_the_domain() {
        // Two generators with the same seed must agree bit for bit, and two
        // with different seeds must put their lattices in different places —
        // otherwise every world shares whatever anomaly is left.
        for domain in [DOM_CONTINENTALNESS, DOM_TERRAIN_DETAIL, DOM_WARP_X] {
            let (ax, ay) = offset_fraction(SEED, domain);
            let (bx, by) = offset_fraction(SEED, domain);
            assert_eq!(ax.to_bits(), bx.to_bits());
            assert_eq!(ay.to_bits(), by.to_bits());

            let mut seen = std::collections::HashSet::new();
            for seed in 0..64_u64 {
                let (x, y) = offset_fraction(seed, domain);
                assert!(
                    seen.insert((x.to_bits(), y.to_bits())),
                    "seed {seed} repeats an offset"
                );
            }
        }
    }

    #[test]
    fn the_sampling_offset_keeps_the_origin_clear_of_the_lattice() {
        // A hash is free to come back near zero, and a near-zero offset would
        // leave the origin all but on a lattice point for that seed. The whole
        // point is that this holds for every seed rather than for most of them.
        for seed in 0..4_096_u64 {
            for domain in [
                DOM_CONTINENTALNESS,
                DOM_TERRAIN_DETAIL,
                DOM_WARP_X,
                DOM_WARP_Y,
            ] {
                let (x, y) = offset_fraction(seed, domain);
                assert!((0.25..0.75).contains(&x), "seed {seed}: {x}");
                assert!((0.25..0.75).contains(&y), "seed {seed}: {y}");
            }
        }
    }

    #[test]
    fn two_domains_do_not_share_a_sampling_offset() {
        // Per domain, not per world: one offset shared by every field would
        // move the coincidence to a different single coordinate rather than
        // removing it.
        let domains = [
            DOM_CONTINENTALNESS,
            crate::hash::DOM_REGIONAL_ELEVATION,
            crate::hash::DOM_RIDGE_STRUCTURE,
            crate::hash::DOM_RELIEF,
            DOM_TERRAIN_DETAIL,
            DOM_WARP_X,
            DOM_WARP_Y,
            crate::hash::DOM_DETAIL_WARP_X,
            crate::hash::DOM_DETAIL_WARP_Y,
        ];
        let mut seen = std::collections::HashSet::new();
        for domain in domains {
            let (x, y) = offset_fraction(SEED, domain);
            assert!(
                seen.insert((x.to_bits(), y.to_bits())),
                "{domain:#x} repeats"
            );
        }
    }

    #[test]
    fn every_field_is_something_other_than_zero_at_the_world_origin() {
        // The direct statement of the defect the offset exists to fix: the
        // world origin is a lattice point of every scale at once, and simplex
        // noise is exactly zero at a lattice point.
        for seed in [1_u64, 2, 42, 0x0123_4567_89ab_cdef, u64::MAX] {
            let fields = Fields::build(seed, &Config::default());
            for (name, field) in [
                ("continentalness", &fields.continentalness),
                ("regional", &fields.regional),
                ("ridge", &fields.ridge),
                ("local", &fields.local),
                ("detail", &fields.detail),
            ] {
                let value = field.sample(0.0, 0.0);
                assert_ne!(value, 0.0, "{name} is zero at the origin for seed {seed}");
            }
        }
    }

    #[test]
    fn the_octaves_do_not_agree_with_one_another_at_the_world_origin() {
        // Why the offset sits above the fbm rather than inside the leaf. An
        // offset folded into the leaf would put every octave at the same
        // fractional lattice position at the origin — identical values, and
        // slopes that add constructively instead of incoherently. Sampling the
        // fbm's own source at the origin, at each octave's frequency, is what
        // that would look like, so this asserts the octaves disagree.
        let fields = Fields::build(SEED, &Config::default());
        let Field::Warp { source, .. } = &fields.continentalness else {
            panic!("continentalness must be warped");
        };
        let Field::Offset {
            source,
            dx_miles,
            dy_miles,
        } = source.as_ref()
        else {
            panic!("continentalness must be offset");
        };
        let Field::Fbm {
            source, lacunarity, ..
        } = source.as_ref()
        else {
            panic!("continentalness must be fbm");
        };

        let mut values = Vec::new();
        let mut frequency = 1.0_f64;
        for _ in 0..Config::default().continental_octaves {
            values.push(source.sample(*dx_miles * frequency, *dy_miles * frequency));
            frequency *= *lacunarity;
        }
        for i in 0..values.len() {
            for j in (i + 1)..values.len() {
                assert_ne!(
                    values[i], values[j],
                    "octaves {i} and {j} agree at the origin"
                );
            }
        }
    }

    #[test]
    fn every_scale_is_an_independent_field() {
        let fields = Fields::build(SEED, &Config::default());
        for (x, y) in positions() {
            let values = [
                fields.continentalness.sample(x, y),
                fields.regional.sample(x, y),
                fields.ridge.sample(x, y),
                fields.local.sample(x, y),
                fields.detail.sample(x, y),
            ];
            for i in 0..values.len() {
                for j in (i + 1)..values.len() {
                    assert_ne!(
                        values[i], values[j],
                        "fields {i} and {j} agreed at ({x}, {y})"
                    );
                }
            }
        }
    }

    /// How much each field moves per hex of travel, summed over a long walk.
    ///
    /// Indexed continentalness, regional, ridge, local, detail. One hex is six
    /// miles.
    fn variation_per_scale() -> [f64; 5] {
        let fields = Fields::build(SEED, &Config::default());
        let step = 6.0;
        let mut totals = [0.0_f64; 5];
        for k in 0..400_i32 {
            let x = f64::from(k) * step;
            let named: [&Field; 5] = [
                &fields.continentalness,
                &fields.regional,
                &fields.ridge,
                &fields.local,
                &fields.detail,
            ];
            for (index, field) in named.into_iter().enumerate() {
                totals[index] += (field.sample(x, 0.0) - field.sample(x + step, 0.0)).abs();
            }
        }
        totals
    }

    #[test]
    fn the_geographic_scales_vary_more_slowly_than_the_short_ones() {
        // The multi-scale table of section 10 is meaningless if the wavelengths
        // do not actually order the fields by how fast they change.
        //
        // The three geographic scales are strictly ordered, and both short
        // scales move faster than all three. What is *not* asserted is an order
        // between `local` and `detail`, and that is a consequence of the
        // Nyquist bound rather than an oversight: the bound truncates the
        // detail ladder hardest — two octaves against the hill scale's four —
        // so the two now carry their fine content in overlapping bands and
        // neither is reliably the faster. Their *base* wavelengths are still
        // ordered, which `config.rs` asserts, and the fine ends of the two
        // ladders — fifteen miles and eighteen — are close enough that
        // asserting an order between them would be pinning a coin toss.
        let totals = variation_per_scale();
        let [continentalness, regional, ridge, local, detail] = totals;
        assert!(continentalness < regional, "{totals:?}");
        assert!(regional < ridge, "{totals:?}");
        assert!(ridge < local, "{totals:?}");
        assert!(ridge < detail, "{totals:?}");
    }
}
