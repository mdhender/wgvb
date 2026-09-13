//! Hierarchical region influence.
//!
//! See `DESIGN.md` sections 11, 11.1, 11.2, 12, 26, 30.5, and 33.4.
//!
//! Continuous noise gives a world texture; it does not give one part of the
//! world a character another part lacks. Regions do, and they do it without
//! storing anything: a lattice of anchors spaced along the axial basis
//! directions, each anchor's parameters derived by hashing its address, and
//! every tile blended from the anchors around it.
//!
//! # No hard boundaries
//!
//! Section 11.2 and section 33.4 are the whole design constraint here. A tile
//! that inherited one region's parameters wholesale would show the addressing
//! lattice as a visible seam, which is the defect regions exist to avoid at a
//! larger scale. So a tile is always a blend, and the blend is smooth rather
//! than merely continuous: a linear interpolant creases along every anchor line,
//! and a crease is visible for the same reason a step is.
//!
//! [`anchors_and_weights`] is where both of those are settled, and its comment
//! is the one to read before changing anything here.
//!
//! # Levels
//!
//! Two anchored levels, coarsest first: macro region and region. Chunks are
//! deliberately not a level. Section 23 makes chunks a caller and cache
//! convenience that does not define geography, and giving them parameters would
//! make the cache size a world input.
//!
//! # Cells are parallelograms; the anchor lattice is triangular
//!
//! Dividing `q` and `r` independently produces parallelogram cells in world
//! space, not regular hexagons — section 11 says so explicitly, and the
//! `512 / 128 / 32` intervals are anchor *spacing along a basis direction*
//! rather than a diameter.
//!
//! The anchors those cells are cornered on are a different matter. The axial
//! basis vectors are 60 degrees apart and the same length, so the anchors form a
//! **triangular** lattice in world space and every cell is two equilateral
//! triangles. The blend follows the lattice rather than the cell, which is the
//! hex-oriented neighborhood section 11.2 permits in place of its four-corner
//! formula.
//!
//! # Wrapped edges
//!
//! Anchor addresses are derived from a [`Coord`], which cannot be constructed
//! non-canonically, so a coordinate and every one of its wrapped images resolve
//! to exactly the same anchors and produce bit-identical parameters. That is the
//! property tile identity, persistence keys, and region lookup depend on, and it
//! holds on all six edges.
//!
//! What the anchor lattice does *not* do is close on itself across a wrapped
//! edge, and it cannot: the wraparound period is `2N + 1 = 65535` hexes, and
//! neither 128 nor 512 divides `65535 = 3 * 5 * 17 * 257`. Forcing closure would
//! constrain every level size to a divisor of 65535 and would still leave the
//! continuous fields of `field.rs` unwrapped. So regions inherit the same
//! accepted world-warp seam as those fields, documented in section 7.1 and
//! measured in `tests/wrap_seam.rs`, rather than adding a second, different
//! discontinuity of their own.
//!
//! # No cache
//!
//! Section 26 names region parameters as the likeliest first optimization and
//! then says to profile first. There is deliberately no cache here: correctness
//! must never depend on cache history, a cache behind `&self` needs interior
//! mutability, and interior mutability inside [`crate::Generator`] would break
//! the `Send + Sync` assertion in `lib.rs` at compile time. If profiling ever
//! justifies one, it belongs outside the generator — which that assertion
//! enforces rather than suggests.

use crate::field::normalize;
use crate::hash::{DOM_REGION_STYLE, DOM_RIDGE_ORIENTATION, hash_n, signed_pair, signed_unit_f64};
use crate::{Config, Coord, Seed};

/// A unit direction vector: the `(cos, sin)` of an angle that is never formed.
///
/// Section 25.2 bars `sin` and `cos` from the generation path because they route
/// to the platform libm and are not bit-identical across targets. Storing the
/// vector instead of the angle is not a micro-optimization, it is what makes
/// ridge orientation expressible at all — every operation that produces or
/// consumes one of these is a multiply, an add, a divide, or a `sqrt`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitVec2 {
    pub x: f64,
    pub y: f64,
}

impl UnitVec2 {
    /// The `+x` direction. Also the deterministic fallback wherever an
    /// orientation is genuinely undefined.
    pub const X: UnitVec2 = UnitVec2 { x: 1.0, y: 0.0 };

    /// The squared length, which a test requires to be one.
    #[must_use]
    pub fn length_squared(self) -> f64 {
        self.x * self.x + self.y * self.y
    }
}

/// The deterministic character of one place, blended across anchors.
///
/// Every scalar is normalized to `[-1, +1]` and is a *bias*, not a quantity: it
/// says how this place differs from the average, and the phase that consumes it
/// decides what magnitude that difference has. Keeping the amplitudes with the
/// consumer is what lets phases 4 through 6 tune elevation, climate, and terrain
/// independently without redefining what a region is.
///
/// Regions bias fields; they do not assign terrain. See section 33.4.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegionParams {
    /// Average uplift: how much higher or lower this place sits than the
    /// continental field alone would put it. Weighted by
    /// [`Config::uplift_weight`] into the elevation scalar.
    pub elevation_bias: f64,
    /// Wet or dry tendency, the "wet/dry tendency" of section 11.1. Consumed by
    /// phase 5.
    pub moisture_bias: f64,
    /// Warm or cool tendency, independent of moisture. Consumed by phase 5.
    pub heat_bias: f64,
    /// Rough or smooth tendency: how strongly local relief reads here.
    ///
    /// Elevation reads it twice — it scales the ridge structure term to zero in
    /// a smooth region, and it scales hill relief between half and full
    /// strength — so a smooth region is a gentle plain and a rough one is a
    /// mountain belt. Phase 6 reads it again for terrain.
    pub roughness: f64,
    /// Tendency toward enclosed low ground. Consumed by phase 6, which decides
    /// whether bounded local generation can give inland water coherent
    /// membership at all.
    pub basin_bias: f64,
    /// Volcanic tendency. Consumed by phase 6.
    pub volcanic: f64,
    /// Terrain variation: how mixed or uniform this place's terrain reads.
    pub variation: f64,

    /// Ridge orientation, as a direction rather than an angle.
    ///
    /// This is a *line*, not an arrow: `v` and `-v` name the same ridge
    /// orientation, and consumers must treat them as equal — take `|dot|`, not
    /// `dot`. The blend relies on that equivalence, and the returned
    /// representative always has `x >= 0`.
    pub ridge: UnitVec2,
}

