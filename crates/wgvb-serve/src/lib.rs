//! A local web viewer for one WGVB seed, with hex-direction scrolling.
//!
//! `wgvb-map` renders one window to one file, so looking at a world means
//! running it, opening the PNG, working out the next window's coordinates by
//! hand, and running it again. That is fine for recording a golden image and
//! hopeless for finding out what a seed looks like, which is the thing the
//! renderer exists for: `DESIGN.md` section 29 says visual quality cannot be
//! established by unit tests, and the tool that establishes it should not cost a
//! shell round trip per window.
//!
//! # Everything is a URL
//!
//! ```text
//! GET /seed/{seed}                        the viewer page, centered on the origin
//! GET /seed/{seed}?q=-87&r=6543           the viewer page, centered on (-87, 6543)
//! GET /seed/{seed}/map.png?q=..&r=..      the rendered window itself
//! ```
//!
//! **This is deliberately not a single-page application.** No client-side
//! panning, no canvas, no script needed to move the view. Page refresh is fine.
//! Every state the viewer can be in is a URL, which also makes "look at this" a
//! link somebody can paste into an issue.
//!
//! `{seed}` is sixteen hexadecimal digits, case-insensitive, with no `0x`,
//! because hex is how a seed is written everywhere else in the repository.
//! Coordinates are **canonical**, not frame-relative: this viewer has no
//! player, so a canonical link means the same tile to everybody and is the same
//! number `wgvb-map --q --r` takes.
//!
//! # Bounded, like everything else that renders
//!
//! A request costs `cols * rows` generator calls, and seven times that for the
//! relief layer, all chosen by whoever typed the URL. Tile counts and the hex
//! radius are clamped before a viewport is built, and a window that is still
//! too large for [`wgvb_render::MAX_IMAGE_PIXELS`] is a 400 rather than a 500.
//! The clamp is the answer rather than a cache; section 26 asks for a profile
//! before a cache.
//!
//! **The default bind address is `127.0.0.1`.** This is a diagnostic tool with
//! no authentication and an endpoint whose cost the caller chooses.
//!
//! # This is not a saved world
//!
//! The generator is built in memory from the seed in the route and the default
//! configuration, so output is diagnostic and does not represent a saved world.
//! The page says so. When persistence lands this server takes a database, the
//! database supplies the seed, algorithm version, and effective configuration,
//! and the seed in the route becomes a check against the stored one rather than
//! the source of it.

mod page;
mod reply;
mod view;

use std::io;
use std::net::{IpAddr, ToSocketAddrs};
use std::thread;

use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

pub use page::page;
pub use reply::{HTML, PNG, Reply, TEXT, reply};
pub use view::{
    Axis, COMPASS, Compass, DEFAULT_COLS, DEFAULT_HEX_RADIUS, DEFAULT_ROWS, MAX_COLS,
    MAX_HEX_RADIUS, MAX_ROWS, MIN_HEX_RADIUS, RequestError, Route, View,
};

/// How the server was asked to listen.
#[derive(Debug, Clone)]
pub struct Options {
    /// Interface to bind. Loopback by default, and deliberately so.
    pub host: String,
    /// Port to bind.
    pub port: u16,
    /// Worker threads. Rendering is CPU-bound, so this is a pool sized to the
    /// machine rather than an async runtime: there is no IO here to overlap.
    pub workers: usize,
    /// Whether to print one line per request.
    pub log: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            host: DEFAULT_HOST.to_string(),
            port: DEFAULT_PORT,
            workers: default_workers(),
            log: true,
        }
    }
}

/// The default bind address. Not `0.0.0.0`, and not by accident.
pub const DEFAULT_HOST: &str = "127.0.0.1";
/// The default port.
pub const DEFAULT_PORT: u16 = 8080;

/// Workers to run when the caller does not say: one per available core.
#[must_use]
pub fn default_workers() -> usize {
    thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get)
}

