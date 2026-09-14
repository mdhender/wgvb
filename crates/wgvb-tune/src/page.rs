//! The three tabs.
//!
//! One HTML document each, no script, no stylesheet to fetch. Every control is
//! either a link to another URL of this server or a form that posts to one, so
//! the browser's back button is still the undo history for everything except
//! the configuration — which is the one thing here that is not in the URL.
//!
//! # Escaping
//!
//! `wgvb-serve`'s page needs almost no escaping and says why: every value it
//! interpolates is a number, a coordinate component, or a `&'static str` from
//! the core crate. This page cannot make that claim. The configuration form
//! echoes back what somebody typed, so that a refused edit does not throw their
//! typing away, and echoed text is the one thing on a page that can carry
//! markup. Everything that came from a request goes through [`escape`].

use std::fmt::Write as _;

use wgvb::{ALGORITHM_VERSION, Climate, Coord, Generator, HeatBand, MoistureBand, Terrain};
use wgvb_render::{
    Grid, Key, Layer, RENDER_VERSION, Scale, Viewport, climate_color, color, terrain_color,
};
use wgvb_view::{COMPASS, Compass, MAX_HEX_RADIUS, MIN_HEX_RADIUS};

use crate::reply::coord_text;
use crate::route::{MAX_GRID_COLS, MAX_SCALE, Tab, Window};
use crate::session::State;

/// Hex radii the zoom control offers, smallest first.
///
/// A ladder rather than a box, because zooming is the thing somebody does
/// twenty times in a row. The page the viewer ships tells a reader to edit the
/// address bar instead; this is that sentence turned into a control.
const ZOOM_STEPS: [f32; 8] = [1.0, 2.0, 3.0, 4.0, 6.0, 10.0, 16.0, 24.0];

/// Grid window sizes the size control offers, in tiles on a side.
///
/// All odd, because a window with no center cell has nothing for the turn to
/// pivot on and nothing for a click to read back.
const GRID_STEPS: [u32; 5] = [251, 501, 1001, 1501, MAX_GRID_COLS];

/// The hex map.
#[must_use]
pub fn map(window: &Window, viewport: &Viewport, generator: &Generator, state: &State) -> String {
    let (width, height) = viewport.image_size();
    let (cols, rows) = viewport.tile_counts();
    let tiles = u64::from(cols) * u64::from(rows);

    let mut html = shell(window, Tab::Map, state);

    let _ = writeln!(
        html,
        "<div class=\"map\">{}</div>",
        clickable(
            window,
            Tab::Map,
            &window.url(Tab::MapImage),
            width,
            height,
            &format!(
                "the {} layer around {}",
                window.view.layer.name(),
                coord_text(window.view.center)
            ),
        )
    );

    html.push_str(&compass_rose(window));
    html.push_str(&turns(window, Tab::Map, false));
    html.push_str(&zoom(window));
    html.push_str(&layers(window, Tab::Map));
    html.push_str(&legend(window.view.layer));

    html.push_str("<dl class=\"readout\">\n");
    let _ = writeln!(
        html,
        "<dt>center</dt><dd><code>{}</code> canonical <code>(q, r, s)</code></dd>",
        coord_text(window.view.center)
    );
    html.push_str(&tile_readout(generator, window.view.center));
    let _ = writeln!(
        html,
        "<dt>window</dt><dd>{cols} x {rows} tiles, {tiles} in all, at hex radius {} px \
         ({width} x {height} px), turn {}</dd>",
        window.view.hex_radius, window.turn,
    );
    html.push_str("</dl>\n");

    html.push_str(&window_form(window));
    html.push_str(&jump_form(window, Tab::Map));
    html.push_str(&footer(state));
    html.push_str("</body>\n</html>\n");
    html
}

