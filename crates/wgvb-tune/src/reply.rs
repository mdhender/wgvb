//! Turning a request into a response, with no socket in sight.
//!
//! The routing, the rendering, the form handling, and the error mapping are a
//! function of the request and the session, so every test in this crate is an
//! ordinary unit test rather than a client talking to a listening port. The
//! `tiny_http` adapter in [`crate::serve`] does nothing but move bytes.
//!
//! "A function of the request" and not "a pure function of the request": the
//! session is the second argument, and it is what the viewer does not have. A
//! `POST` changes it, and every `GET` afterwards sees the change.

use std::time::Instant;

use wgvb::{ALGORITHM_VERSION, Config, Coord};
use wgvb_config::{FileError, from_toml, to_toml, with_field};
use wgvb_render::{Layer, RENDER_VERSION, encode_png, render, render_grid};
use wgvb_view::ViewError;

use crate::form;
use crate::page;
use crate::route::{Change, Tab, Target, Window};
use crate::session::Session;
use crate::{PAGE_VERSION, form::fields};

/// `text/html`, as this server writes it.
pub const HTML: &str = "text/html; charset=utf-8";
/// `text/plain`, which every refusal is answered in.
pub const TEXT: &str = "text/plain; charset=utf-8";
/// `image/png`, for both image routes.
pub const PNG: &str = "image/png";
/// `application/toml`, for the configuration download.
pub const TOML: &str = "application/toml; charset=utf-8";

/// A complete response, minus the connection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reply {
    /// HTTP status code.
    pub status: u16,
    /// The `Content-Type` header's value.
    pub content_type: &'static str,
    /// The body.
    pub body: Vec<u8>,
    /// A strong `ETag`, for the responses that are a function of their URL and
    /// of the configuration in force.
    pub etag: Option<String>,
    /// A `Location` header, for a redirect.
    pub location: Option<String>,
    /// A filename, for the one response that is a download.
    pub download: Option<String>,
    /// A `Server-Timing` header, so a browser's network panel shows what a
    /// window cost without a page of its own.
    pub timing: Option<String>,
    /// What to append to this request's log line. The measurement half of this
    /// tool: every render says how many tiles it drew and how long it took.
    pub note: String,
}

impl Reply {
    /// The body as text, for a test reading a page or an error message.
    ///
    /// # Panics
    ///
    /// Panics on a body that is not UTF-8, which only the image routes produce.
    #[must_use]
    pub fn text(&self) -> &str {
        std::str::from_utf8(&self.body).expect("this reply's body is text")
    }
}

/// Answers one request.
///
/// `posting` and the body are what a `GET` does not have. Everything else is
/// the target and the session.
#[must_use]
pub fn answer(
    target: &str,
    posting: bool,
    content_type: &str,
    body: &[u8],
    session: &Session,
    budget: u64,
) -> Reply {
    let (what, window) = match Window::parse(target) {
        Ok(parsed) => parsed,
        Err(error) => return refuse_view(&error),
    };

    match (what, posting) {
        (Target::Root, _) => {
            let home = Window::origin_of(session.seed()).url(Tab::Map);
            Reply {
                status: 302,
                content_type: TEXT,
                body: format!("the tuner lives at {home}\n").into_bytes(),
                location: Some(home),
                ..Reply::default()
            }
        }
        (Target::Download, _) => download(session),
        (Target::Write(change), true) => write(change, &window, content_type, body, session),
        // A `POST` to a page, or a `GET` to a change, are both somebody's
        // mistake rather than a state this server has.
        (Target::Write(_), false) | (Target::Read(_), true) => Reply {
            status: 405,
            content_type: TEXT,
            body: b"this route is not read and written the same way\n".to_vec(),
            ..Reply::default()
        },
        (Target::Read(tab), false) => match recentered(tab, &window, query_of(target)) {
            Some(reply) => reply,
            None => read(tab, &window, session, budget),
        },
        (Target::Open, _) => {
            // The seed box. The seed arrives as a form field and leaves as a
            // route, so that what is in the address bar afterwards is a link
            // somebody can keep.
            let text = wgvb_view::lookup(query_of(target), "seed").unwrap_or_default();
            match wgvb_view::parse_seed(text.trim()) {
                Ok(seed) => see_other(&window.with_seed(seed).url(Tab::Map)),
                Err(error) => refuse_view(&error),
            }
        }
        (Target::Random, _) => see_other(&window.with_seed(a_seed_nobody_chose()).url(Tab::Map)),
    }
}