/// Why the server could not start.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("cannot resolve {address}: {source}")]
    Resolve { address: String, source: io::Error },
    #[error("no address to listen on for {address}")]
    NoAddress { address: String },
    #[error("cannot listen on {address}: {source}")]
    Bind {
        address: String,
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Listens, and serves until the process is stopped.
///
/// # Errors
///
/// Returns a [`ServeError`] if the address cannot be resolved or bound. Once
/// listening, a failure to answer one request is logged and the worker
/// continues: a malformed request must not take the server down.
pub fn serve(options: &Options) -> Result<(), ServeError> {
    let address = format!("{}:{}", options.host, options.port);
    let resolved = address
        .to_socket_addrs()
        .map_err(|source| ServeError::Resolve {
            address: address.clone(),
            source,
        })?
        .next()
        .ok_or_else(|| ServeError::NoAddress {
            address: address.clone(),
        })?;

    if !resolved.ip().is_loopback() {
        eprintln!(
            "wgvb-serve: warning: listening on {resolved}, which is not loopback. \
             This viewer has no authentication and renders whatever window the \
             caller asks for."
        );
    }

    let server = Server::http(resolved).map_err(|source| ServeError::Bind {
        address: address.clone(),
        source,
    })?;

    let workers = options.workers.max(1);
    println!(
        "wgvb-serve: listening on http://{resolved}/ with {workers} worker{}",
        if workers == 1 { "" } else { "s" }
    );
    println!(
        "wgvb-serve: try http://{resolved}{}",
        View::origin_of(0).page_url()
    );

    thread::scope(|scope| {
        for _ in 1..workers {
            scope.spawn(|| work(&server, options.log));
        }
        work(&server, options.log);
    });

    Ok(())
}

/// One worker's whole life: take a request, answer it, take the next.
fn work(server: &Server, log: bool) {
    while let Ok(request) = server.recv() {
        let target = request.url().to_string();
        let method = request.method().clone();
        match answer(request) {
            Ok(status) if log => println!("wgvb-serve: {method} {target} {status}"),
            Ok(_) => {}
            Err(error) => eprintln!("wgvb-serve: {method} {target} failed: {error}"),
        }
    }
}

/// Answers one request, returning the status it sent.
fn answer(request: Request) -> io::Result<u16> {
    let method = request.method().clone();
    if method != Method::Get && method != Method::Head {
        let status = 405;
        request.respond(
            Response::from_string("only GET and HEAD are served here\n")
                .with_status_code(StatusCode(status))
                .with_header(content_type(TEXT))
                .with_header(header("Allow", "GET, HEAD")),
        )?;
        return Ok(status);
    }

    let reply = reply(request.url());

    // A strong `ETag` is only worth a header if something revalidates against
    // it. `tiny_http` sends no body for a HEAD, so one code path serves both.
    let known = request
        .headers()
        .iter()
        .find(|header| header.field.equiv("If-None-Match"))
        .map(|header| header.value.as_str().to_string());
    if let (Some(tag), Some(known)) = (reply.etag.as_deref(), known.as_deref())
        && known.split(',').any(|candidate| candidate.trim() == tag)
    {
        let status = 304;
        request.respond(
            Response::empty(StatusCode(status))
                .with_header(header("ETag", tag))
                .with_header(header("Cache-Control", "no-cache")),
        )?;
        return Ok(status);
    }

    let status = reply.status;
    let mut response = Response::from_data(reply.body)
        .with_status_code(StatusCode(status))
        .with_header(content_type(reply.content_type));
    if let Some(tag) = &reply.etag {
        // `no-cache` means "revalidate", not "do not store": the browser keeps
        // the image and asks whether the tag still matches, which is exactly
        // what a tag built from the algorithm and render versions is for.
        response = response
            .with_header(header("ETag", tag.as_str()))
            .with_header(header("Cache-Control", "no-cache"));
    }
    if let Some(location) = &reply.location {
        response = response.with_header(header("Location", location.as_str()));
    }
    request.respond(response)?;
    Ok(status)
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
/// literal or a URL this crate built out of hexadecimal digits and decimal
/// numbers, so a panic here is a bug in this file rather than an input.
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