/// The grid: one pixel per hex, for a very large area.
#[must_use]
pub fn grid(window: &Window, grid: &Grid, generator: &Generator, state: &State) -> String {
    let (width, height) = grid.image_size();
    let (cols, rows) = grid.tile_counts();

    let mut html = shell(window, Tab::Grid, state);

    let _ = writeln!(
        html,
        "<div class=\"map\">{}</div>",
        clickable(
            window,
            Tab::Grid,
            &window.url(Tab::GridImage),
            width,
            height,
            &format!(
                "the {} layer around {}, one pixel per hex",
                window.view.layer.name(),
                coord_text(window.view.center)
            ),
        )
    );

    html.push_str(&turns(window, Tab::Grid, grid.is_sheared()));
    html.push_str(&grid_sizes(window));
    html.push_str(&scales(window));
    html.push_str(&layers(window, Tab::Grid));
    html.push_str(&legend(window.view.layer));

    html.push_str("<dl class=\"readout\">\n");
    let _ = writeln!(
        html,
        "<dt>center</dt><dd><code>{}</code> canonical <code>(q, r, s)</code></dd>",
        coord_text(window.view.center)
    );
    html.push_str(&tile_readout(generator, window.view.center));
    let _ = writeln!(
        html,
        "<dt>window</dt><dd>{cols} x {rows} tiles, {} in all, at {} px per hex \
         ({width} x {height} px), turn {}</dd>",
        grid.tiles(),
        window.scale,
        window.turn,
    );
    let _ = writeln!(
        html,
        "<dt>cost</dt><dd>{} generator evaluations, at {} each for the \
         <code>{}</code> layer</dd>",
        grid.tiles() * u64::from(window.view.layer.cost()),
        window.view.layer.cost(),
        window.view.layer.name(),
    );
    html.push_str("</dl>\n");

    let _ = writeln!(
        html,
        "<p class=\"notice\">A hex row's centers are <code>sqrt(3) r</code> apart and a \
         column's are <code>1.5 r</code>, so drawing both as one pixel stretches this \
         image vertically by about 15% and flattens the half-hex stagger between \
         columns. That is the whole of the distortion at turn 0 and turn 3, and it is \
         the price of seeing {} tiles at once.</p>",
        grid.tiles(),
    );

    html.push_str(&jump_form(window, Tab::Grid));
    html.push_str(&footer(state));
    html.push_str("</body>\n</html>\n");
    html
}

/// The configuration, as a form.
#[must_use]
pub fn config(
    window: &Window,
    state: &State,
    error: Option<&str>,
    posted: &[(String, String)],
) -> String {
    let mut html = shell(window, Tab::Config, state);

    if let Some(error) = error {
        let _ = writeln!(
            html,
            "<p class=\"notice refused\"><strong>Nothing was changed.</strong> {}</p>",
            escape(error)
        );
    }

    let fields = match wgvb_config::fields(&state.config) {
        Ok(fields) => fields,
        Err(error) => {
            let _ = writeln!(
                html,
                "<p class=\"notice refused\">{}</p>",
                escape(&error.to_string())
            );
            return html;
        }
    };
    let changed = fields.iter().filter(|field| field.is_changed()).count();

    let _ = writeln!(
        html,
        "<p class=\"notice\">The complete effective configuration: {} fields, {changed} \
         of them moved off the value this binary ships. A field is applied only if you \
         change it, so submitting this form untouched does nothing. Everything here is \
         <strong>session state</strong> — it is not in the URL, it is not written to any \
         file, and it is gone when this server stops. Download it to keep it.</p>",
        fields.len(),
    );

    let _ = write!(
        html,
        "<form method=\"post\" action=\"{}\">\n<table class=\"config\">\n\
         <tr><th>field</th><th>value</th><th>default</th></tr>\n",
        window.url_for_change("fields")
    );
    for field in &fields {
        // What was typed wins over what is in force, so a refused edit keeps
        // the number somebody was trying rather than silently reverting it.
        let shown = posted
            .iter()
            .find(|(name, _)| name == &field.name)
            .map_or(field.value.clone(), |(_, value)| value.clone());
        let _ = writeln!(
            html,
            "<tr class=\"{}\"><th scope=\"row\"><label for=\"f-{}\">{}</label></th>\
             <td><input id=\"f-{}\" name=\"{}\" value=\"{}\" \
             inputmode=\"{}\" size=\"18\"></td><td><code>{}</code></td></tr>",
            if field.is_changed() { "moved" } else { "" },
            escape(&field.name),
            escape(&field.name),
            escape(&field.name),
            escape(&field.name),
            escape(&shown),
            if field.whole { "numeric" } else { "decimal" },
            escape(&field.default),
        );
    }
    html.push_str("</table>\n<p><button type=\"submit\">apply</button></p>\n</form>\n");

    let _ = write!(
        html,
        "<form method=\"post\" action=\"{}\">\n\
         <p><button type=\"submit\">reset to this binary's defaults</button></p>\n</form>\n",
        window.url_for_change("reset")
    );

    let _ = write!(
        html,
        "<h2>start from a file</h2>\n\
         <form method=\"post\" action=\"{}\" enctype=\"multipart/form-data\">\n\
         <p><input type=\"file\" name=\"file\" accept=\".toml,text/plain\"> \
         <button type=\"submit\">upload</button></p>\n</form>\n",
        window.url_for_change("upload")
    );
    let _ = write!(
        html,
        "<form method=\"post\" action=\"{}\">\n\
         <p><textarea name=\"pasted\" rows=\"6\" cols=\"64\" \
         placeholder=\"paste a configuration file here\"></textarea></p>\n\
         <p><button type=\"submit\">use what is pasted</button></p>\n</form>\n",
        window.url_for_change("upload")
    );

    let _ = writeln!(
        html,
        "<p><a href=\"/config.toml\" download>download the configuration in force</a> \
         — the complete effective configuration as TOML, with the algorithm version \
         and the fingerprint in the header. <code>wgvb-tune --config</code> reads it \
         back.</p>"
    );

    html.push_str(&footer(state));
    html.push_str("</body>\n</html>\n");
    html
}

