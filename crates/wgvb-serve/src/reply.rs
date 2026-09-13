//! Turning a request target into a response, with no socket in sight.
//!
//! The routing, the rendering, and the error mapping are a pure function of the
//! request target, so every test in this crate is an ordinary unit test rather
//! than a client talking to a listening port. The `tiny_http` adapter in
//! [`crate::serve`] does nothing but move bytes between this function and a
//! connection.

use wgvb::{ALGORITHM_VERSION, Generator};
use wgvb_render::{Overlays, RENDER_VERSION, Viewport, encode_png, render_player};
use wgvb_store::{Bounds, World};

use crate::PAGE_VERSION;
use crate::page::page;
use crate::source::Source;
use crate::view::{RequestError, Route, View};

/// `text/html`, as this server writes it.
pub const HTML: &str = "text/html; charset=utf-8";
/// `text/plain`, which every refusal is answered in.
pub const TEXT: &str = "text/plain; charset=utf-8";
/// `image/png`, the one binary type here.
pub const PNG: &str = "image/png";

/// A complete response, minus the connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// HTTP status code.
    pub status: u16,
    /// The `Content-Type` header's value.
    pub content_type: &'static str,
    /// The body.
    pub body: Vec<u8>,
    /// A strong `ETag`, for the responses that are a pure function of their
    /// URL.
    pub etag: Option<String>,
    /// A `Location` header, for the one redirect this server performs.
    pub location: Option<String>,
}

impl Reply {
    /// The body as text, for a test reading an HTML page or an error message.
    ///
    /// # Panics
    ///
    /// Panics on a body that is not UTF-8, which only the PNG route produces.
    #[must_use]
    pub fn text(&self) -> &str {
        std::str::from_utf8(&self.body).expect("this reply's body is text")
    }
}

/// Answers one request target against the program defaults.
///
/// The convenience form, and what a server started without `--db` does for
/// every request. Output is diagnostic: the generator is built in memory from
/// the seed in the route, so it does not represent a saved world.
///
/// # Panics
///
/// Panics if the default configuration cannot be canonically encoded, which
/// would mean this binary shipped a configuration it cannot fingerprint.
#[must_use]
pub fn reply(target: &str) -> Reply {
    let source = Source::open(None).expect("the default configuration fingerprints");
    reply_from(target, &source)
}

/// Answers one request target.
///
/// Still a pure function of the target and the source: nothing is remembered
/// between requests, so nothing between requests can affect what is drawn. What
/// the source adds is *which world* — and, when that is a stored one, what the
/// player has seen and built, which is read fresh from the database on every
/// request because it is the one thing here that can change while the server
/// runs.
#[must_use]
pub fn reply_from(target: &str, source: &Source) -> Reply {
    // The one redirect: a bare root is not a state of the viewer, so it is sent
    // to one that is rather than being given a page of its own to keep in step.
    if target == "/" || target.is_empty() {
        let home = View::origin_of(0).page_url();
        return Reply {
            status: 302,
            content_type: TEXT,
            body: format!("the viewer lives at {home}\n").into_bytes(),
            etag: None,
            location: Some(home),
        };
    }

    match View::parse(target) {
        Ok((route, view)) => match draw(route, &view, source) {
            Ok(reply) => reply,
            Err(error) => refuse(&error),
        },
        Err(error) => refuse(&error),
    }
}

/// Renders whichever of the two routes was asked for.
///
/// The two arms differ in exactly one thing that matters: where the generator
/// comes from. A server without a database serves *any* seed, because the seed
/// in the route is the source of the world; a server with one serves exactly
/// the world it holds, because the seed in the route is a check against it.
fn draw(route: Route, view: &View, source: &Source) -> Result<Reply, RequestError> {
    let viewport = view.viewport()?;
    match source {
        Source::Defaults { .. } => {
            // One generator per request, built before the route is chosen: the
            // page reads the center tile and the image reads the whole window,
            // and two generators would be two chances to disagree about the
            // configuration behind one URL.
            let generator = Generator::with_defaults(view.seed);
            compose(
                route,
                view,
                &viewport,
                &generator,
                &Overlays::none(),
                source,
            )
        }
        Source::Stored { world, generator } => {
            // The check section 29.1 asked for. Serving the stored world under
            // somebody else's seed in the address bar would be a link that
            // means one thing to the person who pasted it and another to the
            // person who opens it.
            if view.seed != world.seed() {
                return Err(RequestError::OtherWorld {
                    asked: view.seed,
                    stored: world.seed(),
                });
            }
            let overlays = overlays_in(world, &viewport)?;
            compose(route, view, &viewport, generator, &overlays, source)
        }
    }
}