/// One deterministic region parameter.
///
/// The discriminants are hash inputs, so they are pinned for the same reason
/// [`crate::Terrain`]'s are: renumbering them silently changes every world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Param {
    ElevationBias = 0,
    MoistureBias = 1,
    HeatBias = 2,
    Roughness = 3,
    BasinBias = 4,
    Volcanic = 5,
    Variation = 6,
}

/// Every scalar parameter, in the order [`params`] accumulates them.
const PARAMS: [Param; 7] = [
    Param::ElevationBias,
    Param::MoistureBias,
    Param::HeatBias,
    Param::Roughness,
    Param::BasinBias,
    Param::Volcanic,
    Param::Variation,
];

/// The anchored levels of the hierarchy, coarsest first.
///
/// Like [`Param`], the discriminants are hash inputs: the level tag is what
/// keeps macro anchor `(3, 5)` from being the same hash as region anchor
/// `(3, 5)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Level {
    MacroRegion = 0,
    Region = 1,
}

/// Both levels, coarsest first. Section 25.3: the accumulation order is part of
/// the result, so it is written down once rather than left to a caller.
const LEVELS: [Level; 2] = [Level::MacroRegion, Level::Region];

impl Level {
    /// The anchor spacing for this level, in hexes.
    const fn size_hexes(self, config: &Config) -> u32 {
        match self {
            Level::MacroRegion => config.macro_region_size_hexes,
            Level::Region => config.region_size_hexes,
        }
    }

    /// This level's relative weight in the combined result.
    const fn influence(self, config: &Config) -> f64 {
        match self {
            Level::MacroRegion => config.macro_region_influence,
            Level::Region => config.region_influence,
        }
    }
}

/// Anchors per blend: the three corners of the triangle containing the tile.
const ANCHOR_COUNT: usize = 3;

/// Attempts allowed when rejection-sampling a ridge orientation.
///
/// The same budget and the same acceptance annulus as the noise gradients, for
/// the same reason: each attempt accepts with probability about `0.736`, so
/// twelve attempts fail for roughly one anchor in `10^7`.
const RIDGE_ATTEMPTS: i64 = 12;

/// Smallest accepted squared length for a candidate ridge direction. Rejecting
/// the short candidates keeps the normalization well conditioned; rejecting the
/// long ones keeps the accepted directions uniform in angle.
const RIDGE_MIN_LEN2: f64 = 0.0625;

/// Smallest squared length at which a blended director still names a direction.
///
/// Below this the anchors have cancelled — two ridge orientations at right
/// angles blend to nothing, the way two opposed arrows do — and no orientation
/// is defined. The fallback is deterministic rather than correct, exactly like
/// the noise gradient fallback, because there is no correct answer at a point
/// defect.
const DIRECTOR_MIN_LEN2: f64 = 1.0e-12;

/// The anchors around a tile and their blend weights, for one level.
///
/// Section 11.2 gives the four-corner form `w00 * region00 + ...` and then
/// permits a hex-oriented neighborhood instead. This is the hex-oriented one,
/// and the reason to take it is visible in a rendered region-influence layer:
///
/// The axial basis vectors are 60 degrees apart, so anchors at
/// `(i * size, j * size)` form a **triangular** lattice in world space, not a
/// square one. A cell is a 60-degree rhombus whose short diagonal — from
/// `(cq+1, cr)` to `(cq, cr+1)` — is exactly one edge long, so the rhombus is
/// two equilateral triangles. Interpolating bilinearly across the rhombus
/// ignores that: it makes the long diagonal special, and a field built that way
/// draws the lattice as rows of aligned lozenges that are obvious at a wide zoom
/// even though nothing about the field is discontinuous. Interpolating across
/// the containing *triangle* has no preferred diagonal, and the result carries
/// the lattice's own six-fold symmetry instead of a four-fold one that is not
/// there.
///
/// Barycentric coordinates on the triangle are already a partition of unity, but
/// a linear one creases along every triangle edge. Squaring each coordinate and
/// renormalizing keeps the sum exactly one while making a corner's weight *and
/// its first derivative* vanish as that corner drops out of the neighborhood, so
/// the join across a triangle edge — and across a cell boundary, which is one —
/// is smooth rather than merely continuous.
///
/// The exponent is a decision read off a rendered map, not a free parameter.
/// Squaring is the smallest power that gives a smooth join. Cubing joins
/// smoothly too and looks worse: the weights become peaked enough that each
/// anchor owns a plateau, the plateaus meet along the hexagonal boundaries of
/// the lattice's own Voronoi cells, and the lattice is visible again by another
/// route. Changing this changes every world, so look at a wide
/// `region-influence` render on both sides of the change before deciding.
///
/// Cell addressing is [`Coord::cell`], which is `div_euclid` with a positive
/// divisor — exactly floor division — so the lattice is correct across the
/// origin rather than mirrored about it. Section 24.
fn anchors_and_weights(
    coord: Coord,
    size_hexes: u32,
) -> ([(i64, i64); ANCHOR_COUNT], [f64; ANCHOR_COUNT]) {
    let (cell_q, cell_r) = coord.cell(size_hexes);
    let (offset_q, offset_r) = coord.cell_offset(size_hexes);
    let size = f64::from(size_hexes);
    let u = offset_fraction(offset_q, size);
    let v = offset_fraction(offset_r, size);

    // Which of the rhombus's two triangles the tile is in. The two corners on
    // the shared diagonal come second and third either way, so the accumulation
    // order is fixed across the split rather than per branch.
    let (anchors, barycentric) = if u + v < 1.0 {
        (
            [(cell_q, cell_r), (cell_q + 1, cell_r), (cell_q, cell_r + 1)],
            [1.0 - u - v, u, v],
        )
    } else {
        (
            [
                (cell_q + 1, cell_r + 1),
                (cell_q + 1, cell_r),
                (cell_q, cell_r + 1),
            ],
            [u + v - 1.0, 1.0 - v, 1.0 - u],
        )
    };

    let mut weights = [0.0_f64; ANCHOR_COUNT];
    let mut total = 0.0_f64;
    for (weight, coordinate) in weights.iter_mut().zip(barycentric) {
        *weight = coordinate * coordinate;
        total += *weight;
    }
    // The three barycentric coordinates are non-negative and sum to one, so at
    // least one is at least a third and the total is at least `1/9`. There is no
    // division by zero to guard against.
    for weight in &mut weights {
        *weight /= total;
    }

    (anchors, weights)
}

