-- Schema version 0 -> 1. Never edit a released migration (DESIGN.md 27.4).
--
-- One database holds exactly one world, so no table carries a `world_id`
-- column. Every coordinate-keyed table is WITHOUT ROWID with PRIMARY KEY
-- (q, r): the table then *is* a B-tree keyed by coordinate, so loading a
-- viewport or a chunk is one ordered range scan rather than a rowid lookup per
-- row through a secondary index (DESIGN.md 27.2).

-- Singleton world metadata. The `id` column exists only to pin the row count
-- at one; the CHECK is what makes a second world a constraint violation rather
-- than a silent second row that some later query picks arbitrarily.
--
-- `seed` is the u64 world seed reinterpreted as the i64 SQLite stores, not a
-- narrowed value: every bit pattern round-trips.
--
-- `config` is the canonical CBOR of the complete effective configuration,
-- including every value that came from a default, and `fingerprint` is the
-- SHA-256 of the algorithm version and those bytes (DESIGN.md 21.2). Storing
-- the bytes rather than re-serializing on read is what lets the fingerprint
-- gate detect tampering with either one.
CREATE TABLE world (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    seed              INTEGER NOT NULL,
    algorithm_version INTEGER NOT NULL,
    config            BLOB    NOT NULL,
    fingerprint       BLOB    NOT NULL
);

-- Fog of war. A row means the player has seen this tile. Authoritative player
-- state: it is not derivable from the seed and it must survive discarding
-- every generated cache, which is why no foreign key points at one
-- (DESIGN.md 27.3).
CREATE TABLE overlay_discovery (
    q INTEGER NOT NULL,
    r INTEGER NOT NULL,
    PRIMARY KEY (q, r)
) WITHOUT ROWID;

-- Named places the player has founded or recorded.
CREATE TABLE overlay_settlement (
    q    INTEGER NOT NULL,
    r    INTEGER NOT NULL,
    name TEXT    NOT NULL,
    PRIMARY KEY (q, r)
) WITHOUT ROWID;

PRAGMA user_version = 1;