/// A click on a map, turned into the window it asks for.
///
/// `<input type="image">` submits `click.x` and `click.y`, which is the whole
/// of how a page with no script lets somebody click a map. The pixel becomes a
/// cell through the same [`wgvb_render::Viewport`] or [`wgvb_render::Grid`]
/// that drew it — hit testing on the hex tab, integer division on the grid —
/// and the answer is a redirect, so what lands in the address bar afterwards is
/// an ordinary link to the new window.
fn recentered(tab: Tab, window: &Window, query: &str) -> Option<Reply> {
    let (x, y) = clicked(query, "click")?;
    let center = match tab {
        Tab::Map => window.viewport().ok()?.locate(x, y)?,
        Tab::Grid => window.grid().ok()?.locate(x, y)?,
        _ => return None,
    };
    Some(see_other(&window.centered_on(center).url(tab)))
}

/// The query string of a target, for the two routes that read a field out of it
/// rather than out of a [`Window`].
fn query_of(target: &str) -> &str {
    wgvb_view::split_target(target).1
}

/// A redirect that turns a form submission into a link.
fn see_other(location: &str) -> Reply {
    Reply {
        status: 303,
        content_type: TEXT,
        body: format!("see {location}\n").into_bytes(),
        location: Some(location.to_string()),
        ..Reply::default()
    }
}

/// A seed nobody chose.
///
/// The clock, mixed. Not `rand`: CLAUDE.md permits that crate for tooling and
/// this is tooling, but a dependency for one number that never touches
/// generation is a dependency that has to be justified at every audit. Wrapping
/// arithmetic throughout, which is the rule every mixer in this project follows
/// — plain `*` and `+` here would panic in a debug build on the first overflow.
fn a_seed_nobody_chose() -> wgvb::Seed {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the low bits of a nanosecond clock are the entropy; the high ones are the date"
    )]
    let mut seed = now as u64;
    seed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    seed = (seed ^ (seed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    seed = (seed ^ (seed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    seed ^ (seed >> 31)
}

/// Draws one tab.
fn read(tab: Tab, window: &Window, session: &Session, budget: u64) -> Reply {
    let state = session.state();
    let generator = state.generator(window.view.seed);

    match tab {
        Tab::Config => Reply {
            status: 200,
            content_type: HTML,
            body: page::config(window, &state, None, &[]).into_bytes(),
            etag: Some(etag(
                window,
                "config",
                PAGE_VERSION,
                &state.tag_fingerprint(),
            )),
            ..Reply::default()
        },
        Tab::Map => {
            let viewport = match window.viewport() {
                Ok(viewport) => viewport,
                Err(error) => return refuse_view(&ViewError::from(error)),
            };
            // The page costs real work now: the distribution readout is a whole
            // `Tile` per cell, which is seven evaluations whatever layer is
            // being drawn. So the page goes through the same budget the image
            // does, counted at the terrain layer's price rather than the
            // selected layer's.
            let (cols, rows) = viewport.tile_counts();
            let tiles = u64::from(cols) * u64::from(rows);
            if let Some(refusal) = over_budget(tiles, Layer::Terrain, budget) {
                return refusal;
            }
            Reply {
                status: 200,
                content_type: HTML,
                body: page::map(window, &viewport, &generator, &state).into_bytes(),
                etag: Some(etag(window, "map", PAGE_VERSION, &state.tag_fingerprint())),
                ..Reply::default()
            }
        }
        Tab::Grid => {
            let grid = match window.grid() {
                Ok(grid) => grid,
                Err(error) => return refuse_view(&ViewError::from(error)),
            };
            if let Some(refusal) = over_budget(grid.tiles(), window.view.layer, budget) {
                return refusal;
            }
            Reply {
                status: 200,
                content_type: HTML,
                body: page::grid(window, &grid, &generator, &state).into_bytes(),
                etag: Some(etag(window, "grid", PAGE_VERSION, &state.tag_fingerprint())),
                ..Reply::default()
            }
        }
        Tab::MapImage => {
            let viewport = match window.viewport() {
                Ok(viewport) => viewport,
                Err(error) => return refuse_view(&ViewError::from(error)),
            };
            let (cols, rows) = viewport.tile_counts();
            let tiles = u64::from(cols) * u64::from(rows);
            if let Some(refusal) = over_budget(tiles, window.view.layer, budget) {
                return refusal;
            }
            let drawn = Instant::now();
            let image = render(&generator, &viewport, window.view.layer);
            image_reply(image, tiles, drawn, window, &state)
        }
        Tab::GridImage => {
            let grid = match window.grid() {
                Ok(grid) => grid,
                Err(error) => return refuse_view(&ViewError::from(error)),
            };
            if let Some(refusal) = over_budget(grid.tiles(), window.view.layer, budget) {
                return refusal;
            }
            let drawn = Instant::now();
            let image = render_grid(&generator, &grid, window.view.layer);
            image_reply(image, grid.tiles(), drawn, window, &state)
        }
    }
}

/// A rendered image, with what it cost attached.
///
/// The cost is the point. `wgvb-serve` renders a window and says nothing about
/// how long it took, because a window there is a few thousand tiles; a window
/// here is a few million, and "measure before optimizing" needs a measurement
/// to come from somewhere.
fn image_reply(
    image: wgvb_render::Image,
    tiles: u64,
    drawn: Instant,
    window: &Window,
    state: &crate::session::State,
) -> Reply {
    let generated = drawn.elapsed();
    let encoding = Instant::now();
    let png = match encode_png(&image) {
        Ok(png) => png,
        Err(error) => return refuse_view(&ViewError::from(error)),
    };
    let encoded = encoding.elapsed();

    let rate = if generated.as_secs_f64() > 0.0 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a tile count printed as a rate, not a value anything computes with"
        )]
        let tiles = tiles as f64;
        tiles / generated.as_secs_f64()
    } else {
        0.0
    };

    Reply {
        status: 200,
        content_type: PNG,
        body: png,
        etag: Some(etag(
            window,
            "png",
            RENDER_VERSION,
            &state.tag_fingerprint(),
        )),
        timing: Some(format!(
            "generate;dur={:.1}, encode;dur={:.1}",
            generated.as_secs_f64() * 1000.0,
            encoded.as_secs_f64() * 1000.0
        )),
        note: format!(
            " ({tiles} tiles, {:.0} ms generate, {:.0} ms encode, {rate:.0} tiles/s)",
            generated.as_secs_f64() * 1000.0,
            encoded.as_secs_f64() * 1000.0,
        ),
        ..Reply::default()
    }
}