/// The document head, the heading, and the tab bar.
fn shell(window: &Window, tab: Tab, state: &State) -> String {
    let mut html = String::with_capacity(16 * 1024);
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    html.push_str("<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = writeln!(
        html,
        "<title>wgvb-tune {} {}</title>",
        window.view.seed_text(),
        window.view.layer.name()
    );
    html.push_str(STYLE);
    html.push_str("</head>\n<body>\n");

    let _ = writeln!(
        html,
        "<h1>seed <code>{}</code> <span class=\"quiet\">configuration</span> \
         <code>{}</code></h1>",
        window.view.seed_text(),
        state.short_fingerprint(),
    );

    html.push_str("<nav class=\"tabs\" aria-label=\"tabs\">\n");
    for (name, each) in [
        ("map", Tab::Map),
        ("grid", Tab::Grid),
        ("config", Tab::Config),
    ] {
        if each == tab {
            let _ = write!(
                html,
                "<a href=\"{}\" aria-current=\"page\" class=\"current\">{name}</a>",
                window.url(each)
            );
        } else {
            let _ = write!(html, "<a href=\"{}\">{name}</a>", window.url(each));
        }
    }
    html.push_str("\n</nav>\n");
    html
}

/// The image, wrapped in the form that turns a click into a new center.
///
/// `<input type="image">` submits the click position as `click.x` and
/// `click.y`, which is how a page with no script lets somebody click a map.
/// Everything else about the window rides along as hidden fields, so clicking
/// moves the center and changes nothing else.
fn clickable(window: &Window, tab: Tab, src: &str, width: u32, height: u32, alt: &str) -> String {
    let mut html = String::new();
    let _ = write!(
        html,
        "<form method=\"get\" action=\"{}\">{}\
         <input type=\"image\" name=\"click\" src=\"{src}\" width=\"{width}\" \
         height=\"{height}\" alt=\"{}\" title=\"click to center here\">",
        window.path(tab),
        hidden(window),
        escape(alt),
    );
    html.push_str("</form>");
    html
}

/// Every part of the window as hidden form fields.
fn hidden(window: &Window) -> String {
    let mut html = String::new();
    for (name, value) in window.parameters() {
        let _ = write!(
            html,
            "<input type=\"hidden\" name=\"{name}\" value=\"{}\">",
            escape(&value)
        );
    }
    html
}

