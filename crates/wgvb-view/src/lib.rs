//! The URL grammar a WGVB web front end presents.
//!
//! A [`View`] is a window into the world — a seed, a center tile, a tile count,
//! a pixel scale, and a layer — together with the parsing that turns a request
//! target into one and the spelling that turns one back into a link. Every link
//! a front end emits parses back to the state that produced it: no client-side
//! panning, no canvas, no script, and therefore nothing to get out of step with
//! the address bar.
//!
//! # Why this is a crate and not a module
//!
//! There are two front ends. `wgvb-serve` looks at a saved world and keeps the
//! promise that *every* state it can be in is a URL; `wgvb-tune` holds a
//! configuration in memory, because a hundred-field form does not fit in an
//! address bar. They present different pages and different route tables, and
//! without this crate they would present two copies of the same window grammar.
//!
//! That grammar is not markup. It encodes invariants with tests behind them —
//! a scroll step is a whole number of hexes, a step followed by its opposite
//! returns to exactly the coordinate it started from, and north is absolute
//! direction 2 — and two copies of an invariant is one copy and one liability.
//!
//! # What is not here
//!
//! The route table. `/seed/{seed}` and `/seed/{seed}/map.png` are the viewer's
//! two routes and live with [`Route`]; the tuner has tabs of its own and builds
//! them from [`seed_route`] instead. A front end owns its own paths and shares
//! everything below them.

use std::fmt;

use wgvb::{Component, Coord, DIRECTIONS, Seed, direction_index};
use wgvb_render::{Layer, RenderError, Viewport};

/// Window width in tiles, when the URL does not say.
pub const DEFAULT_COLS: u32 = 61;
/// Window height in tiles, when the URL does not say.
pub const DEFAULT_ROWS: u32 = 45;
/// Hex radius in pixels, when the URL does not say.
pub const DEFAULT_HEX_RADIUS: f32 = 10.0;

/// Widest window the server will draw, in tiles.
///
/// `DESIGN.md` section 29 bounds rendering even though generation is unbounded,
/// and a server is an easier place to forget that than a CLI: the cost of a
/// request is `cols * rows` generator calls, and seven times that for
/// [`Layer::Relief`], all chosen by whoever typed the URL. The clamp is the
/// answer rather than a cache — section 26 asks for a profile before a cache.
///
/// Odd, so that clamping a wild request lands on a window that still has a
/// center cell.
pub const MAX_COLS: u32 = 201;
/// Tallest window the server will draw, in tiles. Odd, for the same reason as
/// [`MAX_COLS`].
pub const MAX_ROWS: u32 = 201;
/// Smallest hex radius the server will draw, in pixels.
pub const MIN_HEX_RADIUS: f32 = 1.0;
/// Largest hex radius the server will draw, in pixels.
pub const MAX_HEX_RADIUS: f32 = 48.0;

/// The query parameters a [`View`] is made of.
///
/// A front end passes this, plus whatever it adds, to [`reject_unknown`]: an
/// unrecognized parameter is refused rather than ignored, for the same reason
/// `Config` carries `deny_unknown_fields`: a silently dropped `?col=61` looks
/// exactly like a server that does not work.
pub const KNOWN_PARAMETERS: [&str; 6] = ["q", "r", "cols", "rows", "hex-radius", "layer"];

/// Which window dimension a scroll step is measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Rows down the window: the north and south controls.
    Rows,
    /// Columns across the window: the four diagonal controls.
    Cols,
}

/// One scroll control: a compass point, the absolute direction it means, and
/// the window dimension its step is measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Compass {
    /// The label on the control.
    pub name: &'static str,
    /// The heading spelled out, for a link title.
    pub heading: &'static str,
    /// The absolute direction index this compass point is in the admin frame.
    pub direction: i32,
    /// The window dimension a step along this direction is counted in.
    pub axis: Axis,
}

impl Compass {
    /// How many whole hexes one press of this control moves the view.
    ///
    /// **Whole hexes, never pixels.** A step counted in tiles means the same
    /// thing at every zoom, and a step followed by its opposite returns to
    /// exactly the coordinate it started from. Both counts are integer division
    /// of an odd number, so both are a whole number of hexes.
    ///
    /// One north step moves the view one row, so half the window's height in
    /// rows is half the view. One diagonal step moves one column across and
    /// half a row down or up, so half the window's width in columns is half the
    /// view sideways.
    #[must_use]
    pub const fn steps(self, cols: u32, rows: u32) -> u32 {
        match self.axis {
            Axis::Rows => rows / 2,
            Axis::Cols => cols / 2,
        }
    }
}

