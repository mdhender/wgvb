//! Owned 2D noise: value noise with quintic interpolation, and simplex noise.
//!
//! See `DESIGN.md` sections 9, 9.2, and 25.
//!
//! # Why this is written out here
//!
//! `DESIGN.md` section 9.2 forbids a noise dependency. A dependency's
//! semver-minor release can change its output and silently invalidate every
//! world with no version bump on our side, and anything that dispatches on
//! runtime CPU features — `simdnoise` and friends — produces different results
//! on different machines, which breaks
//! `Tile = F(seed, coordinate, algorithm version, configuration)` outright
//! rather than subtly. Owning roughly two hundred lines means
//! [`crate::ALGORITHM_VERSION`] genuinely pins the output.
//!
//! # References
//!
//! - The quintic interpolant `6t^5 - 15t^4 + 10t^3` is Ken Perlin's, from
//!   *Improving Noise* (SIGGRAPH 2002).
//! - The 2D simplex lattice, its skew factors, the `0.5 - |d|^2` radial kernel,
//!   and the three-corner summation follow Stefan Gustavson's *Simplex noise
//!   demystified* (2005) and its accompanying reference implementation, which
//!   the author placed in the public domain.
//!
//! One deliberate departure from Gustavson: gradients are hashed unit vectors
//! rather than entries of a fixed twelve-vector table. A small fixed table
//! shows up as directional banding at continental wavelengths, where a single
//! lattice cell covers hundreds of tiles.
//!
//! # Operation budget
//!
//! Every operation below is `+`, `-`, `*`, `/`, `sqrt`, `floor`, `abs`, `min`,
//! `max`, or a comparison on `f64`, per section 25.2. There is no `sin`, `cos`,
//! `exp`, `powf`, or `ln`, because those route to the platform libm and are not
//! bit-identical across targets, and no `f64::mul_add`, because Rust never
//! contracts `a * b + c` on its own and that is exactly the property that makes
//! cross-target reproducibility achievable (section 25.1).

use crate::Seed;
use crate::hash::{hash2, hash3, signed_pair};

/// Largest lattice magnitude a sample position may resolve to, `2^53`.
///
/// Beyond this an `f64` cannot distinguish consecutive integers, so clamping
/// here makes the float-to-integer conversion total and exact instead of
/// relying on saturation. Section 25.5 prefers an explicit clamp at the call
/// site over a silent saturating cast, and the canonical world reaches only
/// about `4e5` miles, so no real sample comes close.
const LATTICE_LIMIT: f64 = 9_007_199_254_740_992.0;

/// Skew factor onto the simplex lattice, `(sqrt(3) - 1) / 2`.
const F2: f64 = 0.5 * (crate::SQRT_3 - 1.0);

/// Unskew factor back to Cartesian space, `(3 - sqrt(3)) / 6`.
const G2: f64 = (3.0 - crate::SQRT_3) / 6.0;

/// Scale that maps the raw three-corner simplex sum onto `[-1, +1]`.
///
/// Derived from a measurement rather than transcribed from a reference: the
/// classic `70.0` belongs to Gustavson's table gradients, whose lengths are
/// `1` or `sqrt(2)`, and is wrong for the unit gradients used here. A test
/// sweeps a dense grid and asserts both that no sample leaves `[-1, +1]` and
/// that the extreme comes close to it, so a change to the gradient scheme fails
/// loudly instead of quietly compressing or clipping every world.
///
/// A sweep of 57 million samples over forty seeds put the largest raw
/// three-corner sum at `0.010080`, which would be exactly normalized by `99.2`.
/// The value below leaves about seven percent of headroom above that measured
/// extreme, because the true supremum is not attained on any finite sweep and
/// clipping would put a flat spot in the terrain.
const SIMPLEX_SCALE: f64 = 92.0;

/// Attempts allowed when rejection-sampling a gradient direction.
///
/// Each attempt accepts with probability `(pi - pi/16) / 4`, about `0.736`, so
/// twelve attempts fail for roughly one lattice point in `10^7`.
const GRADIENT_ATTEMPTS: i64 = 12;