/// Applies a change to the session's configuration.
///
/// Every one of these answers with a 303 to the tab the person was on, so that
/// a refresh afterwards re-reads a page rather than re-submitting a form. A
/// refusal answers with the form again, carrying the value that was typed and
/// the reason it was refused, because a plain-text 400 would throw away
/// everything else on the page.
fn write(
    change: Change,
    window: &Window,
    content_type: &str,
    body: &[u8],
    session: &Session,
) -> Reply {
    let state = session.state();
    let outcome = match change {
        Change::Reset => Ok(Config::default()),
        Change::Upload => upload(content_type, body),
        Change::Fields => apply(&state.config, &fields(body)),
    };

    match outcome.and_then(|config| {
        session.adopt(config.clone())?;
        Ok(config)
    }) {
        Ok(_) => {
            let home = window.url(Tab::Config);
            Reply {
                status: 303,
                content_type: TEXT,
                body: format!("done; see {home}\n").into_bytes(),
                location: Some(home),
                note: format!(" (configuration {})", session.state().short_fingerprint()),
                ..Reply::default()
            }
        }
        Err(error) => Reply {
            status: 400,
            content_type: HTML,
            // The posted fields go back into the form, so a refused edit keeps
            // what somebody was trying instead of silently reverting it. This
            // is the one page that echoes request text, and `page::escape` is
            // why that is safe.
            body: page::config(window, &state, Some(&error.to_string()), &fields(body))
                .into_bytes(),
            note: " (refused)".to_string(),
            ..Reply::default()
        },
    }
}

/// The configuration a form posted, field by field.
///
/// Only the fields that actually differ from what is in force are applied, so
/// a form that posts a hundred unchanged values does a hundred comparisons and
/// no work. It also means the error a person sees names the field they edited
/// rather than the first field in the file.
fn apply(base: &Config, posted: &[(String, String)]) -> Result<Config, FileError> {
    let mut config = base.clone();
    let current = wgvb_config::fields(base)?;
    for (name, value) in posted {
        let Some(field) = current.iter().find(|field| &field.name == name) else {
            // A form that posts something that is not a field is a form this
            // server did not write. Naming it is more useful than ignoring it.
            return Err(FileError::UnknownField { name: name.clone() });
        };
        if field.value == value.trim() {
            continue;
        }
        config = with_field(&config, name, value)?;
    }
    Ok(config)
}

