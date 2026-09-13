//! The route table, the scroll controls, and the links the page emits.
//!
//! Everything here goes through [`wgvb_serve::reply`], which is a pure function
//! of a request target, so none of it opens a socket.

use wgvb::{Climate, Coord, DIRECTIONS, HeatBand, MoistureBand, WORLD_RADIUS, direction_index};
use wgvb_render::{Layer, MAX_IMAGE_PIXELS, Viewport, climate_color, color};
use wgvb_serve::{
    COMPASS, DEFAULT_COLS, DEFAULT_HEX_RADIUS, DEFAULT_ROWS, HTML, MAX_COLS, MAX_HEX_RADIUS,
    MAX_ROWS, PNG, Reply, Route, TEXT, View, reply,
};

/// The golden seed, as it is written in the route.
const SEED: &str = "0123456789abcdef";

/// The page for one query string.
fn page(query: &str) -> Reply {
    reply(&format!("/seed/{SEED}{query}"))
}

/// Every `href` a page emits, in document order.
fn hrefs(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("href=\"") {
        rest = &rest[start + 6..];
        let end = rest.find('"').expect("an href is a quoted attribute");
        out.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    out
}

/// The `src` of the page's image.
fn image_src(html: &str) -> String {
    let start = html.find("src=\"").expect("the page carries an image");
    let rest = &html[start + 5..];
    let end = rest.find('"').expect("a src is a quoted attribute");
    rest[..end].to_string()
}

// -- routes ------------------------------------------------------------------

#[test]
fn a_good_seed_serves_a_page_centered_on_the_origin() {
    let reply = page("");
    assert_eq!(reply.status, 200);
    assert_eq!(reply.content_type, HTML);

    let (route, view) = View::parse(&format!("/seed/{SEED}")).expect("a bare route parses");
    assert_eq!(route, Route::Page);
    assert_eq!(view.center, Coord::ORIGIN, "the default page is the origin");
    assert_eq!(view.seed, 0x0123_4567_89ab_cdef);
    assert_eq!(view.cols, DEFAULT_COLS);
    assert_eq!(view.rows, DEFAULT_ROWS);
    assert_eq!(view.hex_radius, DEFAULT_HEX_RADIUS);
    assert_eq!(view.layer, Layer::Elevation);

    // The counts are odd, so the window has an exact center cell. That is the
    // property a scroll step's reversibility rests on.
    assert_eq!(DEFAULT_COLS % 2, 1);
    assert_eq!(DEFAULT_ROWS % 2, 1);
}

#[test]
fn the_seed_is_sixteen_hexadecimal_digits_in_either_case() {
    for text in ["0123456789abcdef", "0123456789ABCDEF", "0000000000000000"] {
        let (_, view) = View::parse(&format!("/seed/{text}")).expect("a good seed parses");
        assert_eq!(view.seed_text(), text.to_ascii_lowercase());
    }
    assert_eq!(
        View::parse("/seed/ffffffffffffffff")
            .expect("the largest seed")
            .1
            .seed,
        u64::MAX
    );
}

#[test]
fn the_image_route_renders_a_png() {
    let reply = reply(&format!("/seed/{SEED}/map.png?q=0&r=0&cols=11&rows=9"));
    assert_eq!(reply.status, 200);
    assert_eq!(reply.content_type, PNG);
    assert_eq!(
        &reply.body[..8],
        b"\x89PNG\r\n\x1a\n",
        "not a PNG signature"
    );

    let decoder = png::Decoder::new(std::io::Cursor::new(reply.body));
    let reader = decoder.read_info().expect("a readable PNG");
    let info = reader.info();
    let viewport =
        Viewport::centered_on(Coord::ORIGIN, 11, 9, DEFAULT_HEX_RADIUS).expect("a valid window");
    assert_eq!((info.width, info.height), viewport.image_size());
}

#[test]
fn a_bare_root_redirects_to_a_state_of_the_viewer() {
    let reply = reply("/");
    assert_eq!(reply.status, 302);
    let location = reply.location.expect("a redirect carries a location");
    let (route, view) = View::parse(&location).expect("the redirect target is a route");
    assert_eq!(route, Route::Page);
    assert_eq!(view.seed, 0);
    assert_eq!(view.center, Coord::ORIGIN);
}

#[test]
fn an_unknown_path_is_a_404_rather_than_a_500() {
    for target in [
        "/seed",
        "/seed/",
        "/favicon.ico",
        "/seed/0123456789abcdef/other.png",
    ] {
        let reply = reply(target);
        assert_eq!(reply.status, 404, "{target} answered {}", reply.status);
        assert_eq!(reply.content_type, TEXT);
    }
}

// -- refusals ----------------------------------------------------------------

/// Every refusal is a 400, is plain text, and names the thing it is refusing.
fn assert_refused(target: &str, must_mention: &[&str]) {
    let reply = reply(target);
    assert_eq!(reply.status, 400, "{target} answered {}", reply.status);
    assert_eq!(reply.content_type, TEXT);
    for needle in must_mention {
        assert!(
            reply.text().contains(needle),
            "{target} answered {:?}, which does not mention {needle:?}",
            reply.text()
        );
    }
}

#[test]
fn a_seed_of_the_wrong_length_is_refused() {
    assert_refused("/seed/0123456789abcde", &["seed", "16"]);
    assert_refused("/seed/0123456789abcdef0", &["seed", "16"]);
    assert_refused("/seed/0", &["seed", "16"]);
    assert_refused("/seed/0123456789abcde/map.png", &["seed", "16"]);
}

#[test]
fn a_seed_that_is_not_hexadecimal_is_refused() {
    // `0x` included: the route spells a seed without it, and accepting both
    // would make two spellings of the same URL.
    assert_refused("/seed/0x123456789abcde", &["hexadecimal"]);
    assert_refused("/seed/zzzzzzzzzzzzzzzz", &["hexadecimal"]);
    assert_refused("/seed/+123456789abcdef", &["hexadecimal"]);
    assert_refused("/seed/ -123456789abcde", &["hexadecimal"]);
}

#[test]
fn half_a_center_is_refused_rather_than_half_applied() {
    assert_refused(&format!("/seed/{SEED}?r=6543"), &["r", "q"]);
    assert_refused(&format!("/seed/{SEED}?q=-87"), &["q", "r"]);
}

#[test]
fn a_coordinate_outside_the_world_is_refused() {
    assert_refused(&format!("/seed/{SEED}?q=40000&r=0"), &["q", "40000"]);
    assert_refused(&format!("/seed/{SEED}?q=0&r=-40000"), &["r", "-40000"]);
    assert_refused(&format!("/seed/{SEED}?q=0&r=99999999999999999999"), &["r"]);
    assert_refused(&format!("/seed/{SEED}?q=north&r=0"), &["q", "north"]);
    assert_refused(&format!("/seed/{SEED}?q=0&r=1.5"), &["r", "1.5"]);
}

#[test]
fn an_unknown_layer_is_refused_and_the_known_ones_are_listed() {
    assert_refused(
        &format!("/seed/{SEED}?layer=terrain"),
        &["terrain", "elevation-raw", "region-influence"],
    );
    assert_refused(&format!("/seed/{SEED}?layer="), &["layer"]);
}

#[test]
fn an_unknown_parameter_is_refused_rather_than_ignored() {
    // A silently dropped `?col=61` looks exactly like a server that does not
    // work. Same reason `Config` carries `deny_unknown_fields`.
    assert_refused(&format!("/seed/{SEED}?col=61"), &["col", "cols"]);
    assert_refused(&format!("/seed/{SEED}?zoom=4"), &["zoom"]);
}

#[test]
fn a_window_with_no_center_cell_is_refused() {
    assert_refused(&format!("/seed/{SEED}?cols=60"), &["odd"]);
    assert_refused(&format!("/seed/{SEED}?rows=44"), &["odd"]);
}

#[test]
fn an_unrenderable_window_is_a_400_rather_than_a_500() {
    // The clamps keep a careless request cheap; this is the backstop for the
    // combination that survives them. `MAX_COLS` columns at `MAX_HEX_RADIUS`
    // is a wider image than `wgvb-render` will make.
    let target =
        format!("/seed/{SEED}?cols={MAX_COLS}&rows={MAX_ROWS}&hex-radius={MAX_HEX_RADIUS}");
    let reply = reply(&target);
    assert_eq!(reply.status, 400, "{}", reply.text());
    assert!(
        reply.text().contains(&MAX_IMAGE_PIXELS.to_string()),
        "{}",
        reply.text()
    );
}

#[test]
fn no_target_produces_a_server_error() {
    // Nothing a caller can type is this server's fault. Sweep the shapes that
    // would otherwise be a panic or a 500.
    let targets = [
        "",
        "/",
        "//",
        "/seed//map.png",
        "/seed/0123456789abcdef?",
        "/seed/0123456789abcdef?&&&",
        "/seed/0123456789abcdef?q",
        "/seed/0123456789abcdef?q=&r=",
        "/seed/0123456789abcdef?q=0&r=0&cols=0",
        "/seed/0123456789abcdef?cols=-1",
        "/seed/0123456789abcdef?cols=99999999999999999999",
        "/seed/0123456789abcdef?hex-radius=NaN",
        "/seed/0123456789abcdef?hex-radius=inf",
        "/seed/0123456789abcdef?hex-radius=-4",
        "/seed/0123456789abcdef?hex-radius=abc",
        "/seed/0123456789abcdef?layer=%zz",
        "/seed/0123456789abcdef/map.png?q=%2D87&r=6543",
        "/seed/0123456789abcdef%2Fmap.png",
    ];
    for target in targets {
        let reply = reply(target);
        assert!(
            reply.status < 500,
            "{target} answered {}: {}",
            reply.status,
            reply.text()
        );
    }
}

// -- clamps ------------------------------------------------------------------

#[test]
fn the_window_is_clamped_before_a_viewport_is_built() {
    // A request costs `cols * rows` generator calls, and seven times that for
    // relief. `DESIGN.md` section 29 bounds rendering; the clamp is where.
    let (_, view) = View::parse(&format!("/seed/{SEED}?cols=4000&rows=4000")).expect("clamped");
    assert_eq!(view.cols, MAX_COLS);
    assert_eq!(view.rows, MAX_ROWS);
    assert_eq!(MAX_COLS % 2, 1, "the clamp must land on an odd count");
    assert_eq!(MAX_ROWS % 2, 1, "the clamp must land on an odd count");

    let (_, view) = View::parse(&format!("/seed/{SEED}?hex-radius=9000")).expect("clamped");
    assert_eq!(view.hex_radius, MAX_HEX_RADIUS);

    let (_, view) =
        View::parse(&format!("/seed/{SEED}?cols=-5&rows=0&hex-radius=0.01")).expect("clamped");
    assert_eq!((view.cols, view.rows), (1, 1));
    assert_eq!(view.hex_radius, 1.0);
}

// -- scrolling ---------------------------------------------------------------

#[test]
fn each_control_moves_the_documented_number_of_hexes_in_the_documented_direction() {
    let expected: [(&str, i32, bool); 6] = [
        ("N", 2, true),
        ("NE", 1, false),
        ("SE", 0, false),
        ("S", 5, true),
        ("SW", 4, false),
        ("NW", 3, false),
    ];
    assert_eq!(
        COMPASS.map(|compass| compass.name),
        expected.map(|(name, _, _)| name),
        "the compass table is the clockwise walk from north"
    );

    for (cols, rows) in [(61_u32, 45_u32), (1, 1), (3, 101), (MAX_COLS, MAX_ROWS)] {
        let (_, view) = View::parse(&format!(
            "/seed/{SEED}?q=100&r=-250&cols={cols}&rows={rows}"
        ))
        .expect("a valid view");

        for (index, (name, direction, vertical)) in expected.into_iter().enumerate() {
            let compass = COMPASS[index];
            assert_eq!(compass.name, name);
            assert_eq!(compass.direction, direction);

            let steps = if vertical { rows / 2 } else { cols / 2 };
            assert_eq!(compass.steps(cols, rows), steps, "{name}");

            let (dq, dr) = DIRECTIONS[direction_index(direction)];
            let moved = view.scrolled(compass);
            assert_eq!(
                moved.center,
                Coord::new(
                    i64::from(view.center.q()) + i64::from(steps) * i64::from(dq),
                    i64::from(view.center.r()) + i64::from(steps) * i64::from(dr),
                ),
                "{name} at {cols}x{rows}"
            );
            // Nothing but the center moves.
            assert_eq!(moved.seed, view.seed);
            assert_eq!((moved.cols, moved.rows), (view.cols, view.rows));
            assert_eq!(moved.hex_radius, view.hex_radius);
            assert_eq!(moved.layer, view.layer);
        }
    }
}

#[test]
fn the_opposite_control_returns_to_exactly_where_it_started() {
    // The step is a tile count and never a pixel count, so this holds at every
    // zoom rather than approximately at one of them.
    let opposites = [("N", "S"), ("NE", "SW"), ("SE", "NW")];
    for radius in [1.0_f32, 4.0, 10.0, 33.0, MAX_HEX_RADIUS] {
        for (q, r) in [(0_i64, 0_i64), (100, -250), (-32_767, 1), (12_345, -20_000)] {
            let (_, view) = View::parse(&format!(
                "/seed/{SEED}?q={q}&r={r}&cols=61&rows=45&hex-radius={radius}"
            ))
            .expect("a valid view");

            for (there, back) in opposites {
                let out = *COMPASS.iter().find(|c| c.name == there).unwrap();
                let home = *COMPASS.iter().find(|c| c.name == back).unwrap();
                assert_eq!(
                    view.scrolled(out).scrolled(home).center,
                    view.center,
                    "{there} then {back} from ({q}, {r}) at radius {radius}"
                );
                assert_eq!(
                    view.scrolled(home).scrolled(out).center,
                    view.center,
                    "{back} then {there} from ({q}, {r}) at radius {radius}"
                );
            }
        }
    }
}

#[test]
fn the_label_on_a_control_is_the_direction_the_view_actually_moves() {
    // Not "the coordinate changed by the right vector" — that is the test
    // above. This one asks the renderer: after pressing north, the tile in the
    // center cell must be the tile that was `rows / 2` north of center before.
    let (_, view) =
        View::parse(&format!("/seed/{SEED}?q=40&r=-12&cols=61&rows=45")).expect("a valid view");
    let before = view.viewport().expect("a valid window");
    let center = before.center().expect("an odd window has a center");
    assert_eq!(center, view.center);

    for compass in COMPASS {
        let steps = compass.steps(view.cols, view.rows);

        // Walk the named direction one neighbor at a time, which uses only
        // `Coord::neighbor` and so cannot share an arithmetic mistake with the
        // server's own step.
        let mut walked = center;
        for _ in 0..steps {
            walked = walked.neighbor(compass.direction);
        }

        let after = view.scrolled(compass).viewport().expect("a valid window");
        assert_eq!(
            after.center().expect("an odd window has a center"),
            walked,
            "pressing {} did not land on the tile {steps} hexes {}",
            compass.name,
            compass.heading
        );
    }
}

#[test]
fn scrolling_across_every_edge_wraps_to_the_canonical_representative() {
    // The world is a wrapped hexagon, so a view can be scrolled off any of its
    // six edges. What comes back is the canonical representative, and the link
    // generated there parses back to the same tile. It will look like a seam,
    // because it is one: the accepted world-warp seam of section 7.1.
    for compass in COMPASS {
        let (dq, dr) = DIRECTIONS[direction_index(compass.direction)];

        // Start on the edge in the direction we are about to scroll.
        let start = Coord::new(WORLD_RADIUS * i64::from(dq), WORLD_RADIUS * i64::from(dr));
        let (_, view) = View::parse(&format!(
            "/seed/{SEED}?q={}&r={}&cols=61&rows=45",
            start.q(),
            start.r()
        ))
        .expect("an edge coordinate is inside the world");
        assert_eq!(view.center, start);

        let moved = view.scrolled(compass);

        // Canonical: every component is inside the world, which is what
        // `Coord` guarantees and what the link therefore prints.
        for component in [moved.center.q(), moved.center.r(), moved.center.s()] {
            assert!(
                i64::from(component).abs() <= WORLD_RADIUS,
                "{} left ({}, {}) outside the canonical hexagon",
                compass.name,
                moved.center.q(),
                moved.center.r()
            );
        }

        // Wrapped, not merely moved: the naive sum is outside the world.
        let steps = i64::from(compass.steps(view.cols, view.rows));
        let naive = (
            i64::from(start.q()) + steps * i64::from(dq),
            i64::from(start.r()) + steps * i64::from(dr),
        );
        let unwrapped_outside = naive
            .0
            .abs()
            .max(naive.1.abs())
            .max((-naive.0 - naive.1).abs())
            > WORLD_RADIUS;
        assert!(
            unwrapped_outside,
            "{} did not leave the hexagon",
            compass.name
        );

        // The link generated at the far side parses back to the same tile.
        let (route, parsed) = View::parse(&moved.page_url()).expect("the far-side link parses");
        assert_eq!(route, Route::Page);
        assert_eq!(parsed.center, moved.center);
        assert_eq!(parsed, moved);

        // And the far side is a page rather than a refusal.
        assert_eq!(reply(&moved.page_url()).status, 200);
        assert_eq!(reply(&moved.image_url()).status, 200);
    }
}

// -- links -------------------------------------------------------------------

#[test]
fn every_link_the_page_emits_parses_back_to_the_state_that_produced_it() {
    // The page and the parser cannot be allowed to disagree about the URL
    // format, so the page's own output is the input to the parser.
    let target = format!("/seed/{SEED}?q=-87&r=6543&cols=13&rows=9&hex-radius=6&layer=relief");
    let (_, view) = View::parse(&target).expect("a valid view");
    let html = page("?q=-87&r=6543&cols=13&rows=9&hex-radius=6&layer=relief");
    assert_eq!(html.status, 200);
    let html = html.text();

    let links = hrefs(html);
    assert_eq!(
        links.len(),
        COMPASS.len() + Layer::ALL.len(),
        "six scroll controls and one link per layer"
    );

    let mut seen_scrolls = Vec::new();
    for link in &links {
        let (route, parsed) = View::parse(link).expect("a link this page emitted parses");
        assert_eq!(route, Route::Page);
        assert_eq!(parsed.seed, view.seed);
        assert_eq!((parsed.cols, parsed.rows), (view.cols, view.rows));
        assert_eq!(parsed.hex_radius, view.hex_radius);
        if parsed.layer == view.layer {
            seen_scrolls.push(parsed.center);
        }
        assert_eq!(reply(link).status, 200, "{link}");
    }

    // The six scroll links, plus the layer link that points back at the
    // current layer, all keep the layer; only the six move the center.
    let expected: Vec<Coord> = COMPASS
        .iter()
        .map(|compass| view.scrolled(*compass).center)
        .chain(std::iter::once(view.center))
        .collect();
    seen_scrolls.sort_unstable();
    let mut expected = expected;
    expected.sort_unstable();
    assert_eq!(seen_scrolls, expected);

    // The image the page shows is the image route for the same state.
    let src = image_src(html);
    let (route, parsed) = View::parse(&src).expect("the image source parses");
    assert_eq!(route, Route::Image);
    assert_eq!(parsed, view);
    assert_eq!(reply(&src).content_type, PNG);
}

#[test]
fn a_layer_link_changes_only_the_layer() {
    let html = page("?q=5&r=-5&cols=11&rows=9&hex-radius=7&layer=elevation");
    let html = html.text();
    let mut layers = Vec::new();
    for link in hrefs(html) {
        let (_, parsed) = View::parse(&link).expect("a link parses");
        if parsed.center == Coord::new(5, -5) {
            layers.push(parsed.layer);
        }
    }
    layers.sort_unstable_by_key(|layer| layer.name());
    let mut all: Vec<Layer> = Layer::ALL.to_vec();
    all.sort_unstable_by_key(|layer| layer.name());
    assert_eq!(layers, all, "every layer is reachable in one click");
}

#[test]
fn every_scalar_layer_shows_a_labeled_ramp() {
    // The shared ramp is the reason this key exists: the same blue is `deep` on
    // one layer, `cold` on the next and `dry` on the one after, and a reader
    // who has to remember which is which eventually will not.
    for layer in Layer::ALL {
        let Some(scale) = layer.scale() else {
            continue;
        };
        let html = page(&format!("?layer={}", layer.name()));
        assert_eq!(html.status, 200, "{}", layer.name());
        let text = html.text();
        assert!(
            text.contains(&format!("<span class=\"end\">{}</span>", scale.low)),
            "{} does not label the bottom of its ramp",
            layer.name()
        );
        assert!(
            text.contains(&format!("<span class=\"end\">{}</span>", scale.high)),
            "{} does not label the top of its ramp",
            layer.name()
        );
        // The ramp is drawn from the palette, at the layer's own range. Relief
        // is unsigned, so its key must start at flat ground rather than at the
        // bottom of a ramp it never reaches.
        let (low, high) = scale.range;
        for end in [low, high] {
            let rgba = color(end);
            assert!(
                text.contains(&format!(
                    "background:#{:02x}{:02x}{:02x}",
                    rgba[0], rgba[1], rgba[2]
                )),
                "{} does not draw the color of {end}",
                layer.name()
            );
        }
        assert!(
            !text.contains("<table class=\"key\">"),
            "{} drew the climate table",
            layer.name()
        );
    }
}

#[test]
fn the_climate_layer_shows_the_two_axis_table_instead() {
    // Climate is not a scale, so it does not get a ramp: a pair of bands has no
    // position on one, and flattening the two axes onto one is the single mixed
    // scale the model is built to avoid.
    let html = page("?layer=climate");
    assert_eq!(html.status, 200);
    let text = html.text();
    assert!(!text.contains("class=\"ramp\""), "climate drew a ramp");
    assert!(text.contains("<table class=\"key\">"));

    for heat in HeatBand::ALL {
        assert!(text.contains(heat.name()), "{} is missing", heat.name());
        for moisture in MoistureBand::ALL {
            assert!(
                text.contains(moisture.name()),
                "{} is missing",
                moisture.name()
            );
            let rgba = climate_color(Climate { heat, moisture });
            assert!(
                text.contains(&format!(
                    "background:#{:02x}{:02x}{:02x}",
                    rgba[0], rgba[1], rgba[2]
                )),
                "{} {} is missing from the key",
                heat.name(),
                moisture.name()
            );
        }
    }
}

#[test]
fn the_key_is_not_navigation() {
    // A test because it would be easy and wrong to make a swatch a link: the
    // scroll controls and the layer selector are the page's navigation, and the
    // link count above is what pins that.
    for layer in [Layer::Elevation, Layer::Climate] {
        let html = page(&format!("?layer={}", layer.name()));
        let text = html.text();
        let key_start = text
            .find("class=\"legend\"")
            .or_else(|| text.find("<table class=\"key\">"))
            .expect("the page carries a key");
        let key = &text[key_start..];
        let key_end = key.find("<dl").expect("the readout follows the key");
        assert!(
            !key[..key_end].contains("href="),
            "{} put a link in its key",
            layer.name()
        );
    }
}

#[test]
fn the_page_names_the_center_and_says_what_it_is_not() {
    let html = page("?q=-87&r=6543");
    let text = html.text();
    let center = Coord::new(-87, 6543);
    assert!(
        text.contains(&format!("({}, {}, {})", center.q(), center.r(), center.s())),
        "the page does not print the center in readable form"
    );
    assert!(
        text.contains("not a saved world"),
        "the page does not say that its output is diagnostic"
    );
}

// -- caching -----------------------------------------------------------------

#[test]
fn the_same_url_is_the_same_bytes_and_the_same_tag() {
    let target = format!("/seed/{SEED}/map.png?q=3&r=4&cols=11&rows=9&layer=relief");
    let first = reply(&target);
    let second = reply(&target);
    assert_eq!(first, second, "the same URL rendered differently twice");
    assert!(first.etag.is_some());
}

#[test]
fn every_parameter_that_changes_the_bytes_changes_the_tag() {
    let base = format!("/seed/{SEED}/map.png?q=3&r=4&cols=11&rows=9&hex-radius=6&layer=relief");
    let tag = reply(&base).etag.expect("the image route carries a tag");
    let variants = [
        "/seed/0123456789abcde0/map.png?q=3&r=4&cols=11&rows=9&hex-radius=6&layer=relief"
            .to_string(),
        format!("/seed/{SEED}/map.png?q=4&r=4&cols=11&rows=9&hex-radius=6&layer=relief"),
        format!("/seed/{SEED}/map.png?q=3&r=5&cols=11&rows=9&hex-radius=6&layer=relief"),
        format!("/seed/{SEED}/map.png?q=3&r=4&cols=13&rows=9&hex-radius=6&layer=relief"),
        format!("/seed/{SEED}/map.png?q=3&r=4&cols=11&rows=11&hex-radius=6&layer=relief"),
        format!("/seed/{SEED}/map.png?q=3&r=4&cols=11&rows=9&hex-radius=7&layer=relief"),
        format!("/seed/{SEED}/map.png?q=3&r=4&cols=11&rows=9&hex-radius=6&layer=elevation"),
    ];
    for variant in variants {
        let other = reply(&variant).etag.expect("the image route carries a tag");
        assert_ne!(tag, other, "{variant} shares a tag with the base request");
    }

    // The page and the image are different resources at the same state.
    let page_tag = page("?q=3&r=4&cols=11&rows=9&hex-radius=6&layer=relief")
        .etag
        .expect("the page carries a tag");
    assert_ne!(tag, page_tag);
}

#[test]
fn a_percent_encoded_url_decodes_to_the_same_state() {
    // A link that has been through a browser, a chat client, or an issue
    // tracker may arrive encoded. `%2D` is `-`, `%68` is `h`.
    let plain = format!("/seed/{SEED}?q=-87&r=6543&cols=11&rows=9&layer=relief");
    let encoded = format!("/seed/{SEED}?q=%2D87&r=6543&cols=11&rows=9&la%79er=relief");
    assert_eq!(
        View::parse(&plain).expect("plain").1,
        View::parse(&encoded).expect("encoded").1
    );
}

#[test]
fn a_malformed_escape_in_front_of_a_multibyte_character_does_not_panic() {
    // Slicing a string at an unvalidated `%` would index into the middle of a
    // multi-byte character. These are the shapes that would do it.
    for tail in [
        "%€",
        "%e€",
        "%",
        "%e",
        "€%",
        "%%%",
        "%c3%28",
        "%f0%9f%92%a9",
    ] {
        for parameter in ["q", "r", "cols", "rows", "hex-radius", "layer"] {
            let reply = reply(&format!("/seed/{SEED}?{parameter}={tail}"));
            assert!(reply.status < 500, "{parameter}={tail}");
        }
        let reply = reply(&format!("/seed/{SEED}?{tail}=1"));
        assert!(reply.status < 500, "{tail}=1");
    }
}