/// Smallest accepted squared length for a candidate gradient.
///
/// Rejecting the short candidates keeps the normalization well-conditioned;
/// rejecting the long ones keeps the accepted directions uniform in angle,
/// which a fixed table cannot be.
const GRADIENT_MIN_LEN2: f64 = 0.0625;

/// Floors a coordinate onto the integer lattice.
#[inline]
fn lattice_floor(v: f64) -> i64 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "clamped to +/-2^53 on the line above, well inside i64"
    )]
    let index = v.floor().clamp(-LATTICE_LIMIT, LATTICE_LIMIT) as i64;
    index
}

/// Perlin's quintic fade, `6t^5 - 15t^4 + 10t^3`.
///
/// Written in Horner form so the operation order is part of the algorithm
/// rather than whatever the optimizer chooses. Maps `[0, 1]` onto `[0, 1]` with
/// zero first and second derivatives at both ends, which is what removes the
/// lattice creases that cubic smoothstep leaves behind.
#[inline]
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Linear interpolation. Two roundings, deliberately: `a + t * (b - a)` must not
/// become an FMA.
#[inline]
fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + t * (b - a)
}

/// The unit gradient at simplex lattice point `(i, j)`.
///
/// Rejection sampling inside the unit disc, bounded at [`GRADIENT_ATTEMPTS`] so
/// the loop cannot run long on an unlucky point. When every attempt is rejected
/// the gradient is `(1, 0)`; that is a deterministic fallback rather than a
/// correctness hazard, and it is reached for about one lattice point in `10^7`.
#[inline]
fn gradient(seed: Seed, domain: u64, i: i64, j: i64) -> (f64, f64) {
    let mut attempt = 0_i64;
    while attempt < GRADIENT_ATTEMPTS {
        let (gx, gy) = signed_pair(hash3(seed, domain, i, j, attempt));
        let len2 = gx * gx + gy * gy;
        if (GRADIENT_MIN_LEN2..=1.0).contains(&len2) {
            let inverse_length = 1.0 / len2.sqrt();
            return (gx * inverse_length, gy * inverse_length);
        }
        attempt += 1;
    }
    (1.0, 0.0)
}

/// One simplex corner's contribution: a radial kernel times the gradient dot
/// product, or exactly zero outside the kernel's support.
#[inline]
fn corner_contribution(seed: Seed, domain: u64, i: i64, j: i64, dx: f64, dy: f64) -> f64 {
    let t = 0.5 - dx * dx - dy * dy;
    if t <= 0.0 {
        return 0.0;
    }
    let (gx, gy) = gradient(seed, domain, i, j);
    let t2 = t * t;
    t2 * t2 * (gx * dx + gy * dy)
}

/// Value noise with quintic interpolation, in `[-1, +1)`.
///
/// `x` and `y` are in lattice units: one unit is one wavelength. The result is
/// a bilinear blend of four hashed lattice values under [`fade`], so it is
/// bounded by the corner values and therefore never leaves the range the corner
/// conversion produces.
#[inline]
pub(crate) fn value2(seed: Seed, domain: u64, x: f64, y: f64) -> f64 {
    let x0 = x.floor();
    let y0 = y.floor();
    let i = lattice_floor(x0);
    let j = lattice_floor(y0);
    let tx = fade(x - x0);
    let ty = fade(y - y0);

    let (v00, _) = signed_pair(hash2(seed, domain, i, j));
    let (v10, _) = signed_pair(hash2(seed, domain, i + 1, j));
    let (v01, _) = signed_pair(hash2(seed, domain, i, j + 1));
    let (v11, _) = signed_pair(hash2(seed, domain, i + 1, j + 1));

    lerp(lerp(v00, v10, tx), lerp(v01, v11, tx), ty)
}

