//! Boundary continuity of the continuous fields.
//!
//! `DESIGN.md` sections 11.2, 30.5, and 30.6: crossing a chunk, region, or
//! macro-region boundary must not introduce a discontinuity merely because the
//! addressing cell changed. Chunks and regions are a caller and cache
//! convenience; they do not define geography.
//!
//! The tests below do not measure "is the field smooth" in the abstract. They
//! compare the step across a boundary against the steps everywhere else on the
//! same line, so a field that is simply rough everywhere still passes and a
//! field with a seam at a cell edge still fails.
//!
//! Two different kinds of scalar are measured, and the distinction matters when
//! one of these tests fails. The four noise fields and `elevation_raw` never
//! read an addressing index at all, so for them a seam would mean something had
//! leaked in that has no business being there.
//!
//! `regional_uplift`, `roughness`, `ridge`, and `elevation` *are* addressed by
//! region, and they are what section 30.5 is really about. The first two are
//! blended across the anchors around a tile; `ridge` reads the blended ridge
//! orientation, so it fails here if the orientation blend creases even where
//! the scalars do not; and `elevation` folds all of them together, which makes
//! it the one that would show a seam a reader could actually see on a map. A
//! seam in any of them would mean the blend had degenerated into the hard
//! region boundary section 33.4 forbids.

use wgvb::{Config, Coord, Generator, Sample};

const SEED: u64 = 0x00c0_ffee_0bad_f00d;

