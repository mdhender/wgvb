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
//! Phase 1 defined these types and nothing that assigned them. Phase 4 assigns
//! [`Tile::elevation_value`], [`Tile::relief_value`], and [`Tile::elevation`];
//! phase 5 assigns [`Tile::heat_value`], [`Tile::moisture_value`], and
//! [`Tile::climate`]; phase 6 assigns [`Tile::terrain`]. Every field of a
//! [`Tile`] is now generated, and section 4.1 is satisfied in full.

use crate::Coord;

/// Elevation band. See `DESIGN.md` section 14.1.
///
/// Ordered, and the order is the ladder: the derived [`Ord`] compares
/// discriminants, the discriminants ascend with height, and section 14.1 pins
/// them. So `band >= Elevation::Highland` means what it reads as, and terrain
/// classification says "highland or above" without a five-arm `matches!`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum Elevation {
    DeepWater = 0,
    ShallowWater = 1,
    Lowland = 2,
    Upland = 3,
    Highland = 4,
    Mountain = 5,
}

impl Elevation {
    /// Whether this band is ocean water.
    ///
    /// Section 15 defines water by the scalar — `elevation <= sea_level` — and
    /// this is the same partition read off the band, which the classifier
    /// derives from that comparison. A unit test in `elevation.rs` holds the
    /// two definitions together at the threshold itself, where they could
    /// otherwise drift apart unnoticed.
    ///
    /// Inland water is not this: a lake would sit on potential land, and this
    /// version does not produce one. See [`Terrain`].
    #[must_use]
    pub const fn is_water(self) -> bool {
        matches!(self, Elevation::DeepWater | Elevation::ShallowWater)
    }

    /// Whether this band is potential land.
    #[must_use]
    pub const fn is_land(self) -> bool {
        !self.is_water()
    }
}

/// Temperature band. Independent of [`MoistureBand`] on purpose: cold and arid
/// are not mutually exclusive. See `DESIGN.md` section 16.1.
///
/// Ordered coldest first, on the same terms as [`Elevation`]: the derived
/// [`Ord`] is discriminant order and the discriminants are the ladder.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum HeatBand {
    Polar = 0,
    Cold = 1,
    Temperate = 2,
    Warm = 3,
    Hot = 4,
}

impl HeatBand {
    /// Every band, coldest first — which is also discriminant order.
    ///
    /// Exists because a renderer has to be able to draw a key, and a key built
    /// from a hand-written list somewhere else is a list that can fall out of
    /// step with this enum without anything failing to compile.
    pub const ALL: [HeatBand; 5] = [
        HeatBand::Polar,
        HeatBand::Cold,
        HeatBand::Temperate,
        HeatBand::Warm,
        HeatBand::Hot,
    ];

    /// The band's name, lowercase, for a legend or a command line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            HeatBand::Polar => "polar",
            HeatBand::Cold => "cold",
            HeatBand::Temperate => "temperate",
            HeatBand::Warm => "warm",
            HeatBand::Hot => "hot",
        }
    }
}

/// Moisture band, ordered driest first. See `DESIGN.md` section 16.1 and the
/// note on [`Elevation`] about what the derived [`Ord`] means.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[repr(u8)]
pub enum MoistureBand {
    Arid = 0,
    Dry = 1,
    Moderate = 2,
    Humid = 3,
    Saturated = 4,
}

impl MoistureBand {
    /// Every band, driest first — which is also discriminant order.
    pub const ALL: [MoistureBand; 5] = [
        MoistureBand::Arid,
        MoistureBand::Dry,
        MoistureBand::Moderate,
        MoistureBand::Humid,
        MoistureBand::Saturated,
    ];

    /// The band's name, lowercase, for a legend or a command line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            MoistureBand::Arid => "arid",
            MoistureBand::Dry => "dry",
            MoistureBand::Moderate => "moderate",
            MoistureBand::Humid => "humid",
            MoistureBand::Saturated => "saturated",
        }
    }
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
/// # Inland water
///
/// [`Terrain::Lake`] and [`Terrain::InlandSea`] are in the vocabulary and **no
/// world produced by this version contains either**. Section 17 allows inland
/// water only where bounded local generation can give neighboring water tiles
/// coherent membership, surface elevation, depth, and shorelines, and requires
/// it to be omitted rather than approximated otherwise. The decision and its
/// reasoning are recorded in the `terrain` module comment; the two variants
/// keep their pinned discriminants so the version that does emit them does not
/// have to renumber the twenty-one variants after them.
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

impl Terrain {
    /// Every terrain, in discriminant order — which is the family order of
    /// `DESIGN.md` section 17's table.
    ///
    /// Exists for the same reason [`HeatBand::ALL`] does: a renderer has to
    /// draw a key, and a key built from a list kept somewhere else is a list
    /// that can quietly lose a variant. A test asserts this list *is* the
    /// discriminants, in order.
    pub const ALL: [Terrain; 27] = [
        Terrain::DeepOcean,
        Terrain::Ocean,
        Terrain::ShallowSea,
        Terrain::CoastalWater,
        Terrain::InlandSea,
        Terrain::Lake,
        Terrain::GlacialIce,
        Terrain::Tundra,
        Terrain::Marsh,
        Terrain::Swamp,
        Terrain::Bog,
        Terrain::Desert,
        Terrain::Badlands,
        Terrain::Scrubland,
        Terrain::Plains,
        Terrain::Grassland,
        Terrain::Steppe,
        Terrain::Savanna,
        Terrain::BorealForest,
        Terrain::TemperateForest,
        Terrain::Rainforest,
        Terrain::Hills,
        Terrain::Mountain,
        Terrain::Alpine,
        Terrain::Volcano,
        Terrain::VolcanicHighland,
        Terrain::Coast,
    ];

