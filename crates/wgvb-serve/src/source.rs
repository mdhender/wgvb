//! Where a request's world comes from.
//!
//! `DESIGN.md` section 29.1 always said this: *when persistence lands this
//! server takes a database, the database supplies the seed, algorithm version,
//! and effective configuration, and the seed in the route becomes a check
//! against the stored one rather than the source of it.* This module is that
//! sentence.
//!
//! # Why a source is per worker and not shared
//!
//! `wgvb_store::World` is `Send` and not `Sync`, because `rusqlite::Connection`
//! is not. Section 27.7 gives two ways to live with that — one connection per
//! thread, or one behind a `Mutex` — and this server takes the first, because
//! the second would serialize every overlay read behind one lock in a server
//! whose whole job is to run renders in parallel. Every worker opens the
//! database for itself, and [`Source::open_all`] does it **before the port is
//! bound**, so a database this binary cannot read is a startup failure rather
//! than a connection refused halfway through an afternoon.

use std::path::Path;

use wgvb::{Config, Generator};
use wgvb_store::{Fingerprint, OpenError, World, fingerprint};

/// What one worker knows about the world it is drawing.
///
/// The two variants are wildly different sizes — a fingerprint against an open
/// database and a whole field graph — and that costs nothing here, which is why
/// it is not boxed. Exactly one `Source` exists per worker thread and it lives
/// for the life of the process; the enum is never in a collection, never moved
/// in a hot path, and never allocated twice.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "one per worker thread for the life of the process; boxing would \
              add an indirection to every request to save nothing"
)]
pub enum Source {
    /// No database. Any seed in the route is served, built in memory from the
    /// program defaults, and output is diagnostic.
    Defaults {
        /// The fingerprint of [`Config::default`], so the `ETag` names the
        /// configuration behind the pixels even when nobody chose it.
        fingerprint: Fingerprint,
    },
    /// One stored world. The seed in the route is a check against this one.
    Stored {
        /// The open database, for overlay reads.
        world: World,
        /// Built once from the stored seed and the stored configuration, not
        /// once per request: unlike the defaults case there is exactly one
        /// world here, so there is exactly one generator.
        generator: Generator,
    },
}

impl Source {
    /// One source per worker, opened before the server binds.
    ///
    /// # Errors
    ///
    /// Whichever opening gate of section 27.5 refused. A source that cannot be
    /// opened is a server that does not start.
    pub fn open_all(database: Option<&Path>, workers: usize) -> Result<Vec<Source>, OpenError> {
        (0..workers.max(1))
            .map(|_| Source::open(database))
            .collect()
    }

    /// One source.
    ///
    /// # Errors
    ///
    /// Whichever opening gate of section 27.5 refused, or the canonical
    /// encoding of the default configuration in the no-database case, which
    /// cannot fail for a configuration this binary shipped.
    pub fn open(database: Option<&Path>) -> Result<Source, OpenError> {
        match database {
            None => Ok(Source::Defaults {
                fingerprint: fingerprint(wgvb::ALGORITHM_VERSION, &Config::default())?,
            }),
            Some(path) => {
                let world = World::open(path)?;
                let generator = world.generator()?;
                Ok(Source::Stored { world, generator })
            }
        }
    }

    /// The fingerprint of the configuration behind everything this source
    /// draws.
    ///
    /// The `ETag` in `reply.rs` carried a note saying it would start lying the
    /// moment a configuration could vary. This is what stops that: a page
    /// rendered under one configuration and a page rendered under another now
    /// carry different tags even when every other part of the URL agrees.
    #[must_use]
    pub const fn fingerprint(&self) -> &Fingerprint {
        match self {
            Source::Defaults { fingerprint } => fingerprint,
            Source::Stored { world, .. } => world.fingerprint(),
        }
    }

    /// The stored world, if there is one.
    #[must_use]
    pub const fn world(&self) -> Option<&World> {
        match self {
            Source::Defaults { .. } => None,
            Source::Stored { world, .. } => Some(world),
        }
    }

    /// The first eight hexadecimal digits of the fingerprint, for a tag and a
    /// line a human reads.
    ///
    /// Four bytes is enough to notice that two views came from different
    /// configurations. Nothing compares configurations by this prefix; the
    /// store compares all thirty-two bytes.
    #[must_use]
    pub fn fingerprint_prefix(&self) -> String {
        self.fingerprint()[..4]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}
