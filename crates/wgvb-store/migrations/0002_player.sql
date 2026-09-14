-- Schema version 1 -> 2. Never edit a released migration (DESIGN.md 27.4).
--
-- Player frames: where each player's own (0, 0) sits in canonical coordinates,
-- and which absolute direction they call north. Appendix A, *Coordinate
-- frames*, is the transform these three scalars feed:
--
--     absolute           = normalize( rotate^k ( relative ) + player_origin )
--     absolute_direction = ( player_direction + k ) mod 6
--
-- **A rung rather than an edit to 0001.** Folding these columns into the
-- initial migration was defensible while nothing had been written with it —
-- the only tag is v0.5.0, well before 0001 existed — but it stopped being
-- free the moment `wgvb-map --db` created a real file: anyone holding a
-- `.wgvb` from that build has a database whose schema would no longer match a
-- rewritten migration 1, and no gate would notice. A rung costs one file.
--
-- Keyed by player identity, not by (q, r): the origin is a *value* here, and
-- one player has one frame. WITHOUT ROWID for the reason section 27.2 gives —
-- the table then is a B-tree in the key's own order, so listing players is one
-- ordered scan and looking one up is one descent rather than a rowid hop
-- through a secondary index.
--
-- The frame is authoritative player state (section 27.6): it is never derived
-- from the seed, never regenerated, and survives discarding every cache. No
-- foreign key points at generated data (section 27.3), and there is no
-- `world_id` column (section 27.6).
--
-- `rotation` is a direction index. The CHECK is the third of three guards, not
-- the only one — `wgvb-store` validates on write and again on read, because a
-- CHECK says nothing about a row written by a tool that is not this one.
CREATE TABLE player (
    name     TEXT    NOT NULL PRIMARY KEY,
    origin_q INTEGER NOT NULL,
    origin_r INTEGER NOT NULL,
    rotation INTEGER NOT NULL CHECK (rotation BETWEEN 0 AND 5)
) WITHOUT ROWID;

PRAGMA user_version = 2;