/// The configuration an upload carries, from a file or from a pasted textarea.
fn upload(content_type: &str, body: &[u8]) -> Result<Config, FileError> {
    if let Some(text) = form::uploaded_file(content_type, body) {
        if text.trim().is_empty() {
            return Err(FileError::Syntax(
                "no file was chosen, and nothing was pasted".to_string(),
            ));
        }
        return from_toml(&text);
    }
    let pasted = fields(body)
        .into_iter()
        .find(|(name, _)| name == "pasted")
        .map(|(_, value)| value)
        .unwrap_or_default();
    if pasted.trim().is_empty() {
        return Err(FileError::Syntax(
            "no file was chosen, and nothing was pasted".to_string(),
        ));
    }
    from_toml(&pasted)
}

/// The configuration in force, as a file.
fn download(session: &Session) -> Reply {
    let state = session.state();
    match to_toml(&state.config) {
        Ok(text) => Reply {
            status: 200,
            content_type: TOML,
            body: text.into_bytes(),
            download: Some(format!("wgvb-{}.toml", state.short_fingerprint())),
            ..Reply::default()
        },
        Err(error) => Reply {
            status: 500,
            content_type: TEXT,
            body: format!("{error}\n").into_bytes(),
            ..Reply::default()
        },
    }
}

/// Refuses a window that would cost more than this server will spend.
///
/// The clamp of `DESIGN.md` section 29 applied to a tool whose windows are a
/// thousand times larger than the viewer's. It is counted in generator
/// evaluations rather than tiles, and the message says so, because the fix is
/// usually "ask for a cheaper layer" rather than "ask for less map".
fn over_budget(tiles: u64, layer: Layer, budget: u64) -> Option<Reply> {
    let cost = tiles.saturating_mul(u64::from(layer.cost()));
    (cost > budget).then(|| Reply {
        status: 400,
        content_type: TEXT,
        body: format!(
            "this window is {tiles} tiles of {}, which costs {cost} generator \
             evaluations against a budget of {budget}. Ask for a smaller window, \
             a cheaper layer, or start the server with a larger --budget.\n",
            layer.name(),
        )
        .into_bytes(),
        note: " (over budget)".to_string(),
        ..Reply::default()
    })
}

/// Answers a refusal as plain text.
///
/// Plain text on purpose: the message quotes back what was sent, and quoted
/// input is inert in a `text/plain` body in a way it is not in an HTML one.
/// The one refusal that is answered in HTML is a rejected configuration edit,
/// which has a form to put back on the screen and escapes what it echoes.
fn refuse_view(error: &ViewError) -> Reply {
    Reply {
        status: error.status(),
        content_type: TEXT,
        body: format!("{error}\n").into_bytes(),
        ..Reply::default()
    }
}

/// A strong `ETag` for a response.
///
/// It stops being a pure function of the URL here, and that is the departure
/// this tool makes: the configuration is session state, so two requests for one
/// URL can legitimately differ. What keeps the validator *strong* is that the
/// configuration's fingerprint is in the tag. Move a field and every tag moves
/// with it, so a browser holding an old image asks again and gets the new one.
///
/// Eight bytes of fingerprint rather than the four a person reads: a tuning
/// session walks hundreds of configurations under otherwise identical URLs.
fn etag(window: &Window, kind: &str, revision: u32, fingerprint: &str) -> String {
    format!(
        "\"{kind}{revision}.a{ALGORITHM_VERSION}.c{fingerprint}.s{:016x}.q{}.r{}.{}x{}.h{:08x}.t{}.z{}.g{}x{}.{}\"",
        window.view.seed,
        window.view.center.q(),
        window.view.center.r(),
        window.view.cols,
        window.view.rows,
        window.view.hex_radius.to_bits(),
        window.turn,
        window.scale,
        window.grid_cols,
        window.grid_rows,
        window.view.layer.name(),
    )
}

/// The coordinate a click on an image names, if the request carried one.
///
/// `<input type="image">` submits `name.x` and `name.y`, which is how a page
/// with no script lets somebody click a map. The caller has already turned the
/// pixel into a cell; this is only the parsing.
#[must_use]
pub fn clicked(query: &str, name: &str) -> Option<(u32, u32)> {
    let x = wgvb_view::lookup(query, &format!("{name}.x"))?;
    let y = wgvb_view::lookup(query, &format!("{name}.y"))?;
    Some((x.parse().ok()?, y.parse().ok()?))
}

/// A coordinate as the page prints it.
#[must_use]
pub fn coord_text(coord: Coord) -> String {
    format!("({}, {}, {})", coord.q(), coord.r(), coord.s())
}
