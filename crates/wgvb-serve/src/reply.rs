//! Turning a request target into a response, with no socket in sight.
//!
//! The routing, the rendering, and the error mapping are a pure function of the
//! request target, so every test in this crate is an ordinary unit test rather
//! than a client talking to a listening port. The `tiny_http` adapter in
//! [`crate::serve`] does nothing but move bytes between this function and a
//! connection.

use wgvb::{ALGORITHM_VERSION, Generator};
use wgvb_render::{RENDER_VERSION, encode_png, render};

use crate::PAGE_VERSION;
use crate::page::page;
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

/// Answers one request target.
///
/// The generator is constructed here, per request, from the seed in the route
/// and the default configuration. That is cheap — it builds a field graph, not
/// a world — and it is what keeps this server stateless in the same sense the
/// generator is: nothing is remembered between requests, so nothing between
/// requests can affect what is drawn.
///
/// **Output is diagnostic and does not represent a saved world.** When
/// persistence arrives this function takes the generator from the database
/// instead, and the seed in the route becomes a check against the stored one
/// rather than the source of it.
#[must_use]
pub fn reply(target: &str) -> Reply {
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
        Ok((route, view)) => match draw(route, &view) {
            Ok(reply) => reply,
            Err(error) => refuse(&error),
        },
        Err(error) => refuse(&error),
    }
}

/// Renders whichever of the two routes was asked for.
fn draw(route: Route, view: &View) -> Result<Reply, RequestError> {
    let viewport = view.viewport()?;
    let generator = Generator::with_defaults(view.seed);
    match route {
        Route::Page => Ok(Reply {
            status: 200,
            content_type: HTML,
            // One generator per request, built before the route is chosen:
            // the page now reads the center tile and the image reads the
            // whole window, and two generators would be two chances to
            // disagree about the configuration behind one URL.
            body: page(view, &viewport, &generator).into_bytes(),
            etag: Some(etag(view, "page", PAGE_VERSION)),
            location: None,
        }),
        Route::Image => {
            let image = render(&generator, &viewport, view.layer);
            Ok(Reply {
                status: 200,
                content_type: PNG,
                body: encode_png(&image)?,
                etag: Some(etag(view, "png", RENDER_VERSION)),
                location: None,
            })
        }
    }
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
/// **This is incomplete until the configuration fingerprint of section 21.2
/// exists.** Every generator here is built by `Generator::with_defaults`, so
/// the effective configuration is currently a constant and the tag is sound;
/// the moment a configuration can vary — a `--db` flag, a config file — the
/// fingerprint has to join this list or the tag starts lying.
fn etag(view: &View, kind: &str, revision: u32) -> String {
    format!(
        "\"{kind}{revision}.a{ALGORITHM_VERSION}.s{:016x}.q{}.r{}.{}x{}.h{:08x}.{}\"",
        view.seed,
        view.center.q(),
        view.center.r(),
        view.cols,
        view.rows,
        view.hex_radius.to_bits(),
        view.layer.name(),
    )
}
