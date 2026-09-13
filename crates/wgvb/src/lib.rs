//! WGVB — deterministic, effectively unbounded procedural hex-world generator.
//!
//! The central invariant of this crate is:
//!
//! ```text
//! Tile = F(seed, coordinate, algorithm version, configuration)
//! ```
//!
//! with no dependency on generation order, thread scheduling, cache contents,
//! explored area, or persisted mutable state. Read `DESIGN.md` before changing
//! generator behavior or public APIs.
//!
//! This crate is intentionally free of persistence and rendering dependencies.

/// Generator algorithm version.
///
/// Changing any hash domain, field composition, threshold, default, the
/// coordinate-to-world conversion, or any classification rule changes existing
/// worlds and requires bumping this value. See `DESIGN.md` section 27.
pub const ALGORITHM_VERSION: u32 = 1;

/// Axial coordinate component type for the alpha world.
///
/// The world radius is derived from this choice. Widening it to `i32` changes
/// world topology and therefore requires a new [`ALGORITHM_VERSION`], even
/// though no schema migration is needed. See `DESIGN.md` section 4.
pub type Component = i16;

/// Radius of the canonical wrapped hexagonal map, in tiles.
///
/// The canonical domain is `-N <= q, r, s <= +N` with `q + r + s == 0`,
/// containing `1 + 3*N*(N+1)` tiles. See `DESIGN.md` section 7.1.
pub const WORLD_RADIUS: i64 = Component::MAX as i64;

/// Apothem of one hex, in miles. Adjacent hex centers are `2 * APOTHEM_MILES`
/// apart. See `DESIGN.md` section 7.
pub const APOTHEM_MILES: f64 = 3.0;

// Phase 1 begins here. See DESIGN.md section 32 for the ordered plan and each
// phase's exit condition.
