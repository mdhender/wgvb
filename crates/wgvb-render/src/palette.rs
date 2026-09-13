//! The diagnostic palettes.
//!
//! See `DESIGN.md` section 29.
//!
//! One ramp is used for every *scalar* layer, deliberately. A per-layer palette
//! would make two layers of the same window impossible to compare by eye, which
//! is the main thing the tuning renderer is for. The ramp reads as terrain —
//! deep water through shallows, shore, vegetation, rock, and snow — because that
//! is the shape a reader already knows how to interpret, not because any layer
//! other than elevation means those things.
//!
//! Climate is the exception, and it has to be: it is not a scalar. A pair of
//! bands has no position on a ramp, and flattening the two axes onto one would
//! be the single mixed scale `DESIGN.md` section 16.1 forbids the model to
//! have. [`climate_color`] is a two-dimensional table instead — one color per
//! pair — so the two axes stay legible as two axes.

use wgvb::Climate;

/// Palette stops: a scalar position in `[-1, +1]` and its color.
///
/// Ascending, with `-1.0` and `+1.0` present, so lookup needs no special case at
/// either end. Changing a stop changes rendered output and so requires bumping
/// [`crate::RENDER_VERSION`].
const STOPS: [(f64, [u8; 3]); 11] = [
    (-1.000, [4, 16, 56]),
    (-0.500, [16, 62, 128]),
    (-0.100, [56, 128, 186]),
    // A shallow-water stop close to zero, so that a value just below the shore
    // still reads as water. Without it the ramp spends the whole last tenth
    // fading into sand and the coastline becomes impossible to find.
    (-0.005, [116, 182, 214]),
    (0.000, [214, 202, 160]),
    (0.030, [188, 180, 126]),
    (0.150, [88, 138, 70]),
    (0.450, [136, 120, 70]),
    (0.700, [118, 94, 74]),
    (0.900, [168, 164, 158]),
    (1.000, [252, 252, 252]),
];

/// The color of a pixel that belongs to no tile in the viewport.
pub const BACKGROUND: [u8; 4] = [24, 24, 28, 255];

/// One color per `(heat, moisture)` band pair, indexed by discriminant.
///
/// Read it as a grid with the two axes it draws: rows run polar to hot, columns
/// arid to saturated. Two properties make it legible as a grid rather than as
/// twenty-five unrelated swatches, and both are asserted by tests rather than
/// left to the eye:
///
/// - along a row, green gains on red as the ground gets wetter;
/// - down a column, red gains on blue as it gets warmer.
///
/// So a reader can tell which way is wetter and which way is warmer from the
/// image alone, without a key. Changing an entry changes rendered output and so
/// requires bumping [`crate::RENDER_VERSION`].
const CLIMATE_COLORS: [[[u8; 3]; 5]; 5] = [
    // Polar: pale, cold, and light enough that ice reads as ice.
    [
        [214, 222, 232],
        [198, 210, 226],
        [176, 196, 220],
        [150, 180, 214],
        [124, 164, 210],
    ],
    // Cold: grey through teal.
    [
        [178, 176, 150],
        [150, 166, 148],
        [118, 156, 146],
        [92, 146, 148],
        [66, 134, 150],
    ],
    // Temperate: straw through green.
    [
        [198, 182, 120],
        [176, 186, 106],
        [132, 176, 92],
        [92, 160, 84],
        [56, 140, 84],
    ],
    // Warm: gold through green.
    [
        [222, 186, 110],
        [214, 182, 86],
        [178, 174, 70],
        [126, 162, 62],
        [74, 146, 62],
    ],
    // Hot: desert orange through jungle green.
    [
        [228, 158, 96],
        [218, 156, 74],
        [198, 152, 62],
        [150, 156, 52],
        [58, 134, 44],
    ],
];

/// The color of one climate, as an opaque RGBA.
///
/// A total function over the pair, by construction: the table is indexed by the
/// pinned discriminants of [`wgvb::HeatBand`] and [`wgvb::MoistureBand`], both
/// of which have exactly five variants, so every climate a generator can
/// produce has an entry and none of them can be missed.
#[must_use]
pub fn climate_color(climate: Climate) -> [u8; 4] {
    let rgb = CLIMATE_COLORS[climate.heat as usize][climate.moisture as usize];
    [rgb[0], rgb[1], rgb[2], 255]
}

