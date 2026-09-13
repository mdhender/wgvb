//! The viewer page.
//!
//! One HTML document, no script, no stylesheet to fetch. Every control is a
//! link to another URL of this same server, so the browser's back button is the
//! undo history and a window worth arguing about is a link somebody can paste
//! into an issue.

use std::fmt::Write as _;

use wgvb::{ALGORITHM_VERSION, Climate, Coord, Generator, HeatBand, MoistureBand, Terrain};
use wgvb_render::{
    Key, Layer, RENDER_VERSION, Scale, Viewport, climate_color, color, terrain_color,
};

use crate::view::{COMPASS, Compass, MAX_HEX_RADIUS, MIN_HEX_RADIUS, View};

/// Renders the viewer page for one view.
///
/// Every value interpolated here has already been through [`View::parse`], so
/// it is a `u64`, a [`wgvb::Coord`] component, a bounded tile count, or one of
/// the fixed layer names. The tile readout adds generated data to that list,
/// and it is inert for the same reason: a band or terrain name is a
/// `&'static str` from the core crate, and a scalar is formatted as a number.
/// Nothing unvalidated reaches this function, which is why there is no
/// escaping in it; refusals are answered as `text/plain` instead, where a
/// quoted-back URL is inert.
///
/// `generator` is the caller's, because the image route already builds one and
/// two generators for one request would be two chances to disagree about the
/// configuration.
#[must_use]
pub fn page(view: &View, viewport: &Viewport, generator: &Generator) -> String {
    let (width, height) = viewport.image_size();
    let tiles = u64::from(view.cols) * u64::from(view.rows);

    let mut html = String::with_capacity(8 * 1024);
    html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    html.push_str("<meta charset=\"utf-8\">\n");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = writeln!(
        html,
        "<title>wgvb {} {}</title>",
        view.seed_text(),
        view.layer.name()
    );
    html.push_str(STYLE);
    html.push_str("</head>\n<body>\n");

    let _ = writeln!(html, "<h1>seed <code>{}</code></h1>", view.seed_text());

    html.push_str("<div class=\"map\">");
    let _ = write!(
        html,
        "<img src=\"{}\" width=\"{width}\" height=\"{height}\" \
         alt=\"the {} layer around {}\">",
        view.image_url(),
        view.layer.name(),
        view,
    );
    html.push_str("</div>\n");

    html.push_str(&compass_rose(view));
    html.push_str(&layers(view));
    html.push_str(&legend(view.layer));

    html.push_str("<dl class=\"readout\">\n");
    let _ = writeln!(
        html,
        "<dt>center</dt><dd><code>{view}</code> canonical <code>(q, r, s)</code></dd>"
    );
    html.push_str(&tile_readout(generator, view.center));
    let _ = writeln!(
        html,
        "<dt>window</dt><dd>{} x {} tiles, {tiles} in all, at hex radius {} px \
         ({width} x {height} px)</dd>",
        view.cols, view.rows, view.hex_radius
    );
    let _ = writeln!(
        html,
        "<dt>versions</dt><dd>algorithm {ALGORITHM_VERSION}, render {RENDER_VERSION}</dd>"
    );
    html.push_str("</dl>\n");

    let _ = writeln!(
        html,
        "<p class=\"notice\"><strong>This is not a saved world.</strong> \
         The generator is built in memory from the seed in the route and the \
         default configuration, so this output is diagnostic and does not \
         represent a saved world. A world file opened later will not reproduce \
         these images unless it happens to carry the same seed and \
         configuration.</p>"
    );

    let _ = writeln!(
        html,
        "<p class=\"notice\">Coordinates in the URL are <strong>canonical</strong>. \
         Scrolling past an edge of the world wraps to the opposite edge, and the \
         join is a real world-warp seam rather than a drawing error. \
         Edit <code>hex-radius</code> in the address bar to zoom, between \
         {MIN_HEX_RADIUS} and {MAX_HEX_RADIUS} pixels.</p>"
    );

    html.push_str("</body>\n</html>\n");
    html
}

