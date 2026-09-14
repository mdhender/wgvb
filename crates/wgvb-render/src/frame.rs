//! The player frame: one player's origin and rotation, and the conversion
//! between their coordinates and canonical ones.
//!
//! Every player is assigned an origin hex **and** a rotation when they are
//! created, and sees the world through that frame on a flat-top layout. One
//! player's `(0, 0)` is not another's, and their norths differ: rotation is a
//! direction offset, so a player at rotation `k` perceives absolute direction
//! `k` as north. Two players can describe the same tile with different
//! coordinates and the same heading with different direction numbers. See
//! `DESIGN.md` appendix A, *Coordinate frames*.
//!
//! ```text
//! absolute           = normalize( rotate^k ( relative ) + player_origin )
//! absolute_direction = (player_direction + k) mod 6
//! ```
//!
//! Every step of that is exact integer arithmetic — no angle, no matrix, no
//! `f64` — so nothing here can drift between targets the way section 25 is
//! about. [`wgvb::Coord::rotate`] is the world-frame half and lives in the core
//! crate because the same permutation generates the mirror centers; the origin,
//! the rotation, and every compass name are this crate's.
//!
//! # Why this lives in `wgvb-render`
//!
//! A frame exists to present the world to somebody, which makes it the
//! presentation layer's concern, and this is where the flat-top layout and the
//! screen-axis offsets already compose. `wgvb-render` and `wgvb-store` are
//! siblings, so a type both needed would have to sit in `wgvb` — the one crate
//! whose job is to have no player concepts. The store therefore persists the
//! three scalars (origin `q`, origin `r`, rotation) and never sees this type.
//! Section 28 records the decision.
//!
//! # A frame never reaches the generator
//!
//! Fields are sampled on canonical coordinates only. A `Generator` method that
//! took a frame, a rotation, or a player id would give
//! `Tile = F(seed, coordinate, version, configuration)` a per-player term, and
//! two players standing on one tile would not be standing on one tile.
//! `crates/wgvb-render/tests/player_frame.rs` pins the sampling signatures at
//! compile time; the dependency graph does the rest, since `wgvb` cannot name
//! [`PlayerFrame`] at all.
//!
//! # What this does not do yet
//!
//! It converts. It does not *draw*: rendering through a frame is #3's, and it
//! is not a bare substitution of relative coordinates for absolute ones. The
//! unrotated layout puts absolute direction **2** at the top of the image (see
//! the crate docs), while a frame's north is relative direction **0**, so a
//! renderer that laid out relative coordinates directly would put the player's
//! north two sixths of a turn off the top of the screen.

use wgvb::{Coord, DIRECTION_COUNT, direction_index};

/// Why a rotation is not a rotation.
///
/// Out-of-range is refused rather than reduced. A player's rotation is
/// authoritative state that anchors every coordinate they have ever been told,
/// so a stored `7` is a file that has been edited or a caller that has computed
/// the wrong thing — and quietly calling it `1` would answer both with a world
/// silently rotated a sixth of a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// The rotation is not a direction index.
    #[error("rotation {0} is not a direction index in 0..6")]
    Rotation(u8),
}

/// One player's view of the world: where their `(0, 0)` is, and which absolute
/// direction they call north.
///
/// The fields are private for the reason [`Coord`]'s are: the rotation is
/// checked once, at construction, so "a frame's rotation is a direction index"
/// is a property of the type rather than a convention every caller has to
/// remember. There is deliberately no setter for either field — a frame is
/// assigned when the player is created and kept, because changing it would
/// silently change the meaning of every coordinate that player has already been
/// given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerFrame {
    origin: Coord,
    rotation: u8,
}

impl PlayerFrame {
    /// A frame anchored at `origin` whose north is absolute direction
    /// `rotation`.
    ///
    /// # Errors
    ///
    /// [`FrameError::Rotation`] if `rotation` is not in `0..6`.
    pub fn new(origin: Coord, rotation: u8) -> Result<PlayerFrame, FrameError> {
        if usize::from(rotation) >= DIRECTION_COUNT {
            return Err(FrameError::Rotation(rotation));
        }
        Ok(PlayerFrame { origin, rotation })
    }

    /// The canonical tile this player calls `(0, 0)`.
    #[must_use]
    pub const fn origin(self) -> Coord {
        self.origin
    }

    /// The absolute direction this player calls north, in `0..6`.
    #[must_use]
    pub const fn rotation(self) -> u8 {
        self.rotation
    }

    /// The rotation as the signed step count [`Coord::rotate`] takes.
    #[inline]
    fn steps(self) -> i32 {
        i32::from(self.rotation)
    }

    /// The canonical tile a player-relative coordinate names.
    ///
    /// `normalize(rotate^k(relative) + origin)`. Rotation commutes with
    /// normalization — the canonical domain is six-fold symmetric about the
    /// origin — so normalizing once, on the way out of [`Coord::new`], is
    /// enough.
    #[must_use]
    pub fn to_absolute(self, relative: Coord) -> Coord {
        let rotated = relative.rotate(self.steps());
        Coord::new(
            i64::from(rotated.q()) + i64::from(self.origin.q()),
            i64::from(rotated.r()) + i64::from(self.origin.r()),
        )
    }

    /// What this player calls a canonical tile.
    ///
    /// The inverse of [`to_absolute`](Self::to_absolute):
    /// `rotate^-k(absolute - origin)`.
    #[must_use]
    pub fn to_relative(self, absolute: Coord) -> Coord {
        Coord::new(
            i64::from(absolute.q()) - i64::from(self.origin.q()),
            i64::from(absolute.r()) - i64::from(self.origin.r()),
        )
        .rotate(-self.steps())
    }

    /// The absolute direction a player-relative direction points.
    ///
    /// `(player_direction + k) mod 6`. Any integer is accepted and normalized,
    /// on the same terms as [`Coord::neighbor`]; the result is always in
    /// `0..6`.
    #[must_use]
    pub fn to_absolute_direction(self, relative: i32) -> u8 {
        // `direction_index` first, so the sum cannot overflow for an extreme
        // input: both terms are then at most 5.
        let relative = i32::try_from(direction_index(relative)).expect("an index below six");
        index_as_u8(direction_index(relative + self.steps()))
    }

    /// What this player calls an absolute direction.
    ///
    /// The inverse of
    /// [`to_absolute_direction`](Self::to_absolute_direction):
    /// `(absolute_direction - k) mod 6`.
    #[must_use]
    pub fn to_relative_direction(self, absolute: i32) -> u8 {
        let absolute = i32::try_from(direction_index(absolute)).expect("an index below six");
        index_as_u8(direction_index(absolute - self.steps()))
    }
}

/// A direction index as the `u8` a rotation is stored and compared as.
#[inline]
fn index_as_u8(index: usize) -> u8 {
    u8::try_from(index).expect("a direction index is below six")
}
