//! Canonical hex coordinates for the wrapped world.
//!
//! See `DESIGN.md` sections 4, 7, 7.1, 23, 24, and appendix A.
//!
//! [`Coord`] has private fields and no constructor that skips normalization.
//! That is load-bearing rather than stylistic: it makes "values that normalize
//! to the same coordinate identify the same tile" a property of the type, so the
//! derived [`PartialEq`], [`Hash`], and [`Ord`] are correct for tile identity,
//! persistence keys, region lookup, and set membership. Do not add a public
//! constructor, a `pub` field, or a `From<(i16, i16)>` that bypasses
//! [`Coord::new`].

use crate::{APOTHEM_MILES, Component, SQRT_3, WORLD_RADIUS};

/// Number of hex directions.
pub const DIRECTION_COUNT: usize = 6;

/// The six canonical direction vectors in axial form, pinned by the algorithm
/// version.
///
/// Increasing the index by one steps to the next neighbor counter-clockwise, in
/// the order Red Blob Games gives them; decreasing it steps clockwise. The cube
/// form is `(q, r, -q - r)`. See `DESIGN.md` appendix A — changing or
/// renumbering this table changes every world and is an algorithm compatibility
/// change.
///
/// This crate has no north, so it has no compass and no reliable sense of
/// clockwise: those are properties of a *viewer*, and a viewer lives in the
/// presentation layer. Everything here is index arithmetic, `(d + 1) mod 6`.
/// Appendix A's *Rotation senses* is the one place the two are reconciled —
/// read it before writing "clockwise" in a comment anywhere in this crate,
/// because the compass walk a player sees runs the *other* way, `d - 1`.
///
/// | Direction | Cube `(q, r, s)` | Axial `(q, r)` |
/// |---:|---|---|
/// | 0 | `(+1,  0, -1)` | `(+1,  0)` |
/// | 1 | `(+1, -1,  0)` | `(+1, -1)` |
/// | 2 | `( 0, -1, +1)` | `( 0, -1)` |
/// | 3 | `(-1,  0, +1)` | `(-1,  0)` |
/// | 4 | `(-1, +1,  0)` | `(-1, +1)` |
/// | 5 | `( 0, +1, -1)` | `( 0, +1)` |
///
/// Directions carry no compass names in this crate. North is a property of a
/// viewing player, not of the world: a player is assigned an origin hex and a
/// rotation, so one player's north is absolute direction `k` and another's is
/// not. Compass naming and the player-frame transform belong to the rendering
/// layer. See `DESIGN.md` appendix A, *Coordinate frames*.
pub const DIRECTIONS: [(Component, Component); DIRECTION_COUNT] =
    [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];

/// Normalizes any integer direction to an index in `0..6`.
///
/// Values differing by a multiple of six identify the same direction. This uses
/// `rem_euclid`, which is non-negative for a positive divisor, so no sign
/// adjustment branch is needed. See `DESIGN.md` section 24 and appendix A.
#[inline]
#[must_use]
pub const fn direction_index(direction: i32) -> usize {
    // `rem_euclid` with a positive divisor is non-negative, so the unsigned
    // reinterpretation and the widening to `usize` are both lossless.
    direction.rem_euclid(6).cast_unsigned() as usize
}

/// One step in index order on the cube form: direction `d` becomes `d + 1`.
///
/// This is an exact integer permutation with sign changes — no rotation matrix
/// and no angle — so it is available anywhere in the generation path without
/// violating the exact-operation rule in `DESIGN.md` section 25.2. Six
/// applications are the identity. The inverse step is `(x, y, z) -> (-y, -z, -x)`.
///
/// Named for the index rather than for a rotation sense, because the sense
/// depends on who is looking: advancing the index turns *counter*-clockwise as
/// any viewer sees the world, and clockwise only under a plot of canonical
/// world space with `+y` upward, which nothing here draws. The old name,
/// `rotate_cw`, was wrong twice over — it named a sense that holds in no frame
/// this project presents, in an abbreviation that reads as either "clockwise"
/// or "compass walk". Those two run opposite ways through the index; see
/// `DESIGN.md` appendix A, *Rotation senses*, and do not bring `cw` back.
///
/// The same function generates [`mirror_centers`], which is why it is written
/// once here instead of transcribing six triples by hand.
#[inline]
const fn rotate_once(v: (i64, i64, i64)) -> (i64, i64, i64) {
    let (x, y, z) = v;
    (-z, -x, -y)
}

/// The six mirror centers for a wrapped hexagonal map of radius `n`: the
/// rotations of `(2n+1, -n, -n-1)` under [`rotate_once`].
///
/// Every center sums to zero and the sequence closes after six steps. For
/// `n = i16::MAX` two of the six have components at `±65535`, outside
/// [`Component`] range — which is exactly what the "every intermediate in `i64`"
/// rule protects. See `DESIGN.md` section 7.1.
const fn mirror_centers(n: i64) -> [(i64, i64, i64); DIRECTION_COUNT] {
    let mut out = [(0, 0, 0); DIRECTION_COUNT];
    let mut v = (2 * n + 1, -n, -n - 1);
    let mut i = 0;
    while i < DIRECTION_COUNT {
        out[i] = v;
        v = rotate_once(v);
        i += 1;
    }
    out
}