/// The six scroll controls, clockwise from north.
///
/// This is `DESIGN.md` appendix A's compass walk in the admin frame, which the
/// renderer draws without rotation: north is absolute direction `2`, and
/// reading the compass clockwise *decreases* the index, because index order is
/// counter-clockwise as a viewer sees it. The mapping is pinned by a test in
/// `wgvb-render`, not measured by hand here.
///
/// When a player frame arrives with rotation `k`, north becomes absolute
/// direction `k` and the same walk applies from there. No rotation ever reaches
/// the generator.
pub const COMPASS: [Compass; 6] = [
    Compass {
        name: "N",
        heading: "north",
        direction: 2,
        axis: Axis::Rows,
    },
    Compass {
        name: "NE",
        heading: "northeast",
        direction: 1,
        axis: Axis::Cols,
    },
    Compass {
        name: "SE",
        heading: "southeast",
        direction: 0,
        axis: Axis::Cols,
    },
    Compass {
        name: "S",
        heading: "south",
        direction: 5,
        axis: Axis::Rows,
    },
    Compass {
        name: "SW",
        heading: "southwest",
        direction: 4,
        axis: Axis::Cols,
    },
    Compass {
        name: "NW",
        heading: "northwest",
        direction: 3,
        axis: Axis::Cols,
    },
];

/// Why a request was refused. One variant per gate, so tests assert on variants
/// rather than on message strings — `DESIGN.md` section 19.1, applied to the
/// route table.
///
/// Every variant names the parameter it is complaining about, because the
/// person reading it is looking at a URL they typed.
///
/// Everything here is something about a *URL*. A front end with refusals of its
/// own — a seed that is not the one a database holds, a database that stopped
/// answering — wraps this in an error of its own rather than adding a variant
/// here that only one caller can ever construct.
#[derive(Debug, thiserror::Error)]
pub enum ViewError {
    #[error("no route for {path}; try /seed/0123456789abcdef")]
    NoRoute { path: String },
    #[error("seed {seed:?} is {got} characters; a seed is exactly 16 hexadecimal digits")]
    SeedLength { seed: String, got: usize },
    #[error("seed {seed:?} is not hexadecimal; a seed is 16 digits of 0-9 and a-f, with no 0x")]
    SeedDigits { seed: String },
    #[error("{present} was given without {missing}; a center needs both q and r")]
    HalfACenter {
        present: &'static str,
        missing: &'static str,
    },
    #[error("{parameter}={value:?} is not a whole number")]
    NotAnInteger {
        parameter: &'static str,
        value: String,
    },
    #[error(
        "{parameter}={value} is outside the world's coordinate range of \
         {min} to {max}"
    )]
    CoordinateRange {
        parameter: &'static str,
        value: i64,
        min: i64,
        max: i64,
    },
    #[error("{parameter}={value:?} is not a number")]
    NotANumber {
        parameter: &'static str,
        value: String,
    },
    #[error("unknown layer {name:?}; known layers are {known}")]
    UnknownLayer { name: String, known: String },
    #[error("unknown parameter {name:?}; this viewer understands {known}")]
    UnknownParameter { name: String, known: String },
    #[error("the window cannot be drawn: {0}")]
    Unrenderable(#[from] RenderError),
}

impl ViewError {
    /// The HTTP status this refusal answers with.
    ///
    /// Everything here is the caller's fault, so nothing here is a 500. A
    /// window too large for [`wgvb_render::MAX_IMAGE_PIXELS`] included: the
    /// caller chose the size.
    ///
    /// The one thing that is not a 400 is a path that names no route, which is
    /// a 404 because that is what "no route" means.
    #[must_use]
    pub const fn status(&self) -> u16 {
        match self {
            ViewError::NoRoute { .. } => 404,
            _ => 400,
        }
    }
}

/// What a URL asks to see.
///
/// Coordinates here are **canonical**, not frame-relative. This viewer has no
/// player, a canonical link means the same tile to everyone who opens it, and
/// it is the same number `wgvb-map --q --r` takes, so a window moves between
/// the two tools without a conversion. See `DESIGN.md` appendix A,
/// *Coordinate frames*.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    /// The world seed, written as sixteen hexadecimal digits in the route.
    pub seed: Seed,
    /// The tile in the exact center cell of the window.
    pub center: Coord,
    /// Window width in tiles. Odd, so a center cell exists.
    pub cols: u32,
    /// Window height in tiles. Odd, for the same reason.
    pub rows: u32,
    /// Hex radius in pixels.
    pub hex_radius: f32,
    /// The layer to draw.
    pub layer: Layer,
}

