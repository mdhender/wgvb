//! Player frames: the three scalars, and nothing that knows what they are for.
//!
//! Every player is assigned an origin hex and a rotation when they are created,
//! and sees the world through that frame. This module stores origin `q`, origin
//! `r`, and the rotation, and converts nothing: the conversion — and the type
//! that carries it, `wgvb_render::PlayerFrame` — belongs to the presentation
//! layer. `wgvb-render` and `wgvb-store` are siblings, so a type both needed
//! would have to sit in `wgvb`, the one crate whose job is to have no player
//! concepts, and storing the scalars is what keeps it out of there. Section 28
//! records the decision; appendix A has the transform.
//!
//! A frame is authoritative player state on exactly the terms section 27.6
//! gives overlays: it is not derived from the seed, it is never regenerated,
//! and it survives discarding every cache. It is also the state that gives
//! every *other* piece of player-facing data its meaning — a settlement the
//! player calls `(3, -1)` is a different tile under a different frame — which
//! is why nothing here will quietly repair a bad value.

use rusqlite::params;
use wgvb::{Coord, DIRECTION_COUNT};

use crate::World;

/// A player and the frame they see the world through.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Player {
    /// Who they are. The primary key: one name, one frame.
    pub name: String,
    /// The canonical tile this player calls `(0, 0)`.
    pub origin: Coord,
    /// The absolute direction this player calls north, in `0..6`.
    pub rotation: u8,
}

/// Why a player could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    /// The rotation is not a direction index — on the way in or on the way out.
    ///
    /// **Refused, not reduced.** A stored `7` is a file somebody has edited or
    /// a caller that computed the wrong thing, and `rem_euclid` would answer
    /// both by handing back a world rotated a sixth of a turn from the one this
    /// player has been walking around in. That is the malformed-metadata case,
    /// not a value to normalize.
    #[error("rotation {found} for player {player:?} is not a direction index in 0..6")]
    RotationOutOfRange { player: String, found: i64 },
    /// A player by that name already exists, with a frame of their own.
    #[error("a player named {0:?} already exists")]
    Exists(String),
    /// The database itself refused.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

impl World {
    /// Creates a player with the frame they will keep.
    ///
    /// **There is deliberately no way to move a frame afterwards.** A player's
    /// origin and rotation are what every coordinate and heading that player
    /// has ever been given *mean*; changing either one would silently relabel
    /// all of it, and a second `add_player` for an existing name is therefore
    /// [`PlayerError::Exists`] rather than an upsert. A frame that genuinely
    /// has to change is a new player, or a migration that says so out loud.
    ///
    /// # Errors
    ///
    /// [`PlayerError::RotationOutOfRange`] if `rotation` is not in `0..6`,
    /// [`PlayerError::Exists`] if the name is taken, or whatever SQLite
    /// refused.
    pub fn add_player(&self, name: &str, origin: Coord, rotation: u8) -> Result<(), PlayerError> {
        // Validated on write as well as on read. The CHECK constraint in
        // migration 2 would catch this too, but it would arrive as a constraint
        // violation naming a column rather than as the variant a caller can
        // match on.
        let rotation = checked_rotation(name, i64::from(rotation))?;

        // The existence check and the insert are one transaction, so two
        // callers racing to claim a name cannot both find it free.
        let transaction = self.connection().unchecked_transaction()?;
        let taken: i64 = transaction.query_row(
            "SELECT count(*) FROM player WHERE name = ?1",
            [name],
            |row| row.get(0),
        )?;
        if taken != 0 {
            // Dropping the transaction rolls it back; nothing was written.
            return Err(PlayerError::Exists(name.to_owned()));
        }
        transaction.execute(
            "INSERT INTO player (name, origin_q, origin_r, rotation) VALUES (?1, ?2, ?3, ?4)",
            params![
                name,
                i64::from(origin.q()),
                i64::from(origin.r()),
                i64::from(rotation),
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// The frame belonging to one player, or `None` if there is no such player.
    ///
    /// # Errors
    ///
    /// [`PlayerError::RotationOutOfRange`] if the stored rotation is not a
    /// direction index, or whatever SQLite refused.
    pub fn player(&self, name: &str) -> Result<Option<Player>, PlayerError> {
        let row = self.connection().query_row(
            "SELECT name, origin_q, origin_r, rotation FROM player WHERE name = ?1",
            [name],
            stored_player,
        );
        match row {
            Ok(stored) => Ok(Some(stored.into_player()?)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Every player, in name order.
    ///
    /// `ORDER BY name` is the primary key's own order, so this is the ordered
    /// scan the `WITHOUT ROWID` table exists to make cheap.
    ///
    /// # Errors
    ///
    /// [`PlayerError::RotationOutOfRange`] if any stored rotation is not a
    /// direction index, or whatever SQLite refused.
    pub fn players(&self) -> Result<Vec<Player>, PlayerError> {
        let mut statement = self
            .connection()
            .prepare("SELECT name, origin_q, origin_r, rotation FROM player ORDER BY name")?;
        let rows = statement.query_map([], stored_player)?;
        let mut players = Vec::new();
        for row in rows {
            players.push(row?.into_player()?);
        }
        Ok(players)
    }
}

/// One row as SQLite stores it, before the rotation has been believed.
struct StoredPlayer {
    name: String,
    origin_q: i64,
    origin_r: i64,
    rotation: i64,
}

impl StoredPlayer {
    /// The player, if the stored rotation is a direction index.
    fn into_player(self) -> Result<Player, PlayerError> {
        let rotation = checked_rotation(&self.name, self.rotation)?;
        Ok(Player {
            name: self.name,
            // Through `Coord::new`, so a hand-edited out-of-range origin
            // normalizes to the tile it names rather than becoming a coordinate
            // the type says cannot exist. An origin is a tile; a rotation is
            // not, which is why only one of the two is repaired.
            origin: Coord::new(self.origin_q, self.origin_r),
            rotation,
        })
    }
}

/// Reads the four columns without interpreting them.
fn stored_player(row: &rusqlite::Row<'_>) -> Result<StoredPlayer, rusqlite::Error> {
    Ok(StoredPlayer {
        name: row.get(0)?,
        origin_q: row.get(1)?,
        origin_r: row.get(2)?,
        rotation: row.get(3)?,
    })
}

/// A rotation that is a direction index, or the typed refusal.
fn checked_rotation(player: &str, found: i64) -> Result<u8, PlayerError> {
    let count = i64::try_from(DIRECTION_COUNT).expect("six fits an i64");
    if !(0..count).contains(&found) {
        return Err(PlayerError::RotationOutOfRange {
            player: player.to_owned(),
            found,
        });
    }
    u8::try_from(found).map_err(|_| PlayerError::RotationOutOfRange {
        player: player.to_owned(),
        found,
    })
}
