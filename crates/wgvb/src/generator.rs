//! The generator: an immutable seed plus configuration.
//!
//! See `DESIGN.md` sections 19, 22, and 26.
//!
//! A configured `Generator` is safe for concurrent read-only use, and that is
//! checked by the compiler rather than promised in a comment: the `Send + Sync`
//! assertion in `lib.rs` holds only while this type contains no interior
//! mutability. A cache with interior mutability would appear in the type and
//! break the build, which is the point — caches must be concurrency-safe,
//! optional, or outside the core.
//!
//! Phase 4 adds [`Generator::elevation_at`], [`Generator::relief`],
//! [`Generator::tile`], and the batch API. `tile` has nothing to re-normalize
//! because [`Coord`] already guarantees canonical input, and it cannot recurse
//! because both it and `relief` reach the field composition through the private
//! [`Generator::elevation_scalar`] rather than through each other. See
//! `DESIGN.md` section 18.
//!
//! Phase 5 adds climate, through the private [`Generator::climate_scalars`] for
//! the same reason: [`Generator::sample`] and [`Generator::tile`] both need the
//! temperature and moisture composites, and one private function is what keeps
//! them from becoming two.

use crate::field::Fields;
use crate::relief;
use crate::{
    Config, ConfigError, Coord, DIRECTION_COUNT, Elevation, RegionParams, Seed, Terrain, Tile,
    Vec2, climate, elevation, region,
};

/// The continuous scalar fields at one coordinate.
///
/// Diagnostic output for the renderer and for tuning, not a [`crate::Tile`]:
/// these are the raw fields of `DESIGN.md` section 10 before any classification.
/// Every value is in `[-1, +1]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// The coordinate sampled. Canonical by construction.
    pub coord: Coord,
    /// Where that coordinate sits in canonical world space, in miles. Carried
    /// so a diagnostic caller need not re-derive it, and so the renderer can
    /// show a position without reaching into the conversion itself.
    pub world: Vec2,

    /// Broad continental land and ocean structure.
    pub continentalness: f64,
    /// Regional uplift on top of continentalness.
    pub regional: f64,
    /// Hill-scale relief.
    pub local: f64,
    /// The finest terrain texture.
    pub detail: f64,

    /// The ridge structure term of `DESIGN.md` section 10, before the region
    /// roughness that scales it in the composite.
    ///
    /// Elongated along the region's ridge orientation and ridged, so a crest is
    /// a line rather than a peak. Near `+1` on a crest and near `-1` in the
    /// trough between two of them.
    pub ridge: f64,

    /// The weighted sum of the four *noise* scales alone, in `[-1, +1]`.
    ///
    /// This is **not** the elevation of [`crate::Tile`], and it is not a
    /// stepping stone to it either: [`Sample::elevation`] is, and it adds
    /// region uplift, ridge structure, the sea-level offset, and the contrast
    /// shaping on top of the same four fields.
    ///
    /// It stays in the public sample because it is the layer that separates
    /// "the noise is wrong" from "the composition is wrong" when a rendered
    /// window looks off. Nothing in the generator reads it.
    pub elevation_raw: f64,

    /// The blended regional elevation bias of `DESIGN.md` sections 11 and 12,
    /// in `[-1, +1]`.
    ///
    /// Deliberately *not* folded into [`Sample::elevation_raw`], which is the
    /// noise composite and nothing else. [`Sample::elevation`] is where this
    /// bias becomes uplift, weighted by
    /// [`crate::Config::uplift_weight`]: regions bias fields, and how much
    /// uplift a bias is worth is elevation's decision, not the region's.
    ///
    /// Carrying it separately is also how "no visible implementation-region
    /// boundaries" is checked — the tuning renderer can draw the region field
    /// on its own.
    ///
    /// [`Generator::region_params`] returns this alongside the climate,
    /// roughness, basin, volcanic, variation, and ridge parameters that phases
    /// 5 and 6 consume.
    pub regional_uplift: f64,

    /// The blended region roughness bias, in `[-1, +1]`.
    ///
    /// Carried because it is the reason a ridge belt is strong in one place and
    /// absent a few hundred miles away, and a tuning renderer that shows the
    /// ridge term without it cannot explain what it is looking at.
    pub roughness: f64,

    /// The elevation scalar of `DESIGN.md` section 14, in `[-1, +1]`.
    ///
    /// Bit-identical to [`Generator::elevation_at`] and to
    /// [`crate::Tile::elevation_value`] at the same coordinate: all three are
    /// the same function of the same inputs, not three implementations that
    /// agree.
    ///
    /// Unlike [`Sample::elevation_raw`] this is the real thing — region uplift,
    /// ridge structure, and the contrast shaping included — and it is what sea
    /// level and the band ladder are compared against.
    pub elevation: f64,

    /// The temperature scalar of `DESIGN.md` section 16, in `[-1, +1]`.
    ///
    /// Bit-identical to [`crate::Tile::heat_value`] at the same coordinate, and
    /// the value [`crate::Climate::heat`] is classified from. Elevation cooling
    /// is already folded in, which is why a mountain range shows on this layer
    /// as well as on the elevation one.
    pub heat: f64,

    /// The moisture scalar of `DESIGN.md` section 16, in `[-1, +1]`.
    ///
    /// Bit-identical to [`crate::Tile::moisture_value`] at the same coordinate.
    /// It does not read elevation at all, so a mountain range that is visible
    /// here is a coincidence of the fields rather than a coupling.
    pub moisture: f64,
}

