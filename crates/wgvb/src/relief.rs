//! Local relief, estimated from the six neighboring elevations.
//!
//! See `DESIGN.md` sections 18, 25.3, and 30.7.
//!
//! # Why this module exists at all
//!
//! Section 18 forbids recursive terrain classification and names the exact trap:
//! calling `tile()` from inside `tile()`. Rust will not catch that — it compiles
//! and then overflows the stack at run time, or worse, terminates by luck.
//!
//! The structural answer is that relief is a function of *elevation scalars*,
//! not of tiles. This module cannot express the recursion: it takes seven `f64`
//! values and returns one. [`crate::Generator::tile`] and
//! [`crate::Generator::relief`] both reach it through the same private
//! elevation scalar, so there is no path by which a tile can ask for a tile.
//!
//! # Pipeline direction
//!
//! ```text
//! raw fields -> elevation scalar -> neighbor elevation samples -> derived relief
//! ```
//!
//! One direction only. Relief reads elevation; elevation never reads relief.

use crate::DIRECTION_COUNT;

/// Local relief from a center elevation and its six neighbors, in `[0, 1]`.
///
/// Zero is flat, one is as steep as [`reference_delta_per_hex`] says a slope can
/// usefully read. The estimate is the mean absolute elevation difference to the
/// six neighbors, which is a gentler statistic than the range: a single high
/// neighbor makes a tile somewhat steep rather than maximally steep, and a tile
/// on a uniform slope reads as sloped rather than as flat.
///
/// Relief is unsigned by construction. Which way the ground falls is a
/// direction, not a magnitude, and section 18 asks only whether a location is
/// flat, hilly, or steep.
///
/// # Accumulation order
///
/// `neighbors` is indexed by direction, and the loop runs `0..6` in that order.
/// Section 25.3: floating-point addition is not associative, so the direction
/// order is part of the result. The caller must fill the array by
/// [`crate::Coord::neighbor`] index for the value to mean what it says, and
/// nothing here can check that — which is why there is exactly one caller.
#[must_use]
pub(crate) fn from_neighbors(
    here: f64,
    neighbors: &[f64; DIRECTION_COUNT],
    reference_delta_per_hex: f64,
) -> f64 {
    let mut total = 0.0_f64;
    for neighbor in neighbors {
        total += (neighbor - here).abs();
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "DIRECTION_COUNT is 6; the conversion is exact"
    )]
    let count = DIRECTION_COUNT as f64;
    let mean = total / count;

    // Explicit clamp rather than a saturating cast or a `min` chain, so the
    // intended range is visible where it is produced. Section 25.5.
    (mean / reference_delta_per_hex).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCE: f64 = 0.02;

    #[test]
    fn flat_ground_has_no_relief() {
        for elevation in [-1.0, -0.25, 0.0, 0.5, 1.0] {
            assert_eq!(
                from_neighbors(elevation, &[elevation; DIRECTION_COUNT], REFERENCE),
                0.0,
                "{elevation}"
            );
        }
    }

    #[test]
    fn relief_is_the_mean_absolute_difference_over_the_reference() {
        // Derived independently: six neighbors at +0.01, -0.01, +0.02, -0.02,
        // 0.0, 0.0 from a center of 0.3 have a mean absolute difference of
        // 0.06 / 6 = 0.01, which is half the reference.
        let here = 0.3;
        let neighbors = [0.31, 0.29, 0.32, 0.28, 0.3, 0.3];
        assert!((from_neighbors(here, &neighbors, REFERENCE) - 0.5).abs() < 1.0e-12);
    }

    #[test]
    fn relief_is_unsigned() {
        // Uphill and downhill are the same amount of relief.
        let up = [0.31, 0.32, 0.33, 0.34, 0.35, 0.36];
        let down = [0.29, 0.28, 0.27, 0.26, 0.25, 0.24];
        assert_eq!(
            from_neighbors(0.3, &up, REFERENCE),
            from_neighbors(0.3, &down, REFERENCE)
        );
    }

    #[test]
    fn a_slope_steeper_than_the_reference_saturates_rather_than_overflowing() {
        let neighbors = [1.0, -1.0, 1.0, -1.0, 1.0, -1.0];
        assert_eq!(from_neighbors(0.0, &neighbors, REFERENCE), 1.0);
        assert_eq!(from_neighbors(0.0, &neighbors, 1.0e-300), 1.0);
    }

    #[test]
    fn every_result_is_a_finite_number_in_the_unit_range() {
        for step in -20..=20_i32 {
            let here = f64::from(step) / 20.0;
            for spread in [0.0, 1.0e-9, 0.001, 0.05, 2.0] {
                let mut neighbors = [0.0_f64; DIRECTION_COUNT];
                for (index, slot) in neighbors.iter_mut().enumerate() {
                    #[expect(
                        clippy::cast_possible_truncation,
                        clippy::cast_possible_wrap,
                        reason = "the loop index is at most 5"
                    )]
                    let index = index as i32;
                    *slot = here + spread * f64::from(index - 3);
                }
                let value = from_neighbors(here, &neighbors, REFERENCE);
                assert!(value.is_finite(), "{here} / {spread} gave {value}");
                assert!((0.0..=1.0).contains(&value), "{here} / {spread}");
            }
        }
    }

    #[test]
    fn the_direction_order_is_the_accumulation_order() {
        // Section 25.3. Reproduce the loop by hand in the documented direction
        // and require a bit-exact match, so a "harmless" `iter().sum()` or a
        // reordering shows up here rather than in a golden a phase later.
        let here = -0.125_f64;
        let neighbors = [0.1_f64, -0.7, 0.35, 0.9, -0.2, 0.0];
        let mut total = 0.0_f64;
        for neighbor in &neighbors {
            total += (neighbor - here).abs();
        }
        let expected = ((total / 6.0) / REFERENCE).clamp(0.0, 1.0);
        assert_eq!(
            from_neighbors(here, &neighbors, REFERENCE).to_bits(),
            expected.to_bits()
        );
    }
}
