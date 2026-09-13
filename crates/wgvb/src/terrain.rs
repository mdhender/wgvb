//! Terrain classification.
//!
//! See `DESIGN.md` sections 15, 17, 18, 25.3, 33.1, and 33.4.
//!
//! Terrain is a *classification*, never a draw. Section 33.1 names the defect
//! this whole design exists to avoid — `hash(seed, q, r) % TERRAIN_COUNT` — and
//! the structural answer is that nothing in this module hashes anything. Every
//! input is a continuous field or a band read off one, so two neighboring tiles
//! disagree only where a field crossed a threshold between them.
//!
//! ```text
//! terrain = f(elevation, relief, temperature, moisture,
//!             basin influence, volcanic tendency,
//!             regional character, local variation)
//! ```
//!
//! Regional character and local variation are in that list and are not
//! separate arguments here, deliberately: they reach terrain through the values
//! that already carry them. Elevation carries the region uplift and roughness
//! biases, heat and moisture carry the region climate biases, moisture carries
//! the section 16 local variation term, and basin influence carries the region
//! basin bias. Reading the region a second time here would be a second opinion
//! about the same place, and section 33.4 is clear that regions bias fields
//! rather than assigning terrain.
//!
//! # Rule order
//!
//! Section 17 gives the order and the reason for it — so that exceptional
//! terrain is not hidden by a broad biome rule:
//!
//! ```text
//! ocean and inland water
//!     -> glacial ice
//!     -> volcano
//!     -> mountain and alpine terrain
//!     -> wetland
//!     -> climate-driven land cover
//! ```
//!
//! [`classify`] is that list, top to bottom, with coastal land sitting between
//! wetland and the climate cover: a saturated shoreline is a marsh, which says
//! more than *coast* does, and anything else at the water's edge is a coast
//! before it is a grassland.
//!
//! # Decision record — inland water is omitted in this version
//!
//! **No coordinate in any world produced by this version is classified
//! [`Terrain::Lake`] or [`Terrain::InlandSea`]**, and
//! `tests/terrain.rs::no_tile_is_ever_inland_water` holds that.
//!
//! Section 17 permits inland water only if bounded local generation can give
//! neighboring water tiles coherent membership, surface elevation, depth, and
//! shorelines, and says in as many words: *if those invariants cannot be
//! achieved simply and deterministically, omit inland-water terrain from the
//! first implementation rather than emitting inconsistent per-tile water.* They
//! cannot, and the obstruction is not a matter of effort:
//!
//! - A lake has **one** surface elevation. Every tile of one lake has to agree
//!   on it, or the water runs downhill inside itself and the depth of a tile is
//!   not a depth.
//! - Which tiles are "one lake" is a **connected component** of the ground
//!   below that surface. Finding it is a traversal whose extent is the lake's,
//!   which is unbounded in principle — and section 15 and section 2.3 both
//!   forbid global connectivity and flood fill outright.
//! - The obvious dodge is a *smooth* water-surface field `L(x)`, with a tile
//!   under water where its elevation falls below `L`. That is bounded and
//!   deterministic, and it is wrong in a way a player can see: the surface
//!   varies across the lake, so the lake is tilted, two hollows a few hexes
//!   apart have different water levels, and the shoreline is wherever two
//!   smooth fields happen to cross rather than a level line.
//! - The other dodge is to give each addressing cell a lake with a hashed
//!   surface elevation. That reintroduces the hard region boundary of section
//!   33.4, draws the lattice on the map, and puts water on hillsides wherever
//!   the cell's elevation does not match the ground.
//!
//! So the geography ships and the water does not. [`crate::basin`] produces
//! basin influence at three scales, terrain reads it, and an enclosed basin
//! reads as what an enclosed basin is in a dry climate: section 17's own
//! example, "dry endorheic regions such as the Great Basin are valid
//! geographic results". A wet one reads as marsh, swamp, or bog. What is
//! missing is open water in the middle, and a world with no lakes is a poorer
//! world than one with lakes — but it is a *coherent* world, which one with
//! per-tile water would not be.
//!
//! [`Terrain::Lake`] and [`Terrain::InlandSea`] stay in the vocabulary with
//! their discriminants pinned. They are part of the persisted value space and
//! the version that emits them must not renumber everything after them.