/// Mirror centers for this world's radius, computed at compile time.
///
/// This is `const` data, never a lookup table of tiles: the map has
/// 3,221,127,169 tiles and normalization is arithmetic.
const MIRROR_CENTERS: [(i64, i64, i64); DIRECTION_COUNT] = mirror_centers(WORLD_RADIUS);

/// A canonical axial coordinate: one tile of the wrapped world.
///
/// The only ways to obtain one are [`Coord::new`], [`Coord::ORIGIN`], and
/// operations such as [`Coord::neighbor`] that normalize before returning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Coord {
    q: Component,
    r: Component,
}

impl Coord {
    /// The canonical origin, `(0, 0, 0)`.
    pub const ORIGIN: Coord = Coord { q: 0, r: 0 };

    /// Normalizes any axial pair into the canonical wrapped map.
    ///
    /// This is the single entry point for unwrapped or intermediate
    /// coordinates. It is total: every `i64` pair, including `i64::MIN`, has a
    /// canonical representative, and `Coord::new` of an already-canonical pair
    /// returns it unchanged, so the operation is idempotent.
    #[inline]
    #[must_use]
    pub fn new(q: i64, r: i64) -> Coord {
        let (q, r) = normalize(WORLD_RADIUS, &MIRROR_CENTERS, q, r);
        Coord {
            q: Component::try_from(q).expect("normalizer proved q in Component range"),
            r: Component::try_from(r).expect("normalizer proved r in Component range"),
        }
    }

    /// The axial `q` component.
    #[inline]
    #[must_use]
    pub const fn q(self) -> Component {
        self.q
    }

    /// The axial `r` component.
    #[inline]
    #[must_use]
    pub const fn r(self) -> Component {
        self.r
    }

    /// The derived cube component, `-q - r`.
    ///
    /// Always in [`Component`] range for a canonical coordinate, which is a
    /// property of the canonical domain rather than of the arithmetic.
    #[inline]
    #[must_use]
    pub const fn s(self) -> Component {
        let s = -(self.q as i64) - (self.r as i64);
        debug_assert!(s >= -WORLD_RADIUS && s <= WORLD_RADIUS);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a canonical coordinate has |s| <= WORLD_RADIUS, which the \
                      debug assertion documents; `Component::try_from` is not \
                      available in const context"
        )]
        let narrowed = s as Component;
        narrowed
    }

    /// The neighbor in the given direction, normalized.
    ///
    /// Any integer direction is accepted; see [`direction_index`].
    #[inline]
    #[must_use]
    pub fn neighbor(self, direction: i32) -> Coord {
        let (dq, dr) = DIRECTIONS[direction_index(direction)];
        Coord::new(
            i64::from(self.q) + i64::from(dq),
            i64::from(self.r) + i64::from(dr),
        )
    }

    /// This coordinate rotated `steps` sixths of a turn about the canonical
    /// origin, in the direction of increasing direction index.
    ///
    /// Exact integer arithmetic, and the canonical domain is six-fold symmetric
    /// about the origin, so rotation maps it onto itself and commutes with
    /// normalization. This is the world-frame half of the player transform
    /// `absolute = normalize(rotate^k(relative) + player_origin)`; the
    /// player's origin, rotation, and any compass naming belong to the
    /// presentation layer. See `DESIGN.md` appendix A.
    ///
    /// `steps` is signed and unbounded; it is normalized like any direction.
    /// One step is counter-clockwise as a viewer sees it, so the compass walk a
    /// player reads — N, NE, SE, S, SW, NW — runs the opposite way, at `-1` a
    /// step. Appendix A's *Rotation senses* is the whole of it, and is why this
    /// method names an index step rather than a rotation sense.
    #[inline]
    #[must_use]
    pub fn rotate(self, steps: i32) -> Coord {
        let mut v = (i64::from(self.q), i64::from(self.r), i64::from(self.s()));
        let mut i = 0;
        let steps = direction_index(steps);
        while i < steps {
            v = rotate_once(v);
            i += 1;
        }
        Coord::new(v.0, v.1)
    }

    /// The cell address containing this coordinate for a cell edge of
    /// `size_hexes`, dividing `q` and `r` independently.
    ///
    /// Used for chunk, region, and macro-region addressing. Cells produced this
    /// way are parallelograms in world space, not regular hexagons, and they do
    /// not define geography — see `DESIGN.md` sections 11 and 23.
    ///
    /// This uses `div_euclid`, which for a positive divisor is exactly floor
    /// division, so cell addresses are correct across the origin: with size 32,
    /// `q = -1` and `q = -32` are both in cell `-1` and `q = -33` is in cell
    /// `-2`. Euclidean and floor division differ for a *negative* divisor, which
    /// is why the size is unsigned and zero is rejected.
    ///
    /// # Panics
    ///
    /// Panics if `size_hexes` is zero.
    #[inline]
    #[must_use]
    pub fn cell(self, size_hexes: u32) -> (i64, i64) {
        let size = i64::from(size_hexes);
        assert!(size > 0, "cell size must be positive");
        (
            i64::from(self.q).div_euclid(size),
            i64::from(self.r).div_euclid(size),
        )
    }

    /// This coordinate's offset within its [`cell`](Coord::cell), in `0..size`
    /// on both axes.
    ///
    /// # Panics
    ///
    /// Panics if `size_hexes` is zero.
    #[inline]
    #[must_use]
    pub fn cell_offset(self, size_hexes: u32) -> (i64, i64) {
        let size = i64::from(size_hexes);
        assert!(size > 0, "cell size must be positive");
        (
            i64::from(self.q).rem_euclid(size),
            i64::from(self.r).rem_euclid(size),
        )
    }
}