/// An immutable, thread-safe world generator.
///
/// Construction validates the configuration; nothing afterwards mutates it, so
/// every tile is a pure function of the seed, the coordinate, the algorithm
/// version, and this configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Generator {
    seed: Seed,
    config: Config,
    /// The field graph, composed once from the seed and configuration.
    ///
    /// Derived state, not input: it is a pure function of the two fields above,
    /// so it does not enter the configuration fingerprint and cannot make two
    /// generators with equal seed and configuration compare unequal. Building
    /// it here rather than per sample keeps the allocation out of the hot path
    /// while leaving the generator immutable.
    fields: Fields,
}

impl Generator {
    /// Builds a generator from a seed and configuration.
    ///
    /// # Errors
    ///
    /// Returns the first [`ConfigError`] the configuration fails on. An invalid
    /// configuration can never reach a generator, so no sampling path has to
    /// re-check it.
    pub fn new(seed: Seed, config: Config) -> Result<Generator, ConfigError> {
        config.validate()?;
        let fields = Fields::build(seed, &config);
        Ok(Generator {
            seed,
            config,
            fields,
        })
    }

    /// Builds a generator from a seed and the default configuration.
    ///
    /// Cannot fail — the defaults are valid, and a test asserts it — so this is
    /// the ergonomic path for tests and the diagnostic harness.
    #[must_use]
    pub fn with_defaults(seed: Seed) -> Generator {
        Generator::new(seed, Config::default()).expect("the default configuration is valid")
    }

    /// The world seed.
    #[must_use]
    pub const fn seed(&self) -> Seed {
        self.seed
    }

    /// The effective configuration.
    #[must_use]
    pub const fn config(&self) -> &Config {
        &self.config
    }

    /// Samples every continuous field at one canonical coordinate.
    ///
    /// A pure function of the seed, the coordinate, [`crate::ALGORITHM_VERSION`],
    /// and the configuration — nothing here reads generation order, a cache, or
    /// any mutable state, which is why concurrent callers see identical values.
    ///
    /// The composite accumulates coarsest to finest in a fixed order, per
    /// `DESIGN.md` section 25.3.
    ///
    /// This is the diagnostic view. It costs one elevation evaluation and no
    /// neighbor evaluations, so it does not carry relief; ask
    /// [`Generator::relief`] or [`Generator::tile`] for that.
    #[must_use]
    pub fn sample(&self, coord: Coord) -> Sample {
        let inputs = elevation::inputs(&self.fields, self.seed, &self.config, coord);
        let elevation = elevation::scalar(&self.config, &inputs);
        let (heat, moisture) = self.climate_scalars(coord, inputs.world, elevation);
        Sample {
            coord,
            world: inputs.world,
            continentalness: inputs.continentalness,
            regional: inputs.regional,
            local: inputs.local,
            detail: inputs.detail,
            ridge: inputs.ridge,
            elevation_raw: elevation::raw_composite(&self.config, &inputs),
            regional_uplift: inputs.uplift,
            roughness: inputs.roughness,
            elevation,
            heat,
            moisture,
        }
    }