/// Every scalar a [`Sample`] carries, named, in a fixed order.
fn scalars(s: &Sample) -> [(&'static str, f64); 9] {
    [
        ("continentalness", s.continentalness),
        ("regional", s.regional),
        ("local", s.local),
        ("detail", s.detail),
        ("ridge", s.ridge),
        ("elevation_raw", s.elevation_raw),
        ("regional_uplift", s.regional_uplift),
        ("roughness", s.roughness),
        ("elevation", s.elevation),
    ]
}

/// The largest and mean absolute step taken by one scalar along a line, split
/// into the steps that cross a cell edge and the steps that do not.
#[derive(Debug, Clone, Copy, Default)]
struct Steps {
    boundary_max: f64,
    boundary_sum: f64,
    boundary_count: u32,
    interior_max: f64,
    interior_sum: f64,
    interior_count: u32,
}

impl Steps {
    fn add(&mut self, delta: f64, crosses: bool) {
        if crosses {
            self.boundary_max = self.boundary_max.max(delta);
            self.boundary_sum += delta;
            self.boundary_count += 1;
        } else {
            self.interior_max = self.interior_max.max(delta);
            self.interior_sum += delta;
            self.interior_count += 1;
        }
    }

    fn boundary_mean(&self) -> f64 {
        self.boundary_sum / f64::from(self.boundary_count)
    }

    fn interior_mean(&self) -> f64 {
        self.interior_sum / f64::from(self.interior_count)
    }
}

/// Walks a straight line of tiles and measures, per scalar, how the steps that
/// cross a cell edge compare with the steps that do not.
///
/// `along_q` picks which axis the line runs along; both are exercised because
/// chunk and region addressing divides `q` and `r` independently.
fn steps_along_a_line(
    generator: &Generator,
    size_hexes: i64,
    along_q: bool,
    fixed: i64,
    span: i64,
) -> [Steps; 9] {
    let mut steps = [Steps::default(); 9];
    let mut previous: Option<Sample> = None;

    for step in -span..=span {
        let coord = if along_q {
            Coord::new(step, fixed)
        } else {
            Coord::new(fixed, step)
        };
        let current = generator.sample(coord);
        if let Some(previous) = previous {
            // The step from `step - 1` to `step` crosses a cell edge exactly
            // when `step` is the first tile of a new cell.
            let crosses = step.rem_euclid(size_hexes) == 0;
            for (index, ((_, before), (_, after))) in scalars(&previous)
                .into_iter()
                .zip(scalars(&current))
                .enumerate()
            {
                steps[index].add((after - before).abs(), crosses);
            }
        }
        previous = Some(current);
    }
    steps
}

/// How much larger a boundary step may be than the steps around it.
///
/// A genuine seam is not a near miss. Either side of a hard boundary would be
/// uncorrelated with the other, so the step would be a large fraction of the
/// `[-1, +1]` range — two orders of magnitude above the roughly `0.01` a
/// six-mile step produces. The factor below sits far from both, so the test
/// cannot fail because one boundary step happened to be the largest on the
/// line, and cannot pass if a boundary is real.
///
/// For `regional_uplift` the margin is far wider than it needs to be: the
/// quintic interpolant flattens the blend at an anchor line, so its boundary
/// steps are the *smallest* on the line rather than merely comparable. The unit
/// tests in `region.rs` assert that stronger property directly, without a
/// factor at all.
const SEAM_FACTOR: f64 = 2.0;

/// Asserts that no cell boundary is a special place on the line.
fn assert_no_seam(generator: &Generator, label: &str, size_hexes: i64) {
    let names = scalars(&generator.sample(Coord::ORIGIN)).map(|(name, _)| name);
    // Long enough that every cell size produces many crossings, so the mean
    // below is a measurement rather than a handful of samples.
    let span = (size_hexes * 24).max(1_500);
    for (along_q, fixed) in [(true, 0_i64), (true, -97), (false, 0), (false, 411)] {
        let steps = steps_along_a_line(generator, size_hexes, along_q, fixed, span);
        assert!(
            steps[0].boundary_count >= 24,
            "too few crossings to measure"
        );
        for (name, measured) in names.into_iter().zip(steps) {
            let where_ = format!("along_q = {along_q}, fixed = {fixed}");
            assert!(
                measured.interior_max > 0.0,
                "{name} never changed along the line; the test proves nothing ({where_})"
            );
            assert!(
                measured.boundary_max <= measured.interior_max * SEAM_FACTOR,
                "{label} boundary every {size_hexes} hexes is a seam in {name}: \
                 largest step at a boundary {}, largest step elsewhere {} ({where_})",
                measured.boundary_max,
                measured.interior_max
            );
            assert!(
                measured.boundary_mean() <= measured.interior_mean() * SEAM_FACTOR,
                "{label} boundary every {size_hexes} hexes biases {name}: \
                 mean step at a boundary {}, mean step elsewhere {} ({where_})",
                measured.boundary_mean(),
                measured.interior_mean()
            );
        }
    }
}

#[test]
fn crossing_a_chunk_boundary_introduces_no_discontinuity() {
    let generator = Generator::with_defaults(SEED);
    let size = i64::from(generator.config().chunk_size_hexes);
    assert_no_seam(&generator, "chunk", size);
}

#[test]
fn crossing_a_region_boundary_introduces_no_discontinuity() {
    let generator = Generator::with_defaults(SEED);
    let size = i64::from(generator.config().region_size_hexes);
    assert_no_seam(&generator, "region", size);
}

#[test]
fn crossing_a_macro_region_boundary_introduces_no_discontinuity() {
    let generator = Generator::with_defaults(SEED);
    let size = i64::from(generator.config().macro_region_size_hexes);
    assert_no_seam(&generator, "macro region", size);
}

#[test]
fn a_non_default_cell_size_is_equally_unremarkable() {
    // The addressing sizes are configuration. A field that happened to be
    // smooth at 32 and 128 because of the wavelengths chosen would still be
    // wrong; a prime cell size no wavelength divides catches that.
    let config = Config {
        chunk_size_hexes: 37,
        region_size_hexes: 149,
        ..Config::default()
    };
    let generator = Generator::new(SEED, config).expect("configuration is valid");
    assert_no_seam(&generator, "prime chunk", 37);
    assert_no_seam(&generator, "prime region", 149);
}

#[test]
fn neighboring_tiles_stay_close_together() {
    // Section 30.7. Adjacent tiles are six miles apart, and the shortest field
    // in the composite has a 36-mile wavelength, so no neighbor pair may jump
    // anywhere near the full range. A field that had collapsed into per-tile
    // noise would fail here.
    let generator = Generator::with_defaults(SEED);
    let mut worst = 0.0_f64;
    for q in -60..=60_i64 {
        for r in -60..=60_i64 {
            let here = generator.sample(Coord::new(q * 13, r * 13));
            for direction in 0..6 {
                let there = generator.sample(Coord::new(q * 13, r * 13).neighbor(direction));
                worst = worst.max((here.elevation_raw - there.elevation_raw).abs());
            }
        }
    }
    assert!(worst < 0.25, "adjacent tiles differed by {worst}");
    assert!(worst > 0.0, "the field is constant");
}