/// The six scroll controls, laid out as they point.
///
/// Flat-top hexes put north and south straight up and down and the other four
/// on the diagonals, so a three-by-three grid with the readout in the middle is
/// the honest arrangement. The order in the grid is fixed here; the order in
/// [`COMPASS`] is the clockwise walk, and the two are deliberately separate so
/// that nothing depends on a table being written in drawing order.
fn compass_rose(view: &View) -> String {
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
                let steps = compass.steps(view.cols, view.rows);
                let _ = write!(
                    html,
                    "<a href=\"{}\" title=\"move the view {} by {steps} hexes, \
                     absolute direction {}\">{}<small>{steps}</small></a>",
                    view.scrolled(compass).page_url(),
                    compass.heading,
                    compass.direction,
                    compass.name,
                );
            }
            // The middle of the rose is where the view already is.
            None if index == 4 => html.push_str("<span class=\"hub here\">here</span>"),
            None => html.push_str("<span class=\"hub\"></span>"),
        }
    }
    html.push_str("\n</nav>\n");
    html
}

/// The layer selector: one link per layer, the current one marked.
fn layers(view: &View) -> String {
    let mut html = String::new();
    html.push_str("<nav class=\"layers\" aria-label=\"choose a layer\">\n");
    for layer in Layer::ALL {
        if layer == view.layer {
            let _ = write!(
                html,
                "<a href=\"{}\" aria-current=\"page\" class=\"current\">{}</a>",
                view.with_layer(layer).page_url(),
                layer.name()
            );
        } else {
            let _ = write!(
                html,
                "<a href=\"{}\">{}</a>",
                view.with_layer(layer).page_url(),
                layer.name()
            );
        }
    }
    html.push_str("\n</nav>\n");
    html
}

/// The key for whichever layer is being drawn.
///
/// A map with an unlabeled palette is a picture. Every scalar layer shares one
/// ramp — deliberately, so two of them can be compared by eye — which means the
/// same blue is *deep* on one layer, *cold* on the next, and *dry* on the one
/// after; a reader who has to hold that in their head will eventually not.
/// [`Layer::key`] is where the words and the shape come from, because the
/// renderer owns the palette and this server is a front end rather than a
/// second renderer.
///
/// A `match` over [`Key`] rather than over an `Option`, so a fourth kind of
/// key added to the renderer is a compile error here rather than a page that
/// silently draws nothing.
///
/// Nothing here emits a link. The scroll and layer controls are the page's
/// navigation, and a test counts them.
fn legend(layer: Layer) -> String {
    match layer.key() {
        Key::Ramp(scale) => ramp_key(scale),
        Key::Climate => climate_key(),
        Key::Terrain => terrain_key(),
    }
}

/// How many swatches a ramp key is drawn with.
///
/// Enough that the stops of the palette are visible as stops rather than as one
/// gradient, and few enough that the row fits a narrow window.
const RAMP_STEPS: u32 = 40;

/// The scalar ramp, labeled at both ends.
fn ramp_key(scale: Scale) -> String {
    let mut html = String::new();
    let _ = write!(
        html,
        "<div class=\"legend\" aria-label=\"what the colors mean\">\n         <span class=\"end\">{}</span><span class=\"ramp\">",
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
        "</span><span class=\"end\">{}</span>\n</div>",
        scale.high
    );
    html
}