/// The six scroll controls, laid out as they point.
fn compass_rose(window: &Window) -> String {
    let at = |name: &str| -> Compass {
        *COMPASS
            .iter()
            .find(|compass| compass.name == name)
            .expect("the compass table names every point of the rose")
    };

    let mut html = String::new();
    html.push_str("<nav class=\"rose\" aria-label=\"scroll the view\">\n");
    let rose = [
        Some(at("NW")),
        Some(at("N")),
        Some(at("NE")),
        None,
        None,
        None,
        Some(at("SW")),
        Some(at("S")),
        Some(at("SE")),
    ];
    for (index, cell) in rose.into_iter().enumerate() {
        match cell {
            Some(compass) => {
                let steps = compass.steps(window.view.cols, window.view.rows);
                let _ = write!(
                    html,
                    "<a href=\"{}\" title=\"move the view {} by {steps} hexes\">{}\
                     <small>{steps}</small></a>",
                    window.scrolled(compass).url(Tab::Map),
                    compass.heading,
                    compass.name,
                );
            }
            None if index == 4 => html.push_str("<span class=\"hub here\">here</span>"),
            None => html.push_str("<span class=\"hub\"></span>"),
        }
    }
    html.push_str("\n</nav>\n");
    html
}

/// The turn control: six links, and the warning when the grid is shearing.
fn turns(window: &Window, tab: Tab, sheared: bool) -> String {
    let mut html = String::new();
    html.push_str(
        "<nav class=\"layers\" aria-label=\"turn the view\"><span class=\"quiet\">turn</span>",
    );
    for turn in 0..6_u8 {
        let class = if turn == window.turn {
            " class=\"current\""
        } else {
            ""
        };
        let _ = write!(
            html,
            "<a href=\"{}\"{class} title=\"{turn} sixths of a turn\">{turn}</a>",
            window.with_turn(turn).url(tab),
        );
    }
    html.push_str("</nav>\n");

    if sheared {
        let _ = writeln!(
            html,
            "<p class=\"notice warning\"><strong>This turn shears the picture.</strong> \
             One pixel per hex squashes one screen axis against the other, and that \
             distortion has two-fold symmetry while the hex grid has six-fold — so only \
             turn 0 and turn 3 line up with it. At turn {}, angles are wrong, a rounded \
             coastline reads as a tilted ellipse, and the shear adds apparent \
             directionality that changes with the turn, which is exactly the artifact a \
             turned view is usually being used to look for. The <a href=\"{}\">map \
             tab</a> draws real hexagons and is where a rotated shape stays the same \
             shape.</p>",
            window.turn,
            window.url(Tab::Map),
        );
    }
    html
}

/// The zoom control: one link per hex radius.
fn zoom(window: &Window) -> String {
    let mut html = String::new();
    html.push_str(
        "<nav class=\"layers\" aria-label=\"zoom\"><span class=\"quiet\">hex radius</span>",
    );
    for radius in ZOOM_STEPS {
        if radius < MIN_HEX_RADIUS || radius > MAX_HEX_RADIUS {
            continue;
        }
        let mut zoomed = *window;
        zoomed.view.hex_radius = radius;
        let class = if (radius - window.view.hex_radius).abs() < f32::EPSILON {
            " class=\"current\""
        } else {
            ""
        };
        let _ = write!(
            html,
            "<a href=\"{}\"{class}>{radius}</a>",
            zoomed.url(Tab::Map)
        );
    }
    html.push_str("</nav>\n");
    html
}

/// The grid size control: one link per square window.
fn grid_sizes(window: &Window) -> String {
    let mut html = String::new();
    html.push_str(
        "<nav class=\"layers\" aria-label=\"window size\"><span class=\"quiet\">tiles</span>",
    );
    for size in GRID_STEPS {
        let class = if size == window.grid_cols && size == window.grid_rows {
            " class=\"current\""
        } else {
            ""
        };
        let _ = write!(
            html,
            "<a href=\"{}\"{class}>{size}&sup2;</a>",
            window.with_grid(size, size).url(Tab::Grid)
        );
    }
    html.push_str("</nav>\n");
    html
}

/// The pixels-per-hex control.
fn scales(window: &Window) -> String {
    let mut html = String::new();
    html.push_str(
        "<nav class=\"layers\" aria-label=\"pixels per hex\"><span class=\"quiet\">px per hex</span>",
    );
    for scale in 1..=MAX_SCALE {
        if !scale.is_power_of_two() {
            continue;
        }
        let class = if scale == window.scale {
            " class=\"current\""
        } else {
            ""
        };
        let _ = write!(
            html,
            "<a href=\"{}\"{class}>{scale}</a>",
            window.with_scale(scale).url(Tab::Grid)
        );
    }
    html.push_str("</nav>\n");
    html
}

