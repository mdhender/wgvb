//! What is actually in a window, counted.
//!
//! `crates/wgvb/tests/terrain.rs` and `crates/wgvb/tests/elevation.rs` already
//! measure this globally: every terrain is reachable, none dominates, and the
//! bands come out in the proportions the thresholds intend. What they cannot
//! say is whether the threshold somebody just moved did what they meant *here*,
//! in the window they are looking at — and that is the question a tuning pass
//! asks a few hundred times in a row.
//!
//! # Why this is in the renderer
//!
//! It is a measurement over a window, and a window is this crate's. Putting it
//! here means `wgvb-map` can print the same numbers a web front end shows, and
//! that two callers cannot disagree about what a window contains.
//!
//! # It costs seven evaluations a tile
//!
//! Terrain and the climate bands come from a whole [`wgvb::Tile`], which is
//! seven elevation evaluations. That is the same price [`Layer::Terrain`] pays
//! to draw one, so a window that can be drawn in terrain can be counted; it is
//! not a price a very large window should pay twice, which is why the grid tab
//! does not show one.
//!
//! # This is not the accumulation the batch rules forbid
//!
//! CLAUDE.md and `DESIGN.md` section 20 forbid a batch operation that
//! accumulates across tiles, because a work-stealing split would decide the
//! result. That rule is about floating-point sums in a parallel fill. These are
//! integer counts accumulated in one thread, and integer addition is
//! associative and commutative — a test asserts the count does not depend on
//! the order the coordinates arrive in.

use wgvb::{Coord, Elevation, Generator, HeatBand, MoistureBand, Terrain};

/// How many tiles of each kind a window holds.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Distribution {
    tiles: u64,
    terrain: [u64; Terrain::ALL.len()],
    elevation: [u64; Elevation::ALL.len()],
    heat: [u64; HeatBand::ALL.len()],
    moisture: [u64; MoistureBand::ALL.len()],
}

impl Distribution {
    /// Counts every tile of a window.
    ///
    /// The caller supplies the coordinates — usually [`crate::Viewport::coords`]
    /// — so that the thing counted is exactly the thing drawn, including a
    /// window that wraps an edge and therefore shows one tile twice. Counting
    /// it twice is correct: it is on the screen twice.
    #[must_use]
    pub fn of(generator: &Generator, coords: impl IntoIterator<Item = Coord>) -> Distribution {
        let mut counted = Distribution::default();
        for coord in coords {
            let tile = generator.tile(coord);
            counted.tiles += 1;
            counted.terrain[index(tile.terrain as u8)] += 1;
            counted.elevation[index(tile.elevation as u8)] += 1;
            counted.heat[index(tile.climate.heat as u8)] += 1;
            counted.moisture[index(tile.climate.moisture as u8)] += 1;
        }
        counted
    }

    /// How many tiles were counted.
    #[must_use]
    pub const fn tiles(&self) -> u64 {
        self.tiles
    }

    /// Whether anything was counted at all.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.tiles == 0
    }

    /// Every terrain and its count, in vocabulary order.
    ///
    /// Vocabulary order rather than sorted by size, and every terrain rather
    /// than only the ones present: a row that reads zero is the useful row.
    /// "No wetland anywhere in this window" is a thing a tuning pass needs to
    /// see, and a list that hid it would hide exactly the tile counts somebody
    /// is trying to move off zero.
    pub fn terrain(&self) -> impl Iterator<Item = (Terrain, u64)> + '_ {
        Terrain::ALL
            .into_iter()
            .map(|terrain| (terrain, self.terrain[index(terrain as u8)]))
    }

    /// Every elevation band and its count, low to high.
    pub fn elevation(&self) -> impl Iterator<Item = (Elevation, u64)> + '_ {
        Elevation::ALL
            .into_iter()
            .map(|band| (band, self.elevation[index(band as u8)]))
    }

    /// Every heat band and its count, cold to hot.
    pub fn heat(&self) -> impl Iterator<Item = (HeatBand, u64)> + '_ {
        HeatBand::ALL
            .into_iter()
            .map(|band| (band, self.heat[index(band as u8)]))
    }

    /// Every moisture band and its count, dry to wet.
    pub fn moisture(&self) -> impl Iterator<Item = (MoistureBand, u64)> + '_ {
        MoistureBand::ALL
            .into_iter()
            .map(|band| (band, self.moisture[index(band as u8)]))
    }

    /// One count as a percentage of the window.
    ///
    /// Zero for an empty window rather than a division by zero. Presentation
    /// arithmetic on counts that have already been made, so this is not
    /// generation-path floating point and section 25 has nothing to say about
    /// it.
    #[must_use]
    pub fn share(&self, count: u64) -> f64 {
        if self.tiles == 0 {
            return 0.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a percentage for a person to read, not a value anything computes with"
        )]
        let (count, tiles) = (count as f64, self.tiles as f64);
        count / tiles * 100.0
    }
}

/// A pinned enum discriminant as an array index.
///
/// Every counted enum is `#[repr(u8)]` with explicit discriminants running from
/// zero, which `DESIGN.md` requires because the values are persisted. This
/// borrows that guarantee to index a counter array, and the test module asserts
/// it rather than assuming it: a variant renumbered out of order would put two
/// terrains in one bucket, which is a silently wrong histogram rather than a
/// crash.
const fn index(discriminant: u8) -> usize {
    discriminant as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_counted_enum_is_numbered_from_zero_without_a_gap() {
        // The contract `index` borrows. If this fails, the histograms are
        // wrong rather than absent, which is the worse of the two.
        for (at, terrain) in Terrain::ALL.into_iter().enumerate() {
            assert_eq!(index(terrain as u8), at, "{}", terrain.name());
        }
        for (at, band) in Elevation::ALL.into_iter().enumerate() {
            assert_eq!(index(band as u8), at, "{}", band.name());
        }
        for (at, band) in HeatBand::ALL.into_iter().enumerate() {
            assert_eq!(index(band as u8), at, "{}", band.name());
        }
        for (at, band) in MoistureBand::ALL.into_iter().enumerate() {
            assert_eq!(index(band as u8), at, "{}", band.name());
        }
    }

    #[test]
    fn an_empty_window_counts_nothing_and_divides_by_nothing() {
        let empty = Distribution::default();
        assert!(empty.is_empty());
        assert_eq!(empty.tiles(), 0);
        assert!((empty.share(0) - 0.0).abs() < f64::EPSILON);
        assert_eq!(empty.terrain().count(), Terrain::ALL.len());
    }
}
