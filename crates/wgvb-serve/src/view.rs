//! The viewer's state, which is entirely a URL.
//!
//! Every state this server can present is reachable by typing a link, and every
//! link it emits parses back to the state that produced it. That is the whole
//! design: no client-side panning, no canvas, no script, and therefore nothing
//! to get out of step with the address bar.

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

/// The query parameters this server understands.
///
/// An unrecognized parameter is rejected rather than ignored, for the same
/// reason `Config` carries `deny_unknown_fields`: a silently dropped `?col=61`
/// looks exactly like a server that does not work.
const KNOWN_PARAMETERS: [&str; 6] = ["q", "r", "cols", "rows", "hex-radius", "layer"];

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
#[derive(Debug, thiserror::Error)]
pub enum RequestError {
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

impl RequestError {
    /// The HTTP status this refusal answers with.
    ///
    /// Everything here is the caller's fault, so nothing here is a 500. A
    /// window too large for [`wgvb_render::MAX_IMAGE_PIXELS`] included: the
    /// caller chose the size.
    #[must_use]
    pub const fn status(&self) -> u16 {
        match self {
            RequestError::NoRoute { .. } => 404,
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
    /// The scalar layer to draw.
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
    /// Every malformed piece of a URL is a [`RequestError`] naming the piece.
    /// Nothing here defaults silently and nothing here panics.
    pub fn parse(target: &str) -> Result<(Route, View), RequestError> {
        let (path, query) = split_target(target);
        let (route, seed_text) = parse_route(path)?;
        let view = View::from_query(parse_seed(seed_text)?, query)?;
        Ok((route, view))
    }

    /// Parses the query string of a request against this seed.
    fn from_query(seed: Seed, query: &str) -> Result<View, RequestError> {
        let mut view = View::origin_of(seed);

        for (key, _) in pairs(query) {
            if !KNOWN_PARAMETERS.contains(&key.as_str()) {
                return Err(RequestError::UnknownParameter {
                    name: key,
                    known: KNOWN_PARAMETERS.join(", "),
                });
            }
        }

        let q = lookup(query, "q");
        let r = lookup(query, "r");
        match (q.as_deref(), r.as_deref()) {
            (Some(q), Some(r)) => {
                view.center = Coord::new(component("q", q)?, component("r", r)?);
            }
            (Some(_), None) => {
                return Err(RequestError::HalfACenter {
                    present: "q",
                    missing: "r",
                });
            }
            (None, Some(_)) => {
                return Err(RequestError::HalfACenter {
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
            view.layer = Layer::parse(&text).map_err(|_| RequestError::UnknownLayer {
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
        format!("/seed/{:016x}?{}", self.seed, self.query())
    }

    /// The URL of the rendered window on its own.
    #[must_use]
    pub fn image_url(&self) -> String {
        format!("/seed/{:016x}/map.png?{}", self.seed, self.query())
    }

    /// The query string both URLs carry. Always complete: a link this server
    /// emits never relies on a default, so following it cannot change meaning
    /// when a default does.
    fn query(&self) -> String {
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

/// Splits a request target into its path and its query string.
fn split_target(target: &str) -> (&str, &str) {
    match target.split_once('?') {
        Some((path, query)) => (path, query),
        None => (target, ""),
    }
}

/// Matches a path against the two routes, returning the seed text it carries.
fn parse_route(path: &str) -> Result<(Route, &str), RequestError> {
    let trimmed = path.strip_suffix('/').unwrap_or(path);
    if let Some(rest) = trimmed.strip_prefix("/seed/") {
        if let Some(seed) = rest.strip_suffix("/map.png") {
            return Ok((Route::Image, seed));
        }
        if !rest.contains('/') {
            return Ok((Route::Page, rest));
        }
    }
    Err(RequestError::NoRoute {
        path: path.to_string(),
    })
}

/// Parses the sixteen hexadecimal digits of a seed.
///
/// Hex because that is how a seed is written everywhere else in the repository:
/// `0x0123_4567_89ab_cdef` is the golden seed and `/seed/0123456789abcdef` is
/// the same number. Case-insensitive, no `0x`, exactly sixteen digits, and
/// anything else is a refusal rather than a silent zero.
fn parse_seed(text: &str) -> Result<Seed, RequestError> {
    if text.len() != 16 {
        return Err(RequestError::SeedLength {
            seed: text.to_string(),
            got: text.chars().count(),
        });
    }
    if !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(RequestError::SeedDigits {
            seed: text.to_string(),
        });
    }
    Seed::from_str_radix(text, 16).map_err(|_| RequestError::SeedDigits {
        seed: text.to_string(),
    })
}

/// Parses one coordinate component.
///
/// Out of [`Component`] range is a refusal rather than a normalization. Every
/// link this server emits is already canonical, so a number outside the range
/// came from somewhere else and the reader deserves to be told, not to be shown
/// a different tile than the one they named.
fn component(parameter: &'static str, text: &str) -> Result<i64, RequestError> {
    let value: i64 = text.parse().map_err(|_| RequestError::NotAnInteger {
        parameter,
        value: text.to_string(),
    })?;
    let (min, max) = (i64::from(Component::MIN), i64::from(Component::MAX));
    if value < min || value > max {
        return Err(RequestError::CoordinateRange {
            parameter,
            value,
            min,
            max,
        });
    }
    Ok(value)
}

/// Parses a tile count and clamps it into the range this server will draw.
///
/// Clamped rather than refused, because the bound is a property of this server
/// and not of the request: the caller asked for a window, and a window is what
/// they get. What is refused is a value that is not a number at all.
fn count(parameter: &'static str, text: &str, max: u32) -> Result<u32, RequestError> {
    let value: i64 = text.parse().map_err(|_| RequestError::NotAnInteger {
        parameter,
        value: text.to_string(),
    })?;
    let clamped = value.clamp(1, i64::from(max));
    Ok(u32::try_from(clamped).expect("a value clamped into 1..=max fits a u32"))
}

/// Parses a hex radius and clamps it into the range this server will draw.
fn radius(parameter: &'static str, text: &str) -> Result<f32, RequestError> {
    let value: f32 = text.parse().map_err(|_| RequestError::NotANumber {
        parameter,
        value: text.to_string(),
    })?;
    if !value.is_finite() {
        return Err(RequestError::NotANumber {
            parameter,
            value: text.to_string(),
        });
    }
    Ok(value.clamp(MIN_HEX_RADIUS, MAX_HEX_RADIUS))
}

/// The first value a query string gives for one key.
fn lookup(query: &str, key: &str) -> Option<String> {
    pairs(query)
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

/// Every `key=value` pair of a query string, percent-decoded.
fn pairs(query: &str) -> impl Iterator<Item = (String, String)> + '_ {
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
/// Nothing this viewer emits needs encoding — seeds are hex digits, coordinates
/// are decimal, layer names are ASCII — but a URL that has been through a
/// browser, a chat client, or an issue tracker may well arrive encoded anyway.
/// Invalid escapes are left as written rather than dropped, so an error message
/// quotes back what was actually sent.
fn decode(text: &str) -> String {
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