    /// The elevation scalar at one canonical coordinate, in `[-1, +1]`.
    ///
    /// `-1.0` is deep ocean and `+1.0` is extreme highland; sea level is
    /// [`Config::sea_level`], which is configurable and is not required to be
    /// zero. See `DESIGN.md` section 14.
    #[must_use]
    pub fn elevation_at(&self, coord: Coord) -> f64 {
        self.elevation_scalar(coord)
    }

    /// Local relief at one canonical coordinate, in `[0, 1]`.
    ///
    /// Zero is flat ground and one is as steep as
    /// [`Config::relief_reference_delta_per_hex`] says a slope can usefully
    /// read. Estimated from the six neighboring elevations in fixed direction
    /// order `0..6`, per `DESIGN.md` sections 18 and 25.3.
    ///
    /// This costs seven elevation evaluations. That is the price of a stateless
    /// generator and it is deliberate: section 26 asks for a profile before a
    /// cache, and a cache would have to live outside this type.
    #[must_use]
    pub fn relief(&self, coord: Coord) -> f64 {
        let here = self.elevation_scalar(coord);
        relief::from_neighbors(
            here,
            &self.neighbor_elevations(coord),
            self.config.relief_reference_delta_per_hex,
        )
    }

    /// The generated tile at one canonical coordinate.
    ///
    /// A pure function of the seed, the coordinate,
    /// [`crate::ALGORITHM_VERSION`], and the configuration. [`Coord`] has no
    /// constructor that skips normalization, so there is nothing here to
    /// re-normalize.
    ///
    /// # Phase 5 completeness
    ///
    /// Elevation, relief, and climate are generated; terrain is provisional and
    /// is documented as such on [`Tile::terrain`].
    #[must_use]
    pub fn tile(&self, coord: Coord) -> Tile {
        let elevation_value = self.elevation_scalar(coord);
        let relief_value = relief::from_neighbors(
            elevation_value,
            &self.neighbor_elevations(coord),
            self.config.relief_reference_delta_per_hex,
        );
        let band = elevation::classify(&self.config, elevation_value);
        let (heat_value, moisture_value) =
            self.climate_scalars(coord, crate::axial_to_world(coord), elevation_value);

        Tile {
            coord,
            elevation_value,
            heat_value,
            moisture_value,
            relief_value,
            elevation: band,
            climate: climate::classify(&self.config, heat_value, moisture_value),
            terrain: provisional_terrain(band),
        }
    }

    /// Fills `out` with one tile per coordinate.
    ///
    /// Every tile is a pure function of its own coordinate written to its own
    /// slot, so the result is bit-identical to calling [`Generator::tile`] in
    /// any order, on any number of threads. `DESIGN.md` section 20 grants
    /// permission to parallelize a fill for exactly that reason, and forbids
    /// any batch operation that *accumulates* across tiles — a sum, a min or
    /// max, a histogram — because a work-stealing split would then reach the
    /// result. Nothing here accumulates, and nothing added here may.
    ///
    /// # Panics
    ///
    /// Panics if `coords` and `out` have different lengths.
    pub fn tiles_into(&self, coords: &[Coord], out: &mut [Tile]) {
        assert_eq!(
            coords.len(),
            out.len(),
            "tiles_into needs one output slot per coordinate"
        );
        for (coord, slot) in coords.iter().zip(out.iter_mut()) {
            *slot = self.tile(*coord);
        }
    }

    /// One tile per coordinate.
    ///
    /// Prefer [`Generator::tiles_into`] in a hot path: [`Tile`] is `Copy`, so a
    /// caller can reuse one buffer across frames with no allocation.
    #[must_use]
    pub fn tiles(&self, coords: &[Coord]) -> Vec<Tile> {
        coords.iter().map(|coord| self.tile(*coord)).collect()
    }