use crate::relief;
use crate::{Climate, Config, Elevation, HeatBand, MoistureBand, Terrain, climate};

/// Everything one tile's terrain is classified from.
///
/// A struct rather than ten arguments, because the order of ten `f64`
/// parameters is a defect waiting to happen and because the classifier is
/// tested directly against constructed values — `tests/terrain.rs` can state
/// "a saturated flat lowland in a basin is a wetland" without generating a
/// world that happens to contain one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Facts {
    /// The elevation scalar, in `[-1, +1]`.
    pub(crate) elevation_value: f64,
    /// The band that scalar was classified into.
    pub(crate) band: Elevation,
    /// Local relief, in `[0, 1]`.
    pub(crate) relief: f64,
    /// How far the tile stands above its highest neighbor, in `[0, 1]`. See
    /// [`crate::relief::peak_prominence`].
    pub(crate) prominence: f64,
    /// The temperature scalar, with elevation cooling already in it.
    pub(crate) heat_value: f64,
    /// The moisture scalar, before basin amplification.
    pub(crate) moisture_value: f64,
    /// The two climate bands.
    pub(crate) climate: Climate,
    /// Basin influence, in `[-1, +1]`.
    pub(crate) basin: f64,
    /// Volcanic tendency, in `[-1, +1]`.
    pub(crate) volcanic: f64,
    /// Whether any of the six neighbors is ocean water.
    pub(crate) adjacent_water: bool,
    /// Whether any of the six neighbors is land.
    pub(crate) adjacent_land: bool,
}

/// Whether each of the six neighbors is water, and the summary terrain needs.
///
/// Filled in direction order `0..6` from the neighbor elevations the relief
/// estimate already costs, so coastal adjacency is free rather than a second
/// set of seven evaluations. Section 18's pipeline runs one way: terrain reads
/// elevation scalars, and elevation never reads terrain.
pub(crate) fn adjacency(
    config: &Config,
    neighbors: &[f64; crate::DIRECTION_COUNT],
) -> (bool, bool) {
    let mut water = false;
    let mut land = false;
    for neighbor in neighbors {
        // Section 15's rule, literally: at or below sea level is water.
        if *neighbor <= config.sea_level {
            water = true;
        } else {
            land = true;
        }
    }
    (water, land)
}

/// Terrain wetness: moisture as a basin amplifies it, in `[-1, +1]`.
///
/// `moisture + weight * basin * moisture`. The *product* is the whole idea:
/// closed drainage concentrates whatever water arrives, so a basin makes a wet
/// climate wetter and a dry one drier, and a rise — negative influence — sheds
/// water whichever climate it is in. Section 17 asks for exactly this outcome
/// when it says dry endorheic regions such as the Great Basin are valid
/// geographic results.
///
/// Multiply, multiply, add, clamp. The clamp is needed and is the reason this
/// is not a weighted average: the product term is not a value in `[-1, +1]`
/// being averaged in, it is an amplification of a value that already is one.
pub(crate) fn wetness(config: &Config, moisture: f64, basin: f64) -> f64 {
    (moisture + config.terrain_basin_weight * (basin * moisture)).clamp(-1.0, 1.0)
}