    /// The terrain's name, lowercase, for a legend or a command line.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Terrain::DeepOcean => "deep ocean",
            Terrain::Ocean => "ocean",
            Terrain::ShallowSea => "shallow sea",
            Terrain::CoastalWater => "coastal water",
            Terrain::InlandSea => "inland sea",
            Terrain::Lake => "lake",
            Terrain::GlacialIce => "glacial ice",
            Terrain::Tundra => "tundra",
            Terrain::Marsh => "marsh",
            Terrain::Swamp => "swamp",
            Terrain::Bog => "bog",
            Terrain::Desert => "desert",
            Terrain::Badlands => "badlands",
            Terrain::Scrubland => "scrubland",
            Terrain::Plains => "plains",
            Terrain::Grassland => "grassland",
            Terrain::Steppe => "steppe",
            Terrain::Savanna => "savanna",
            Terrain::BorealForest => "boreal forest",
            Terrain::TemperateForest => "temperate forest",
            Terrain::Rainforest => "rainforest",
            Terrain::Hills => "hills",
            Terrain::Mountain => "mountain",
            Terrain::Alpine => "alpine",
            Terrain::Volcano => "volcano",
            Terrain::VolcanicHighland => "volcanic highland",
            Terrain::Coast => "coast",
        }
    }

    /// Whether this terrain is open water of any kind, ocean or inland.
    ///
    /// A `match` rather than a discriminant range, so a variant added to
    /// either water family has to be placed here rather than silently counting
    /// as land.
    #[must_use]
    pub const fn is_water(self) -> bool {
        matches!(
            self,
            Terrain::DeepOcean
                | Terrain::Ocean
                | Terrain::ShallowSea
                | Terrain::CoastalWater
                | Terrain::InlandSea
                | Terrain::Lake
        )
    }

    /// Whether this terrain is inland water.
    ///
    /// Never true of a tile this version generates; see the note on
    /// [`Terrain`]. It exists so that the test making that claim, and any
    /// caller that wants to assert it too, does not have to spell out the two
    /// variants.
    #[must_use]
    pub const fn is_inland_water(self) -> bool {
        matches!(self, Terrain::InlandSea | Terrain::Lake)
    }
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
    /// Normalized heat: `-1.0` the coldest the world gets, `+1.0` the warmest.
    ///
    /// The value [`Climate::heat`] was classified from, and elevation cooling
    /// is already in it. Ordinary tile data, not a diagnostic: a renderer
    /// drawing a temperature gradient inside one band needs the number, not the
    /// band.
    pub heat_value: f64,
    /// Normalized moisture: `-1.0` driest, `+1.0` wettest.
    ///
    /// The value [`Climate::moisture`] was classified from.
    pub moisture_value: f64,
    /// Normalized local relief, estimated from the six neighboring elevations.
    pub relief_value: f64,

    pub elevation: Elevation,

    /// The two independent climate bands, classified from [`Tile::heat_value`]
    /// and [`Tile::moisture_value`] against the configured ladders.
    pub climate: Climate,

    /// The game-facing terrain, classified from elevation, relief, climate,
    /// basin influence, volcanic tendency, and the neighboring elevations.
    ///
    /// Never a per-tile draw; see the `terrain` module for the rule order and
    /// for why no world produced by this version contains inland water.
    ///
    /// The other fields of this struct stay available beside it on purpose:
    /// section 17 keeps elevation, relief, and climate in the tile so a game
    /// can draw forested hills or a glaciated mountain without a terrain
    /// constant for every combination.
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
    fn the_water_bands_are_exactly_the_two_lowest() {
        let all = [
            Elevation::DeepWater,
            Elevation::ShallowWater,
            Elevation::Lowland,
            Elevation::Upland,
            Elevation::Highland,
            Elevation::Mountain,
        ];
        for band in all {
            assert_eq!(band.is_water(), !band.is_land(), "{band:?}");
            assert_eq!(
                band.is_water(),
                (band as u8) <= Elevation::ShallowWater as u8
            );
        }
        assert_eq!(all.iter().filter(|b| b.is_water()).count(), 2);
    }

    #[test]
    fn every_band_list_is_in_discriminant_order_and_complete() {
        // The lists a renderer draws a key from. If one of them ever misses a
        // variant the key silently stops showing it, so the check is that the
        // list *is* the discriminants, in order.
        for (index, band) in HeatBand::ALL.into_iter().enumerate() {
            assert_eq!(usize::from(band as u8), index, "{band:?}");
        }
        for (index, band) in MoistureBand::ALL.into_iter().enumerate() {
            assert_eq!(usize::from(band as u8), index, "{band:?}");
        }
    }

    #[test]
    fn every_band_name_is_distinct_and_lowercase() {
        let mut seen = Vec::new();
        for name in HeatBand::ALL
            .map(HeatBand::name)
            .into_iter()
            .chain(MoistureBand::ALL.map(MoistureBand::name))
        {
            assert_eq!(name, name.to_lowercase(), "{name}");
            assert!(!seen.contains(&name), "{name} is used twice");
            seen.push(name);
        }
        assert_eq!(seen.len(), 10);
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
