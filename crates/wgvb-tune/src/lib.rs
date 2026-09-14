//! A local web instrument for tuning a WGVB configuration.
//!
//! `DESIGN.md` section 32's remaining work is tuning frequencies, weights,
//! thresholds, and warp strengths until the world looks right, and section 29
//! says that judgement cannot be made by unit tests. This is the instrument it
//! gets made in.
//!
//! # Not the viewer, and not its successor
//!
//! `wgvb-serve` looks at a saved world and keeps one promise above all others:
//! **every state it can be in is a URL**, so a link means the same thing to
//! everybody who opens it. This server cannot keep that promise, because the
//! thing it exists to change is a hundred numeric fields and a hundred fields
//! do not fit in an address bar.
//!
//! So the two tools are separate, and each is allowed to be what it is. The
//! viewer is stateless. The tuner holds one configuration in memory, changes it
//! through a form, and prints its fingerprint on every page so that a picture
//! can still be traced to the configuration that made it.
//!
//! ```text
//! GET  /seed/{seed}                  the hex map, as the viewer draws it
//! GET  /seed/{seed}/map.png          that window's image
//! GET  /seed/{seed}/grid             one pixel per hex, for a very large area
//! GET  /seed/{seed}/grid.png         that window's image
//! GET  /seed/{seed}/config           the whole configuration, as a form
//! POST /seed/{seed}/config/fields    apply the form
//! POST /seed/{seed}/config/upload    adopt an uploaded file
//! POST /seed/{seed}/config/reset     go back to this binary's defaults
//! GET  /config.toml                  download what is being drawn
//! ```
//!
//! # What it cannot do
//!
//! Open a world, create one, or write to one. `wgvb-store` is not in this
//! crate's dependency graph, so that is a property of the build rather than a
//! promise in a comment. The only file it produces is a configuration somebody
//! asked it to download.
//!
//! # Bounded, like everything else that renders
//!
//! A grid window is a million tiles by default and can be four. The clamp is a
//! budget counted in generator evaluations rather than in tiles, because
//! `relief`, `climate`, and `terrain` cost seven apiece; see
//! [`route::DEFAULT_BUDGET`]. Every request that renders logs what it cost,
//! because measuring that is half of why this tool exists.

mod form;
mod page;
mod reply;
pub mod route;
mod session;

use std::io;
use std::net::{IpAddr, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use wgvb::{Config, Seed};

pub use reply::{HTML, PNG, Reply, TEXT, TOML, answer};
pub use route::{Change, Tab, Target, Window};
pub use session::{Session, State};

/// The default bind address. Not `0.0.0.0`, and not by accident: this is a
/// diagnostic tool with no authentication that renders whatever window the
/// caller asks for, and now also accepts a `POST` that changes what it draws.
pub const DEFAULT_HOST: &str = "127.0.0.1";
/// The default port. One above the viewer's, so both can run at once — which is
/// how two configurations get compared.
pub const DEFAULT_PORT: u16 = 8081;
/// The seed a bare `/` lands on when the operator does not choose one.
pub const DEFAULT_SEED: Seed = 0x0123_4567_89ab_cdef;

/// Largest request body this server will read, in bytes.
///
/// A configuration file is a few kilobytes. This is three orders of magnitude
/// more than that and still small enough that a mistaken upload cannot exhaust
/// memory.
pub const MAX_BODY_BYTES: usize = 256 * 1024;

/// Revision of the tuner's markup, for the `ETag`.
///
/// The same argument as the viewer's `PAGE_VERSION`: the algorithm version
/// covers the world and the render version covers the pixels, and neither says
/// anything about the HTML, so a release that changes the page alone would
/// serve a stale one to a browser holding the old bytes.
///
/// # History
///
/// - **1** — the tuner as it first shipped.
/// - **2** — the distribution readout. The map tab now says what is *in* the
///   window as well as drawing it.
pub const PAGE_VERSION: u32 = 2;

/// How the server was asked to listen, and what it was asked to draw.
#[derive(Debug, Clone)]
pub struct Options {
    /// The seed `/` redirects to. Every other seed is still servable by typing
    /// it, because the route is where a seed comes from.
    pub seed: Seed,
    /// A configuration file to start from, or `None` for this binary's
    /// defaults.
    pub config: Option<PathBuf>,
    /// Interface to bind. Loopback by default, and deliberately so.
    pub host: String,
    /// Port to bind.
    pub port: u16,
    /// Worker threads.
    pub workers: usize,
    /// Generator evaluations one request may cost.
    pub budget: u64,
    /// Whether to print one line per request.
    pub log: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            seed: DEFAULT_SEED,
            config: None,
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            workers: default_workers(),
            budget: route::DEFAULT_BUDGET,
            log: true,
        }
    }
}