/// The climate table, as the two axes it is.
///
/// A grid rather than a row, because climate is not a scale: the heat band runs
/// down it and the moisture band across, which is the shape of the model in
/// `DESIGN.md` section 16.1 and the shape of the palette that draws it.
fn climate_key() -> String {
    let mut html = String::new();
    html.push_str("<table class=\"key\">\n");
    html.push_str("<caption>heat down, moisture across</caption>\n<tr><td></td>");
    for moisture in MoistureBand::ALL {
        let _ = write!(html, "<th scope=\"col\">{}</th>", moisture.name());
    }
    html.push_str("</tr>\n");
    for heat in HeatBand::ALL {
        let _ = write!(html, "<tr><th scope=\"row\">{}</th>", heat.name());
        for moisture in MoistureBand::ALL {
            let rgba = climate_color(Climate { heat, moisture });
            // No text in the cell: the row and column headers already name
            // the band, and a label repeated twenty-five times is noise a
            // screen reader has to read out.
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

/// What the generator says about the tile in the center cell.
///
/// The page names the center coordinate and, until terrain existed, said
/// nothing about what was at it — so a link to a window was a link to a
/// picture and a reader had to count swatches against the key to find out
/// what they were looking at. This is one tile's worth of the public
/// [`wgvb::Tile`], which is the whole of what a game would see there.
///
/// One tile costs seven elevation evaluations, against `cols * rows` for the
/// image beside it. `DESIGN.md` section 29.1 asks the clamp to be the answer
/// to cost rather than a cache, and this is far below the clamp.
///
/// The classifications and the scalars are two rows rather than one, because
/// they answer different questions: the first says what a game would call
/// this tile, and the second says how close it is to being called something
/// else. A tile at `elevation +0.004` is a coast that is nearly a shallow
/// sea, and no amount of staring at the band name says so.
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
        "<dt>values</dt><dd>elevation <code>{:+.3}</code>, relief <code>{:.3}</code>, heat <code>{:+.3}</code>, moisture <code>{:+.3}</code>, basin <code>{:+.3}</code>, volcanic <code>{:+.3}</code></dd>",
        tile.elevation_value,
        tile.relief_value,
        tile.heat_value,
        tile.moisture_value,
        sample.basin_influence,
        sample.volcanic,
    );
    html
}

/// The terrain key: one named swatch per terrain, in vocabulary order.
///
/// A list rather than a ramp or a grid, because terrain is neither ordered nor
/// two-dimensional: a rainforest is not more of anything than a desert is.
/// Every entry is named, which the climate table does not need to do — there
/// the row and column headers say it once each — and here there is nothing but
/// the name to say which swatch is which.
///
/// Inland water is in the list even though no tile ever carries it. That is
/// the point: a reader who sees one of those two colors on a map is looking at
/// a defect, and a key that hid them would make it harder to notice.
fn terrain_key() -> String {
    let mut html = String::new();
    html.push_str("<ul class=\"swatches\" aria-label=\"what the colors mean\">\n");
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

/// The whole stylesheet, inline, because a second request for eight hundred
/// bytes is not worth a second route.
const STYLE: &str = r#"<style>
:root { color-scheme: light dark; --ink: #1a1a1a; --paper: #fbfaf7; --edge: #c9c4b8; --quiet: #5c574c; }
@media (prefers-color-scheme: dark) {
  :root { --ink: #e8e4da; --paper: #17161a; --edge: #3a3740; --quiet: #9b958a; }
}
* { box-sizing: border-box; }
body { margin: 0; padding: 16px; background: var(--paper); color: var(--ink);
       font: 14px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace; }
h1 { font-size: 15px; font-weight: 600; margin: 0 0 12px; color: var(--quiet); }
h1 code { color: var(--ink); }
.map { border: 1px solid var(--edge); display: inline-block; line-height: 0; max-width: 100%; overflow-x: auto; }
.map img { display: block; max-width: 100%; height: auto; image-rendering: pixelated; }
.rose { display: grid; grid-template-columns: repeat(3, 4.5em); gap: 4px; margin: 16px 0; width: max-content; }
.rose a { display: flex; flex-direction: column; align-items: center; justify-content: center;
          padding: 8px 0; border: 1px solid var(--edge); border-radius: 4px;
          text-decoration: none; color: var(--ink); font-weight: 600; }
.rose a:hover { border-color: var(--ink); }
.rose small { font-weight: 400; color: var(--quiet); font-size: 11px; }
.rose .hub { border: 1px dashed var(--edge); border-radius: 4px; opacity: 0.5;
            min-height: 3.2em; }
.rose .hub.here { display: flex; align-items: center; justify-content: center; color: var(--quiet); }
.layers { display: flex; flex-wrap: wrap; gap: 4px; margin: 16px 0; }
.layers a { padding: 4px 8px; border: 1px solid var(--edge); border-radius: 4px;
            text-decoration: none; color: var(--quiet); }
.layers a:hover { color: var(--ink); border-color: var(--ink); }
.layers a.current { color: var(--paper); background: var(--ink); border-color: var(--ink); }
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
.notice { max-width: 62ch; color: var(--quiet); border-left: 2px solid var(--edge); padding-left: 10px; }
</style>
"#;