/// Which of the two routes a target names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// `/seed/{seed}` — the viewer page.
    Page,
    /// `/seed/{seed}/map.png` — the rendered window on its own.
    Image,
}

impl View {
    /// The view a bare `/seed/{seed}` shows: the origin, at the defaults.
    #[must_use]
    pub fn origin_of(seed: Seed) -> View {
        View {
            seed,
            center: Coord::ORIGIN,
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
            hex_radius: DEFAULT_HEX_RADIUS,
            layer: Layer::Elevation,
        }
    }

    /// Parses a request target — path and query together, as a server receives
    /// it — into a route and the view it names.
    ///
    /// # Errors
    ///
    /// Every malformed piece of a URL is a [`ViewError`] naming the piece.
    /// Nothing here defaults silently and nothing here panics.
    pub fn parse(target: &str) -> Result<(Route, View), ViewError> {
        let (path, query) = split_target(target);
        let (route, seed_text) = parse_route(path)?;
        reject_unknown(query, &KNOWN_PARAMETERS)?;
        let view = View::from_query(parse_seed(seed_text)?, query)?;
        Ok((route, view))
    }

    /// Parses the query string of a request against this seed.
    ///
    /// Public because a front end with a route table of its own — the tuner's
    /// tabs, rather than the viewer's two paths — still wants exactly this
    /// window grammar underneath it.
    ///
    /// Unknown parameters are *not* rejected here. A front end that adds
    /// parameters of its own knows its own vocabulary, so it calls
    /// [`reject_unknown`] with it; [`View::parse`] does that for the viewer's.
    ///
    /// # Errors
    ///
    /// Every malformed value is a [`ViewError`] naming the parameter.
    pub fn from_query(seed: Seed, query: &str) -> Result<View, ViewError> {
        let mut view = View::origin_of(seed);

        let q = lookup(query, "q");
        let r = lookup(query, "r");
        match (q.as_deref(), r.as_deref()) {
            (Some(q), Some(r)) => {
                view.center = Coord::new(component("q", q)?, component("r", r)?);
            }
            (Some(_), None) => {
                return Err(ViewError::HalfACenter {
                    present: "q",
                    missing: "r",
                });
            }
            (None, Some(_)) => {
                return Err(ViewError::HalfACenter {
                    present: "r",
                    missing: "q",
                });
            }
            (None, None) => {}
        }

        if let Some(text) = lookup(query, "cols") {
            view.cols = count("cols", &text, MAX_COLS)?;
        }
        if let Some(text) = lookup(query, "rows") {
            view.rows = count("rows", &text, MAX_ROWS)?;
        }
        if let Some(text) = lookup(query, "hex-radius") {
            view.hex_radius = radius("hex-radius", &text)?;
        }
        if let Some(text) = lookup(query, "layer") {
            view.layer = Layer::parse(&text).map_err(|_| ViewError::UnknownLayer {
                name: text.clone(),
                known: Layer::ALL
                    .iter()
                    .map(|layer| layer.name())
                    .collect::<Vec<_>>()
                    .join(", "),
            })?;
        }

        Ok(view)
    }

    /// The viewport this view draws.
    ///
    /// # Errors
    ///
    /// Returns the render gate that refused, which the caller turns into a 400.
    /// An even tile count reaches here rather than being rounded away, because
    /// rounding a center is what makes a scroll step stop being reversible.
    pub fn viewport(&self) -> Result<Viewport, RenderError> {
        Viewport::centered_on(self.center, self.cols, self.rows, self.hex_radius)
    }

    /// This view scrolled one press of one control.
    ///
    /// `n` whole steps of a vector from [`DIRECTIONS`], and nothing else: no
    /// offset-coordinate arithmetic lives in this crate. [`Coord::new`]
    /// normalizes, so scrolling off an edge of the canonical hexagon wraps to
    /// the opposite edge with no special case. It will look like a seam,
    /// because it is one — the accepted world-warp seam of `DESIGN.md`
    /// section 7.1.
    #[must_use]
    pub fn scrolled(&self, compass: Compass) -> View {
        let (dq, dr) = DIRECTIONS[direction_index(compass.direction)];
        let steps = i64::from(compass.steps(self.cols, self.rows));
        View {
            center: Coord::new(
                i64::from(self.center.q()) + steps * i64::from(dq),
                i64::from(self.center.r()) + steps * i64::from(dr),
            ),
            ..*self
        }
    }

    /// This view with a different layer selected.
    #[must_use]
    pub fn with_layer(&self, layer: Layer) -> View {
        View { layer, ..*self }
    }

