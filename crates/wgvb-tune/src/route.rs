//! What a URL asks this server for.
//!
//! The window grammar is `wgvb-view`'s and is shared with the viewer, so a
//! coordinate, a tile count, and a layer are spelled the same way in both
//! tools and a window can be carried between them by copying a query string.
//! What is here is what the tuner adds: three tabs instead of one page, a turn,
//! a pixel scale, and a grid window that is measured in thousands of tiles
//! rather than in hundreds.
//!
//! Everything about the *view* is still in the URL. Only the configuration is
//! not, and that is the whole of the departure.

use wgvb::Seed;
use wgvb_render::{Grid, Layer, RenderError, Viewport};
use wgvb_view::{View, ViewError, lookup, seed_route, split_target, tile_count};

/// Grid tiles across, when the URL does not say.
///
/// Odd, and that is not a typo for a round thousand: a window with an even tile
/// count has no center cell, and this tab needs one twice over — the turn
/// pivots on it and a click reads it back. One extra hex buys both.
pub const DEFAULT_GRID_COLS: u32 = 1001;
/// Grid tiles down, when the URL does not say.
pub const DEFAULT_GRID_ROWS: u32 = 1001;
/// Widest grid window, in tiles.
pub const MAX_GRID_COLS: u32 = 2001;
/// Tallest grid window, in tiles.
pub const MAX_GRID_ROWS: u32 = 2001;
/// Largest pixels-per-tile the grid will draw at.
pub const MAX_SCALE: u32 = 8;

/// Generator evaluations a single request may cost, unless the operator says
/// otherwise.
///
/// Counted in evaluations rather than in tiles, because `relief`, `climate`,
/// and `terrain` cost seven each and everything else costs one: a budget that
/// could not tell them apart would either refuse a cheap window or accept one
/// seven times longer than anybody meant. See [`wgvb_render::Layer::cost`].
///
/// Eight million is about a second of eight cores at the 7.4 microseconds a
/// tile costs today, which makes the default window — a thousand by a thousand
/// of any layer — comfortable, and a two-thousand-square window of `terrain` a
/// refusal rather than a minute of silence. `--budget` raises it, because
/// measuring how long a large window takes is a thing this tool exists for.
pub const DEFAULT_BUDGET: u64 = 8_000_000;

/// The parameters this server understands on top of a [`View`]'s.
///
/// `click.x` and `click.y` are what `<input type="image">` submits, which is
/// how a page with no script turns a click on a map into a coordinate.
/// `seed` is the seed box, which posts to `/seed` and is redirected from there.
const EXTRA_PARAMETERS: [&str; 7] = [
    "turn",
    "scale",
    "grid-cols",
    "grid-rows",
    "click.x",
    "click.y",
    "seed",
];

/// Which tab, or which endpoint under one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// `/seed/{seed}` — the hex map.
    Map,
    /// `/seed/{seed}/map.png` — the hex map's image.
    MapImage,
    /// `/seed/{seed}/grid` — one pixel per hex.
    Grid,
    /// `/seed/{seed}/grid.png` — the grid's image.
    GridImage,
    /// `/seed/{seed}/config` — the configuration form.
    Config,
}

/// What a POST asks this server to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Apply the edited fields of the configuration form.
    Fields,
    /// Adopt an uploaded configuration file.
    Upload,
    /// Go back to the configuration this binary ships.
    Reset,
}

/// Everything a URL names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// A tab or an image.
    Read(Tab),
    /// A change to the configuration.
    Write(Change),
    /// The configuration file itself, which belongs to the session rather than
    /// to any seed and so hangs off the root.
    Download,
    /// The bare root, which redirects to a tab.
    Root,
    /// The seed box, which redirects to the seed it names.
    Open,
    /// A seed nobody chose, which redirects to one.
    Random,
}

/// The whole view a URL asks for: a [`View`] plus what the tuner adds.
///
/// The grid's tile counts are separate parameters from the hex window's, and
/// deliberately: they differ by an order of magnitude, so one pair clamped to
/// serve both would either cripple the grid or let the hex tab ask for four
/// million hexagons. Keeping them apart means switching tabs never resizes the
/// tab you came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    /// The hex window: seed, center, tile counts, hex radius, layer.
    pub view: View,
    /// Sixths of a turn the sampled region is rotated by.
    pub turn: u8,
    /// Pixels per tile on the grid tab.
    pub scale: u32,
    /// Grid tiles across.
    pub grid_cols: u32,
    /// Grid tiles down.
    pub grid_rows: u32,
}