/// How far through its cell a tile sits, in `[0, 1)`.
///
/// The conversion is exact: [`Coord::cell_offset`] returns a value in
/// `0..size_hexes` and `size_hexes` is a `u32`, so no cast can lose a bit and
/// none is written.
fn offset_fraction(offset: i64, size: f64) -> f64 {
    let offset = u32::try_from(offset).expect("cell_offset returns a value in 0..size_hexes");
    f64::from(offset) / size
}

/// One anchor's value for one parameter, in `[-1, +1)`.
///
/// Nothing is stored: the anchor address, the level, and the parameter are the
/// whole input. Arity four separates these hashes from every noise hash in the
/// crate, which use arities two and three, so a region anchor and a noise
/// lattice point at the same integers cannot alias.
fn anchor_scalar(seed: Seed, level: Level, anchor: (i64, i64), param: Param) -> f64 {
    signed_unit_f64(hash_n(
        seed,
        DOM_REGION_STYLE,
        [level as i64, anchor.0, anchor.1, param as i64],
    ))
}

/// One anchor's ridge orientation.
///
/// Rejection sampling inside the unit disc, bounded at [`RIDGE_ATTEMPTS`] so an
/// unlucky anchor cannot run the loop long. Multiply, add, and `sqrt` only —
/// section 25.2 — which is the point of storing a direction instead of an angle.
fn anchor_ridge(seed: Seed, level: Level, anchor: (i64, i64)) -> UnitVec2 {
    let mut attempt = 0_i64;
    while attempt < RIDGE_ATTEMPTS {
        let (x, y) = signed_pair(hash_n(
            seed,
            DOM_RIDGE_ORIENTATION,
            [level as i64, anchor.0, anchor.1, attempt],
        ));
        let length_squared = x * x + y * y;
        if (RIDGE_MIN_LEN2..=1.0).contains(&length_squared) {
            let inverse_length = 1.0 / length_squared.sqrt();
            return UnitVec2 {
                x: x * inverse_length,
                y: y * inverse_length,
            };
        }
        attempt += 1;
    }
    UnitVec2::X
}

/// The doubled-angle representation of a line orientation, `(cos 2t, sin 2t)`.
///
/// **Orientations cannot be averaged as vectors.** Two ridges that run the same
/// way but were hashed to opposite arrows would blend to nothing, so a tile
/// between them would get an orientation unrelated to either and the result
/// would flip wildly over a few tiles — a discontinuity produced by the
/// representation rather than by the world.
///
/// Doubling the angle removes the distinction: `v` and `-v` have the same
/// director, so the two ridges reinforce instead of cancelling. The identities
/// `cos 2t = cos^2 t - sin^2 t` and `sin 2t = 2 sin t cos t` make the doubling
/// pure arithmetic on the components, with no angle formed and no trigonometric
/// function called.
fn director_of(v: UnitVec2) -> (f64, f64) {
    (v.x * v.x - v.y * v.y, 2.0 * v.x * v.y)
}

/// Recovers a line orientation from a blended director: the half-angle.
///
/// `cos t = sqrt((1 + cos 2t) / 2)` and `sin t = sqrt((1 - cos 2t) / 2)`, taking
/// the sign of `sin t` from `sin 2t`. The half-angle formulas are used rather
/// than `sin t = sin 2t / (2 cos t)` because that form is `0 / 0` exactly where
/// the orientation approaches a quarter turn, which is an ordinary place for a
/// ridge to point.
///
/// Taking the positive root for `cos t` picks the representative with `x >= 0`,
/// one of the two arrows naming the same line. Consumers must already treat `v`
/// and `-v` as equal, so that choice costs nothing and makes the result a
/// function rather than a choice.
fn orientation_of(x: f64, y: f64) -> UnitVec2 {
    let length_squared = x * x + y * y;
    if length_squared < DIRECTOR_MIN_LEN2 {
        return UnitVec2::X;
    }
    let inverse_length = 1.0 / length_squared.sqrt();
    // The clamp is a contract guard for the square roots below, not a shaping
    // step: `x / |(x, y)|` is already within `[-1, +1]` up to rounding.
    let cos_double = (x * inverse_length).clamp(-1.0, 1.0);
    let sin_double = y * inverse_length;

    let cos = ((1.0 + cos_double) * 0.5).sqrt();
    let sin = ((1.0 - cos_double) * 0.5).sqrt();
    UnitVec2 {
        x: cos,
        y: if sin_double < 0.0 { -sin } else { sin },
    }
}