    /// The URL of the viewer page showing this view.
    #[must_use]
    pub fn page_url(&self) -> String {
        self.url("")
    }

    /// This view's URL under one path below `/seed/{seed}`.
    ///
    /// `""` is the page itself, `"/map.png"` the image, and a front end with
    /// tabs of its own passes `"/grid"` or `"/config"`. The seed and the window
    /// are spelled in exactly one place whatever the path is, which is what
    /// stops two tabs disagreeing about how to write a coordinate.
    #[must_use]
    pub fn url(&self, path: &str) -> String {
        format!("/seed/{:016x}{path}?{}", self.seed, self.query())
    }

    /// The URL of the rendered window on its own.
    #[must_use]
    pub fn image_url(&self) -> String {
        self.url("/map.png")
    }

    /// The query string every URL carries. Always complete: a link a front end
    /// emits never relies on a default, so following it cannot change meaning
    /// when a default does.
    ///
    /// A front end with parameters of its own appends them to this rather than
    /// respelling it.
    #[must_use]
    pub fn query(&self) -> String {
        format!(
            "q={}&r={}&cols={}&rows={}&hex-radius={}&layer={}",
            self.center.q(),
            self.center.r(),
            self.cols,
            self.rows,
            self.hex_radius,
            self.layer.name(),
        )
    }

    /// The seed as it appears in a route.
    #[must_use]
    pub fn seed_text(&self) -> String {
        format!("{:016x}", self.seed)
    }
}

impl fmt::Display for View {
    /// The center in readable form, the way the page prints it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({}, {}, {})",
            self.center.q(),
            self.center.r(),
            self.center.s()
        )
    }
}

/// Rejects any parameter a front end does not understand.
///
/// An unrecognized parameter is refused rather than ignored, for the same
/// reason [`wgvb::Config`] carries `deny_unknown_fields`: a silently dropped
/// `?col=61` looks exactly like a front end that does not work.
///
/// `known` is the caller's whole vocabulary, [`KNOWN_PARAMETERS`] plus whatever
/// it adds, because only the caller knows what its own pages accept.
///
/// # Errors
///
/// [`ViewError::UnknownParameter`], naming the parameter and listing the ones
/// that would have worked.
pub fn reject_unknown(query: &str, known: &[&str]) -> Result<(), ViewError> {
    for (key, _) in pairs(query) {
        if !known.contains(&key.as_str()) {
            return Err(ViewError::UnknownParameter {
                name: key,
                known: known.join(", "),
            });
        }
    }
    Ok(())
}

/// Splits a request target into its path and its query string.
#[must_use]
pub fn split_target(target: &str) -> (&str, &str) {
    match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    }
}

/// Splits a `/seed/{seed}/...` path into the seed and whatever follows it.
///
/// The shared half of a route table: every path either front end serves starts
/// this way, and what comes after it is the front end's own business — `""` for
/// a page, `"/map.png"` for an image, `"/grid"` or `"/config"` for a tab the
/// tuner has and the viewer does not.
///
/// A trailing slash is trimmed, so `/seed/{seed}/` is the page.
///
/// # Errors
///
/// [`ViewError::NoRoute`] for a path that does not start `/seed/`, and whatever
/// [`parse_seed`] refuses for the digits that follow.
pub fn seed_route(path: &str) -> Result<(Seed, &str), ViewError> {
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    let rest = trimmed
        .strip_prefix("/seed/")
        .ok_or_else(|| ViewError::NoRoute {
            path: path.to_string(),
        })?;
    let cut = rest.find('/').unwrap_or(rest.len());
    Ok((parse_seed(&rest[..cut])?, &rest[cut..]))
}

/// Matches a path against the two routes, returning the seed text it carries.
fn parse_route(path: &str) -> Result<(Route, &str), ViewError> {
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    if let Some(rest) = trimmed.strip_prefix("/seed/") {
        if let Some(seed) = rest.strip_suffix("/map.png") {
            return Ok((Route::Image, seed));
        }
        if !rest.contains('/') {
            return Ok((Route::Page, rest));
        }
    }
    Err(ViewError::NoRoute {
        path: path.to_string(),
    })
}