impl Window {
    /// The window a bare `/seed/{seed}` shows.
    #[must_use]
    pub fn origin_of(seed: Seed) -> Window {
        Window {
            view: View::origin_of(seed),
            turn: 0,
            scale: 1,
            grid_cols: DEFAULT_GRID_COLS,
            grid_rows: DEFAULT_GRID_ROWS,
        }
    }

    /// Parses a request target into what it names and the window it names it
    /// for.
    ///
    /// # Errors
    ///
    /// Every malformed piece of a URL is a [`ViewError`] naming the piece.
    pub fn parse(target: &str) -> Result<(Target, Window), ViewError> {
        let (path, query) = split_target(target);
        if path.is_empty() || path == "/" {
            return Ok((Target::Root, Window::origin_of(0)));
        }
        if path == "/config.toml" {
            return Ok((Target::Download, Window::origin_of(0)));
        }
        // The seed box and the random link both land on a window that has not
        // been given a seed yet, so they parse the rest of the query against a
        // placeholder and the handler supplies the seed.
        if path == "/seed" {
            return Ok((Target::Open, Window::from_query(0, query)?));
        }
        if path == "/random" {
            return Ok((Target::Random, Window::from_query(0, query)?));
        }

        let (seed, tail) = seed_route(path)?;
        let target = match tail {
            "" => Target::Read(Tab::Map),
            "/map.png" => Target::Read(Tab::MapImage),
            "/grid" => Target::Read(Tab::Grid),
            "/grid.png" => Target::Read(Tab::GridImage),
            "/config" => Target::Read(Tab::Config),
            "/config/fields" => Target::Write(Change::Fields),
            "/config/upload" => Target::Write(Change::Upload),
            "/config/reset" => Target::Write(Change::Reset),
            _ => {
                return Err(ViewError::NoRoute {
                    path: path.to_string(),
                });
            }
        };

        // A write carries a body rather than a window, but it still carries the
        // window it was made from, so that the redirect afterwards lands back
        // where the person was looking.
        Ok((target, Window::from_query(seed, query)?))
    }

    /// Parses the query string of a request against this seed.
    fn from_query(seed: Seed, query: &str) -> Result<Window, ViewError> {
        let known: Vec<&str> = wgvb_view::KNOWN_PARAMETERS
            .iter()
            .copied()
            .chain(EXTRA_PARAMETERS)
            .collect();
        wgvb_view::reject_unknown(query, &known)?;

        let mut window = Window::origin_of(seed);
        window.view = View::from_query(seed, query)?;

        if let Some(text) = lookup(query, "turn") {
            // Not `tile_count`: that clamps into `1..=max`, because a window of
            // zero tiles is not a window. A turn of zero is the commonest turn
            // there is, so it has its own clamp.
            window.turn = turn("turn", &text)?;
        }
        if let Some(text) = lookup(query, "scale") {
            window.scale = tile_count("scale", &text, MAX_SCALE)?;
        }
        if let Some(text) = lookup(query, "grid-cols") {
            window.grid_cols = odd(tile_count("grid-cols", &text, MAX_GRID_COLS)?);
        }
        if let Some(text) = lookup(query, "grid-rows") {
            window.grid_rows = odd(tile_count("grid-rows", &text, MAX_GRID_ROWS)?);
        }

        Ok(window)
    }

    /// The hex viewport this window draws.
    ///
    /// # Errors
    ///
    /// Whichever render gate refused, which the caller turns into a 400.
    pub fn viewport(&self) -> Result<Viewport, RenderError> {
        self.view.viewport()?.turned(self.turn)
    }

    /// The grid window this window draws.
    ///
    /// # Errors
    ///
    /// Whichever render gate refused.
    pub fn grid(&self) -> Result<Grid, RenderError> {
        Grid::centered_on(
            self.view.center,
            self.grid_cols,
            self.grid_rows,
            self.scale,
            self.turn,
        )
    }

    /// This window with one thing changed, for the links a page emits.
    #[must_use]
    pub fn with_turn(&self, turn: u8) -> Window {
        Window { turn, ..*self }
    }

    /// This window at a different pixel scale.
    #[must_use]
    pub fn with_scale(&self, scale: u32) -> Window {
        Window { scale, ..*self }
    }