/// The terrain at one tile.
///
/// Section 17's rule order, top to bottom. Every branch reads a named
/// threshold from [`Config`], so the whole classification is recorded in a
/// world file rather than scattered through this function as constants.
pub(crate) fn classify(config: &Config, facts: &Facts) -> Terrain {
    // The moisture terrain actually reads. Banded through the same ladder the
    // climate axis uses, so "saturated" means the same thing on both.
    let wet = wetness(config, facts.moisture_value, facts.basin);
    let wet_band = climate::moisture_band(config, wet);

    // 1. Ocean. The elevation scalar and the sea-level threshold decide water,
    //    exactly as section 15 says, and depth and adjacency to land decide
    //    which water it is.
    if facts.band.is_water() {
        if facts.adjacent_land {
            return Terrain::CoastalWater;
        }
        if facts.elevation_value <= config.deep_water_level {
            return Terrain::DeepOcean;
        }
        if facts.elevation_value <= config.ocean_level {
            return Terrain::Ocean;
        }
        return Terrain::ShallowSea;
    }

    // 2. Inland water. Deliberately omitted; see the decision record in the
    //    module comment. There is no branch here, and that absence is the
    //    decision.

    // 3. Glacial ice. Below the cold end of the polar band, where there is
    //    something to freeze. Elevation reaches this through the cooling term
    //    already folded into `heat_value`, which is why a high range inside a
    //    temperate zone can carry ice; moisture reaches it here, because a
    //    polar desert is tundra and not an icecap.
    if facts.heat_value <= config.glacial_ice_level && wet_band > MoistureBand::Arid {
        return Terrain::GlacialIce;
    }

    // 4. Volcanic. Three conditions, and the conjunction is where rarity comes
    //    from: restless crust, uplift, and a tile that stands above all six of
    //    its neighbors. Section 33.1 — never a per-tile draw.
    //
    //    Prominence carries most of the rarity, and it is the condition that
    //    makes a volcano a *place* rather than a value: a strict local maximum
    //    of the elevation field is a cone, so the cones land where the ground
    //    already has cones and no two of them are adjacent.
    if facts.volcanic >= config.volcano_level
        && facts.prominence >= config.volcano_prominence_level
        && facts.band >= Elevation::Upland
    {
        return Terrain::Volcano;
    }
    if facts.volcanic >= config.volcanic_highland_level && facts.band >= Elevation::Upland {
        return Terrain::VolcanicHighland;
    }

    // 5. Mountain and alpine. Above the tree line is alpine, and the tree line
    //    is a temperature rather than a height — which is what having the
    //    cooled heat scalar here buys.
    match facts.band {
        Elevation::Mountain => {
            return if facts.heat_value <= config.alpine_level {
                Terrain::Alpine
            } else {
                Terrain::Mountain
            };
        }
        Elevation::Highland => {
            return if facts.relief >= config.mountain_relief_level {
                Terrain::Mountain
            } else {
                Terrain::Hills
            };
        }
        _ => {}
    }

    // 6. Wetland. Saturated ground that is flat enough to hold water and low
    //    enough for water to have arrived. Marsh, swamp, and bog are separated
    //    by climate and vegetation tendency, per section 17: marshes favor open
    //    saturated lowlands, swamps favor the warmer or forested ones, and a
    //    bog is what a cold one is.
    if facts.band == Elevation::Lowland
        && facts.relief <= config.wetland_relief_level
        && wet >= config.wetland_level
    {
        return match facts.climate.heat {
            HeatBand::Polar | HeatBand::Cold => Terrain::Bog,
            HeatBand::Temperate => Terrain::Marsh,
            HeatBand::Warm | HeatBand::Hot => Terrain::Swamp,
        };
    }

    // 7. Coastal land. Section 17's two conditions and not one: near sea level
    //    *and* next to ocean water. A cliff rising out of the sea is not a
    //    coast, and inland ground at the same height is not one either.
    if facts.band == Elevation::Lowland
        && facts.elevation_value <= config.coast_level
        && facts.adjacent_water
    {
        return Terrain::Coast;
    }

    // 8. Badlands. Dry ground that has been cut up. Placed before the hill rule
    //    because it is the more specific reading of the same relief, and before
    //    the climate cover because "desert" says nothing about the shape of the
    //    ground.
    if wet_band <= MoistureBand::Dry && facts.relief >= config.badlands_relief_level {
        return Terrain::Badlands;
    }

    // 9. Hills. Upland ground with enough relief to be more hill than plain.
    if facts.band == Elevation::Upland && facts.relief >= config.hill_relief_level {
        return Terrain::Hills;
    }

    // 10. Climate-driven land cover, and nothing else is left.
    cover(facts.climate.heat, wet_band)
}