/// A point in canonical world space, in miles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f64,
    pub y: f64,
}

/// Converts a canonical coordinate to continuous world space, in miles.
///
/// The pointy-top embedding of `DESIGN.md` section 7, with adjacent hex centers
/// exactly `2 * APOTHEM_MILES` apart:
///
/// ```text
/// x = 6 * (q + r/2)
/// y = 3 * sqrt(3) * r
/// ```
///
/// Every continuous field samples through this one conversion, so it is pinned
/// by the algorithm version. The parenthesization is deliberate and must not be
/// "simplified": the operation order is part of the bit-exact result, and
/// `f64::mul_add` must never appear here. `i16` to `f64` is exact, so the
/// conversion loses nothing.
#[inline]
#[must_use]
pub fn axial_to_world(c: Coord) -> Vec2 {
    let q = f64::from(c.q());
    let r = f64::from(c.r());
    Vec2 {
        x: (2.0 * APOTHEM_MILES) * (q + 0.5 * r),
        y: (APOTHEM_MILES * SQRT_3) * r,
    }
}

/// True if `(q, r)` is inside the canonical hexagonal domain of radius `n`.
///
/// `q` and `r` are bounded before `s` is formed, because `-q - r` overflows for
/// extreme inputs and the fast path in [`normalize`] sees arbitrary `i64`.
#[inline]
const fn in_domain(n: i64, q: i64, r: i64) -> bool {
    if q < -n || q > n || r < -n || r > n {
        return false;
    }
    let s = -q - r;
    s >= -n && s <= n
}

/// Hex distance from the origin: `max(|q|, |r|, |s|)`.
///
/// Only called on coordinates the coarse reduction has already brought near the
/// origin, so forming `s` cannot overflow.
#[inline]
fn hex_distance_from_origin(q: i64, r: i64) -> u64 {
    let s = -q - r;
    q.unsigned_abs().max(r.unsigned_abs()).max(s.unsigned_abs())
}

/// Round-half-up integer division for a possibly negative divisor.
#[inline]
fn div_nearest(num: i128, den: i128) -> i128 {
    debug_assert!(den != 0);
    let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
    let quotient = num.div_euclid(den);
    let remainder = num.rem_euclid(den);
    if 2 * remainder >= den {
        quotient + 1
    } else {
        quotient
    }
}

/// Brings an arbitrary `i64` pair within one lattice cell of the origin in one
/// step, by solving for the nearest wraparound-lattice multiple.
///
/// The wraparound lattice is generated by mirror centers `0` and `1`, whose
/// axial forms are `(2n+1, -n)` and `(n+1, -(2n+1))`. Inverting that basis gives
/// the real multiples directly; rounding each to the nearest integer leaves a
/// residual of hex distance at most `2n+1`, which the greedy fix-up in
/// [`normalize`] then walks into the canonical hexagon in a step or two.
///
/// This is why normalization is not a loop over single mirror translations:
/// `Coord::new(i64::MAX, 0)` is legal input, and stepping one mirror center at a
/// time would need about `1.4e14` iterations.
///
/// **The solve is `i128`.** Products such as `(2n+1) * q` reach `9.2e18` for
/// `|q|` near `i64::MAX`, which is the top of the `i64` range; the `i64`
/// intermediate rule exists to prevent silent wraparound, and widening the one
/// place that genuinely needs it serves that rule rather than bending it. The
/// residual is small by construction and returns to `i64`.
fn coarse_reduce(n: i64, q: i64, r: i64) -> (i64, i64) {
    let n = i128::from(n);
    let two_n_plus_1 = 2 * n + 1;
    let determinant = -(3 * n * n + 3 * n + 1);

    let q128 = i128::from(q);
    let r128 = i128::from(r);

    let a = div_nearest(-two_n_plus_1 * q128 - (n + 1) * r128, determinant);
    let b = div_nearest(n * q128 + two_n_plus_1 * r128, determinant);

    let residual_q = q128 - a * two_n_plus_1 - b * (n + 1);
    let residual_r = r128 + a * n + b * two_n_plus_1;

    (
        i64::try_from(residual_q).expect("lattice residual is bounded by 2n+1"),
        i64::try_from(residual_r).expect("lattice residual is bounded by 2n+1"),
    )
}