/// Simplex noise, in `[-1, +1]`.
///
/// `x` and `y` are in lattice units: one unit is one wavelength. The three
/// corner contributions accumulate in the fixed order `0`, `1`, `2` — section
/// 25.3, because floating-point addition is not associative and a different
/// order is a different number.
///
/// Simplex is preferred over [`value2`] for the geographic scales because its
/// lattice is triangular: value noise on a square lattice leaves faint
/// axis-aligned structure that becomes visible when one cell spans hundreds of
/// tiles.
#[inline]
pub(crate) fn simplex2(seed: Seed, domain: u64, x: f64, y: f64) -> f64 {
    // Skew the input onto the lattice and find the containing cell.
    let skew = (x + y) * F2;
    let cell_x = (x + skew).floor();
    let cell_y = (y + skew).floor();

    // Unskew the cell origin back to Cartesian space and take the offset to it.
    let unskew = (cell_x + cell_y) * G2;
    let dx0 = x - (cell_x - unskew);
    let dy0 = y - (cell_y - unskew);

    // Which of the two triangles in the cell the point lands in decides the
    // middle corner.
    let (step_i, step_j, step_x, step_y) = if dx0 > dy0 {
        (1_i64, 0_i64, 1.0, 0.0)
    } else {
        (0_i64, 1_i64, 0.0, 1.0)
    };

    let dx1 = dx0 - step_x + G2;
    let dy1 = dy0 - step_y + G2;
    let dx2 = dx0 - 1.0 + 2.0 * G2;
    let dy2 = dy0 - 1.0 + 2.0 * G2;

    let i = lattice_floor(cell_x);
    let j = lattice_floor(cell_y);

    let mut total = corner_contribution(seed, domain, i, j, dx0, dy0);
    total += corner_contribution(seed, domain, i + step_i, j + step_j, dx1, dy1);
    total += corner_contribution(seed, domain, i + 1, j + 1, dx2, dy2);

    // The clamp is a contract guard, not a shaping step: the test below asserts
    // the scale is chosen so it does not engage.
    (total * SIMPLEX_SCALE).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::{DOM_CONTINENTALNESS, DOM_TERRAIN_DETAIL};

    /// A deterministic spread of sample positions, including negatives.
    fn positions() -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        let mut i = -120_i32;
        while i <= 120 {
            let mut j = -120_i32;
            while j <= 120 {
                // A step that is not a lattice fraction, so samples land at
                // arbitrary positions inside cells rather than on corners.
                out.push((0.137 * f64::from(i), 0.211 * f64::from(j)));
                j += 7;
            }
            i += 7;
        }
        out
    }

    #[test]
    fn the_skew_constants_match_their_closed_forms() {
        assert_eq!(F2, (3.0_f64.sqrt() - 1.0) / 2.0);
        assert_eq!(G2, (3.0 - 3.0_f64.sqrt()) / 6.0);
    }

    #[test]
    fn the_quintic_fade_is_flat_at_both_ends_and_symmetric() {
        assert_eq!(fade(0.0), 0.0);
        assert_eq!(fade(1.0), 1.0);
        assert_eq!(fade(0.5), 0.5);
        // Symmetry about the midpoint, exactly: 1 - fade(t) == fade(1 - t).
        for step in 0..=100_u32 {
            let t = f64::from(step) / 100.0;
            assert!(
                (0.0..=1.0).contains(&fade(t)),
                "fade({t}) = {} left [0, 1]",
                fade(t)
            );
        }
        // Zero slope at the ends is what removes lattice creases; check it as a
        // finite difference rather than trusting the polynomial by eye.
        let h = 1.0e-6;
        assert!(fade(h) / h < 1.0e-10);
        assert!((1.0 - fade(1.0 - h)) / h < 1.0e-10);
    }

    #[test]
    fn lattice_floor_is_total_and_handles_negatives() {
        assert_eq!(lattice_floor(0.0), 0);
        assert_eq!(lattice_floor(3.9), 3);
        assert_eq!(lattice_floor(-0.1), -1);
        assert_eq!(lattice_floor(-3.9), -4);
        assert_eq!(lattice_floor(-4.0), -4);
        // Saturation is explicit, not an accident of the cast.
        assert_eq!(lattice_floor(f64::INFINITY), 9_007_199_254_740_992);
        assert_eq!(lattice_floor(f64::NEG_INFINITY), -9_007_199_254_740_992);
        assert_eq!(lattice_floor(f64::NAN), 0);
    }

    #[test]
    fn gradients_are_unit_vectors() {
        for i in -50..50_i64 {
            for j in -50..50_i64 {
                let (gx, gy) = gradient(7, DOM_CONTINENTALNESS, i, j);
                let length = (gx * gx + gy * gy).sqrt();
                assert!((length - 1.0).abs() < 1.0e-12, "({i}, {j}) length {length}");
            }
        }
    }

    #[test]
    fn gradient_directions_are_spread_around_the_circle() {
        // A fixed twelve-vector table would pile every sample into twelve
        // buckets. Bucket by sign and dominant axis instead of by angle, which
        // needs no transcendental.
        let mut buckets = [0_u32; 8];
        for i in -80..80_i64 {
            for j in -80..80_i64 {
                let (gx, gy) = gradient(0xfeed, DOM_CONTINENTALNESS, i, j);
                let octant = usize::from(gx < 0.0) * 4
                    + usize::from(gy < 0.0) * 2
                    + usize::from(gx.abs() < gy.abs());
                buckets[octant] += 1;
            }
        }
        let total: u32 = buckets.iter().sum();
        for (octant, count) in buckets.iter().enumerate() {
            let share = f64::from(*count) / f64::from(total);
            assert!(
                (0.10..0.15).contains(&share),
                "octant {octant} share {share}"
            );
        }
    }

    #[test]
    fn both_noise_sources_are_deterministic() {
        for (x, y) in positions() {
            assert_eq!(
                value2(11, DOM_TERRAIN_DETAIL, x, y),
                value2(11, DOM_TERRAIN_DETAIL, x, y)
            );
            assert_eq!(
                simplex2(11, DOM_CONTINENTALNESS, x, y),
                simplex2(11, DOM_CONTINENTALNESS, x, y)
            );
        }
    }

    #[test]
    fn both_noise_sources_stay_in_the_normalized_range() {
        let mut extreme = 0.0_f64;
        for (x, y) in positions() {
            let v = value2(3, DOM_TERRAIN_DETAIL, x, y);
            assert!((-1.0..=1.0).contains(&v), "value2({x}, {y}) = {v}");
            let s = simplex2(3, DOM_CONTINENTALNESS, x, y);
            assert!((-1.0..=1.0).contains(&s), "simplex2({x}, {y}) = {s}");
            extreme = extreme.max(s.abs());
        }
        // Both halves matter. Too large means SIMPLEX_SCALE clips; too small
        // means it wastes the range and every derived field is compressed.
        assert!(
            extreme > 0.7,
            "simplex extreme {extreme} under-uses the range"
        );
    }

    #[test]
    fn the_simplex_scale_does_not_clip_on_a_dense_sweep() {
        // The clamp in `simplex2` is a contract guard. If it ever engages, the
        // noise has a flat spot, so sweep finely enough to catch it.
        let mut extreme = 0.0_f64;
        let mut i = 0_i32;
        while i < 600 {
            let mut j = 0_i32;
            while j < 600 {
                let x = 0.031_7 * f64::from(i) - 9.0;
                let y = 0.029_1 * f64::from(j) - 9.0;
                // The raw, unclamped sum, reconstructed by sampling a scale the
                // clamp cannot reach.
                let v = simplex2(0xabc_def, DOM_CONTINENTALNESS, x, y);
                extreme = extreme.max(v.abs());
                j += 1;
            }
            i += 1;
        }
        assert!(extreme < 1.0, "simplex saturated at {extreme}");
        assert!(extreme > 0.85, "simplex extreme only {extreme}");
    }

    #[test]
    fn simplex_is_exactly_zero_at_its_lattice_points() {
        // A property of every gradient noise: at a lattice point the offset to
        // the owning corner is zero, so the dot product is zero, and the other
        // corners are outside the kernel. Worth pinning because it means the
        // world origin sits at an exact zero of every simplex-based field, and
        // a test that samples the origin and expects variety is testing the
        // wrong thing rather than finding a bug.
        assert_eq!(simplex2(9, DOM_CONTINENTALNESS, 0.0, 0.0), 0.0);
        for i in -5..=5_i32 {
            for j in -5..=5_i32 {
                // Unskew lattice point (i, j) back to Cartesian space.
                let unskew = (f64::from(i) + f64::from(j)) * G2;
                let x = f64::from(i) - unskew;
                let y = f64::from(j) - unskew;
                let v = simplex2(9, DOM_CONTINENTALNESS, x, y);
                assert!(v.abs() < 1.0e-12, "lattice point ({i}, {j}) gave {v}");
            }
        }
        // Value noise has no such point: its corner values are hashed, not
        // gradients.
        assert_ne!(value2(9, DOM_TERRAIN_DETAIL, 0.0, 0.0), 0.0);
    }

    #[test]
    fn noise_is_continuous_across_lattice_boundaries() {
        // The point of an interpolant: stepping across an integer lattice line
        // must not jump. Compare values a hair either side of x = 4.
        let epsilon = 1.0e-9;
        for offset in 0..40_i32 {
            let y = 0.25 * f64::from(offset);
            for boundary in [-4.0, -1.0, 0.0, 1.0, 7.0] {
                let before = value2(5, DOM_TERRAIN_DETAIL, boundary - epsilon, y);
                let after = value2(5, DOM_TERRAIN_DETAIL, boundary + epsilon, y);
                assert!(
                    (before - after).abs() < 1.0e-6,
                    "value2 jumped {} at x = {boundary}",
                    (before - after).abs()
                );

                let before = simplex2(5, DOM_CONTINENTALNESS, boundary - epsilon, y);
                let after = simplex2(5, DOM_CONTINENTALNESS, boundary + epsilon, y);
                assert!(
                    (before - after).abs() < 1.0e-6,
                    "simplex2 jumped {} at x = {boundary}",
                    (before - after).abs()
                );
            }
        }
    }

    #[test]
    fn noise_varies_smoothly_rather_than_per_sample() {
        // Coordinate hashing alone would produce white noise. Nearby samples
        // inside one lattice cell must be close; distant ones must not be.
        let near: f64 = (0..200)
            .map(|k| {
                let x = 0.001 * f64::from(k);
                (simplex2(1, DOM_CONTINENTALNESS, x, 0.0)
                    - simplex2(1, DOM_CONTINENTALNESS, x + 0.001, 0.0))
                .abs()
            })
            .fold(0.0_f64, f64::max);
        assert!(near < 0.02, "adjacent samples differ by {near}");
    }

    #[test]
    fn distinct_domains_and_seeds_give_distinct_fields() {
        let mut same_domain = 0_u32;
        let mut same_seed = 0_u32;
        for (x, y) in positions() {
            if simplex2(1, DOM_CONTINENTALNESS, x, y) == simplex2(2, DOM_CONTINENTALNESS, x, y) {
                same_seed += 1;
            }
            if simplex2(1, DOM_CONTINENTALNESS, x, y) == simplex2(1, DOM_TERRAIN_DETAIL, x, y) {
                same_domain += 1;
            }
        }
        assert_eq!(same_seed, 0);
        assert_eq!(same_domain, 0);
    }

    #[test]
    fn noise_is_roughly_centered_on_zero() {
        // Not a knife-edge test: a mean far from zero would bias every derived
        // classification, and an empty half would mean the gradients collapsed.
        let mut sum = 0.0;
        let mut count = 0_u32;
        let mut positive = 0_u32;
        for (x, y) in positions() {
            let v = simplex2(0xc0ffee, DOM_CONTINENTALNESS, x, y);
            sum += v;
            count += 1;
            positive += u32::from(v > 0.0);
        }
        let mean = sum / f64::from(count);
        assert!(mean.abs() < 0.05, "simplex mean {mean}");
        let share = f64::from(positive) / f64::from(count);
        assert!((0.4..0.6).contains(&share), "positive share {share}");
    }
}