/// One blended parameter at one coordinate, in `[-1, +1]`.
///
/// The reference form of the blend, written for exactly one parameter with
/// nothing else in the loop. Production callers take [`params`] or the narrowed
/// [`elevation_inputs`]; this exists so the tests can state what those two are
/// supposed to compute without restating either of them, and so a test of one
/// parameter reads as a test of one parameter.
///
/// All three perform the same operations in the same order, so all three agree
/// bit for bit.
#[cfg(test)]
pub(crate) fn scalar(seed: Seed, config: &Config, coord: Coord, param: Param) -> f64 {
    let mut total = 0.0_f64;
    let mut weight = 0.0_f64;

    // Coarse to fine, always. Section 25.3.
    for level in LEVELS {
        let influence = level.influence(config);
        let (anchors, weights) = anchors_and_weights(coord, level.size_hexes(config));

        let mut blended = 0.0_f64;
        for (anchor_weight, anchor) in weights.into_iter().zip(anchors) {
            blended += anchor_weight * anchor_scalar(seed, level, anchor, param);
        }

        total += influence * blended;
        weight += influence;
    }

    normalize(total, weight)
}

/// The three region parameters the elevation scalar consumes.
///
/// [`params`] computes all seven scalars plus the ridge orientation, and the
/// ridge orientation alone costs a rejection-sampling loop per anchor per
/// level. Elevation is evaluated seven times per tile — once at the tile and
/// once at each of its six neighbors, for relief — so paying for moisture,
/// heat, basin, volcanic, and variation seven times over would be most of the
/// cost of a tile for values elevation never reads.
///
/// This is a narrowing, not a second implementation: each quantity accumulates
/// with the same operations in the same order as in [`params`], so the two
/// agree bit for bit. A test asserts exactly that, because "agrees bit for bit"
/// is a claim that decays silently if either function is edited alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ElevationInputs {
    pub(crate) elevation_bias: f64,
    pub(crate) roughness: f64,
    pub(crate) ridge: UnitVec2,
}

pub(crate) fn elevation_inputs(seed: Seed, config: &Config, coord: Coord) -> ElevationInputs {
    let mut elevation_bias = 0.0_f64;
    let mut roughness = 0.0_f64;
    let mut director = (0.0_f64, 0.0_f64);
    let mut weight = 0.0_f64;

    // Coarse to fine, always. Section 25.3.
    for level in LEVELS {
        let influence = level.influence(config);
        let (anchors, weights) = anchors_and_weights(coord, level.size_hexes(config));

        for (param, total) in [
            (Param::ElevationBias, &mut elevation_bias),
            (Param::Roughness, &mut roughness),
        ] {
            let mut blended = 0.0_f64;
            for (anchor_weight, anchor) in weights.into_iter().zip(anchors) {
                blended += anchor_weight * anchor_scalar(seed, level, anchor, param);
            }
            *total += influence * blended;
        }

        // Orientations blend as directors, never as arrows. See `director_of`.
        let mut blended = (0.0_f64, 0.0_f64);
        for (anchor_weight, anchor) in weights.into_iter().zip(anchors) {
            let (dx, dy) = director_of(anchor_ridge(seed, level, anchor));
            blended.0 += anchor_weight * dx;
            blended.1 += anchor_weight * dy;
        }
        director.0 += influence * blended.0;
        director.1 += influence * blended.1;

        weight += influence;
    }

    ElevationInputs {
        elevation_bias: normalize(elevation_bias, weight),
        roughness: normalize(roughness, weight),
        ridge: orientation_of(normalize(director.0, weight), normalize(director.1, weight)),
    }
}