/// Climate-driven land cover: the broad biome rule, last in the order.
///
/// A `match` over the two band enums rather than a lookup table, so the
/// classification is *total* in the way section 14.1 says enums make it: a
/// forgotten pair is a compile error, and a band added to either axis breaks
/// the build here rather than falling through to a default nobody chose.
///
/// The two axes are read together and stay independent, per section 16.1. Down
/// a column, the same wetness reads as a different cover as the ground warms;
/// along a row, the same warmth reads as a different cover as it wets.
const fn cover(heat: HeatBand, moisture: MoistureBand) -> Terrain {
    use HeatBand::{Cold, Hot, Polar, Temperate, Warm};
    use MoistureBand::{Arid, Dry, Humid, Moderate, Saturated};

    match (heat, moisture) {
        // Polar. Nothing grows tall enough to be a forest, and the wet end of
        // the row has already been taken by glacial ice and bog.
        (Polar, _) => Terrain::Tundra,

        // Cold. Tundra at the dry end, then steppe, then the boreal forest.
        (Cold, Arid) => Terrain::Tundra,
        (Cold, Dry) => Terrain::Steppe,
        (Cold, Moderate | Humid | Saturated) => Terrain::BorealForest,

        // Temperate. Scrub, grass, plains, then temperate forest.
        (Temperate, Arid) => Terrain::Scrubland,
        (Temperate, Dry) => Terrain::Grassland,
        (Temperate, Moderate) => Terrain::Plains,
        (Temperate, Humid | Saturated) => Terrain::TemperateForest,

        // Warm. Desert at the dry end and savanna in the middle, which is the
        // band that gives the map its open warm country.
        (Warm, Arid) => Terrain::Desert,
        (Warm, Dry) => Terrain::Scrubland,
        (Warm, Moderate) => Terrain::Savanna,
        (Warm, Humid | Saturated) => Terrain::TemperateForest,

        // Hot. Desert, scrub, savanna, then the rainforest.
        (Hot, Arid) => Terrain::Desert,
        (Hot, Dry) => Terrain::Scrubland,
        (Hot, Moderate) => Terrain::Savanna,
        (Hot, Humid | Saturated) => Terrain::Rainforest,
    }
}

/// Everything terrain needs from the six neighboring elevations.
///
/// One place that reads the neighbor array, so relief, prominence, and coastal
/// adjacency cannot disagree about which array they read or in what order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Neighborhood {
    pub(crate) relief: f64,
    pub(crate) prominence: f64,
    pub(crate) adjacent_water: bool,
    pub(crate) adjacent_land: bool,
}