/// Workers to run when the caller does not say: one per available core.
#[must_use]
pub fn default_workers() -> usize {
    thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get)
}

/// Why the server could not start.
#[derive(Debug, thiserror::Error)]
pub enum TuneError {
    #[error("cannot resolve {address}: {source}")]
    Resolve { address: String, source: io::Error },
    #[error("no address to listen on for {address}")]
    NoAddress { address: String },
    #[error("cannot listen on {address}: {source}")]
    Bind {
        address: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("cannot read {path}: {source}")]
    ReadConfig { path: String, source: io::Error },
    /// The configuration this server was told to start from is not one.
    ///
    /// Refused before the port is bound. A configuration this binary cannot
    /// draw is a server that should not start, not a server that answers every
    /// request with an error.
    #[error("{0}")]
    Config(#[from] wgvb_config::FileError),
}

/// Reads the configuration a run starts from.
///
/// # Errors
///
/// [`TuneError::ReadConfig`] if the file cannot be read, and
/// [`TuneError::Config`] if it is not a configuration this binary can draw.
pub fn starting_config(path: Option<&std::path::Path>) -> Result<Config, TuneError> {
    match path {
        None => Ok(Config::default()),
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|source| TuneError::ReadConfig {
                path: path.display().to_string(),
                source,
            })?;
            Ok(wgvb_config::from_toml(&text)?)
        }
    }
}

/// Listens, and serves until the process is stopped.
///
/// # Errors
///
/// Returns a [`TuneError`] if the configuration cannot be read or the address
/// cannot be bound. Once listening, a failure to answer one request is logged
/// and the worker continues.
pub fn serve(options: &Options) -> Result<(), TuneError> {
    let config = starting_config(options.config.as_deref())?;
    let session = Arc::new(Session::new(options.seed, config)?);

    let address = format!("{}:{}", options.host, options.port);
    let resolved = address
        .to_socket_addrs()
        .map_err(|source| TuneError::Resolve {
            address: address.clone(),
            source,
        })?
        .next()
        .ok_or_else(|| TuneError::NoAddress {
            address: address.clone(),
        })?;

    if !resolved.ip().is_loopback() {
        eprintln!(
            "wgvb-tune: warning: listening on {resolved}, which is not loopback. \
             This instrument has no authentication, renders whatever window the \
             caller asks for, and accepts a POST that changes what it draws."
        );
    }

    let server = Server::http(resolved).map_err(|source| TuneError::Bind {
        address: address.clone(),
        source,
    })?;

    let workers = options.workers.max(1);
    println!(
        "wgvb-tune: listening on http://{resolved}/ with {workers} worker{}",
        if workers == 1 { "" } else { "s" }
    );
    println!(
        "wgvb-tune: configuration {} ({} evaluations per request)",
        session.state().short_fingerprint(),
        options.budget,
    );
    println!(
        "wgvb-tune: try http://{resolved}{}",
        Window::origin_of(options.seed).url(Tab::Map)
    );

    let server = &server;
    thread::scope(|scope| {
        for _ in 1..workers {
            let session = Arc::clone(&session);
            scope.spawn(move || work(server, &session, options));
        }
        work(server, &session, options);
    });

    Ok(())
}

/// One worker's whole life: take a request, answer it, take the next.
fn work(server: &Server, session: &Session, options: &Options) {
    while let Ok(request) = server.recv() {
        let target = request.url().to_string();
        let method = request.method().clone();
        match respond(request, session, options) {
            Ok((status, note)) if options.log => {
                println!("wgvb-tune: {method} {target} {status}{note}");
            }
            Ok(_) => {}
            Err(error) => eprintln!("wgvb-tune: {method} {target} failed: {error}"),
        }
    }
}

