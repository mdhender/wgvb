//! The singleton world: creation, the opening gates, and stored metadata.
//!
//! See `DESIGN.md` sections 27 and 27.5.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use wgvb::{ALGORITHM_VERSION, Config, Generator, Seed};

use crate::fingerprint::{Fingerprint, canonical_config_bytes, fingerprint_of_bytes};
use crate::schema::{SCHEMA_VERSION, application_id, migrate, user_version};
use crate::{APPLICATION_ID, OpenError};

/// One database, one world.
///
/// Holds the connection and the metadata that survived the opening gates. The
/// seed, algorithm version, and complete effective configuration are read from
/// the database and are never substituted from current program defaults — a
/// value the file does not carry is a malformed file, not a value to fill in.
///
/// `rusqlite::Connection` is `Send` but not `Sync`, so this type is too: give
/// each thread its own `World` or put one behind a `Mutex`. Section 27.7 says
/// not to build a pool before there is a measured reason, and there is not: the
/// generator is the hot part and it touches no database at all.
#[derive(Debug)]
pub struct World {
    connection: Connection,
    seed: Seed,
    algorithm_version: u32,
    config: Config,
    fingerprint: Fingerprint,
    created: bool,
}

impl World {
    /// Opens an existing world. Never creates one: a missing file is a
    /// [`rusqlite::Error`], not an empty world.
    ///
    /// # Errors
    ///
    /// One [`OpenError`] variant per gate of section 27.5.
    pub fn open(path: &Path) -> Result<World, OpenError> {
        let connection = connect(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        World::pass_the_gates(connection)
    }

    /// Opens the world at `path`, creating it from `seed` and `config` if the
    /// file is absent or is an entirely empty database.
    ///
    /// **Reopening never overrides.** When the file already holds a world, the
    /// arguments are ignored outright — the stored seed and the stored complete
    /// effective configuration are what generate the world, and a caller that
    /// wants to know whether its arguments were used asks
    /// [`World::was_created`].
    ///
    /// # Errors
    ///
    /// [`OpenError::Config`] if `config` is invalid, or one variant per gate of
    /// section 27.5 when an existing world is opened.
    pub fn open_or_create(path: &Path, seed: Seed, config: &Config) -> Result<World, OpenError> {
        // Validate before touching the filesystem. A rejected configuration
        // must not leave a half-made file behind — and this runs even when an
        // existing world is about to ignore the argument, because a caller
        // holding an invalid configuration has a bug either way and finding out
        // only on the day the file happens to be missing is worse.
        config.validate()?;

        let connection = connect(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )?;
        if is_empty(&connection)? {
            return World::initialize(connection, seed, config);
        }
        World::pass_the_gates(connection)
    }

    /// Gates 1 through 5, in order, with no application write until they pass.
    fn pass_the_gates(connection: Connection) -> Result<World, OpenError> {
        // Gate 1. A WGVA file is rejected here and goes no further.
        let found = application_id(&connection)?;
        if found != APPLICATION_ID {
            return Err(OpenError::WrongApplicationId {
                found,
                expected: APPLICATION_ID,
            });
        }

        // Gate 2. A newer schema is rejected before migration, because this
        // binary cannot know what a rung above its ladder did.
        let schema = user_version(&connection)?;
        if schema > SCHEMA_VERSION {
            return Err(OpenError::SchemaTooNew {
                found: schema,
                supported: SCHEMA_VERSION,
            });
        }

        // Gate 3. The only gate permitted to write, and only after 1 and 2.
        migrate(&connection, schema)?;

        // Gate 4. Read the singleton row, then hash the bytes that are
        // *stored* rather than bytes re-encoded from a decoded configuration.
        // Those differ precisely when somebody has edited the blob, which is
        // what this check is for.
        let (seed, algorithm_version, config_bytes, stored) = read_metadata(&connection)?;
        if fingerprint_of_bytes(algorithm_version, &config_bytes) != stored {
            return Err(OpenError::FingerprintMismatch);
        }

        // Gate 5, before decoding rather than after. A configuration written by
        // an algorithm version this binary cannot reproduce may legitimately
        // fail to decode — section 21.1 forbids `serde(default)`, so a world
        // from another version is *expected* to be missing fields — and
        // reporting that as malformed metadata would hide the real reason.
        if algorithm_version != ALGORITHM_VERSION {
            return Err(OpenError::UnsupportedGeneratorVersion(algorithm_version));
        }

        let config: Config = ciborium::from_reader(config_bytes.as_slice())
            .map_err(|error| OpenError::MalformedMetadata(error.to_string()))?;
        config.validate()?;

        Ok(World {
            connection,
            seed,
            algorithm_version,
            config,
            fingerprint: stored,
            created: false,
        })
    }

    /// Writes the complete effective configuration and its fingerprint into a
    /// fresh database, then returns the world they describe.
    fn initialize(connection: Connection, seed: Seed, config: &Config) -> Result<World, OpenError> {
        let config_bytes = canonical_config_bytes(config)?;
        let fingerprint = fingerprint_of_bytes(ALGORITHM_VERSION, &config_bytes);

        connection.pragma_update(None, "application_id", APPLICATION_ID)?;
        migrate(&connection, 0)?;
        connection.execute(
            "INSERT INTO world (id, seed, algorithm_version, config, fingerprint) \
             VALUES (1, ?1, ?2, ?3, ?4)",
            rusqlite::params![
                seed_to_sqlite(seed),
                i64::from(ALGORITHM_VERSION),
                config_bytes,
                fingerprint.as_slice(),
            ],
        )?;

        Ok(World {
            connection,
            seed,
            algorithm_version: ALGORITHM_VERSION,
            config: config.clone(),
            fingerprint,
            created: true,
        })
    }

    /// The stored world seed.
    #[must_use]
    pub const fn seed(&self) -> Seed {
        self.seed
    }

    /// The stored algorithm version. Equal to [`wgvb::ALGORITHM_VERSION`] for
    /// any world that got past gate 5.
    #[must_use]
    pub const fn algorithm_version(&self) -> u32 {
        self.algorithm_version
    }

    /// The stored complete effective configuration.
    #[must_use]
    pub const fn config(&self) -> &Config {
        &self.config
    }

    /// The stored configuration fingerprint. A cache entry is valid only
    /// against this value and, for rendered output, a render version.
    #[must_use]
    pub const fn fingerprint(&self) -> &Fingerprint {
        &self.fingerprint
    }

    /// Whether this call created the world rather than opening an existing one.
    #[must_use]
    pub const fn was_created(&self) -> bool {
        self.created
    }

    /// A generator for this world's baseline terrain.
    ///
    /// Built from the stored seed and stored configuration, so what it produces
    /// is what the file describes and not what the current program defaults
    /// would have produced.
    ///
    /// # Errors
    ///
    /// [`wgvb::ConfigError`] if the stored configuration is invalid, which gate
    /// 4 has already ruled out for a `World` that exists.
    pub fn generator(&self) -> Result<Generator, wgvb::ConfigError> {
        Generator::new(self.seed, self.config.clone())
    }

    /// Whether `PRAGMA foreign_keys` is on for this connection.
    ///
    /// Nothing in this schema has a foreign key — section 27.3 forbids one
    /// pointing at generated data — so this exists for the test that holds the
    /// pragma in place for the constraint somebody adds later.
    ///
    /// # Errors
    ///
    /// Whatever SQLite refused.
    pub fn foreign_keys_enabled(&self) -> Result<bool, rusqlite::Error> {
        self.connection
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
    }

    /// The connection, for overlay queries.
    pub(crate) const fn connection(&self) -> &Connection {
        &self.connection
    }
}

/// How long a connection waits for a locked database before giving up.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Opens a connection and sets the per-connection pragmas.
fn connect(path: &Path, flags: OpenFlags) -> Result<Connection, rusqlite::Error> {
    let connection = Connection::open_with_flags(path, flags)?;
    // No foreign key points at generated data and none ever should
    // (section 27.3), but a constraint deliberately added later between two
    // *authoritative* tables has to actually be enforced, and SQLite leaves
    // this off per connection by default.
    connection.pragma_update(None, "foreign_keys", "ON")?;
    // One world is routinely open in more than one process at once: the viewer
    // holds a read connection per worker while `wgvb-map --db --discover`
    // writes to the same file, which is the whole point of overlays being read
    // fresh on every request. SQLite's default is to give up on a locked
    // database immediately, which would turn an ordinary overlapping write into
    // a failed page load. Five seconds is far longer than any write here takes
    // and far shorter than a person's patience.
    connection.busy_timeout(BUSY_TIMEOUT)?;
    Ok(connection)
}

/// Whether this database has nothing in it at all.
///
/// An empty file is initialized rather than rejected by gate 1 — that is what
/// the parenthesis in section 27.5 means. Emptiness is *no tables and no
/// application id*: a file with tables but no application id is a foreign
/// database and must be rejected, not adopted.
fn is_empty(connection: &Connection) -> Result<bool, rusqlite::Error> {
    let tables: i64 =
        connection.query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))?;
    Ok(tables == 0 && application_id(connection)? == 0)
}

