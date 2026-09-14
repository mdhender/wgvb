//! Why this server refused a request.
//!
//! Most refusals are about a URL and belong to [`wgvb_view::ViewError`], which
//! both front ends share. What is left is this server's own: it holds at most
//! one world, so it can be asked for a seed it does not have, and it reads a
//! database, so the database can stop answering. Neither of those is a thing
//! the tuner can ever say, which is why they are here and not there.

use wgvb::Seed;
use wgvb_view::ViewError;

/// Why a request was refused. One variant per gate, so tests assert on
/// variants rather than on message strings — `DESIGN.md` section 19.1, applied
/// to the route table.
#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    /// Something about the URL. See [`ViewError`] for the whole list.
    #[error(transparent)]
    View(#[from] ViewError),
    #[error(
        "this server holds the world with seed {stored:016x}, not {asked:016x}; \
         try /seed/{stored:016x}"
    )]
    OtherWorld { asked: Seed, stored: Seed },
    #[error("the world database could not be read: {0}")]
    World(String),
}

impl RequestError {
    /// The HTTP status this refusal answers with.
    ///
    /// Almost everything here is the caller's fault, so almost nothing here is
    /// a 500. A window too large for [`wgvb_render::MAX_IMAGE_PIXELS`]
    /// included: the caller chose the size.
    ///
    /// Two exceptions, and each is its own kind:
    ///
    /// - [`RequestError::OtherWorld`] is a 404. A server holding one world does
    ///   not have the seed that was asked for, and "not found" is what that is;
    ///   the message names the seed it does have, so the fix is a link away.
    /// - [`RequestError::World`] is a 500, and it is the only one. A database
    ///   that stops answering is the server's problem, not the caller's, and
    ///   reporting it as a 400 would send somebody looking at their URL for a
    ///   mistake that is not in it.
    #[must_use]
    pub const fn status(&self) -> u16 {
        match self {
            RequestError::View(error) => error.status(),
            RequestError::OtherWorld { .. } => 404,
            RequestError::World(_) => 500,
        }
    }
}