/// Answers one request, returning the status it sent and what it cost.
fn respond(
    mut request: Request,
    session: &Session,
    options: &Options,
) -> io::Result<(u16, String)> {
    let method = request.method().clone();
    if method != Method::Get && method != Method::Head && method != Method::Post {
        let status = 405;
        request.respond(
            Response::from_string("only GET, HEAD, and POST are served here\n")
                .with_status_code(StatusCode(status))
                .with_header(content_type(TEXT))
                .with_header(header("Allow", "GET, HEAD, POST")),
        )?;
        return Ok((status, String::new()));
    }

    // A POST changes what this server draws, and any page a browser visits can
    // send one to a loopback port. Same-origin is the whole check: a
    // cross-site form post arrives as `Sec-Fetch-Site: cross-site`, and a
    // browser that sends neither header is one that cannot have been steered
    // by a page either.
    if method == Method::Post && !same_origin(&request) {
        let status = 403;
        request.respond(
            Response::from_string("this request did not come from this server's own page\n")
                .with_status_code(StatusCode(status))
                .with_header(content_type(TEXT)),
        )?;
        return Ok((status, String::new()));
    }

    let content_type_header = request
        .headers()
        .iter()
        .find(|header| header.field.equiv("Content-Type"))
        .map(|header| header.value.as_str().to_string())
        .unwrap_or_default();
    let body = read_body(&mut request)?;

    let known = request
        .headers()
        .iter()
        .find(|header| header.field.equiv("If-None-Match"))
        .map(|header| header.value.as_str().to_string());

    let target = request.url().to_string();
    let posting = method == Method::Post;
    let reply = answer(
        &target,
        posting,
        &content_type_header,
        &body,
        session,
        options.budget,
    );

    if let (Some(tag), Some(known)) = (reply.etag.as_deref(), known.as_deref())
        && known.split(',').any(|candidate| candidate.trim() == tag)
    {
        let status = 304;
        request.respond(
            Response::empty(StatusCode(status))
                .with_header(header("ETag", tag))
                .with_header(header("Cache-Control", "no-cache")),
        )?;
        return Ok((status, String::new()));
    }

    let status = reply.status;
    let note = reply.note.clone();
    let mut response = Response::from_data(reply.body)
        .with_status_code(StatusCode(status))
        .with_header(content_type(reply.content_type));
    if let Some(tag) = &reply.etag {
        response = response
            .with_header(header("ETag", tag.as_str()))
            .with_header(header("Cache-Control", "no-cache"));
    }
    if let Some(location) = &reply.location {
        response = response.with_header(header("Location", location.as_str()));
    }
    if let Some(filename) = &reply.download {
        response = response.with_header(header(
            "Content-Disposition",
            &format!("attachment; filename=\"{filename}\""),
        ));
    }
    if let Some(timing) = &reply.timing {
        // Readable in a browser's network panel without a page of its own.
        response = response.with_header(header("Server-Timing", timing.as_str()));
    }
    request.respond(response)?;
    Ok((status, note))
}

/// Reads a request body, refusing one that is absurdly large.
fn read_body(request: &mut Request) -> io::Result<Vec<u8>> {
    // Exactly what the request says it carries, capped. A body larger than the
    // cap is truncated rather than streamed: a configuration file is a few
    // kilobytes, and nothing here has a use for a request that is not one.
    let length = request.body_length().unwrap_or(0).min(MAX_BODY_BYTES);
    let mut body = vec![0_u8; length];
    request.as_reader().read_exact(&mut body)?;
    Ok(body)
}

/// Whether a request came from this server's own pages.
fn same_origin(request: &Request) -> bool {
    let site = request
        .headers()
        .iter()
        .find(|header| header.field.equiv("Sec-Fetch-Site"))
        .map(|header| header.value.as_str().to_string());
    match site.as_deref() {
        Some("same-origin") | Some("none") | None => true,
        Some(_) => false,
    }
}

/// A `Content-Type` header.
fn content_type(value: &str) -> Header {
    header("Content-Type", value)
}

/// One header, from two strings this crate controls.
///
/// # Panics
///
/// Panics on a field name or value that is not ASCII. Every call site passes a
/// literal or a string this crate built out of hexadecimal digits and decimal
/// numbers.
fn header(field: &str, value: &str) -> Header {
    Header::from_bytes(field.as_bytes(), value.as_bytes())
        .expect("this crate's header names and values are ASCII")
}

/// Whether a host string names a loopback interface, for the CLI's warning.
#[must_use]
pub fn is_loopback(host: &str) -> bool {
    host.parse::<IpAddr>()
        .map_or(host.eq_ignore_ascii_case("localhost"), |ip| {
            ip.is_loopback()
        })
}