/// Every region parameter at one coordinate.
///
/// A pure function of the seed, the coordinate, [`crate::ALGORITHM_VERSION`],
/// and the configuration. Anchors are visited in a fixed order and levels
/// coarse to fine, so two callers on two threads get bit-identical results.
pub(crate) fn params(seed: Seed, config: &Config, coord: Coord) -> RegionParams {
    let mut totals = [0.0_f64; PARAMS.len()];
    let mut director = (0.0_f64, 0.0_f64);
    let mut weight = 0.0_f64;

    for level in LEVELS {
        let influence = level.influence(config);
        let (anchors, weights) = anchors_and_weights(coord, level.size_hexes(config));

        for (total, param) in totals.iter_mut().zip(PARAMS) {
            let mut blended = 0.0_f64;
            for (anchor_weight, anchor) in weights.into_iter().zip(anchors) {
                blended += anchor_weight * anchor_scalar(seed, level, anchor, param);
            }
            *total += influence * blended;
        }

        // Orientations blend as directors, never as arrows. See `director_of`.
        let mut blended = (0.0_f64, 0.0_f64);
        for (anchor_weight, anchor) in weights.into_iter().zip(anchors) {
            let (dx, dy) = director_of(anchor_ridge(seed, level, anchor));
            blended.0 += anchor_weight * dx;
            blended.1 += anchor_weight * dy;
        }
        director.0 += influence * blended.0;
        director.1 += influence * blended.1;

        weight += influence;
    }

    RegionParams {
        elevation_bias: normalize(totals[Param::ElevationBias as usize], weight),
        moisture_bias: normalize(totals[Param::MoistureBias as usize], weight),
        heat_bias: normalize(totals[Param::HeatBias as usize], weight),
        roughness: normalize(totals[Param::Roughness as usize], weight),
        basin_bias: normalize(totals[Param::BasinBias as usize], weight),
        volcanic: normalize(totals[Param::Volcanic as usize], weight),
        variation: normalize(totals[Param::Variation as usize], weight),
        ridge: orientation_of(normalize(director.0, weight), normalize(director.1, weight)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WORLD_RADIUS;

    const SEED: Seed = 0x9e37_79b9_0000_0003;

    fn config() -> Config {
        Config::default()
    }

    /// Every scalar of a [`RegionParams`], named, in a fixed order.
    fn scalars(p: &RegionParams) -> [(&'static str, f64); 7] {
        [
            ("elevation_bias", p.elevation_bias),
            ("moisture_bias", p.moisture_bias),
            ("heat_bias", p.heat_bias),
            ("roughness", p.roughness),
            ("basin_bias", p.basin_bias),
            ("volcanic", p.volcanic),
            ("variation", p.variation),
        ]
    }

    /// A spread of canonical coordinates: the origin, both signs at several
    /// magnitudes, cell corners, and the edges of the canonical hexagon.
    fn sample_coords() -> Vec<Coord> {
        let n = WORLD_RADIUS;
        let mut out = Vec::new();
        for q in -3..=3_i64 {
            for r in -3..=3_i64 {
                out.push(Coord::new(q * 131, r * 517));
            }
        }
        for (q, r) in [
            (0, 0),
            (1, -1),
            (-1, 1),
            (127, 127),
            (128, 128),
            (-128, -128),
            (-129, -129),
            (511, -511),
            (512, -512),
            (12_345, -6_789),
            (-12_345, 6_789),
            (n, 0),
            (0, n),
            (-n, 0),
            (0, -n),
            (n, -n),
            (-n, n),
        ] {
            out.push(Coord::new(q, r));
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The six mirror centers in axial form, re-derived from the published
    /// construction rather than imported, so this is a check on the normalizer
    /// rather than a restatement of it.
    fn mirror_centers() -> [(i64, i64); 6] {
        let n = WORLD_RADIUS;
        let mut cube = (2 * n + 1, -n, -n - 1);
        let mut out = [(0_i64, 0_i64); 6];
        for slot in &mut out {
            *slot = (cube.0, cube.1);
            cube = (-cube.2, -cube.0, -cube.1);
        }
        out
    }

    #[test]
    fn the_narrowed_elevation_path_agrees_with_the_full_one_bit_for_bit() {
        // `elevation_inputs` exists only to skip the parameters elevation does
        // not read. The moment it computes anything differently it stops being
        // a narrowing and becomes a second world, so this is the test that
        // makes the optimization safe rather than merely fast.
        let config = config();
        for coord in sample_coords() {
            let full = params(SEED, &config, coord);
            let narrow = elevation_inputs(SEED, &config, coord);
            assert_eq!(
                narrow.elevation_bias.to_bits(),
                full.elevation_bias.to_bits(),
                "{coord:?}"
            );
            assert_eq!(
                narrow.roughness.to_bits(),
                full.roughness.to_bits(),
                "{coord:?}"
            );
            assert_eq!(
                narrow.ridge.x.to_bits(),
                full.ridge.x.to_bits(),
                "{coord:?}"
            );
            assert_eq!(
                narrow.ridge.y.to_bits(),
                full.ridge.y.to_bits(),
                "{coord:?}"
            );
            // And against the one-parameter reference, so a matching pair of
            // wrong implementations would still be caught.
            assert_eq!(
                narrow.elevation_bias.to_bits(),
                scalar(SEED, &config, coord, Param::ElevationBias).to_bits(),
                "{coord:?}"
            );
            assert_eq!(
                narrow.roughness.to_bits(),
                scalar(SEED, &config, coord, Param::Roughness).to_bits(),
                "{coord:?}"
            );
        }
    }

    #[test]
    fn blend_weights_sum_to_one_and_are_non_negative() {
        // Section 11.2's formula is a weighted average or it is nothing: a
        // weight set summing to anything else would rescale the whole field.
        for size in [1_u32, 2, 32, 37, 128, 149, 512, 65_535] {
            for coord in sample_coords() {
                let (_, weights) = anchors_and_weights(coord, size);
                let mut sum = 0.0_f64;
                for weight in weights {
                    assert!(
                        (0.0..=1.0).contains(&weight),
                        "size {size} at {coord:?} gave weight {weight}"
                    );
                    sum += weight;
                }
                assert!(
                    (sum - 1.0).abs() < 1.0e-12,
                    "size {size} at {coord:?} summed to {sum}"
                );
            }
        }
    }

    #[test]
    fn the_anchors_are_distinct_corners_of_the_containing_cell() {
        for size in [1_u32, 32, 128, 512] {
            for coord in sample_coords() {
                let (anchors, _) = anchors_and_weights(coord, size);
                let mut unique = anchors.to_vec();
                unique.sort_unstable();
                unique.dedup();
                assert_eq!(unique.len(), ANCHOR_COUNT, "size {size} at {coord:?}");

                // Whichever triangle the tile is in, every anchor is a corner
                // of the cell `Coord::cell` addresses — the blend never reaches
                // outside the cell it is in.
                let (cell_q, cell_r) = coord.cell(size);
                for anchor in anchors {
                    assert!(
                        (cell_q..=cell_q + 1).contains(&anchor.0)
                            && (cell_r..=cell_r + 1).contains(&anchor.1),
                        "size {size} at {coord:?} reached anchor {anchor:?} \
                         outside cell ({cell_q}, {cell_r})"
                    );
                }

                // Both triangles share the cell's short diagonal, so those two
                // corners are always present and always in the same slots.
                assert_eq!(anchors[1], (cell_q + 1, cell_r));
                assert_eq!(anchors[2], (cell_q, cell_r + 1));
                // The third is whichever end of the long diagonal the tile is
                // nearer, which is what splits the cell into two triangles.
                let (offset_q, offset_r) = coord.cell_offset(size);
                let near_origin = offset_q + offset_r < i64::from(size);
                assert_eq!(
                    anchors[0],
                    if near_origin {
                        (cell_q, cell_r)
                    } else {
                        (cell_q + 1, cell_r + 1)
                    },
                    "size {size} at {coord:?}"
                );
            }
        }
    }

    #[test]
    fn the_two_triangles_agree_on_their_shared_diagonal() {
        // The cell's short diagonal is where the split happens, so it is the one
        // place *inside* a cell where a mistake would show as a crease. Walk
        // across it and compare the step there against the steps beside it.
        let config = config();
        let size = i64::from(config.region_size_hexes);
        let mut crossings = 0_u32;
        for fixed_r in [0_i64, 17, 64, 111] {
            let mut on_diagonal = 0.0_f64;
            let mut elsewhere = 0.0_f64;
            for offset_q in 1..size {
                for param in PARAMS {
                    let here = scalar(SEED, &config, Coord::new(offset_q - 1, fixed_r), param);
                    let next = scalar(SEED, &config, Coord::new(offset_q, fixed_r), param);
                    let delta = (here - next).abs();
                    // The split runs where the two offsets sum to the cell size.
                    if offset_q + fixed_r == size {
                        on_diagonal = on_diagonal.max(delta);
                        crossings += 1;
                    } else {
                        elsewhere = elsewhere.max(delta);
                    }
                }
            }
            assert!(elsewhere > 0.0, "the field is constant at r = {fixed_r}");
            assert!(
                on_diagonal <= elsewhere,
                "r = {fixed_r}: {on_diagonal} across the split, {elsewhere} beside it"
            );
        }
        assert!(crossings > 0, "the walk never crossed the split");
    }

    #[test]
    fn negative_coordinates_address_the_cell_below_zero() {
        // Section 24 and the `div_euclid` rule. With size 128, `q = -1` and
        // `q = -128` are both in cell `-1`, and `q = -129` is in cell `-2`;
        // bare `/` would put all three in cell `0` or `-1` and mirror the
        // lattice about the origin.
        let size = 128_u32;
        for (q, expected_cell, expected_offset) in [
            (0_i64, 0_i64, 0_i64),
            (127, 0, 127),
            (128, 1, 0),
            (-1, -1, 127),
            (-128, -1, 0),
            (-129, -2, 127),
            (-256, -2, 0),
        ] {
            let coord = Coord::new(q, 0);
            let (anchors, weights) = anchors_and_weights(coord, size);
            assert_eq!(anchors[0].0, expected_cell, "q = {q}");
            assert_eq!(coord.cell_offset(size).0, expected_offset, "q = {q}");
            let sum: f64 = weights.iter().sum();
            assert!((sum - 1.0).abs() < 1.0e-12, "q = {q}");
        }
    }

    #[test]
    fn parameters_are_deterministic_and_bit_exact() {
        let config = config();
        for coord in sample_coords() {
            let first = params(SEED, &config, coord);
            let second = params(SEED, &config, coord);
            assert_eq!(first, second, "{coord:?}");
            for ((name, want), (_, got)) in scalars(&first).into_iter().zip(scalars(&second)) {
                assert_eq!(want.to_bits(), got.to_bits(), "{name} at {coord:?}");
            }
            assert_eq!(first.ridge.x.to_bits(), second.ridge.x.to_bits());
            assert_eq!(first.ridge.y.to_bits(), second.ridge.y.to_bits());
        }
    }

    #[test]
    fn every_parameter_is_finite_and_normalized() {
        let config = config();
        for coord in sample_coords() {
            let p = params(SEED, &config, coord);
            for (name, value) in scalars(&p) {
                assert!(value.is_finite(), "{name} at {coord:?} is {value}");
                assert!(
                    (-1.0..=1.0).contains(&value),
                    "{name} at {coord:?} is {value}"
                );
            }
        }
    }

    #[test]
    fn the_single_parameter_path_agrees_with_the_full_one_bit_for_bit() {
        // Two paths that disagreed by one rounding would put a value in
        // `Sample` that no `RegionParams` ever contained.
        let config = config();
        for coord in sample_coords() {
            let p = params(SEED, &config, coord);
            for (param, expected) in [
                (Param::ElevationBias, p.elevation_bias),
                (Param::MoistureBias, p.moisture_bias),
                (Param::HeatBias, p.heat_bias),
                (Param::Roughness, p.roughness),
                (Param::BasinBias, p.basin_bias),
                (Param::Volcanic, p.volcanic),
                (Param::Variation, p.variation),
            ] {
                assert_eq!(
                    scalar(SEED, &config, coord, param).to_bits(),
                    expected.to_bits(),
                    "{param:?} at {coord:?}"
                );
            }
        }
    }

    #[test]
    fn ridge_orientations_are_unit_vectors_pointing_into_the_half_plane() {
        let config = config();
        for coord in sample_coords() {
            let ridge = params(SEED, &config, coord).ridge;
            assert!(
                (ridge.length_squared() - 1.0).abs() < 1.0e-12,
                "{coord:?} gave {ridge:?}"
            );
            assert!(ridge.x >= 0.0, "{coord:?} gave {ridge:?}");
        }
        for level in LEVELS {
            for aq in -20..20_i64 {
                for ar in -20..20_i64 {
                    let ridge = anchor_ridge(SEED, level, (aq, ar));
                    assert!(
                        (ridge.length_squared() - 1.0).abs() < 1.0e-12,
                        "{level:?} anchor ({aq}, {ar}) gave {ridge:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_director_makes_opposite_arrows_identical() {
        // The property the whole doubled-angle representation exists for.
        for i in 0..64_i64 {
            let ridge = anchor_ridge(SEED, Level::Region, (i, i * 7));
            let flipped = UnitVec2 {
                x: -ridge.x,
                y: -ridge.y,
            };
            let (ax, ay) = director_of(ridge);
            let (bx, by) = director_of(flipped);
            assert!((ax - bx).abs() < 1.0e-15 && (ay - by).abs() < 1.0e-15);

            // And the round trip recovers the same line.
            let recovered = orientation_of(ax, ay);
            let dot = recovered.x * ridge.x + recovered.y * ridge.y;
            assert!(
                (dot.abs() - 1.0).abs() < 1.0e-9,
                "{ridge:?} -> {recovered:?}"
            );
        }
    }

    #[test]
    fn a_cancelled_director_falls_back_deterministically() {
        assert_eq!(orientation_of(0.0, 0.0), UnitVec2::X);
        assert_eq!(orientation_of(1.0e-15, -1.0e-15), UnitVec2::X);
        // A quarter turn, where the `sin 2t / (2 cos t)` form would be 0/0.
        let quarter = orientation_of(-1.0, 0.0);
        assert!((quarter.length_squared() - 1.0).abs() < 1.0e-12);
        assert!(quarter.x.abs() < 1.0e-8, "{quarter:?}");
    }

    /// Walks a line of tiles and returns, for one parameter, the largest step
    /// taken at a cell boundary and the largest step taken anywhere else.
    fn boundary_and_interior_steps(
        size_hexes: i64,
        along_q: bool,
        param: Param,
    ) -> (f64, f64, f64, f64) {
        let config = config();
        let span = size_hexes * 6;
        let (mut boundary_max, mut interior_max) = (0.0_f64, 0.0_f64);
        let (mut boundary_sum, mut interior_sum) = (0.0_f64, 0.0_f64);
        let (mut boundary_count, mut interior_count) = (0_u32, 0_u32);
        let mut previous: Option<f64> = None;

        for step in -span..=span {
            let coord = if along_q {
                Coord::new(step, 41)
            } else {
                Coord::new(-97, step)
            };
            let current = scalar(SEED, &config, coord, param);
            if let Some(previous) = previous {
                let delta = (current - previous).abs();
                if step.rem_euclid(size_hexes) == 0 {
                    boundary_max = boundary_max.max(delta);
                    boundary_sum += delta;
                    boundary_count += 1;
                } else {
                    interior_max = interior_max.max(delta);
                    interior_sum += delta;
                    interior_count += 1;
                }
            }
            previous = Some(current);
        }

        assert!(boundary_count >= 6, "too few crossings to measure");
        (
            boundary_max,
            interior_max,
            boundary_sum / f64::from(boundary_count),
            interior_sum / f64::from(interior_count),
        )
    }

    /// How much larger a step across an anchor boundary may be than the steps
    /// beside it.
    ///
    /// The blend is smooth across a cell edge, so a boundary step should be
    /// indistinguishable from an interior one rather than smaller: over every
    /// parameter, both levels, and both axes, the measured ratios reach `1.01`
    /// for the largest step and `1.12` for the mean. The margin below is set
    /// just above that. A real seam would put the two sides of the boundary out
    /// of correlation entirely and the ratio in the hundreds, so nothing sits
    /// anywhere near this threshold in either direction.
    const SEAM_FACTOR: f64 = 1.25;

    #[test]
    fn no_anchor_boundary_is_a_special_place_on_the_line() {
        // Section 30.5 as a direct measurement of the blended field itself,
        // rather than through `Sample`.
        let config = config();
        for size in [
            i64::from(config.region_size_hexes),
            i64::from(config.macro_region_size_hexes),
        ] {
            for along_q in [true, false] {
                for param in PARAMS {
                    let (boundary_max, interior_max, boundary_mean, interior_mean) =
                        boundary_and_interior_steps(size, along_q, param);
                    assert!(interior_max > 0.0, "{param:?} never changed along the line");
                    assert!(
                        boundary_max <= interior_max * SEAM_FACTOR,
                        "{param:?} steps {boundary_max} at a size-{size} boundary \
                         against {interior_max} elsewhere (along_q = {along_q})"
                    );
                    assert!(
                        boundary_mean <= interior_mean * SEAM_FACTOR,
                        "{param:?} averages {boundary_mean} at a size-{size} boundary \
                         against {interior_mean} elsewhere (along_q = {along_q})"
                    );
                }
            }
        }
    }

    #[test]
    fn adjacent_tiles_differ_by_far_less_than_two_anchors_do() {
        // A blend that had degenerated into per-tile hashing would pass the
        // boundary test above and fail here. One region is 128 hexes across, so
        // a single step may move the field by only a small fraction of the
        // distance between neighboring anchors.
        let config = config();
        let mut worst = 0.0_f64;
        for q in -600..600_i64 {
            let here = scalar(SEED, &config, Coord::new(q, -13), Param::ElevationBias);
            let there = scalar(SEED, &config, Coord::new(q + 1, -13), Param::ElevationBias);
            worst = worst.max((here - there).abs());
        }
        assert!(worst > 0.0, "the field is constant");
        assert!(worst < 0.05, "adjacent tiles differed by {worst}");
    }

    #[test]
    fn a_wrapped_coordinate_has_exactly_the_parameters_of_its_canonical_tile() {
        // Region lookup across all six wrapped edges. This holds structurally —
        // `Coord` cannot be constructed non-canonically, so there is only one
        // coordinate to address — and the test pins it because a future anchor
        // scheme that reached for the unwrapped input would break it.
        let config = config();
        let n = WORLD_RADIUS;
        for (q, r) in [
            (0_i64, 0_i64),
            (7, -3),
            (-411, 96),
            (n, -n),
            (-n, n),
            (n, 0),
        ] {
            let canonical = Coord::new(q, r);
            let expected = params(SEED, &config, canonical);
            for (mq, mr) in mirror_centers() {
                for sign in [1_i64, -1] {
                    let translated = Coord::new(q + sign * mq, r + sign * mr);
                    assert_eq!(translated, canonical);
                    assert_eq!(
                        params(SEED, &config, translated),
                        expected,
                        "({q}, {r}) across mirror ({mq}, {mr})"
                    );
                }
            }
        }
    }

    #[test]
    fn anchor_values_are_spread_across_their_range() {
        // Section 30.8. A parameter clustered near zero would give every region
        // the same character, which is the one thing regions exist to prevent.
        for param in PARAMS {
            for level in LEVELS {
                let mut buckets = [0_u32; 10];
                let mut sum = 0.0_f64;
                let mut count = 0_u32;
                for aq in -60..60_i64 {
                    for ar in -60..60_i64 {
                        let value = anchor_scalar(SEED, level, (aq, ar), param);
                        sum += value;
                        count += 1;
                        #[expect(
                            clippy::cast_possible_truncation,
                            clippy::cast_sign_loss,
                            reason = "value is in [-1, 1), so the index is in 0..10"
                        )]
                        let index = ((value + 1.0) * 5.0) as usize;
                        buckets[index.min(9)] += 1;
                    }
                }
                let mean = sum / f64::from(count);
                assert!(mean.abs() < 0.05, "{level:?} {param:?} mean {mean}");
                let low = f64::from(count) * 0.06;
                for (index, bucket) in buckets.iter().enumerate() {
                    assert!(
                        f64::from(*bucket) > low,
                        "{level:?} {param:?} decile {index} held only {bucket}"
                    );
                }
            }
        }
    }

    #[test]
    fn blended_values_still_reach_most_of_the_range() {
        // Blending averages, and averaging compresses: the blended field cannot
        // fill `[-1, +1]` the way the anchors do. It must still produce places
        // that read as distinctly high and distinctly low, or the exit condition
        // for this phase — distant areas with distinct character — is not met.
        let config = config();
        let mut lowest = 1.0_f64;
        let mut highest = -1.0_f64;
        for q in (-20_000..20_000).step_by(97) {
            for r in (-20_000..20_000).step_by(2_003) {
                let value = scalar(SEED, &config, Coord::new(q, r), Param::ElevationBias);
                lowest = lowest.min(value);
                highest = highest.max(value);
            }
        }
        assert!(lowest < -0.4, "the blended field bottoms out at {lowest}");
        assert!(highest > 0.4, "the blended field tops out at {highest}");
    }

    #[test]
    fn ridge_orientations_cover_the_half_circle() {
        // Orientations are lines, so the representatives all have `x >= 0` and
        // the distribution to check is over half a turn. A scheme that had
        // collapsed onto the axes would show up as empty sectors.
        let config = config();
        let mut buckets = [0_u32; 6];
        let mut count = 0_u32;
        for q in (-6_000..6_000).step_by(53) {
            for r in (-6_000..6_000).step_by(509) {
                let ridge = params(SEED, &config, Coord::new(q, r)).ridge;
                // Sector by comparing against the sines of 30 and 60 degrees,
                // which are exact enough as decimal constants for a histogram.
                let index = match ridge.y {
                    y if y < -0.866 => 0,
                    y if y < -0.5 => 1,
                    y if y < 0.0 => 2,
                    y if y < 0.5 => 3,
                    y if y < 0.866 => 4,
                    _ => 5,
                };
                buckets[index] += 1;
                count += 1;
            }
        }
        let low = f64::from(count) * 0.05;
        for (index, bucket) in buckets.iter().enumerate() {
            assert!(
                f64::from(*bucket) > low,
                "ridge sector {index} held only {bucket} of {count}"
            );
        }
    }

    #[test]
    fn levels_and_parameters_do_not_alias() {
        // The level tag and the parameter index are hash inputs precisely so
        // that two things which happen to share an address are still different.
        let mut seen = std::collections::HashSet::new();
        for level in LEVELS {
            for param in PARAMS {
                for aq in -12..12_i64 {
                    for ar in -12..12_i64 {
                        let value = anchor_scalar(SEED, level, (aq, ar), param);
                        assert!(
                            seen.insert(value.to_bits()),
                            "{level:?} {param:?} ({aq}, {ar}) repeated a value"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn region_hashes_do_not_alias_the_noise_lattice() {
        // Arity four against the noise crate's two and three. Same seed, same
        // integers, same domain: the values must still differ.
        for i in -20..20_i64 {
            for j in -20..20_i64 {
                let region = hash_n(SEED, DOM_REGION_STYLE, [0_i64, i, j, 0]);
                assert_ne!(region, crate::hash::hash2(SEED, DOM_REGION_STYLE, i, j));
                assert_ne!(region, crate::hash::hash3(SEED, DOM_REGION_STYLE, i, j, 0));
            }
        }
    }

    #[test]
    fn the_seed_and_the_configuration_both_change_the_parameters() {
        let coord = Coord::new(3_001, -1_777);
        let base = params(SEED, &config(), coord);
        assert_ne!(params(SEED + 1, &config(), coord), base);

        let tuned = Config {
            region_size_hexes: 96,
            ..config()
        };
        assert_ne!(params(SEED, &tuned, coord), base);

        let reweighted = Config {
            region_influence: 0.9,
            ..config()
        };
        assert_ne!(params(SEED, &reweighted, coord), base);
    }

    #[test]
    fn a_single_level_is_exactly_that_level() {
        // With the region level weighted to nothing, the result is the macro
        // blend alone. Arithmetic identity, checked because it is the clearest
        // statement of what the influence weights mean.
        let config = Config {
            macro_region_influence: 1.0,
            region_influence: f64::MIN_POSITIVE,
            ..config()
        };
        let coord = Coord::new(700, -350);
        let (anchors, weights) = anchors_and_weights(coord, config.macro_region_size_hexes);
        let mut expected = 0.0_f64;
        for (anchor_weight, anchor) in weights.into_iter().zip(anchors) {
            expected +=
                anchor_weight * anchor_scalar(SEED, Level::MacroRegion, anchor, Param::HeatBias);
        }
        let actual = scalar(SEED, &config, coord, Param::HeatBias);
        assert!(
            (actual - expected).abs() < 1.0e-12,
            "{actual} vs {expected}"
        );
    }

    #[test]
    fn two_distant_places_have_distinct_character() {
        // The exit condition for this phase, stated as a test: two places a
        // long way apart must not share a profile.
        let config = config();
        let a = params(SEED, &config, Coord::new(-18_000, 7_000));
        let b = params(SEED, &config, Coord::new(18_000, -7_000));
        let differing = scalars(&a)
            .into_iter()
            .zip(scalars(&b))
            .filter(|((_, x), (_, y))| (x - y).abs() > 0.1)
            .count();
        assert!(differing >= 4, "{a:?} and {b:?} are too similar");
    }
}