    /// Every tile within `radius` steps of `center`, in hex distance.
    ///
    /// The result has `1 + 3 * radius * (radius + 1)` entries, ordered by the
    /// offset from the center: ascending `dq`, and within that ascending `dr`.
    /// The order is part of the contract because a caller indexing the result
    /// needs one, and because section 29 asks rendering to consume coordinates
    /// in a stable order.
    ///
    /// Coordinates are normalized, so a region straddling a wrapped edge
    /// returns the canonical representative of each tile.
    ///
    /// # Panics
    ///
    /// Panics if `radius` exceeds [`crate::WORLD_RADIUS`]. Beyond that the
    /// requested hexagon is larger than the world and would name some tiles
    /// more than once.
    #[must_use]
    pub fn region(&self, center: Coord, radius: u32) -> Vec<Tile> {
        self.tiles(&region_coords(center, radius))
    }

    /// The deterministic region parameters at one canonical coordinate.    /// The deterministic region parameters at one canonical coordinate.
    ///
    /// Blended across the anchors around the tile at each level, so there is no
    /// addressing cell a tile belongs to wholesale and no boundary to see. See
    /// `DESIGN.md` sections 11.2 and 33.4.
    ///
    /// Nothing is stored and nothing is cached: this is a pure function of the
    /// seed, the coordinate, [`crate::ALGORITHM_VERSION`], and the
    /// configuration, so concurrent callers see identical values and repeated
    /// calls cost what they cost. Section 26 asks for a profile before a cache,
    /// and a cache would have to live outside this type.
    #[must_use]
    pub fn region_params(&self, coord: Coord) -> RegionParams {
        region::params(self.seed, &self.config, coord)
    }

    /// The temperature and moisture scalars at one canonical coordinate.
    ///
    /// Private and paired for the same reason [`Generator::elevation_scalar`]
    /// is single: `sample` and `tile` are two doors into one function, and two
    /// doors into two copies of a composition is how they drift apart. The
    /// pairing also means the region anchors are walked once for both axes
    /// rather than once each.
    ///
    /// `world` and `elevation` are the caller's, because both callers have
    /// already computed them and recomputing would only create a second place
    /// they could disagree.
    fn climate_scalars(&self, coord: Coord, world: Vec2, elevation: f64) -> (f64, f64) {
        let inputs = climate::inputs(&self.fields, self.seed, &self.config, coord, world);
        (
            climate::heat(&self.config, &inputs, elevation),
            climate::moisture(&self.config, &inputs),
        )
    }

    /// The elevation scalar. The one place elevation is composed.
    ///
    /// Private on purpose. `DESIGN.md` section 18 forbids calling `tile()` from
    /// inside `tile()`, and Rust will not catch that; routing both
    /// [`Generator::tile`] and [`Generator::relief`] through this function means
    /// the recursion cannot be written rather than merely being discouraged.
    fn elevation_scalar(&self, coord: Coord) -> f64 {
        let inputs = elevation::inputs(&self.fields, self.seed, &self.config, coord);
        elevation::scalar(&self.config, &inputs)
    }

    /// The six neighboring elevation scalars, indexed by direction.
    ///
    /// Filled in direction order `0..6`, which is the order
    /// [`crate::relief::from_neighbors`] then accumulates in. Section 25.3.
    fn neighbor_elevations(&self, coord: Coord) -> [f64; DIRECTION_COUNT] {
        let mut out = [0.0_f64; DIRECTION_COUNT];
        for (index, slot) in out.iter_mut().enumerate() {
            let direction = i32::try_from(index).expect("DIRECTION_COUNT is 6");
            *slot = self.elevation_scalar(coord.neighbor(direction));
        }
        out
    }
}

/// The coordinates of a hexagonal region, in the documented order.
///
/// Split out from [`Generator::region`] so the ordering can be tested without a
/// generator, and so the panic message is attached to the thing that decides
/// the shape.
fn region_coords(center: Coord, radius: u32) -> Vec<Coord> {
    let radius = i64::from(radius);
    assert!(
        radius <= crate::WORLD_RADIUS,
        "a region radius above WORLD_RADIUS would name a tile more than once"
    );

    let q = i64::from(center.q());
    let r = i64::from(center.r());
    let mut out = Vec::new();
    for dq in -radius..=radius {
        let low = (-radius).max(-dq - radius);
        let high = radius.min(-dq + radius);
        for dr in low..=high {
            out.push(Coord::new(q + dq, r + dr));
        }
    }
    out
}