/// Reads the singleton row: seed, algorithm version, canonical config bytes,
/// and stored fingerprint.
fn read_metadata(connection: &Connection) -> Result<(Seed, u32, Vec<u8>, Fingerprint), OpenError> {
    let row = connection.query_row(
        "SELECT seed, algorithm_version, config, fingerprint FROM world WHERE id = 1",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, Vec<u8>>(3)?,
            ))
        },
    );

    let (seed, version, config_bytes, fingerprint) = match row {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Err(OpenError::MalformedMetadata(
                "the world table has no row with id 1".to_owned(),
            ));
        }
        Err(error) => return Err(error.into()),
    };

    let version = u32::try_from(version).map_err(|_| {
        OpenError::MalformedMetadata(format!("algorithm version {version} is not a u32"))
    })?;
    let fingerprint: Fingerprint = fingerprint.as_slice().try_into().map_err(|_| {
        OpenError::MalformedMetadata(format!(
            "fingerprint is {} bytes, expected {}",
            fingerprint.len(),
            crate::FINGERPRINT_LEN
        ))
    })?;

    Ok((seed_from_sqlite(seed), version, config_bytes, fingerprint))
}

/// The seed's bit pattern as the `i64` SQLite stores. Not a narrowing: every
/// `u64` round-trips, which a `TryFrom` would not allow for half of them.
const fn seed_to_sqlite(seed: Seed) -> i64 {
    i64::from_le_bytes(seed.to_le_bytes())
}

/// The inverse of [`seed_to_sqlite`].
const fn seed_from_sqlite(stored: i64) -> Seed {
    Seed::from_le_bytes(stored.to_le_bytes())
}