pub(crate) fn neighborhood(
    config: &Config,
    here: f64,
    neighbors: &[f64; crate::DIRECTION_COUNT],
) -> Neighborhood {
    let (adjacent_water, adjacent_land) = adjacency(config, neighbors);
    Neighborhood {
        relief: relief::from_neighbors(here, neighbors, config.relief_reference_delta_per_hex),
        prominence: relief::peak_prominence(here, neighbors, config.relief_reference_delta_per_hex),
        adjacent_water,
        adjacent_land,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DIRECTION_COUNT;

    /// A temperate, moderate, flat lowland well inland: the tile every test
    /// below moves one fact of.
    fn plain() -> Facts {
        Facts {
            elevation_value: 0.10,
            band: Elevation::Lowland,
            relief: 0.10,
            prominence: 0.0,
            heat_value: 0.0,
            moisture_value: 0.0,
            climate: Climate {
                heat: HeatBand::Temperate,
                moisture: MoistureBand::Moderate,
            },
            basin: 0.0,
            volcanic: 0.0,
            adjacent_water: false,
            adjacent_land: true,
        }
    }

    #[test]
    fn the_reference_tile_is_the_broad_biome_rule() {
        assert_eq!(classify(&Config::default(), &plain()), Terrain::Plains);
    }

    #[test]
    fn a_basin_makes_wet_ground_wetter_and_dry_ground_drier() {
        // The product, stated as the property it exists for. Section 17's
        // endorheic result is the second half of this.
        let config = Config::default();
        for moisture in [0.2, 0.5, 0.9] {
            assert!(wetness(&config, moisture, 1.0) > moisture, "{moisture}");
            assert!(wetness(&config, moisture, -1.0) < moisture, "{moisture}");
        }
        for moisture in [-0.2, -0.5, -0.9] {
            assert!(wetness(&config, moisture, 1.0) < moisture, "{moisture}");
            assert!(wetness(&config, moisture, -1.0) > moisture, "{moisture}");
        }
        // Flat ground is unchanged, and the result never leaves the range.
        for moisture in [-1.0, -0.4, 0.0, 0.4, 1.0] {
            assert_eq!(wetness(&config, moisture, 0.0), moisture);
            for basin in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                let value = wetness(&config, moisture, basin);
                assert!(value.is_finite() && (-1.0..=1.0).contains(&value));
            }
        }
    }

    #[test]
    fn water_is_the_depth_and_the_shore_and_nothing_else() {
        let config = Config::default();
        let water = |elevation: f64, band: Elevation, adjacent_land: bool| {
            classify(
                &config,
                &Facts {
                    elevation_value: elevation,
                    band,
                    adjacent_land,
                    adjacent_water: true,
                    ..plain()
                },
            )
        };
        assert_eq!(water(-0.8, Elevation::DeepWater, false), Terrain::DeepOcean);
        assert_eq!(water(-0.10, Elevation::ShallowWater, false), Terrain::Ocean);
        assert_eq!(
            water(-0.02, Elevation::ShallowWater, false),
            Terrain::ShallowSea
        );
        // Adjacency wins over depth: the shore is where the land is.
        assert_eq!(
            water(-0.8, Elevation::DeepWater, true),
            Terrain::CoastalWater
        );
        assert_eq!(
            water(-0.02, Elevation::ShallowWater, true),
            Terrain::CoastalWater
        );
    }

    #[test]
    fn no_input_whatsoever_produces_inland_water() {
        // The decision record, as an exhaustive sweep of the classifier's own
        // input space rather than of a generated world. `tests/terrain.rs`
        // makes the same claim about worlds; this one makes it about the
        // function, so a future branch that emitted a lake would fail here
        // even if no seed happened to reach it.
        let config = Config::default();
        let bands = [
            Elevation::DeepWater,
            Elevation::ShallowWater,
            Elevation::Lowland,
            Elevation::Upland,
            Elevation::Highland,
            Elevation::Mountain,
        ];
        for band in bands {
            for elevation in [-1.0, -0.5, -0.05, 0.0, 0.05, 0.3, 0.6, 1.0] {
                for relief in [0.0, 0.25, 0.5, 0.75, 1.0] {
                    for basin in [-1.0, -0.5, 0.0, 0.5, 1.0] {
                        for heat in HeatBand::ALL {
                            for moisture in MoistureBand::ALL {
                                for volcanic in [-1.0, 0.0, 0.7, 1.0] {
                                    let terrain = classify(
                                        &config,
                                        &Facts {
                                            elevation_value: elevation,
                                            band,
                                            relief,
                                            prominence: relief,
                                            heat_value: -0.8 + elevation,
                                            moisture_value: -1.0 + 0.5 * f64::from(moisture as u8),
                                            climate: Climate { heat, moisture },
                                            basin,
                                            volcanic,
                                            adjacent_water: true,
                                            adjacent_land: true,
                                        },
                                    );
                                    assert!(
                                        !matches!(terrain, Terrain::Lake | Terrain::InlandSea),
                                        "{band:?} {heat:?} {moisture:?} produced {terrain:?}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_rule_order_is_the_documented_one() {
        // Each exceptional rule, checked against a tile the *next* rule down
        // would otherwise have claimed. This is the test section 17's ordering
        // paragraph asks for: a broad biome rule must not swallow the
        // exceptions.
        let config = Config::default();

        // Glacial ice beats the climate cover that would call it tundra.
        let frozen = Facts {
            heat_value: config.glacial_ice_level - 0.01,
            climate: Climate {
                heat: HeatBand::Cold,
                moisture: MoistureBand::Moderate,
            },
            moisture_value: 0.0,
            ..plain()
        };
        assert_eq!(classify(&config, &frozen), Terrain::GlacialIce);
        assert_eq!(
            classify(
                &config,
                &Facts {
                    heat_value: config.glacial_ice_level + 0.01,
                    ..frozen
                }
            ),
            Terrain::BorealForest,
            "the rule under glacial ice is the one it is supposed to be hiding"
        );

        // A volcano beats the mountain rule.
        let volcano = Facts {
            band: Elevation::Mountain,
            elevation_value: 0.6,
            volcanic: config.volcano_level + 0.01,
            prominence: config.volcano_prominence_level + 0.01,
            heat_value: 0.2,
            ..plain()
        };
        assert_eq!(classify(&config, &volcano), Terrain::Volcano);
        assert_eq!(
            classify(
                &config,
                &Facts {
                    prominence: 0.0,
                    ..volcano
                }
            ),
            Terrain::Mountain,
            "a volcano without a peak is the mountain underneath it"
        );

        // The mountain rule beats the wetland and cover rules.
        let alpine = Facts {
            band: Elevation::Mountain,
            elevation_value: 0.6,
            heat_value: config.alpine_level - 0.01,
            moisture_value: 0.9,
            climate: Climate {
                heat: HeatBand::Cold,
                moisture: MoistureBand::Saturated,
            },
            ..plain()
        };
        assert_eq!(classify(&config, &alpine), Terrain::Alpine);

        // A wetland beats the climate cover and the coast.
        let marsh = Facts {
            moisture_value: 0.9,
            climate: Climate {
                heat: HeatBand::Temperate,
                moisture: MoistureBand::Saturated,
            },
            elevation_value: 0.01,
            adjacent_water: true,
            basin: 0.5,
            ..plain()
        };
        assert_eq!(classify(&config, &marsh), Terrain::Marsh);
        assert_eq!(
            classify(
                &config,
                &Facts {
                    relief: config.wetland_relief_level + 0.01,
                    ..marsh
                }
            ),
            Terrain::Coast,
            "ground too steep for a wetland at the shore is a coast"
        );
    }

    #[test]
    fn a_wetland_is_named_for_its_climate() {
        let config = Config::default();
        let wet = Facts {
            moisture_value: 0.95,
            basin: 0.6,
            ..plain()
        };
        for (heat, expected) in [
            (HeatBand::Polar, Terrain::Bog),
            (HeatBand::Cold, Terrain::Bog),
            (HeatBand::Temperate, Terrain::Marsh),
            (HeatBand::Warm, Terrain::Swamp),
            (HeatBand::Hot, Terrain::Swamp),
        ] {
            let facts = Facts {
                heat_value: 0.0,
                climate: Climate {
                    heat,
                    moisture: MoistureBand::Saturated,
                },
                ..wet
            };
            assert_eq!(classify(&config, &facts), expected, "{heat:?}");
        }
    }

    #[test]
    fn every_climate_pair_names_a_cover_and_the_axes_stay_independent() {
        // Totality is a compile-time property of the `match`; what a test can
        // add is that the table is not degenerate. Each row has to change as it
        // wets and each column has to change as it warms, or one of the two
        // axes is decoration.
        for heat in HeatBand::ALL {
            let row: Vec<Terrain> = MoistureBand::ALL.map(|m| cover(heat, m)).to_vec();
            if heat != HeatBand::Polar {
                assert!(
                    row.iter().any(|t| *t != row[0]),
                    "{heat:?} is the same cover however wet it is"
                );
            }
        }
        for moisture in MoistureBand::ALL {
            let column: Vec<Terrain> = HeatBand::ALL.map(|h| cover(h, moisture)).to_vec();
            assert!(
                column.iter().any(|t| *t != column[0]),
                "{moisture:?} is the same cover however warm it is"
            );
        }
    }

    #[test]
    fn adjacency_reads_sea_level_and_both_answers_at_once() {
        let config = Config::default();
        let all_water = [-0.2_f64; DIRECTION_COUNT];
        let all_land = [0.2_f64; DIRECTION_COUNT];
        let shore = [-0.2, 0.2, -0.2, 0.2, -0.2, 0.2];
        assert_eq!(adjacency(&config, &all_water), (true, false));
        assert_eq!(adjacency(&config, &all_land), (false, true));
        assert_eq!(adjacency(&config, &shore), (true, true));

        // Exactly at sea level is water, which is section 15's `<=` and the
        // same convention `elevation::classify` uses.
        assert_eq!(
            adjacency(&config, &[config.sea_level; DIRECTION_COUNT]),
            (true, false)
        );
    }
}
