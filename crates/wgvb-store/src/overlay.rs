//! Sparse coordinate-keyed player overlays.
//!
//! Overlays are the authoritative half of a WGVB database. Generated terrain is
//! a pure function of the seed, the configuration, and the coordinate, and can
//! be thrown away and recomputed; what the player has *seen* and what the
//! player has *built* cannot. Section 27.3 is the consequence: no foreign key
//! points from here at generated data, because dropping a discardable cache
//! must never require dropping a settlement.
//!
//! Both tables are `WITHOUT ROWID` with `PRIMARY KEY (q, r)`, so each table is
//! a B-tree in coordinate order and a viewport load is one ordered range scan.

use std::ops::RangeInclusive;

use rusqlite::Row;
use wgvb::{Component, Coord};

use crate::World;

/// A named place the player has founded or recorded.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Settlement {
    /// Where it sits, canonical.
    pub coord: Coord,
    /// What the player calls it.
    pub name: String,
}

/// An inclusive `(q, r)` box to scan.
///
/// A viewport is a parallelogram in offset space and may wrap, so its tiles are
/// not a box in canonical coordinates. [`Bounds::containing`] therefore takes
/// the tiles themselves and returns the smallest box that holds all of them:
/// a superset is correct, because an overlay outside the viewport is simply
/// never drawn, while a subset would silently lose one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bounds {
    q: RangeInclusive<Component>,
    r: RangeInclusive<Component>,
}

impl Bounds {
    /// The smallest box containing every coordinate, or `None` for none.
    pub fn containing(coords: impl IntoIterator<Item = Coord>) -> Option<Bounds> {
        let mut coords = coords.into_iter();
        let first = coords.next()?;
        let (mut q_lo, mut q_hi) = (first.q(), first.q());
        let (mut r_lo, mut r_hi) = (first.r(), first.r());
        for coord in coords {
            q_lo = q_lo.min(coord.q());
            q_hi = q_hi.max(coord.q());
            r_lo = r_lo.min(coord.r());
            r_hi = r_hi.max(coord.r());
        }
        Some(Bounds {
            q: q_lo..=q_hi,
            r: r_lo..=r_hi,
        })
    }

    /// The whole canonical world.
    #[must_use]
    pub const fn everywhere() -> Bounds {
        Bounds {
            q: Component::MIN..=Component::MAX,
            r: Component::MIN..=Component::MAX,
        }
    }

    /// The four query parameters, widened to what SQLite stores.
    fn params(&self) -> [i64; 4] {
        [
            i64::from(*self.q.start()),
            i64::from(*self.q.end()),
            i64::from(*self.r.start()),
            i64::from(*self.r.end()),
        ]
    }
}

impl World {
    /// Records tiles as seen. Already-seen tiles are left alone.
    ///
    /// # Errors
    ///
    /// Whatever SQLite refused.
    pub fn discover(&self, coords: &[Coord]) -> Result<usize, rusqlite::Error> {
        let transaction = self.connection().unchecked_transaction()?;
        let mut written = 0;
        {
            let mut statement = transaction
                .prepare("INSERT OR IGNORE INTO overlay_discovery (q, r) VALUES (?1, ?2)")?;
            for coord in coords {
                written += statement.execute([i64::from(coord.q()), i64::from(coord.r())])?;
            }
        }
        transaction.commit()?;
        Ok(written)
    }

    /// Every discovered tile in a box, in coordinate order.
    ///
    /// # Errors
    ///
    /// Whatever SQLite refused.
    pub fn discoveries_in(&self, bounds: &Bounds) -> Result<Vec<Coord>, rusqlite::Error> {
        // `ORDER BY q, r` is the primary key's own order, so this is the range
        // scan the `WITHOUT ROWID` table exists to make cheap. Saying it out
        // loud makes the ordering a contract a test can hold rather than an
        // implementation detail that happens to be true today.
        let mut statement = self.connection().prepare(
            "SELECT q, r FROM overlay_discovery \
             WHERE q BETWEEN ?1 AND ?2 AND r BETWEEN ?3 AND ?4 \
             ORDER BY q, r",
        )?;
        let rows = statement.query_map(bounds.params(), coord_of_row)?;
        rows.collect()
    }

    /// Every discovered tile, in coordinate order.
    ///
    /// # Errors
    ///
    /// Whatever SQLite refused.
    pub fn discoveries(&self) -> Result<Vec<Coord>, rusqlite::Error> {
        self.discoveries_in(&Bounds::everywhere())
    }

    /// Names a place, replacing any existing name at that tile.
    ///
    /// # Errors
    ///
    /// Whatever SQLite refused.
    pub fn settle(&self, coord: Coord, name: &str) -> Result<(), rusqlite::Error> {
        self.connection().execute(
            "INSERT INTO overlay_settlement (q, r, name) VALUES (?1, ?2, ?3) \
             ON CONFLICT (q, r) DO UPDATE SET name = excluded.name",
            rusqlite::params![i64::from(coord.q()), i64::from(coord.r()), name],
        )?;
        Ok(())
    }

    /// Every settlement in a box, in coordinate order.
    ///
    /// # Errors
    ///
    /// Whatever SQLite refused.
    pub fn settlements_in(&self, bounds: &Bounds) -> Result<Vec<Settlement>, rusqlite::Error> {
        let mut statement = self.connection().prepare(
            "SELECT q, r, name FROM overlay_settlement \
             WHERE q BETWEEN ?1 AND ?2 AND r BETWEEN ?3 AND ?4 \
             ORDER BY q, r",
        )?;
        let rows = statement.query_map(bounds.params(), |row| {
            Ok(Settlement {
                coord: coord_of_row(row)?,
                name: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    /// Every settlement, in coordinate order.
    ///
    /// # Errors
    ///
    /// Whatever SQLite refused.
    pub fn settlements(&self) -> Result<Vec<Settlement>, rusqlite::Error> {
        self.settlements_in(&Bounds::everywhere())
    }
}

/// The coordinate in columns 0 and 1 of a row.
///
/// Through [`Coord::new`], so a hand-edited out-of-range row normalizes to the
/// tile it names rather than becoming a coordinate the type says cannot exist.
fn coord_of_row(row: &Row<'_>) -> Result<Coord, rusqlite::Error> {
    Ok(Coord::new(row.get(0)?, row.get(1)?))
}