/// The layer selector: one link per layer, the current one marked.
fn layers(window: &Window, tab: Tab) -> String {
    let mut html = String::new();
    html.push_str("<nav class=\"layers\" aria-label=\"choose a layer\">\n");
    for layer in Layer::ALL {
        let class = if layer == window.view.layer {
            " aria-current=\"page\" class=\"current\""
        } else {
            ""
        };
        let _ = write!(
            html,
            "<a href=\"{}\"{class}>{}</a>",
            window.with_layer(layer).url(tab),
            layer.name()
        );
    }
    html.push_str("\n</nav>\n");
    html
}

/// The window size form, which is the one control the viewer has no link for.
fn window_form(window: &Window) -> String {
    format!(
        "<form method=\"get\" action=\"{}\" class=\"inline\">{}\
         <label>cols <input name=\"cols\" value=\"{}\" size=\"5\" inputmode=\"numeric\"></label>\
         <label>rows <input name=\"rows\" value=\"{}\" size=\"5\" inputmode=\"numeric\"></label>\
         <button type=\"submit\">resize</button></form>\n",
        window.path(Tab::Map),
        hidden_without(window, &["cols", "rows"]),
        window.view.cols,
        window.view.rows,
    )
}

/// The seed and jump-to-coordinate forms.
fn jump_form(window: &Window, tab: Tab) -> String {
    format!(
        "<form method=\"get\" action=\"{}\" class=\"inline\">{}\
         <label>q <input name=\"q\" value=\"{}\" size=\"8\" inputmode=\"numeric\"></label>\
         <label>r <input name=\"r\" value=\"{}\" size=\"8\" inputmode=\"numeric\"></label>\
         <button type=\"submit\">go</button></form>\n\
         <form method=\"get\" action=\"/seed\" class=\"inline\">{}\
         <label>seed <input name=\"seed\" value=\"{}\" size=\"18\"></label>\
         <button type=\"submit\">open</button> <a href=\"/random\">random</a></form>\n",
        window.path(tab),
        hidden_without(window, &["q", "r"]),
        window.view.center.q(),
        window.view.center.r(),
        hidden_without(window, &[]),
        window.view.seed_text(),
    )
}

/// Every window parameter as a hidden field, except the named ones.
fn hidden_without(window: &Window, except: &[&str]) -> String {
    let mut html = String::new();
    for (name, value) in window.parameters() {
        if except.contains(&name) {
            continue;
        }
        let _ = write!(
            html,
            "<input type=\"hidden\" name=\"{name}\" value=\"{}\">",
            escape(&value)
        );
    }
    html
}

/// What the generator says about the tile in the center cell.
fn tile_readout(generator: &Generator, center: Coord) -> String {
    let tile = generator.tile(center);
    let sample = generator.sample(center);

    let mut html = String::new();
    let _ = writeln!(
        html,
        "<dt>tile</dt><dd><strong>{}</strong> — {}, {} and {}</dd>",
        tile.terrain.name(),
        tile.elevation.name(),
        tile.climate.heat.name(),
        tile.climate.moisture.name(),
    );
    let _ = writeln!(
        html,
        "<dt>values</dt><dd>elevation <code>{:+.3}</code>, relief <code>{:.3}</code>, \
         heat <code>{:+.3}</code>, moisture <code>{:+.3}</code>, basin <code>{:+.3}</code>, \
         volcanic <code>{:+.3}</code></dd>",
        tile.elevation_value,
        tile.relief_value,
        tile.heat_value,
        tile.moisture_value,
        sample.basin_influence,
        sample.volcanic,
    );
    html
}

/// The key for whichever layer is being drawn.
fn legend(layer: Layer) -> String {
    match layer.key() {
        Key::Ramp(scale) => ramp_key(scale),
        Key::Climate => climate_key(),
        Key::Terrain => terrain_key(),
    }
}

/// How many swatches a ramp key is drawn with.
const RAMP_STEPS: u32 = 40;