/// Arithmetic wraparound normalization onto the canonical hexagonal domain of
/// radius `n`.
///
/// Follows the Red Blob Games hexagonal wraparound construction: translate by
/// mirror centers until the coordinate is canonical. The six centers are the
/// Voronoi-relevant vectors of the wraparound lattice and the canonical hexagon
/// is an exact fundamental domain of it — `1 + 3n(n+1)` tiles for a lattice of
/// the same index — so each coordinate has exactly one canonical representative
/// and greedy descent on hex distance reaches it with no tie to break.
///
/// Deliberately *not* delegated to a hex library. `hexx::Hex::wrap_in_range`
/// computes its divisor as `i32` through `f32`: at `n = i16::MAX` the tile count
/// `3_221_127_169` exceeds `i32::MAX` and is not representable in `f32`, so
/// wrapping one step past the `+q` edge panics in a debug build and silently
/// returns the input unwrapped in a release build. The same defect rules out
/// `Hex::to_lower_res` for chunk addressing.
///
/// `n` and the center table are parameters so tests can verify the algorithm
/// exhaustively at a small radius, where the whole domain and its neighboring
/// copies can be enumerated.
fn normalize(n: i64, centers: &[(i64, i64, i64); DIRECTION_COUNT], q: i64, r: i64) -> (i64, i64) {
    debug_assert!(n > 0);

    // Overwhelmingly the common case: neighbor steps and local offsets near the
    // caller's own area of the map.
    if in_domain(n, q, r) {
        return (q, r);
    }

    let (mut q, mut r) = coarse_reduce(n, q, r);

    // Greedy descent using the six mirror centers. A point that no center
    // improves lies in the fundamental domain, by the halfspace characterization
    // of the Voronoi cell, so this cannot stall outside it. The residual from
    // `coarse_reduce` is within hex distance `2n+1` of the origin, so this
    // takes one or two steps; the bound is a guard, not a schedule.
    const MAX_STEPS: u32 = 8;
    let mut distance = hex_distance_from_origin(q, r);
    for _ in 0..MAX_STEPS {
        if distance <= n.unsigned_abs() {
            break;
        }
        let mut best: Option<(u64, i64, i64)> = None;
        // Fixed order 0..6, never over a set: see DESIGN.md section 25.3.
        for center in centers {
            let candidate_q = q - center.0;
            let candidate_r = r - center.1;
            let candidate_distance = hex_distance_from_origin(candidate_q, candidate_r);
            if candidate_distance < distance
                && best.is_none_or(|(best_distance, _, _)| candidate_distance < best_distance)
            {
                best = Some((candidate_distance, candidate_q, candidate_r));
            }
        }
        match best {
            Some((candidate_distance, candidate_q, candidate_r)) => {
                distance = candidate_distance;
                q = candidate_q;
                r = candidate_r;
            }
            None => break,
        }
    }

    assert!(
        in_domain(n, q, r),
        "normalization left ({q}, {r}) outside the canonical domain of radius {n}"
    );
    (q, r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Cube form of the pinned direction table, `(q, r, -q - r)`.
    fn direction_cube(direction: usize) -> (i64, i64, i64) {
        let (q, r) = DIRECTIONS[direction];
        (i64::from(q), i64::from(r), -i64::from(q) - i64::from(r))
    }

    #[test]
    fn direction_table_matches_the_pinned_vectors() {
        // DESIGN.md appendix A, transcribed independently of the source table.
        let expected: [(i64, i64, i64); 6] = [
            (1, 0, -1),
            (1, -1, 0),
            (0, -1, 1),
            (-1, 0, 1),
            (-1, 1, 0),
            (0, 1, -1),
        ];
        for (d, want) in expected.iter().enumerate() {
            assert_eq!(direction_cube(d), *want, "direction {d}");
        }
    }

    #[test]
    fn one_step_advances_the_direction_index() {
        for d in 0..DIRECTION_COUNT {
            assert_eq!(
                rotate_once(direction_cube(d)),
                direction_cube((d + 1) % 6),
                "direction {d}"
            );
        }
    }

    #[test]
    fn six_rotations_are_the_identity() {
        for d in 0..DIRECTION_COUNT {
            let mut v = direction_cube(d);
            for _ in 0..6 {
                v = rotate_once(v);
            }
            assert_eq!(v, direction_cube(d));
        }
        // And on a value that is not a direction vector.
        let mut v = (12_345, -7, -12_338);
        for _ in 0..6 {
            v = rotate_once(v);
        }
        assert_eq!(v, (12_345, -7, -12_338));
    }

    #[test]
    fn the_inverse_step_undoes_one_step_in_index_order() {
        // Named for the index rather than for a rotation sense: the compass
        // words belong to a viewer, and this crate has none. Appendix A,
        // *Rotation senses*.
        let back = |(x, y, z): (i64, i64, i64)| (-y, -z, -x);
        for d in 0..DIRECTION_COUNT {
            assert_eq!(back(rotate_once(direction_cube(d))), direction_cube(d));
        }
    }

    #[test]
    fn direction_index_normalizes_negative_and_large_inputs() {
        // DESIGN.md appendix A table.
        assert_eq!(direction_index(7), 1);
        assert_eq!(direction_index(6), 0);
        assert_eq!(direction_index(-1), 5);
        assert_eq!(direction_index(-2), 4);
        assert_eq!(direction_index(-6), 0);
        for d in -24..24 {
            assert_eq!(direction_index(d), direction_index(d + 6));
            assert!(direction_index(d) < DIRECTION_COUNT);
        }
        assert!(direction_index(i32::MIN) < DIRECTION_COUNT);
        assert!(direction_index(i32::MAX) < DIRECTION_COUNT);
    }

    #[test]
    fn mirror_centers_close_after_six_rotations_and_sum_to_zero() {
        for n in [1_i64, 2, 5, 17, WORLD_RADIUS] {
            let centers = mirror_centers(n);
            for (i, c) in centers.iter().enumerate() {
                assert_eq!(
                    c.0 + c.1 + c.2,
                    0,
                    "center {i} of radius {n} does not sum to zero"
                );
            }
            let mut v = centers[0];
            for (i, center) in centers.iter().enumerate() {
                assert_eq!(v, *center, "rotation {i} of radius {n}");
                v = rotate_once(v);
            }
            assert_eq!(v, centers[0], "six rotations must close");
        }
    }

    #[test]
    fn mirror_centers_for_this_world_are_the_expected_triples() {
        // Derived by hand from (2N+1, -N, -N-1) with N = 32767, rotating with
        // (x, y, z) -> (-z, -x, -y). Two centers sit at +-65535, outside
        // Component range.
        assert_eq!(
            MIRROR_CENTERS,
            [
                (65_535, -32_767, -32_768),
                (32_768, -65_535, 32_767),
                (-32_767, -32_768, 65_535),
                (-65_535, 32_767, 32_768),
                (-32_768, 65_535, -32_767),
                (32_767, 32_768, -65_535),
            ]
        );
    }

    #[test]
    fn mirror_center_translations_are_the_identity_on_tile_identity() {
        for center in MIRROR_CENTERS {
            for (q, r) in [
                (0_i64, 0_i64),
                (1, 2),
                (-5, 3),
                (WORLD_RADIUS, 0),
                (0, -WORLD_RADIUS),
            ] {
                let direct = Coord::new(q, r);
                let translated = Coord::new(q + center.0, r + center.1);
                assert_eq!(direct, translated, "center {center:?} at ({q}, {r})");
            }
        }
    }

    /// Exhaustive check of the normalizer at a radius small enough to enumerate.
    #[test]
    fn normalization_is_exhaustively_correct_at_small_radius() {
        for n in 1..=6_i64 {
            let centers = mirror_centers(n);
            let count = 1 + 3 * n * (n + 1);

            // Every canonical coordinate is a fixed point.
            let mut canonical = Vec::new();
            for q in -n..=n {
                for r in -n..=n {
                    if in_domain(n, q, r) {
                        canonical.push((q, r));
                        assert_eq!(normalize(n, &centers, q, r), (q, r), "radius {n}");
                    }
                }
            }
            assert_eq!(
                i64::try_from(canonical.len()).unwrap(),
                count,
                "radius {n} tile count"
            );

            // Every coordinate in a box several copies wide normalizes into the
            // domain, is idempotent, and agrees with its mirror translations.
            let span = 4 * n + 3;
            let mut classes: HashSet<(i64, i64)> = HashSet::new();
            for q in -span..=span {
                for r in -span..=span {
                    let (nq, nr) = normalize(n, &centers, q, r);
                    assert!(
                        in_domain(n, nq, nr),
                        "radius {n}: ({q}, {r}) -> ({nq}, {nr})"
                    );
                    assert_eq!(normalize(n, &centers, nq, nr), (nq, nr), "idempotence");
                    classes.insert((nq, nr));
                    for center in centers {
                        assert_eq!(
                            normalize(n, &centers, q + center.0, r + center.1),
                            (nq, nr),
                            "radius {n}: ({q}, {r}) + {center:?}"
                        );
                    }
                }
            }
            // The box covers every class at least once.
            assert_eq!(
                i64::try_from(classes.len()).unwrap(),
                count,
                "radius {n} coverage"
            );
        }
    }

    #[test]
    fn normalization_is_exhaustive_around_zero() {
        // Section 24: the floorDiv/floorMod helpers are gone, the boundary cases
        // are not.
        for q in -3..=3_i64 {
            for r in -3..=3_i64 {
                let c = Coord::new(q, r);
                assert_eq!((i64::from(c.q()), i64::from(c.r())), (q, r), "({q}, {r})");
                assert_eq!(i64::from(c.s()), -q - r);
            }
        }
    }

    #[test]
    fn canonical_coordinates_survive_new_unchanged() {
        for (q, r) in [
            (0_i64, 0_i64),
            (WORLD_RADIUS, 0),
            (0, WORLD_RADIUS),
            (-WORLD_RADIUS, 0),
            (0, -WORLD_RADIUS),
            (WORLD_RADIUS, -WORLD_RADIUS),
            (-WORLD_RADIUS, WORLD_RADIUS),
            (-1, -WORLD_RADIUS + 1),
            (12_345, -7),
        ] {
            let c = Coord::new(q, r);
            assert_eq!(i64::from(c.q()), q, "({q}, {r})");
            assert_eq!(i64::from(c.r()), r, "({q}, {r})");
            assert_eq!(i64::from(c.s()), -q - r);
        }
    }

    #[test]
    fn every_wrapped_edge_lands_on_the_correct_opposite_tile() {
        // Step one tile past each of the six edges of the canonical hexagon and
        // confirm the result is the mirror translation of that step, which is
        // the tile on the far side of the world.
        let n = WORLD_RADIUS;
        for (d, (dq, dr)) in DIRECTIONS.iter().enumerate() {
            let direction = i32::try_from(d).unwrap();
            // A coordinate on the edge that direction d leaves through: rotate
            // the +q edge midpoint into place.
            let edge = Coord::new(n, 0).rotate(direction);
            let stepped = edge.neighbor(direction);
            let raw_q = i64::from(edge.q()) + i64::from(*dq);
            let raw_r = i64::from(edge.r()) + i64::from(*dr);
            assert!(
                !in_domain(n, raw_q, raw_r),
                "direction {d} should leave the domain"
            );

            // The wrapped result differs from the raw step by exactly one
            // mirror center.
            let matched = MIRROR_CENTERS.iter().any(|center| {
                raw_q - center.0 == i64::from(stepped.q())
                    && raw_r - center.1 == i64::from(stepped.r())
            });
            assert!(matched, "direction {d}: ({raw_q}, {raw_r}) -> {stepped:?}");

            // And stepping back returns to the edge tile.
            assert_eq!(stepped.neighbor(direction + 3), edge, "direction {d}");
        }
    }

    #[test]
    fn a_straight_walk_stays_in_the_domain_and_closes_on_itself() {
        // At radius 3 the world holds 37 tiles and 37 is prime, so a straight
        // walk in any direction is a single cycle through every tile: it must
        // stay canonical at every step and return to its start after exactly 37.
        let n = 3_i64;
        let centers = mirror_centers(n);
        let count = 1 + 3 * n * (n + 1);
        for (d, (dq, dr)) in DIRECTIONS.iter().enumerate() {
            let (dq, dr) = (i64::from(*dq), i64::from(*dr));
            let start = (1_i64, -2_i64);
            let mut at = start;
            let mut visited = HashSet::new();
            for step in 1..=count {
                at = normalize(n, &centers, at.0 + dq, at.1 + dr);
                assert!(in_domain(n, at.0, at.1), "direction {d} step {step}");
                if step < count {
                    assert_ne!(at, start, "direction {d} closed early at step {step}");
                    assert!(
                        visited.insert(at),
                        "direction {d} repeated a tile at step {step}"
                    );
                }
            }
            assert_eq!(at, start, "direction {d} must close after {count} steps");
        }
    }

    #[test]
    fn out_of_domain_input_is_idempotent_and_extreme_input_is_total() {
        for (q, r) in [
            (WORLD_RADIUS + 1, 0),
            (0, WORLD_RADIUS + 1),
            (WORLD_RADIUS, WORLD_RADIUS),
            (-WORLD_RADIUS - 1, -WORLD_RADIUS - 1),
            (i64::MAX, 0),
            (0, i64::MAX),
            (i64::MIN, 0),
            (0, i64::MIN),
            (i64::MAX, i64::MIN),
            (i64::MIN, i64::MAX),
            (i64::MAX, i64::MAX),
            (i64::MIN, i64::MIN),
            (1 << 40, -(1 << 41)),
        ] {
            let c = Coord::new(q, r);
            assert!(
                in_domain(WORLD_RADIUS, i64::from(c.q()), i64::from(c.r())),
                "({q}, {r})"
            );
            assert_eq!(
                Coord::new(i64::from(c.q()), i64::from(c.r())),
                c,
                "({q}, {r})"
            );
        }
    }

    /// An independent normalizer: search the wraparound lattice directly for the
    /// representative that lands in the domain. Correct only for inputs within a
    /// few world widths, which is what makes it a check on the `i128` lattice
    /// solve and the greedy walk rather than a restatement of them.
    fn reference_normalize(n: i64, q: i64, r: i64) -> (i64, i64) {
        let centers = mirror_centers(n);
        let (m0q, m0r) = (centers[0].0, centers[0].1);
        let (m1q, m1r) = (centers[1].0, centers[1].1);
        for a in -8..=8_i64 {
            for b in -8..=8_i64 {
                let candidate_q = q - a * m0q - b * m1q;
                let candidate_r = r - a * m0r - b * m1r;
                if in_domain(n, candidate_q, candidate_r) {
                    return (candidate_q, candidate_r);
                }
            }
        }
        panic!("reference normalizer found no representative for ({q}, {r})");
    }

    #[test]
    fn normalization_agrees_with_a_direct_lattice_search() {
        let n = WORLD_RADIUS;
        let span = 4 * (2 * n + 1);

        // Deterministic test coordinates. The generator's own mixer stands in for
        // a PRNG here; the `rand` crate is for tooling, never for anything that
        // has to reproduce.
        let mut cases: Vec<(i64, i64)> = Vec::new();
        for i in 0..2_000_i64 {
            let hq = crate::hash2(0x5eed_0001, crate::DOM_BASIN, i, 0);
            let hr = crate::hash2(0x5eed_0002, crate::DOM_BASIN, i, 1);
            cases.push((
                hq.cast_signed().rem_euclid(2 * span) - span,
                hr.cast_signed().rem_euclid(2 * span) - span,
            ));
        }
        // Plus exact lattice points and points just off them, where a rounding
        // mistake in the solve would show.
        for center in MIRROR_CENTERS {
            for multiple in -3..=3_i64 {
                for (dq, dr) in [(0_i64, 0_i64), (1, 0), (-1, 0), (0, 1), (0, -1), (n, -n)] {
                    cases.push((center.0 * multiple + dq, center.1 * multiple + dr));
                }
            }
        }

        for (q, r) in cases {
            let c = Coord::new(q, r);
            let expected = reference_normalize(n, q, r);
            assert_eq!((i64::from(c.q()), i64::from(c.r())), expected, "({q}, {r})");
        }
    }

    #[test]
    fn values_that_normalize_together_are_equal_and_hash_equally() {
        // A `HashSet` lookup that finds an existing entry has already proved the
        // two values hash to the same bucket and compare equal. No explicit
        // hasher is constructed here: see DESIGN.md section 25.4.
        for center in MIRROR_CENTERS {
            let a = Coord::new(17, -4);
            let b = Coord::new(17 + center.0, -4 + center.1);
            assert_eq!(a, b, "center {center:?}");
            assert_eq!(a.cmp(&b), std::cmp::Ordering::Equal, "center {center:?}");

            let mut set = HashSet::new();
            assert!(set.insert(a));
            assert!(
                !set.insert(b),
                "center {center:?} inserted a duplicate tile"
            );
            assert_eq!(set.len(), 1);
        }
    }

    #[test]
    fn neighbors_are_reciprocal_and_distinct() {
        for (q, r) in [
            (0_i64, 0_i64),
            (WORLD_RADIUS, 0),
            (-5, WORLD_RADIUS),
            (-12_000, 9_000),
        ] {
            let c = Coord::new(q, r);
            let mut seen = HashSet::new();
            for d in 0..6 {
                let n = c.neighbor(d);
                assert_ne!(n, c, "({q}, {r}) direction {d}");
                assert!(
                    seen.insert(n),
                    "({q}, {r}) direction {d} repeated a neighbor"
                );
                // Opposite direction returns.
                assert_eq!(n.neighbor(d + 3), c, "({q}, {r}) direction {d}");
            }
        }
    }

    #[test]
    fn neighbor_accepts_any_direction_by_wrapping() {
        let c = Coord::new(4, 4);
        for d in -13..13 {
            assert_eq!(c.neighbor(d), c.neighbor(d + 6), "direction {d}");
        }
        assert_eq!(c.neighbor(-1), c.neighbor(5));
    }

    #[test]
    fn rotation_maps_the_domain_onto_itself_and_commutes_with_normalization() {
        for (q, r) in [
            (0_i64, 0_i64),
            (1, 0),
            (WORLD_RADIUS, 0),
            (-WORLD_RADIUS, WORLD_RADIUS),
            (12_345, -30_000),
            (WORLD_RADIUS + 5, -3),
            (1 << 40, -(1 << 41)),
        ] {
            for k in 0..6 {
                // Rotating the raw pair then normalizing must agree with
                // normalizing then rotating.
                let mut rotated = (q, r, -q - r);
                for _ in 0..k {
                    rotated = rotate_once(rotated);
                }
                let rotate_first = Coord::new(rotated.0, rotated.1);
                let normalize_first = Coord::new(q, r).rotate(k);
                assert_eq!(rotate_first, normalize_first, "({q}, {r}) rotated {k}");
            }
            // Six rotations are the identity on Coord too.
            assert_eq!(Coord::new(q, r).rotate(6), Coord::new(q, r));
            assert_eq!(Coord::new(q, r).rotate(-1).rotate(1), Coord::new(q, r));
        }
    }

    #[test]
    fn rotation_permutes_directions_by_index() {
        // The player-frame rule: absolute_direction = (player_direction + k) % 6.
        for k in 0..6 {
            for d in 0..6 {
                let rotated = Coord::ORIGIN.neighbor(d).rotate(k);
                assert_eq!(
                    rotated,
                    Coord::ORIGIN.neighbor(d + k),
                    "direction {d} rotated {k}"
                );
            }
        }
    }

    #[test]
    fn cell_addressing_is_correct_across_the_origin() {
        // DESIGN.md section 23, with chunk size 32.
        let size = crate::DEFAULT_CHUNK_SIZE_HEXES;
        for (q, expected) in [
            (0_i64, 0_i64),
            (31, 0),
            (32, 1),
            (-1, -1),
            (-32, -1),
            (-33, -2),
        ] {
            assert_eq!(Coord::new(q, 0).cell(size).0, expected, "q = {q}");
        }
        for (r, expected) in [
            (0_i64, 0_i64),
            (31, 0),
            (32, 1),
            (-1, -1),
            (-32, -1),
            (-33, -2),
        ] {
            assert_eq!(Coord::new(0, r).cell(size).1, expected, "r = {r}");
        }
    }

    #[test]
    fn cell_offsets_are_non_negative_and_reconstruct_the_coordinate() {
        for size in [1_u32, 2, 32, 128, 512] {
            let divisor = i64::from(size);
            for q in -600..=600_i64 {
                let c = Coord::new(q, -q / 2);
                let (cell_q, cell_r) = c.cell(size);
                let (off_q, off_r) = c.cell_offset(size);
                assert!((0..divisor).contains(&off_q), "size {size} q {q}");
                assert!((0..divisor).contains(&off_r), "size {size} q {q}");
                assert_eq!(
                    cell_q * divisor + off_q,
                    i64::from(c.q()),
                    "size {size} q {q}"
                );
                assert_eq!(
                    cell_r * divisor + off_r,
                    i64::from(c.r()),
                    "size {size} q {q}"
                );
            }
        }
    }

    #[test]
    fn cell_sizes_nest() {
        // A region cell is determined by the chunk cell it contains, because
        // every size divides the next.
        let chunk = crate::DEFAULT_CHUNK_SIZE_HEXES;
        let region = crate::DEFAULT_REGION_SIZE_HEXES;
        let macro_region = crate::DEFAULT_MACRO_REGION_SIZE_HEXES;
        assert_eq!(region % chunk, 0);
        assert_eq!(macro_region % region, 0);

        for q in -300..=300_i64 {
            let c = Coord::new(q, 3 - q);
            let per_chunk = i64::from(region / chunk);
            assert_eq!(
                c.cell(region).0,
                c.cell(chunk).0.div_euclid(per_chunk),
                "q = {q}"
            );
            assert_eq!(
                c.cell(region).1,
                c.cell(chunk).1.div_euclid(per_chunk),
                "q = {q}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "cell size must be positive")]
    fn cell_rejects_a_zero_size() {
        let _ = Coord::ORIGIN.cell(0);
    }

    #[test]
    fn axial_to_world_places_the_origin_at_zero() {
        let v = axial_to_world(Coord::ORIGIN);
        assert_eq!(v, Vec2 { x: 0.0, y: 0.0 });
    }

    #[test]
    fn adjacent_hex_centers_are_six_miles_apart() {
        let center = axial_to_world(Coord::ORIGIN);
        for d in 0..6 {
            let neighbor = axial_to_world(Coord::ORIGIN.neighbor(d));
            let dx = neighbor.x - center.x;
            let dy = neighbor.y - center.y;
            let distance = (dx * dx + dy * dy).sqrt();
            assert!(
                (distance - 2.0 * APOTHEM_MILES).abs() < 1e-9,
                "direction {d} distance {distance}"
            );
        }
    }

    #[test]
    fn axial_to_world_is_exact_on_the_q_axis() {
        // x = 6 * q with r = 0, and every term is exact, so this is bit-exact.
        for q in [-32_767_i64, -1, 0, 1, 32_767] {
            let v = axial_to_world(Coord::new(q, 0));
            #[expect(clippy::cast_precision_loss, reason = "|q| <= 32767 is exact in f64")]
            let expected = 6.0 * q as f64;
            assert_eq!(v.x, expected);
            assert_eq!(v.y, 0.0);
        }
    }

    #[test]
    fn axial_to_world_is_deterministic() {
        let c = Coord::new(-12_345, 6_789);
        assert_eq!(axial_to_world(c), axial_to_world(c));
    }
}
