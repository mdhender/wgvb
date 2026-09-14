//! The one thing this server remembers.
//!
//! `wgvb-serve` remembers nothing: every state it can be in is a URL, so a link
//! means the same thing to everybody who opens it. This server cannot keep that
//! promise, because the state it exists to change is a hundred numeric fields
//! and a hundred fields do not fit in an address bar. So the configuration
//! lives here, in memory, for as long as the process runs.
//!
//! What is given up by that is real and worth naming: a link to a window is now
//! a link to *this server's* window, and an hour later it may show something
//! else. What is kept is the fingerprint. Every page prints it, so a picture
//! can always be traced back to the configuration that produced it, and the
//! download turns that configuration back into a file.
//!
//! # Why an `Arc` rather than a lock held across a render
//!
//! A render is seconds of arithmetic. A worker takes the read lock only long
//! enough to clone one `Arc` and then renders against a snapshot, so a
//! configuration change never waits for a million-tile grid to finish and a
//! grid never sees a configuration change halfway down the image.

use std::sync::{Arc, RwLock};

use wgvb::{ALGORITHM_VERSION, Config, Generator, Seed};
use wgvb_config::{FileError, Fingerprint, fingerprint};

/// One configuration, and everything derived from it that does not depend on a
/// seed.
#[derive(Debug, Clone)]
pub struct State {
    /// The complete effective configuration this server is currently drawing.
    pub config: Config,
    /// Its fingerprint, computed once when it is adopted rather than per
    /// request: it goes in every `ETag` and on every page.
    pub fingerprint: Fingerprint,
}

impl State {
    /// Adopts a configuration, refusing one that could not build a world.
    ///
    /// # Errors
    ///
    /// [`FileError::Invalid`] for a configuration the generator refuses, naming
    /// the field and the range it missed.
    pub fn new(config: Config) -> Result<State, FileError> {
        config.validate()?;
        let fingerprint = fingerprint(ALGORITHM_VERSION, &config)?;
        Ok(State {
            config,
            fingerprint,
        })
    }

    /// The first eight hexadecimal digits of the fingerprint, for a line a
    /// human reads.
    #[must_use]
    pub fn short_fingerprint(&self) -> String {
        self.fingerprint[..4]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// The whole fingerprint, for the `ETag`.
    ///
    /// Eight bytes rather than the four a person reads. A tuning session walks
    /// through hundreds of configurations under otherwise identical URLs, and a
    /// strong validator that collided would serve one configuration's pixels
    /// for another's.
    #[must_use]
    pub fn tag_fingerprint(&self) -> String {
        self.fingerprint[..8]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// A generator for one seed under this configuration.
    ///
    /// Built per request rather than held, because the seed comes from the
    /// route and every seed is servable. It costs a field graph — microseconds
    /// — against a window that costs seconds.
    ///
    /// # Panics
    ///
    /// Panics if the configuration does not validate, which [`State::new`] has
    /// already established.
    #[must_use]
    pub fn generator(&self, seed: Seed) -> Generator {
        Generator::new(seed, self.config.clone())
            .expect("a state holds a configuration that already validated")
    }
}

/// The configuration this server is holding, and the swap that replaces it.
#[derive(Debug)]
pub struct Session {
    state: RwLock<Arc<State>>,
    seed: Seed,
}

impl Session {
    /// A session over one configuration, landing on one seed.
    ///
    /// # Errors
    ///
    /// [`FileError::Invalid`] if the configuration could not build a world.
    pub fn new(seed: Seed, config: Config) -> Result<Session, FileError> {
        Ok(Session {
            state: RwLock::new(Arc::new(State::new(config)?)),
            seed,
        })
    }

    /// The seed `/` redirects to. The route is still where a seed comes from,
    /// so every other seed is servable by typing it.
    #[must_use]
    pub const fn seed(&self) -> Seed {
        self.seed
    }

    /// A snapshot to render against.
    ///
    /// # Panics
    ///
    /// Panics if the lock was poisoned, which means a worker panicked while
    /// holding it. Continuing with a configuration nobody can describe is worse
    /// than stopping.
    #[must_use]
    pub fn state(&self) -> Arc<State> {
        Arc::clone(&self.state.read().expect("the session lock is not poisoned"))
    }

    /// Adopts a new configuration, or refuses it and keeps the old one.
    ///
    /// Validation happens before the swap, so a rejected edit leaves the server
    /// showing exactly what it was showing.
    ///
    /// # Errors
    ///
    /// [`FileError::Invalid`] for a configuration the generator refuses.
    ///
    /// # Panics
    ///
    /// Panics if the lock was poisoned. See [`Session::state`].
    pub fn adopt(&self, config: Config) -> Result<(), FileError> {
        let state = Arc::new(State::new(config)?);
        *self
            .state
            .write()
            .expect("the session lock is not poisoned") = state;
        Ok(())
    }
}
