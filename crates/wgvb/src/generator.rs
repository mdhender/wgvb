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
//! Phase 1 provides construction and accessors. The sampling API — `tile`,
//! `elevation_at`, `relief`, `sample` — arrives with the fields it samples, and
//! `tile` will have nothing to re-normalize because [`Coord`] already guarantees
//! canonical input.

use crate::{Config, ConfigError, Seed};

/// An immutable, thread-safe world generator.
///
/// Construction validates the configuration; nothing afterwards mutates it, so
/// every tile is a pure function of the seed, the coordinate, the algorithm
/// version, and this configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct Generator {
    seed: Seed,
    config: Config,
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
        Ok(Generator { seed, config })
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

    #[test]
    fn a_generator_is_usable_from_several_threads_at_once() {
        // This compiles only if `Generator: Sync`, which is the property the
        // assertion in lib.rs pins. Once sampling exists, the same shape proves
        // order independence.
        let generator = Generator::with_defaults(42);
        let g = &generator;
        let coords = [Coord::new(0, 0), Coord::new(-1, 5), Coord::new(i64::MAX, 3)];
        std::thread::scope(|scope| {
            for c in coords {
                scope.spawn(move || {
                    assert_eq!(g.seed(), 42);
                    assert_eq!(c, Coord::new(i64::from(c.q()), i64::from(c.r())));
                });
            }
        });
    }
}