/// A stand-in terrain for phase 4, derived from the elevation band alone.
///
/// **Phase 6 replaces this.** Section 17 derives terrain from elevation,
/// relief, and climate together, and two of those are not generated yet, so
/// nothing here can distinguish tundra from rainforest — the whole world is
/// temperate. See [`Tile::terrain`].
///
/// A `match` on the band rather than a constant, so a caller sees water where
/// there is water and the diagnostic images are readable; and a `match` rather
/// than a lookup table, so adding a band is a compile error here.
const fn provisional_terrain(band: Elevation) -> Terrain {
    match band {
        Elevation::DeepWater => Terrain::DeepOcean,
        Elevation::ShallowWater => Terrain::ShallowSea,
        Elevation::Lowland => Terrain::Plains,
        Elevation::Upland => Terrain::Hills,
        Elevation::Highland => Terrain::Mountain,
        Elevation::Mountain => Terrain::Alpine,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Coord;

    #[test]
    fn with_defaults_never_fails_and_keeps_the_seed() {
        let g = Generator::with_defaults(0xdead_beef);
        assert_eq!(g.seed(), 0xdead_beef);
        assert_eq!(*g.config(), Config::default());
    }

    #[test]
    fn new_rejects_an_invalid_configuration() {
        let config = Config {
            continental_octaves: 0,
            ..Config::default()
        };
        assert_eq!(
            Generator::new(1, config),
            Err(ConfigError::OctaveCount {
                field: "continental_octaves",
                octaves: 0
            })
        );
    }

    #[test]
    fn new_accepts_a_valid_non_default_configuration() {
        let config = Config {
            sea_level: -0.1,
            ..Config::default()
        };
        let g = Generator::new(7, config.clone()).expect("configuration is valid");
        assert_eq!(*g.config(), config);
        assert_ne!(*g.config(), Config::default());
    }

    /// A spread of canonical coordinates, including negatives, wrapped inputs,
    /// and coordinates far from the origin.
    fn sample_coords() -> Vec<Coord> {
        let mut out = Vec::new();
        for q in -6..=6_i64 {
            for r in -6..=6_i64 {
                out.push(Coord::new(q, r));
            }
        }
        for (q, r) in [
            (0, 0),
            (-1, 5),
            (12_345, -6_789),
            (-32_767, 17),
            (32_767, -32_767),
            (i64::MAX, 3),
            (i64::MIN, -7),
        ] {
            out.push(Coord::new(q, r));
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    #[test]
    fn sampling_is_deterministic_and_bit_exact() {
        let g = Generator::with_defaults(0x1234_5678);
        for c in sample_coords() {
            let first = g.sample(c);
            let second = g.sample(c);
            assert_eq!(first, second, "{c:?}");
            assert_eq!(
                first.elevation_raw.to_bits(),
                second.elevation_raw.to_bits(),
                "{c:?}"
            );
            assert_eq!(
                first.regional_uplift.to_bits(),
                second.regional_uplift.to_bits(),
                "{c:?}"
            );
        }
    }

    #[test]
    fn every_sampled_value_is_normalized() {
        let g = Generator::with_defaults(99);
        for c in sample_coords() {
            let s = g.sample(c);
            for (name, value) in [
                ("continentalness", s.continentalness),
                ("regional", s.regional),
                ("local", s.local),
                ("detail", s.detail),
                ("elevation_raw", s.elevation_raw),
                ("regional_uplift", s.regional_uplift),
            ] {
                assert!(value.is_finite(), "{name} at {c:?} is {value}");
                assert!((-1.0..=1.0).contains(&value), "{name} at {c:?} is {value}");
            }
        }
    }

    #[test]
    fn a_sample_reports_the_coordinate_and_world_position_it_used() {
        let g = Generator::with_defaults(5);
        for c in sample_coords() {
            let s = g.sample(c);
            assert_eq!(s.coord, c);
            assert_eq!(s.world, crate::axial_to_world(c));
        }
    }

    #[test]
    fn sampling_order_does_not_affect_any_value() {
        // Section 30.2. Visit the same coordinates forwards, then backwards,
        // then interleaved, and require bit-exact agreement.
        let g = Generator::with_defaults(0xfeed_face);
        let coords = sample_coords();

        let forward: Vec<Sample> = coords.iter().map(|c| g.sample(*c)).collect();

        let mut backward: Vec<Sample> = coords.iter().rev().map(|c| g.sample(*c)).collect();
        backward.reverse();
        assert_eq!(forward, backward);

        let mut interleaved = vec![forward[0]; coords.len()];
        for index in (0..coords.len()).step_by(2) {
            interleaved[index] = g.sample(coords[index]);
        }
        for index in (1..coords.len()).step_by(2) {
            interleaved[index] = g.sample(coords[index]);
        }
        assert_eq!(forward, interleaved);
    }

    #[test]
    fn the_seed_and_the_configuration_both_change_the_world() {
        let c = Coord::new(37, -11);
        let a = Generator::with_defaults(1).sample(c);
        let b = Generator::with_defaults(2).sample(c);
        assert_ne!(a.elevation_raw, b.elevation_raw);

        let tuned = Generator::new(
            1,
            Config {
                continental_wavelength_miles: 5_994.0,
                ..Config::default()
            },
        )
        .expect("configuration is valid");
        assert_ne!(a.elevation_raw, tuned.sample(c).elevation_raw);
    }

    #[test]
    fn a_wrapped_coordinate_samples_exactly_as_its_canonical_representative() {
        // Section 7.1: a coordinate outside the canonical domain names the same
        // tile as its canonical representative, so it must sample identically —
        // bit for bit, not approximately.
        let g = Generator::with_defaults(0xabc);
        let n = crate::WORLD_RADIUS;
        for (q, r) in [(0_i64, 0_i64), (5, -3), (-11, 400), (n, -n), (-n, n)] {
            let canonical = Coord::new(q, r);
            for (mq, mr) in [(2 * n + 1, -n), (n + 1, -(2 * n + 1)), (-(2 * n + 1), n)] {
                let wrapped = Coord::new(q + mq, r + mr);
                assert_eq!(wrapped, canonical);
                assert_eq!(g.sample(wrapped), g.sample(canonical));
            }
        }
    }

    #[test]
    fn every_public_route_to_elevation_gives_the_same_bits() {
        // `sample`, `elevation_at`, and `tile` are three doors into one
        // function. If they ever disagree, one of them has grown its own copy
        // of the composition.
        let g = Generator::with_defaults(0x1111_2222);
        for c in sample_coords() {
            let sample = g.sample(c);
            let tile = g.tile(c);
            assert_eq!(
                sample.elevation.to_bits(),
                g.elevation_at(c).to_bits(),
                "{c:?}"
            );
            assert_eq!(
                tile.elevation_value.to_bits(),
                g.elevation_at(c).to_bits(),
                "{c:?}"
            );
            assert_eq!(tile.relief_value.to_bits(), g.relief(c).to_bits(), "{c:?}");
            assert_eq!(tile.coord, c);
        }
    }

    #[test]
    fn relief_is_built_from_neighbor_elevations_and_nothing_else() {
        // Section 18's pipeline runs one way: raw fields, elevation scalar,
        // neighbor elevation samples, relief. Rust cannot check that `tile`
        // does not call `tile`, so this checks the observable consequence —
        // relief is exactly what the six *elevations* produce, which a
        // tile-level recursion could not be.
        let g = Generator::with_defaults(0x3333_4444);
        for c in sample_coords() {
            let here = g.elevation_at(c);
            let mut neighbors = [0.0_f64; DIRECTION_COUNT];
            for (index, slot) in neighbors.iter_mut().enumerate() {
                let direction = i32::try_from(index).expect("DIRECTION_COUNT is 6");
                *slot = g.elevation_at(c.neighbor(direction));
            }
            let expected =
                relief::from_neighbors(here, &neighbors, g.config().relief_reference_delta_per_hex);
            assert_eq!(g.relief(c).to_bits(), expected.to_bits(), "{c:?}");
        }
    }

    #[test]
    fn every_tile_value_is_finite_and_in_its_documented_range() {
        let g = Generator::with_defaults(0x5555_6666);
        for c in sample_coords() {
            let tile = g.tile(c);
            for (name, value) in [
                ("elevation_value", tile.elevation_value),
                ("heat_value", tile.heat_value),
                ("moisture_value", tile.moisture_value),
            ] {
                assert!(value.is_finite(), "{name} at {c:?} is {value}");
                assert!((-1.0..=1.0).contains(&value), "{name} at {c:?} is {value}");
            }
            assert!(
                (0.0..=1.0).contains(&tile.relief_value),
                "relief at {c:?} is {}",
                tile.relief_value
            );
        }
    }

    #[test]
    fn the_provisional_terrain_partitions_water_and_land_the_same_way_the_band_does() {
        // Phase 6 replaces this mapping, but while it stands it must not
        // contradict the classification it is derived from.
        let water = [Terrain::DeepOcean, Terrain::ShallowSea];
        for band in [
            Elevation::DeepWater,
            Elevation::ShallowWater,
            Elevation::Lowland,
            Elevation::Upland,
            Elevation::Highland,
            Elevation::Mountain,
        ] {
            assert_eq!(
                water.contains(&provisional_terrain(band)),
                band.is_water(),
                "{band:?}"
            );
        }
    }

    #[test]
    fn every_public_route_to_climate_gives_the_same_bits() {
        // `sample` and `tile` are two doors into `climate_scalars`, and the
        // tile's bands must be the classification of the tile's own numbers
        // rather than of a second evaluation that happened to agree.
        let g = Generator::with_defaults(0x7777_8888);
        for c in sample_coords() {
            let sample = g.sample(c);
            let tile = g.tile(c);
            assert_eq!(tile.heat_value.to_bits(), sample.heat.to_bits(), "{c:?}");
            assert_eq!(
                tile.moisture_value.to_bits(),
                sample.moisture.to_bits(),
                "{c:?}"
            );
            assert_eq!(
                tile.climate,
                crate::climate::classify(g.config(), tile.heat_value, tile.moisture_value),
                "{c:?}"
            );
        }
    }

    #[test]
    fn the_world_is_not_uniformly_temperate() {
        // The placeholder this replaces asserted the opposite: before phase 5
        // every tile reported the middle of both axes. A world that had
        // silently gone back to one climate would still pass every bit-exact
        // test above, so the variety is asserted here rather than inferred.
        let g = Generator::with_defaults(1);
        let mut heat = Vec::new();
        let mut moisture = Vec::new();
        for q in -30..=30_i64 {
            for r in -30..=30_i64 {
                let tile = g.tile(Coord::new(q * 211, r * 197));
                if !heat.contains(&tile.climate.heat) {
                    heat.push(tile.climate.heat);
                }
                if !moisture.contains(&tile.climate.moisture) {
                    moisture.push(tile.climate.moisture);
                }
            }
        }
        assert_eq!(heat.len(), 5, "only {heat:?} of the heat bands occur");
        assert_eq!(
            moisture.len(),
            5,
            "only {moisture:?} of the moisture bands occur"
        );
    }

    #[test]
    fn a_generator_is_usable_from_several_threads_at_once() {
        // Section 30.3. This compiles only if `Generator: Sync`, which is the
        // property the assertion in lib.rs pins, and it passes only if thread
        // scheduling cannot reach any value.
        let generator = Generator::with_defaults(42);
        let g = &generator;
        let coords = sample_coords();
        let expected: Vec<Sample> = coords.iter().map(|c| g.sample(*c)).collect();

        std::thread::scope(|scope| {
            for worker in 0..8_usize {
                let coords = &coords;
                let expected = &expected;
                scope.spawn(move || {
                    assert_eq!(g.seed(), 42);
                    // Each worker walks the whole list from a different offset,
                    // so no two threads sample in the same order.
                    for step in 0..coords.len() {
                        let index = (step + worker * 7) % coords.len();
                        assert_eq!(g.sample(coords[index]), expected[index]);
                    }
                });
            }
        });
    }
}