/// The scalar ramp, labeled at both ends.
fn ramp_key(scale: Scale) -> String {
    let mut html = String::new();
    let _ = write!(
        html,
        "<div class=\"legend\"><span class=\"end\">{}</span><span class=\"ramp\">",
        scale.low
    );
    let (low, high) = scale.range;
    for step in 0..=RAMP_STEPS {
        let t = f64::from(step) / f64::from(RAMP_STEPS);
        let value = low + t * (high - low);
        let rgba = color(value);
        let _ = write!(
            html,
            "<i style=\"background:#{:02x}{:02x}{:02x}\" title=\"{value:+.2}\"></i>",
            rgba[0], rgba[1], rgba[2]
        );
    }
    let _ = writeln!(
        html,
        "</span><span class=\"end\">{}</span></div>",
        scale.high
    );
    html
}

/// The climate table, as the two axes it is.
fn climate_key() -> String {
    let mut html = String::new();
    html.push_str(
        "<table class=\"key\">\n<caption>heat down, moisture across</caption>\n<tr><td></td>",
    );
    for moisture in MoistureBand::ALL {
        let _ = write!(html, "<th scope=\"col\">{}</th>", moisture.name());
    }
    html.push_str("</tr>\n");
    for heat in HeatBand::ALL {
        let _ = write!(html, "<tr><th scope=\"row\">{}</th>", heat.name());
        for moisture in MoistureBand::ALL {
            let rgba = climate_color(Climate { heat, moisture });
            let _ = write!(
                html,
                "<td style=\"background:#{:02x}{:02x}{:02x}\"></td>",
                rgba[0], rgba[1], rgba[2]
            );
        }
        html.push_str("</tr>\n");
    }
    html.push_str("</table>\n");
    html
}

/// The terrain key: one named swatch per terrain, in vocabulary order.
fn terrain_key() -> String {
    let mut html = String::new();
    html.push_str("<ul class=\"swatches\">\n");
    for terrain in Terrain::ALL {
        let rgba = terrain_color(terrain);
        let _ = write!(
            html,
            "<li><i style=\"background:#{:02x}{:02x}{:02x}\"></i>{}</li>",
            rgba[0],
            rgba[1],
            rgba[2],
            terrain.name()
        );
    }
    html.push_str("\n</ul>\n");
    html
}

/// The provenance line, which says the one thing this tool must never be vague
/// about.
fn footer(state: &State) -> String {
    format!(
        "<p class=\"notice\"><strong>This is not a saved world.</strong> The \
         configuration is held in memory by this server, it is not in the URL, and no \
         database has it. A link to this window shows what this server is drawing right \
         now, not what it drew when the link was copied — the fingerprint \
         <code>{}</code> is what says which of those you are looking at. \
         <code>wgvb-serve</code> is the viewer for a world somebody saved.</p>\n\
         <p class=\"quiet\">algorithm {ALGORITHM_VERSION}, render {RENDER_VERSION}, \
         configuration <code>{}</code></p>\n",
        state.short_fingerprint(),
        state.short_fingerprint(),
    )
}

/// Escapes text that came from a request.
///
/// Five characters, which is the set that matters inside element content and
/// inside a double-quoted attribute. Nothing here puts request text in an
/// unquoted attribute, or in a script or style context, where this would not be
/// enough — and there is no script on this page at all.
#[must_use]
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(character),
        }
    }
    out
}