/// Draws the page or the image, once the world behind them is settled.
fn compose(
    route: Route,
    view: &View,
    viewport: &Viewport,
    generator: &Generator,
    overlays: &Overlays,
    source: &Source,
) -> Result<Reply, RequestError> {
    match route {
        Route::Page => Ok(Reply {
            status: 200,
            content_type: HTML,
            body: page(view, viewport, generator, overlays, source).into_bytes(),
            etag: Some(etag(view, "page", PAGE_VERSION, source)),
            location: None,
        }),
        Route::Image => {
            let image = render_player(generator, viewport, view.layer, overlays);
            Ok(Reply {
                status: 200,
                content_type: PNG,
                body: encode_png(&image)?,
                // No tag for a world-backed image, and that is the honest
                // answer rather than an omission. A strong `ETag` promises the
                // bytes are a pure function of everything it names, and a
                // player image also depends on the overlays — mutable state
                // with no version anywhere in the system. A tag that ignored
                // them would go on serving an unexplored map after the player
                // explored it, which is precisely the failure a strong
                // validator exists to prevent. See `DESIGN.md` section 29.2.
                etag: match source {
                    Source::Defaults { .. } => Some(etag(view, "png", RENDER_VERSION, source)),
                    Source::Stored { .. } => None,
                },
                location: None,
            })
        }
    }
}

/// Every overlay that could fall inside a window, in coordinate order.
///
/// One range scan over the smallest `(q, r)` box holding the window's tiles.
/// A superset is correct — an overlay outside the window is never drawn — and a
/// wrapped window has no box smaller than this one. Identical to what
/// `wgvb-map` does, through the same [`Viewport::coords`] and the same
/// [`Bounds`], because two front ends over one renderer must not drift.
fn overlays_in(world: &World, viewport: &Viewport) -> Result<Overlays, RequestError> {
    let bounds = Bounds::containing(viewport.coords()).unwrap_or_else(Bounds::everywhere);
    let discovered = world
        .discoveries_in(&bounds)
        .map_err(|error| RequestError::World(error.to_string()))?;
    let settlements = world
        .settlements_in(&bounds)
        .map_err(|error| RequestError::World(error.to_string()))?
        .into_iter()
        .map(|settlement| (settlement.coord, settlement.name))
        .collect();
    Ok(Overlays::new(discovered, settlements))
}

/// Answers a refusal as plain text.
///
/// Plain text on purpose: the message quotes back what was sent, and quoted
/// input is inert in a `text/plain` body in a way it is not in an HTML one.
fn refuse(error: &RequestError) -> Reply {
    Reply {
        status: error.status(),
        content_type: TEXT,
        body: format!("{error}\n").into_bytes(),
        etag: None,
        location: None,
    }
}

/// A strong `ETag` for a response.
///
/// The bytes are a pure function of the seed, the center, the window, the
/// layer, [`ALGORITHM_VERSION`], and whichever revision governs this
/// representation, so naming exactly those is a strong validator and costs
/// one header rather than a cache. `DESIGN.md` section 26 states the same
/// validity rule for any cached render and asks for a profile before an
/// actual cache, which this is not.
///
/// `revision` is the third input and the one that was missing: the page and
/// the image are two different representations of one view, and each has its
/// own thing that can change without the world changing. For the image that
/// is [`RENDER_VERSION`]; for the page it is [`PAGE_VERSION`], because the
/// markup is the server's and no version anywhere else described it. A page
/// whose readout gained a row kept its tag until this argument existed, and
/// browsers went on showing the page without the row.
///
/// The hex radius enters by `to_bits`, so two radii that print the same but are
/// not the same value cannot share a tag.
///
/// The configuration fingerprint is the fourth input, and it closes the hole
/// this function used to carry a warning about. A configuration can now vary —
/// `--db` is exactly that — so without it a page drawn from a stored world and
/// a page drawn from the defaults would share a tag while showing different
/// worlds. Four bytes of it, which is enough to tell two configurations apart
/// and short enough to keep the tag readable.
fn etag(view: &View, kind: &str, revision: u32, source: &Source) -> String {
    format!(
        "\"{kind}{revision}.a{ALGORITHM_VERSION}.c{}.s{:016x}.q{}.r{}.{}x{}.h{:08x}.{}\"",
        source.fingerprint_prefix(),
        view.seed,
        view.center.q(),
        view.center.r(),
        view.cols,
        view.rows,
        view.hex_radius.to_bits(),
        view.layer.name(),
    )
}