/// Maps a scalar in `[-1, +1]` to an opaque RGBA color.
///
/// Values outside the range are clamped rather than wrapped, and a
/// not-a-number maps to the lowest stop. Rendering is not the generation path,
/// so ordinary floating-point arithmetic is fine here; what matters is that the
/// mapping is a pure function of its input, because rendered output is
/// golden-compared.
#[must_use]
pub fn color(value: f64) -> [u8; 4] {
    let value = if value.is_nan() {
        -1.0
    } else {
        value.clamp(-1.0, 1.0)
    };

    let mut lower = STOPS[0];
    for stop in STOPS {
        if value >= stop.0 {
            lower = stop;
        }
    }
    let upper = STOPS
        .into_iter()
        .find(|stop| stop.0 > lower.0)
        .unwrap_or(lower);

    let span = upper.0 - lower.0;
    let t = if span > 0.0 {
        (value - lower.0) / span
    } else {
        0.0
    };

    let mut rgba = [0_u8, 0, 0, 255];
    for (channel, byte_out) in rgba.iter_mut().take(3).enumerate() {
        let a = f64::from(lower.1[channel]);
        let b = f64::from(upper.1[channel]);
        // Round to nearest, then clamp, then cast. An `as` cast on its own
        // truncates and saturates; spelling out the intent keeps the range
        // visible at the site that produces it.
        let blended = (a + t * (b - a) + 0.5).clamp(0.0, 255.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped to [0, 255] on the line above"
        )]
        let byte = blended as u8;
        *byte_out = byte;
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgvb::{HeatBand, MoistureBand};

    /// Every band, in discriminant order, from the core crate rather than from
    /// a copy kept here: a hand-written list is a list that can quietly lose a
    /// variant and take this file's coverage with it.
    const HEAT_BANDS: [HeatBand; 5] = HeatBand::ALL;
    const MOISTURE_BANDS: [MoistureBand; 5] = MoistureBand::ALL;

    #[test]
    fn the_stops_ascend_and_span_the_whole_range() {
        assert_eq!(STOPS[0].0, -1.0);
        assert_eq!(STOPS[STOPS.len() - 1].0, 1.0);
        for pair in STOPS.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{:?} then {:?}", pair[0], pair[1]);
        }
    }

    #[test]
    fn every_stop_reproduces_its_own_color_exactly() {
        for (value, rgb) in STOPS {
            let expected = [rgb[0], rgb[1], rgb[2], 255];
            assert_eq!(color(value), expected, "stop {value}");
        }
    }

    #[test]
    fn out_of_range_values_clamp_to_the_ends() {
        assert_eq!(color(-5.0), color(-1.0));
        assert_eq!(color(5.0), color(1.0));
        assert_eq!(color(f64::NAN), color(-1.0));
        assert_eq!(color(f64::INFINITY), color(1.0));
        assert_eq!(color(f64::NEG_INFINITY), color(-1.0));
    }

    /// Perceived brightness of a color, as a plain channel sum.
    fn brightness(value: f64) -> u32 {
        let c = color(value);
        u32::from(c[0]) + u32::from(c[1]) + u32::from(c[2])
    }

    #[test]
    fn the_extremes_of_the_ramp_are_the_darkest_and_the_lightest() {
        // The ramp is not monotone in brightness — forest is darker than sand,
        // and rock is darker than grassland, which is what makes a terrain ramp
        // readable as terrain rather than as a gradient. What must hold is that
        // the ends are unmistakable: the deepest water is the darkest thing on
        // the map and the highest ground is the lightest.
        let mut darkest = (f64::NAN, u32::MAX);
        let mut lightest = (f64::NAN, 0_u32);
        for step in -1_000..=1_000_i32 {
            let value = f64::from(step) / 1_000.0;
            let b = brightness(value);
            if b < darkest.1 {
                darkest = (value, b);
            }
            if b > lightest.1 {
                lightest = (value, b);
            }
        }
        assert!(darkest.0 < -0.95, "darkest color is at {}", darkest.0);
        assert!(lightest.0 > 0.95, "lightest color is at {}", lightest.0);
    }

    #[test]
    fn the_summit_of_the_ramp_rises_without_interruption() {
        // Above the rock stop the ramp must keep getting lighter, so the
        // highest ground on a map is unambiguous.
        let mut previous = brightness(0.90);
        let mut step = 91;
        while step <= 100 {
            let value = f64::from(step) / 100.0;
            let current = brightness(value);
            assert!(current >= previous, "brightness fell at {value}");
            previous = current;
            step += 1;
        }
    }

    #[test]
    fn adjacent_stops_are_visually_distinct() {
        // Two stops that look alike waste a band of the range.
        for pair in STOPS.windows(2) {
            let difference: u32 = (0..3)
                .map(|c| u32::from(pair[0].1[c].abs_diff(pair[1].1[c])))
                .sum();
            assert!(
                difference > 40,
                "stops {:?} and {:?} are too close",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn the_mapping_is_a_pure_function() {
        for step in -200..=200_i32 {
            let value = f64::from(step) / 200.0;
            assert_eq!(color(value), color(value), "{value}");
        }
    }

    #[test]
    fn every_color_is_fully_opaque() {
        for step in -200..=200_i32 {
            assert_eq!(color(f64::from(step) / 200.0)[3], 255);
        }
    }

    #[test]
    fn every_climate_has_its_own_color() {
        // Two climates that rendered alike would waste an entry of the table
        // and hide a distinction the model is built around.
        let mut seen = Vec::new();
        for heat in HEAT_BANDS {
            for moisture in MOISTURE_BANDS {
                let color = climate_color(Climate { heat, moisture });
                assert_eq!(color[3], 255, "{heat:?}/{moisture:?} is not opaque");
                assert!(
                    !seen.contains(&color),
                    "{heat:?}/{moisture:?} repeats {color:?}"
                );
                seen.push(color);
            }
        }
        assert_eq!(seen.len(), 25);
    }

    #[test]
    fn the_climate_table_reads_as_two_axes() {
        // The property that lets a reader orient the grid without a key. Green
        // gains on red rightwards along a row; red gains on blue downwards
        // through a column. Both are strict, so no two neighboring entries are
        // ambiguous about which way they lie.
        let difference =
            |color: [u8; 4], a: usize, b: usize| i32::from(color[a]) - i32::from(color[b]);

        for heat in HEAT_BANDS {
            let mut previous = i32::MIN;
            for moisture in MOISTURE_BANDS {
                let green_over_red = difference(climate_color(Climate { heat, moisture }), 1, 0);
                assert!(
                    green_over_red > previous,
                    "{heat:?}: {moisture:?} is not greener than the band before it"
                );
                previous = green_over_red;
            }
        }

        for moisture in MOISTURE_BANDS {
            let mut previous = i32::MIN;
            for heat in HEAT_BANDS {
                let red_over_blue = difference(climate_color(Climate { heat, moisture }), 0, 2);
                assert!(
                    red_over_blue > previous,
                    "{moisture:?}: {heat:?} is not warmer than the band before it"
                );
                previous = red_over_blue;
            }
        }
    }

    #[test]
    fn adjacent_climates_are_visually_distinct() {
        // Distinct is not enough on its own: two entries differing by one unit
        // in one channel are distinct and indistinguishable.
        let distance = |a: [u8; 4], b: [u8; 4]| -> u32 {
            (0..3).map(|c| u32::from(a[c].abs_diff(b[c]))).sum()
        };
        for (h, heat) in HEAT_BANDS.into_iter().enumerate() {
            for (m, moisture) in MOISTURE_BANDS.into_iter().enumerate() {
                let here = climate_color(Climate { heat, moisture });
                if m + 1 < MOISTURE_BANDS.len() {
                    let right = climate_color(Climate {
                        heat,
                        moisture: MOISTURE_BANDS[m + 1],
                    });
                    assert!(
                        distance(here, right) > 30,
                        "{heat:?}: {moisture:?} and {:?} are too close",
                        MOISTURE_BANDS[m + 1]
                    );
                }
                if h + 1 < HEAT_BANDS.len() {
                    let below = climate_color(Climate {
                        heat: HEAT_BANDS[h + 1],
                        moisture,
                    });
                    assert!(
                        distance(here, below) > 30,
                        "{moisture:?}: {heat:?} and {:?} are too close",
                        HEAT_BANDS[h + 1]
                    );
                }
            }
        }
    }

    #[test]
    fn no_climate_is_mistakable_for_the_background() {
        for heat in HEAT_BANDS {
            for moisture in MOISTURE_BANDS {
                let color = climate_color(Climate { heat, moisture });
                let distance: u32 = (0..3)
                    .map(|c| u32::from(color[c].abs_diff(BACKGROUND[c])))
                    .sum();
                assert!(
                    distance > 120,
                    "{heat:?}/{moisture:?} looks like the gutter"
                );
            }
        }
    }

    #[test]
    fn water_and_land_are_distinguishable_either_side_of_zero() {
        // The shore stop is where the ramp changes character. A reader has to
        // be able to see a coastline in a single-channel field.
        let water = color(-0.01);
        let land = color(0.01);
        let difference: u32 = (0..3).map(|c| u32::from(water[c].abs_diff(land[c]))).sum();
        assert!(
            difference > 120,
            "water {water:?} and land {land:?} are too close"
        );
    }
}
