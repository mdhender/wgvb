//! Public generated tile data and its classifications.
//!
//! See `DESIGN.md` sections 4.1, 14.1, 16.1, and 17.
//!
//! A [`Tile`] carries the normalized elevation, heat, moisture, and relief
//! values *in addition to* the classifications derived from them. Those values
//! are ordinary tile results, not diagnostics: a game or renderer must not need
//! a separate API to draw forested hills or a glaciated mountain.
//!
//! Every discriminant is pinned explicitly. These values are persisted and
//! appear in cached tiles, so reordering variants would silently change stored
//! data in exactly the way reordering a Go `iota` block does. Writing `= 0`,
//! `= 1`, ... makes the hazard visible in review.
//!
//! Phase 1 defines these types and nothing that assigns them; the classifiers
//! arrive with the fields they classify, in phases 4 through 6.

use crate::Coord;

/// Elevation band. See `DESIGN.md` section 14.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum Elevation {
    DeepWater = 0,
    ShallowWater = 1,
    Lowland = 2,
    Upland = 3,
    Highland = 4,
    Mountain = 5,
}

/// Temperature band. Independent of [`MoistureBand`] on purpose: cold and arid
/// are not mutually exclusive. See `DESIGN.md` section 16.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum HeatBand {
    Polar = 0,
    Cold = 1,
    Temperate = 2,
    Warm = 3,
    Hot = 4,
}

/// Moisture band. See `DESIGN.md` section 16.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum MoistureBand {
    Arid = 0,
    Dry = 1,
    Moderate = 2,
    Humid = 3,
    Saturated = 4,
}

/// Climate as two independent axes. The two-axis representation is part of the
/// public model; do not replace it with one enum mixing temperature and
/// moisture. See `DESIGN.md` section 16.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct Climate {
    pub heat: HeatBand,
    pub moisture: MoistureBand,
}

/// Game-facing terrain classification.
///
/// Derived from the physical fields, never from an independent per-tile random
/// lookup. The vocabulary is grouped by family in the order of `DESIGN.md`
/// section 17; elevation, relief, and climate remain available alongside it, so
/// no variant is needed for every combination of them.
///
/// Inland water is present in the vocabulary but is emitted only when bounded
/// local generation can give neighboring water tiles coherent membership,
/// surface elevation, depth, and shorelines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum Terrain {
    // Ocean.
    DeepOcean = 0,
    Ocean = 1,
    ShallowSea = 2,
    CoastalWater = 3,
    // Inland water.
    InlandSea = 4,
    Lake = 5,
    // Frozen.
    GlacialIce = 6,
    Tundra = 7,
    // Wetland.
    Marsh = 8,
    Swamp = 9,
    Bog = 10,
    // Dry.
    Desert = 11,
    Badlands = 12,
    Scrubland = 13,
    // Open land.
    Plains = 14,
    Grassland = 15,
    Steppe = 16,
    Savanna = 17,
    // Forest.
    BorealForest = 18,
    TemperateForest = 19,
    Rainforest = 20,
    // Elevated.
    Hills = 21,
    Mountain = 22,
    Alpine = 23,
    // Volcanic.
    Volcano = 24,
    VolcanicHighland = 25,
    // Coastal land.
    Coast = 26,
}

/// One generated tile.
///
/// `Copy` and under 64 bytes, so a batch API can fill a `&mut [Tile]` with no
/// allocation and no indirection. See `DESIGN.md` section 4.1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tile {
    pub coord: Coord,

    /// Normalized elevation: `-1.0` deep ocean, `0.0` sea level, `+1.0` extreme
    /// highland.
    pub elevation_value: f64,
    /// Normalized heat, the value [`Climate::heat`] was classified from.
    pub heat_value: f64,
    /// Normalized moisture, the value [`Climate::moisture`] was classified from.
    pub moisture_value: f64,
    /// Normalized local relief, estimated from the six neighboring elevations.
    pub relief_value: f64,

    pub elevation: Elevation,
    pub climate: Climate,
    pub terrain: Terrain,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevation_discriminants_are_pinned() {
        assert_eq!(Elevation::DeepWater as u8, 0);
        assert_eq!(Elevation::ShallowWater as u8, 1);
        assert_eq!(Elevation::Lowland as u8, 2);
        assert_eq!(Elevation::Upland as u8, 3);
        assert_eq!(Elevation::Highland as u8, 4);
        assert_eq!(Elevation::Mountain as u8, 5);
    }

    #[test]
    fn climate_band_discriminants_are_pinned() {
        assert_eq!(HeatBand::Polar as u8, 0);
        assert_eq!(HeatBand::Cold as u8, 1);
        assert_eq!(HeatBand::Temperate as u8, 2);
        assert_eq!(HeatBand::Warm as u8, 3);
        assert_eq!(HeatBand::Hot as u8, 4);

        assert_eq!(MoistureBand::Arid as u8, 0);
        assert_eq!(MoistureBand::Dry as u8, 1);
        assert_eq!(MoistureBand::Moderate as u8, 2);
        assert_eq!(MoistureBand::Humid as u8, 3);
        assert_eq!(MoistureBand::Saturated as u8, 4);
    }

    #[test]
    fn terrain_discriminants_are_pinned_and_contiguous() {
        let all = [
            (Terrain::DeepOcean, 0_u8),
            (Terrain::Ocean, 1),
            (Terrain::ShallowSea, 2),
            (Terrain::CoastalWater, 3),
            (Terrain::InlandSea, 4),
            (Terrain::Lake, 5),
            (Terrain::GlacialIce, 6),
            (Terrain::Tundra, 7),
            (Terrain::Marsh, 8),
            (Terrain::Swamp, 9),
            (Terrain::Bog, 10),
            (Terrain::Desert, 11),
            (Terrain::Badlands, 12),
            (Terrain::Scrubland, 13),
            (Terrain::Plains, 14),
            (Terrain::Grassland, 15),
            (Terrain::Steppe, 16),
            (Terrain::Savanna, 17),
            (Terrain::BorealForest, 18),
            (Terrain::TemperateForest, 19),
            (Terrain::Rainforest, 20),
            (Terrain::Hills, 21),
            (Terrain::Mountain, 22),
            (Terrain::Alpine, 23),
            (Terrain::Volcano, 24),
            (Terrain::VolcanicHighland, 25),
            (Terrain::Coast, 26),
        ];
        for (variant, expected) in all {
            assert_eq!(variant as u8, expected, "{variant:?}");
        }
    }

    #[test]
    fn a_tile_is_copy_and_compares_by_value() {
        let tile = Tile {
            coord: Coord::new(-3, 9),
            elevation_value: 0.25,
            heat_value: -0.5,
            moisture_value: 0.75,
            relief_value: 0.125,
            elevation: Elevation::Upland,
            climate: Climate {
                heat: HeatBand::Temperate,
                moisture: MoistureBand::Moderate,
            },
            terrain: Terrain::Hills,
        };
        let copied = tile;
        assert_eq!(tile, copied);
        assert_eq!(copied.coord, Coord::new(-3, 9));
    }
}