/// The whole stylesheet, inline, because a second request for a kilobyte is not
/// worth a second route.
const STYLE: &str = r#"<style>
:root { color-scheme: light dark; --ink: #1a1a1a; --paper: #fbfaf7; --edge: #c9c4b8; --quiet: #5c574c; --warn: #8a5a00; }
@media (prefers-color-scheme: dark) {
  :root { --ink: #e8e4da; --paper: #17161a; --edge: #3a3740; --quiet: #9b958a; --warn: #e0a94a; }
}
* { box-sizing: border-box; }
body { margin: 0; padding: 16px; background: var(--paper); color: var(--ink);
       font: 14px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
h1 { font-size: 15px; font-weight: 600; margin: 0 0 12px; color: var(--quiet); }
h1 code { color: var(--ink); }
h2 { font-size: 14px; font-weight: 600; margin: 20px 0 8px; color: var(--quiet); }
.quiet { color: var(--quiet); }
.tabs { display: flex; gap: 4px; margin: 0 0 16px; border-bottom: 1px solid var(--edge); }
.tabs a { padding: 6px 14px; text-decoration: none; color: var(--quiet);
          border: 1px solid transparent; border-bottom: none; }
.tabs a:hover { color: var(--ink); }
.tabs a.current { color: var(--ink); border-color: var(--edge); background: var(--paper);
                  margin-bottom: -1px; border-radius: 4px 4px 0 0; }
.map { border: 1px solid var(--edge); line-height: 0; max-width: 100%; max-height: 80vh; overflow: auto; }
.map input[type=image] { display: block; image-rendering: pixelated; }
/* Deliberately not scaled: a browser submits a click in displayed pixels, so a
   shrunk image would name the wrong tile. The container scrolls instead. */
.rose { display: grid; grid-template-columns: repeat(3, 4.5em); gap: 4px; margin: 16px 0; width: max-content; }
.rose a { display: flex; flex-direction: column; align-items: center; justify-content: center;
          padding: 8px 0; border: 1px solid var(--edge); border-radius: 4px;
          text-decoration: none; color: var(--ink); font-weight: 600; }
.rose a:hover { border-color: var(--ink); }
.rose small { font-weight: 400; color: var(--quiet); font-size: 11px; }
.rose .hub { border: 1px dashed var(--edge); border-radius: 4px; opacity: 0.5; min-height: 3.2em; }
.rose .hub.here { display: flex; align-items: center; justify-content: center; color: var(--quiet); }
.layers { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; margin: 12px 0; }
.layers a { padding: 4px 8px; border: 1px solid var(--edge); border-radius: 4px;
            text-decoration: none; color: var(--quiet); }
.layers a:hover { color: var(--ink); border-color: var(--ink); }
.layers a.current { color: var(--paper); background: var(--ink); border-color: var(--ink); }
.layers .quiet { margin-right: 6px; }
.legend { display: flex; align-items: center; gap: 8px; margin: 16px 0; flex-wrap: wrap; }
.legend .end { color: var(--quiet); }
.legend .ramp { display: flex; border: 1px solid var(--edge); }
.legend .ramp i { display: block; width: 8px; height: 16px; }
.key { border-collapse: collapse; margin: 16px 0; }
.key caption { text-align: left; color: var(--quiet); padding-bottom: 4px; }
.key th { font-weight: 400; color: var(--quiet); text-align: right; padding: 0 6px; }
.key th[scope="col"] { text-align: center; }
.key td { width: 5.5em; height: 2.2em; border: 1px solid var(--paper); }
.swatches { display: flex; flex-wrap: wrap; gap: 4px 14px; list-style: none; margin: 16px 0; padding: 0; }
.swatches li { display: flex; align-items: center; gap: 6px; color: var(--quiet); }
.swatches i { display: block; width: 1.6em; height: 1em; border: 1px solid var(--edge); }
.readout { display: grid; grid-template-columns: max-content 1fr; gap: 2px 12px; margin: 16px 0; }
.readout dt { color: var(--quiet); }
.readout dd { margin: 0; }
.notice { max-width: 78ch; color: var(--quiet); border-left: 2px solid var(--edge); padding-left: 10px; }
.notice.warning { color: var(--warn); border-left-color: var(--warn); }
.notice.refused { color: var(--ink); border-left-color: var(--warn); border-left-width: 3px; }
form.inline { display: flex; gap: 10px; align-items: center; margin: 8px 0; flex-wrap: wrap; }
form.inline label { color: var(--quiet); }
input, textarea, button { font: inherit; background: var(--paper); color: var(--ink);
                          border: 1px solid var(--edge); border-radius: 3px; padding: 3px 6px; }
button { cursor: pointer; }
button:hover { border-color: var(--ink); }
table.config { border-collapse: collapse; margin: 12px 0; }
table.config th { font-weight: 400; text-align: left; color: var(--quiet); padding: 2px 12px 2px 0; }
table.config td { padding: 2px 12px 2px 0; }
table.config tr.moved th label { color: var(--ink); font-weight: 600; }
table.config tr.moved input { border-color: var(--warn); }
</style>
"#;
