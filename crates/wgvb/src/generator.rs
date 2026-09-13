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
//! Phase 2 adds [`Generator::sample`], the diagnostic scalar view of the
//! continuous fields that exist so far. `tile`, `elevation_at`, and `relief`
//! arrive with the classifications they depend on, in phases 4 through 6; when
//! they do, `tile` will have nothing to re-normalize because [`Coord`] already
//! guarantees canonical input.

use crate::field::Fields;
use crate::region::{self, Param};
use crate::{Config, ConfigError, Coord, RegionParams, Seed, Vec2, axial_to_world};

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

    /// The weighted multi-scale sum of section 10, normalized to `[-1, +1]`.
    ///
    /// This is *not* the elevation of [`crate::Tile`]. Phase 4 defines
    /// elevation, including sea level and the shaping that makes coastlines
    /// read as coastlines; this is the unshaped composite the tuning renderer
    /// draws in the meantime.
    pub elevation_raw: f64,

    /// The blended regional elevation bias of `DESIGN.md` sections 11 and 12,
    /// in `[-1, +1]`.
    ///
    /// **Not folded into [`Sample::elevation_raw`] yet.** Regions bias fields;
    /// deciding how much uplift a bias is worth is elevation's job, and
    /// elevation arrives in phase 4. Carrying the bias separately here means
    /// the tuning renderer can show the region field on its own — which is how
    /// "no visible implementation-region boundaries" is actually checked —
    /// without phase 3 quietly moving every elevation value in the golden
    /// table.
    ///
    /// [`Generator::region_params`] returns this alongside the climate,
    /// roughness, basin, volcanic, variation, and ridge parameters that phases
    /// 5 and 6 consume.
    pub regional_uplift: f64,
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
    #[must_use]
    pub fn sample(&self, coord: Coord) -> Sample {
        let world = axial_to_world(coord);

        let continentalness = self.fields.continentalness.sample(world.x, world.y);
        let regional = self.fields.regional.sample(world.x, world.y);
        let local = self.fields.local.sample(world.x, world.y);
        let detail = self.fields.detail.sample(world.x, world.y);

        let config = &self.config;
        let mut total = 0.0_f64;
        let mut weight = 0.0_f64;
        total += config.continental_weight * continentalness;
        weight += config.continental_weight;
        total += config.regional_weight * regional;
        weight += config.regional_weight;
        total += config.local_weight * local;
        weight += config.local_weight;
        total += config.detail_weight * detail;
        weight += config.detail_weight;

        Sample {
            coord,
            world,
            continentalness,
            regional,
            local,
            detail,
            elevation_raw: total / weight,
            regional_uplift: region::scalar(self.seed, config, coord, Param::ElevationBias),
        }
    }

    /// The deterministic region parameters at one canonical coordinate.
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
            fbm_octaves: 0,
            ..Config::default()
        };
        assert_eq!(Generator::new(1, config), Err(ConfigError::OctaveCount(0)));
    }

    #[test]
    fn new_accepts_a_valid_non_default_configuration() {
        let config = Config {
            sea_level: -0.25,
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