    /// This window with a different layer selected.
    #[must_use]
    pub fn with_layer(&self, layer: Layer) -> Window {
        Window {
            view: self.view.with_layer(layer),
            ..*self
        }
    }

    /// This window with a different grid size.
    #[must_use]
    pub fn with_grid(&self, cols: u32, rows: u32) -> Window {
        Window {
            grid_cols: odd(cols.min(MAX_GRID_COLS)),
            grid_rows: odd(rows.min(MAX_GRID_ROWS)),
            ..*self
        }
    }

    /// This window centered somewhere else.
    #[must_use]
    pub fn centered_on(&self, center: wgvb::Coord) -> Window {
        let mut moved = *self;
        moved.view.center = center;
        moved
    }

    /// The URL of one tab showing this window.
    #[must_use]
    pub fn url(&self, tab: Tab) -> String {
        format!("{}?{}", self.path(tab), self.query())
    }

    /// One tab's path, without a query.
    ///
    /// What a `<form method="get">` posts to: a form builds its own query out
    /// of its fields, so an action carrying one would have it thrown away.
    #[must_use]
    pub fn path(&self, tab: Tab) -> String {
        let path = match tab {
            Tab::Map => "",
            Tab::MapImage => "/map.png",
            Tab::Grid => "/grid",
            Tab::GridImage => "/grid.png",
            Tab::Config => "/config",
        };
        format!("/seed/{:016x}{path}", self.view.seed)
    }

    /// The URL a configuration change posts to.
    #[must_use]
    pub fn url_for_change(&self, change: &str) -> String {
        format!(
            "/seed/{:016x}/config/{change}?{}",
            self.view.seed,
            self.query()
        )
    }

    /// This window scrolled one press of one compass control.
    #[must_use]
    pub fn scrolled(&self, compass: wgvb_view::Compass) -> Window {
        Window {
            view: self.view.scrolled(compass),
            ..*self
        }
    }

    /// This window with a different seed.
    #[must_use]
    pub fn with_seed(&self, seed: Seed) -> Window {
        let mut moved = *self;
        moved.view.seed = seed;
        moved
    }

    /// Every parameter of this window, for a form that has to carry the whole
    /// of it in hidden fields.
    ///
    /// The same list [`Window::query`] spells, and deliberately the same order:
    /// a form that dropped one would silently reset it on submit, which is the
    /// kind of bug that looks like the server ignoring you.
    #[must_use]
    pub fn parameters(&self) -> Vec<(&'static str, String)> {
        vec![
            ("q", self.view.center.q().to_string()),
            ("r", self.view.center.r().to_string()),
            ("cols", self.view.cols.to_string()),
            ("rows", self.view.rows.to_string()),
            ("hex-radius", self.view.hex_radius.to_string()),
            ("layer", self.view.layer.name().to_string()),
            ("turn", self.turn.to_string()),
            ("scale", self.scale.to_string()),
            ("grid-cols", self.grid_cols.to_string()),
            ("grid-rows", self.grid_rows.to_string()),
        ]
    }

    /// The query string every URL carries. Always complete, so a link never
    /// relies on a default and following one cannot change meaning when a
    /// default does.
    #[must_use]
    pub fn query(&self) -> String {
        format!(
            "{}&turn={}&scale={}&grid-cols={}&grid-rows={}",
            self.view.query(),
            self.turn,
            self.scale,
            self.grid_cols,
            self.grid_rows,
        )
    }
}

/// Parses a turn and clamps it into `0..6`.
///
/// Clamped rather than refused, for the same reason a tile count is: every turn
/// control on a page emits a value in range, so a value out of it came from
/// somewhere else and a picture is a better answer than a refusal. What is
/// refused is text that is not a whole number at all.
fn turn(parameter: &'static str, text: &str) -> Result<u8, ViewError> {
    let value: i64 = text.parse().map_err(|_| ViewError::NotAnInteger {
        parameter,
        value: text.to_string(),
    })?;
    u8::try_from(value.clamp(0, 5)).map_err(|_| ViewError::NotAnInteger {
        parameter,
        value: text.to_string(),
    })
}

/// The next odd number at or below one that was asked for.
///
/// A window needs a center cell — the turn pivots on it and a click reads it
/// back — so an even count is rounded down rather than refused. Rounded *down*
/// so that a clamped maximum stays inside the clamp.
const fn odd(count: u32) -> u32 {
    if count.is_multiple_of(2) && count > 1 {
        count - 1
    } else {
        count
    }
}
