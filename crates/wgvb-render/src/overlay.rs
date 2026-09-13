//! Player overlays, and how they compose over generated terrain.
//!
//! `DESIGN.md` section 27.6 keeps the two apart: generated terrain is a pure
//! function of the seed, the configuration, and the coordinate, and can be
//! thrown away and recomputed; what the player has seen and built cannot. This
//! module is where the two meet, and it meets them **at render time** — nothing
//! here reaches back into generation, and a tile's terrain is the same whether
//! or not anybody has ever looked at it.
//!
//! The type is deliberately ignorant of where the overlays came from.
//! `wgvb-render` does not depend on `wgvb-store`: they are siblings, and a
//! renderer that could open a database would be a renderer that could be handed
//! a world rather than a viewport.

use wgvb::Coord;

/// What the player knows, ready to compose over a rendered layer.
///
/// Both collections are kept sorted by coordinate. That is not tidiness: it is
/// the stable render order section 29 requires. Markers overlap pixels, so two
/// settlements whose markers touch must resolve the same way on every run, and
/// a set with an unspecified iteration order would decide that by hash seed.
/// Lookup is a binary search over the same sorted slice.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overlays {
    discovered: Vec<Coord>,
    settlements: Vec<(Coord, String)>,
}

impl Overlays {
    /// Sorts and deduplicates what the caller has.
    ///
    /// Duplicates resolve last-one-wins for settlements, which is what a store
    /// keyed by `(q, r)` can produce only if somebody handed the same tile
    /// twice; the rule exists so the outcome is stated rather than discovered.
    #[must_use]
    pub fn new(discovered: Vec<Coord>, settlements: Vec<(Coord, String)>) -> Overlays {
        let mut discovered = discovered;
        discovered.sort_unstable();
        discovered.dedup();

        let mut settlements = settlements;
        settlements.sort_by_key(|entry| entry.0);
        settlements.dedup_by(|later, earlier| {
            if later.0 == earlier.0 {
                earlier.1 = std::mem::take(&mut later.1);
                true
            } else {
                false
            }
        });

        Overlays {
            discovered,
            settlements,
        }
    }

    /// Nothing known and nothing built. Renders as plain terrain.
    #[must_use]
    pub fn none() -> Overlays {
        Overlays::default()
    }

    /// Whether fog of war applies at all.
    ///
    /// **An empty discovery list means fog is not in use, not that the player
    /// has seen nothing.** A world that records no discoveries is one where
    /// exploration is not being tracked, and hiding every tile of it would make
    /// a freshly created world render as a solid rectangle of [`super::FOG`] —
    /// an alarming way to say "this feature is switched off". One discovered
    /// tile switches it on.
    #[must_use]
    pub fn fog_of_war(&self) -> bool {
        !self.discovered.is_empty()
    }

    /// Whether the player has seen this tile.
    #[must_use]
    pub fn is_discovered(&self, coord: Coord) -> bool {
        self.discovered.binary_search(&coord).is_ok()
    }

    /// Whether the tile's terrain should be drawn.
    #[must_use]
    pub fn is_visible(&self, coord: Coord) -> bool {
        !self.fog_of_war() || self.is_discovered(coord)
    }

    /// The settlement at a tile, if any.
    #[must_use]
    pub fn settlement_at(&self, coord: Coord) -> Option<&str> {
        self.settlements
            .binary_search_by(|entry| entry.0.cmp(&coord))
            .ok()
            .map(|index| self.settlements[index].1.as_str())
    }

    /// Every settlement, in coordinate order.
    #[must_use]
    pub fn settlements(&self) -> &[(Coord, String)] {
        &self.settlements
    }

    /// Every discovered tile, in coordinate order.
    #[must_use]
    pub fn discovered(&self) -> &[Coord] {
        &self.discovered
    }

    /// Whether there is anything to compose.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.discovered.is_empty() && self.settlements.is_empty()
    }
}