/// Parses the sixteen hexadecimal digits of a seed.
///
/// # Errors
///
/// [`ViewError::SeedLength`] or [`ViewError::SeedDigits`], each quoting back
/// what was written.
///
/// Hex because that is how a seed is written everywhere else in the repository:
/// `0x0123_4567_89ab_cdef` is the golden seed and `/seed/0123456789abcdef` is
/// the same number. Case-insensitive, no `0x`, exactly sixteen digits, and
/// anything else is a refusal rather than a silent zero.
pub fn parse_seed(text: &str) -> Result<Seed, ViewError> {
    if text.len() != 16 {
        return Err(ViewError::SeedLength {
            seed: text.to_string(),
            got: text.chars().count(),
        });
    }
    if !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ViewError::SeedDigits {
            seed: text.to_string(),
        });
    }
    Seed::from_str_radix(text, 16).map_err(|_| ViewError::SeedDigits {
        seed: text.to_string(),
    })
}

/// Parses one coordinate component.
///
/// Out of [`Component`] range is a refusal rather than a normalization. Every
/// link this server emits is already canonical, so a number outside the range
/// came from somewhere else and the reader deserves to be told, not to be shown
/// a different tile than the one they named.
fn component(parameter: &'static str, text: &str) -> Result<i64, ViewError> {
    let value: i64 = text.parse().map_err(|_| ViewError::NotAnInteger {
        parameter,
        value: text.to_string(),
    })?;
    let (min, max) = (i64::from(Component::MIN), i64::from(Component::MAX));
    if value < min || value > max {
        return Err(ViewError::CoordinateRange {
            parameter,
            value,
            min,
            max,
        });
    }
    Ok(value)
}

/// Parses a tile count and clamps it into the range a front end will draw.
///
/// Clamped rather than refused, because the bound is a property of the front
/// end and not of the request: the caller asked for a window, and a window is
/// what they get. What is refused is a value that is not a number at all.
///
/// `max` is the caller's, because the two front ends do not agree on one: the
/// viewer draws hexagons and stops at a couple of hundred tiles a side, and the
/// tuner draws single pixels and goes to a couple of thousand.
///
/// # Errors
///
/// [`ViewError::NotAnInteger`] for text that is not a whole number.
pub fn tile_count(parameter: &'static str, text: &str, max: u32) -> Result<u32, ViewError> {
    count(parameter, text, max)
}

/// Parses a tile count and clamps it into the range this server will draw.
fn count(parameter: &'static str, text: &str, max: u32) -> Result<u32, ViewError> {
    let value: i64 = text.parse().map_err(|_| ViewError::NotAnInteger {
        parameter,
        value: text.to_string(),
    })?;
    let clamped = value.clamp(1, i64::from(max));
    Ok(u32::try_from(clamped).expect("a value clamped into 1..=max fits a u32"))
}

/// Parses a hex radius and clamps it into the range this server will draw.
fn radius(parameter: &'static str, text: &str) -> Result<f32, ViewError> {
    let value: f32 = text.parse().map_err(|_| ViewError::NotANumber {
        parameter,
        value: text.to_string(),
    })?;
    if !value.is_finite() {
        return Err(ViewError::NotANumber {
            parameter,
            value: text.to_string(),
        });
    }
    Ok(value.clamp(MIN_HEX_RADIUS, MAX_HEX_RADIUS))
}

/// The first value a query string gives for one key.
#[must_use]
pub fn lookup(query: &str, key: &str) -> Option<String> {
    pairs(query)
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

/// Every `key=value` pair of a query string, percent-decoded.
pub fn pairs(query: &str) -> impl Iterator<Item = (String, String)> + '_ {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            (decode(key), decode(value))
        })
}

/// Percent-decodes one query component, treating `+` as a space.
///
/// Public because a front end that reads a form body reads exactly this
/// encoding: `application/x-www-form-urlencoded` is a query string that
/// arrived in a body rather than in a path.
///
/// Nothing this viewer emits needs encoding — seeds are hex digits, coordinates
/// are decimal, layer names are ASCII — but a URL that has been through a
/// browser, a chat client, or an issue tracker may well arrive encoded anyway.
/// Invalid escapes are left as written rather than dropped, so an error message
/// quotes back what was actually sent.
#[must_use]
pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            // Both digits are checked before either is used. Slicing `text`
            // on an unvalidated `%` would index into the middle of a multi-byte
            // character and panic, which is a request turning into a crash.
            b'%' if index + 2 < bytes.len()
                && bytes[index + 1].is_ascii_hexdigit()
                && bytes[index + 2].is_ascii_hexdigit() =>
            {
                let high = char::from(bytes[index + 1])
                    .to_digit(16)
                    .expect("checked to be a hexadecimal digit");
                let low = char::from(bytes[index + 2])
                    .to_digit(16)
                    .expect("checked to be a hexadecimal digit");
                out.push(u8::try_from(high * 16 + low).expect("two hex digits are one byte"));
                index += 3;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
