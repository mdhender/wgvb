//! The viewer page.
//!
//! One HTML document, no script, no stylesheet to fetch. Every control is a
//! link to another URL of this same server, so the browser's back button is the
//! undo history and a window worth arguing about is a link somebody can paste
//! into an issue.

use std::fmt::Write as _;

use wgvb::ALGORITHM_VERSION;
use wgvb_render::{Layer, RENDER_VERSION, Viewport};

use crate::view::{COMPASS, Compass, MAX_HEX_RADIUS, MIN_HEX_RADIUS, View};

/// Renders the viewer page for one view.
///
/// Every value interpolated here has already been through [`View::parse`], so
/// it is a `u64`, a [`wgvb::Coord`] component, a bounded tile count, or one of
/// ten fixed layer names. Nothing unvalidated reaches this function, which is
/// why there is no escaping in it; refusals are answered as `text/plain`
/// instead, where a quoted-back URL is inert.
#[must_use]
pub fn page(view: &View, viewport: &Viewport) -> String {
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

    html.push_str("<dl class=\"readout\">\n");
    let _ = writeln!(
        html,
        "<dt>center</dt><dd><code>{view}</code> canonical <code>(q, r, s)</code></dd>"
    );
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

/// The layer selector: ten links, one per layer, the current one marked.
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
.readout { display: grid; grid-template-columns: max-content 1fr; gap: 2px 12px; margin: 16px 0; }
.readout dt { color: var(--quiet); }
.readout dd { margin: 0; }
.notice { max-width: 62ch; color: var(--quiet); border-left: 2px solid var(--edge); padding-left: 10px; }
</style>
"#;
