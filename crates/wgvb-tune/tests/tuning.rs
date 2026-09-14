//! The tuner's routes, its forms, and the two claims that matter most.
//!
//! Everything here goes through [`wgvb_tune::answer`], which takes a request
//! and a session and returns a response, so none of it opens a socket.
//!
//! The two claims: a configuration change is visible in the `ETag`, which is
//! what keeps a strong validator honest once the URL no longer carries the
//! configuration; and a refused change leaves the server drawing exactly what
//! it was drawing.

use wgvb::{Config, Seed};
use wgvb_tune::{HTML, PNG, Reply, Session, Tab, Window, answer};

const SEED: Seed = 0x0123_4567_89ab_cdef;
const SEED_TEXT: &str = "0123456789abcdef";
const BUDGET: u64 = 8_000_000;

/// A session on the default configuration.
fn session() -> Session {
    Session::new(SEED, Config::default()).expect("the defaults are valid")
}

/// A GET.
fn get(target: &str, session: &Session) -> Reply {
    answer(target, false, "", &[], session, BUDGET)
}

/// A POST of a form.
fn post(target: &str, body: &str, session: &Session) -> Reply {
    answer(
        target,
        true,
        "application/x-www-form-urlencoded",
        body.as_bytes(),
        session,
        BUDGET,
    )
}

/// The query the page emits for a default window, so tests name links the way
/// the server spells them.
fn query() -> String {
    Window::origin_of(SEED).query()
}

/// Every `href` and `action` on a page.
fn links(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (attribute, close) in [("href=\"", '"'), ("action=\"", '"')] {
        let mut rest = html;
        while let Some(at) = rest.find(attribute) {
            rest = &rest[at + attribute.len()..];
            let end = rest.find(close).expect("an attribute is closed");
            out.push(rest[..end].to_string());
            rest = &rest[end..];
        }
    }
    out
}

#[test]
fn the_root_redirects_to_the_seed_the_server_was_started_on() {
    let session = Session::new(0xfeed_face, Config::default()).expect("valid");
    let reply = get("/", &session);
    assert_eq!(reply.status, 302);
    let location = reply.location.expect("a redirect names a location");
    assert!(location.starts_with("/seed/00000000feedface"), "{location}");
}

#[test]
fn the_three_tabs_are_three_routes_and_each_links_to_the_others() {
    let session = session();
    for tab in ["", "/grid", "/config"] {
        let reply = get(&format!("/seed/{SEED_TEXT}{tab}?{}", query()), &session);
        assert_eq!(reply.status, 200, "{tab}: {}", reply.text());
        assert_eq!(reply.content_type, HTML);

        let html = reply.text();
        for other in ["", "/grid", "/config"] {
            let path = format!("/seed/{SEED_TEXT}{other}?");
            assert!(
                html.contains(&path),
                "the {tab:?} tab does not link to the {other:?} tab"
            );
        }
    }
}

#[test]
fn every_link_a_page_emits_parses_back_to_a_state_of_this_server() {
    // The same round-trip property the viewer keeps. The configuration is no
    // longer in the URL, but everything that *is* in the URL still has to
    // survive being followed.
    let session = session();
    for tab in ["", "/grid", "/config"] {
        let reply = get(&format!("/seed/{SEED_TEXT}{tab}?{}", query()), &session);
        for link in links(reply.text()) {
            if link.starts_with("/config.toml") || link == "/random" || link == "/seed" {
                continue;
            }
            assert!(
                Window::parse(&link).is_ok(),
                "a link this page emitted does not parse: {link}"
            );
        }
    }
}

#[test]
fn an_unknown_parameter_is_refused_rather_than_ignored() {
    let reply = get(&format!("/seed/{SEED_TEXT}?colz=61"), &session());
    assert_eq!(reply.status, 400);
    assert!(reply.text().contains("colz"), "{}", reply.text());
}

#[test]
fn an_unknown_route_is_a_404_rather_than_a_500() {
    let reply = get("/seed/0123456789abcdef/nowhere", &session());
    assert_eq!(reply.status, 404);
}

#[test]
fn a_page_is_not_written_and_a_change_is_not_read() {
    let session = session();
    assert_eq!(
        post(&format!("/seed/{SEED_TEXT}"), "", &session).status,
        405
    );
    assert_eq!(
        get(&format!("/seed/{SEED_TEXT}/config/reset"), &session).status,
        405
    );
}

#[test]
fn changing_a_field_changes_the_pixels_and_the_tag() {
    // The claim that makes a strong `ETag` honest here. The URL is identical
    // across these two requests; the configuration is not, and the tag has to
    // say so or a browser will go on showing the old map.
    let session = session();
    // The terrain layer, because sea level is a *classification* threshold:
    // it decides what a tile is called and not what its elevation scalar is,
    // so the elevation layer would be identical either side of this change and
    // the test would be asserting nothing.
    let target = format!(
        "/seed/{SEED_TEXT}/map.png?{}",
        Window::origin_of(SEED)
            .with_layer(wgvb_render::Layer::Terrain)
            .query()
    );

    let before = get(&target, &session);
    assert_eq!(before.status, 200);
    assert_eq!(before.content_type, PNG);
    let tag_before = before
        .etag
        .clone()
        .expect("a diagnostic image carries a tag");

    let applied = post(
        &format!("/seed/{SEED_TEXT}/config/fields?{}", query()),
        "sea_level=0.05",
        &session,
    );
    assert_eq!(applied.status, 303, "{}", applied.text());

    let after = get(&target, &session);
    let tag_after = after.etag.clone().expect("a tag");

    assert_ne!(
        tag_before, tag_after,
        "the tag did not move when the configuration did"
    );
    assert_ne!(before.body, after.body, "moving sea level changed no pixel");
}

#[test]
fn a_refused_change_leaves_the_server_drawing_what_it_was_drawing() {
    let session = session();
    let target = format!("/seed/{SEED_TEXT}/map.png?{}", query());
    let before = get(&target, &session);

    let refused = post(
        &format!("/seed/{SEED_TEXT}/config/fields?{}", query()),
        "sea_level=40",
        &session,
    );
    assert_eq!(refused.status, 400);
    assert_eq!(refused.content_type, HTML, "a refusal keeps the form");
    let html = refused.text();
    assert!(html.contains("sea_level"), "the refusal names the field");
    assert!(html.contains("Nothing was changed"), "{html}");
    // What was typed comes back in the box, rather than being thrown away.
    assert!(
        html.contains("value=\"40\""),
        "the typed value is not echoed"
    );

    let after = get(&target, &session);
    assert_eq!(
        before.body, after.body,
        "a refused change altered the world"
    );
    assert_eq!(before.etag, after.etag);
}

#[test]
fn a_form_that_posts_unchanged_values_changes_nothing() {
    // The configuration form posts every field on every submit. Applying them
    // all would be a hundred parses and a hundred chances for a round-trip to
    // go wrong; only what differs is applied.
    let session = session();
    let fields = wgvb_config::fields(&Config::default()).expect("the defaults enumerate");
    let body: String = fields
        .iter()
        .map(|field| format!("{}={}", field.name, field.value))
        .collect::<Vec<_>>()
        .join("&");

    let before = session.state().fingerprint;
    let reply = post(
        &format!("/seed/{SEED_TEXT}/config/fields?{}", query()),
        &body,
        &session,
    );
    assert_eq!(reply.status, 303, "{}", reply.text());
    assert_eq!(
        before,
        session.state().fingerprint,
        "posting the configuration back unchanged changed it"
    );
}

#[test]
fn the_download_round_trips_through_the_upload() {
    let session = session();
    let changed = post(
        &format!("/seed/{SEED_TEXT}/config/fields?{}", query()),
        "sea_level=0.125",
        &session,
    );
    assert_eq!(changed.status, 303);
    let tuned = session.state().fingerprint;

    let file = get("/config.toml", &session);
    assert_eq!(file.status, 200);
    assert!(
        file.download
            .as_deref()
            .is_some_and(|name| name.ends_with(".toml")),
        "the download has no filename"
    );
    let text = file.text().to_string();

    // Back to the defaults, then in through the paste box.
    let reset = post(
        &format!("/seed/{SEED_TEXT}/config/reset?{}", query()),
        "",
        &session,
    );
    assert_eq!(reset.status, 303);
    assert_ne!(session.state().fingerprint, tuned);

    let body = format!("pasted={}", urlencode(&text));
    let uploaded = post(
        &format!("/seed/{SEED_TEXT}/config/upload?{}", query()),
        &body,
        &session,
    );
    assert_eq!(uploaded.status, 303, "{}", uploaded.text());
    assert_eq!(
        session.state().fingerprint,
        tuned,
        "the file did not carry the configuration back exactly"
    );
}

/// Percent-encodes a body the way a browser would.
fn urlencode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(byte));
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[test]
fn a_window_over_the_budget_is_refused_with_a_number_in_it() {
    let session = session();
    let window = Window::origin_of(SEED);
    let target = format!(
        "/seed/{SEED_TEXT}/grid.png?{}",
        window
            .with_layer(wgvb_render::Layer::Terrain)
            .with_grid(2001, 2001)
            .query()
    );
    let reply = answer(&target, false, "", &[], &session, BUDGET);
    assert_eq!(reply.status, 400);
    let text = reply.text();
    assert!(text.contains("budget"), "{text}");
    assert!(text.contains("terrain"), "{text}");
}

#[test]
fn the_grid_warns_on_the_four_sheared_turns_and_not_on_the_other_two() {
    let session = session();
    for turn in 0..6_u8 {
        let target = format!(
            "/seed/{SEED_TEXT}/grid?{}",
            Window::origin_of(SEED)
                .with_turn(turn)
                .with_grid(101, 101)
                .query()
        );
        let reply = get(&target, &session);
        assert_eq!(reply.status, 200, "{}", reply.text());
        assert_eq!(
            reply.text().contains("shears the picture"),
            turn != 0 && turn != 3,
            "turn {turn} is on the wrong side of the warning"
        );
    }
}

#[test]
fn clicking_the_map_recenters_it_on_the_tile_that_was_clicked() {
    let session = session();
    let window = Window::origin_of(SEED);
    let viewport = window.viewport().expect("a valid window");
    // A pixel well inside the image, and the tile it belongs to, computed
    // through the same viewport the page drew with.
    let (x, y) = (40, 30);
    let expected = viewport.locate(x, y).expect("a pixel inside the image");

    let target = format!(
        "/seed/{SEED_TEXT}?{}&click.x={x}&click.y={y}",
        window.query()
    );
    let reply = get(&target, &session);
    assert_eq!(reply.status, 303, "{}", reply.text());

    let location = reply.location.expect("a redirect names a location");
    let (_, moved) = Window::parse(&location).expect("the redirect parses");
    assert_eq!(moved.view.center, expected);
    // And nothing else about the window moved.
    assert_eq!(moved.view.cols, window.view.cols);
    assert_eq!(moved.view.layer, window.view.layer);
    assert_eq!(moved.turn, window.turn);
}

#[test]
fn the_seed_box_turns_a_field_into_a_route() {
    let session = session();
    let reply = get(
        &format!("/seed?{}&seed=00000000feedface", query()),
        &session,
    );
    assert_eq!(reply.status, 303);
    let location = reply.location.expect("a location");
    assert!(
        location.starts_with("/seed/00000000feedface?"),
        "{location}"
    );

    let refused = get(&format!("/seed?{}&seed=nonsense", query()), &session);
    assert_eq!(refused.status, 400);
}

#[test]
fn a_random_seed_is_a_seed() {
    let session = session();
    let reply = get("/random", &session);
    assert_eq!(reply.status, 303);
    let location = reply.location.expect("a location");
    let (target, _) = Window::parse(&location).expect("the redirect parses");
    assert_eq!(target, wgvb_tune::Target::Read(Tab::Map));
}

#[test]
fn an_image_says_what_it_cost() {
    // The measurement half of this tool. A window that renders and says nothing
    // about how long it took is a window nobody can tune against.
    let session = session();
    let reply = get(&format!("/seed/{SEED_TEXT}/map.png?{}", query()), &session);
    assert_eq!(reply.status, 200);
    assert!(reply.timing.is_some(), "no Server-Timing header");
    assert!(reply.note.contains("tiles"), "{}", reply.note);
    assert!(reply.note.contains("generate"), "{}", reply.note);
}

#[test]
fn the_map_tab_says_what_is_in_the_window() {
    // The readout is the numeric half of a tuning pass: a picture says a
    // threshold moved something, and this says what and by how much.
    let session = session();
    let reply = get(&format!("/seed/{SEED_TEXT}?{}", query()), &session);
    assert_eq!(reply.status, 200, "{}", reply.text());
    let html = reply.text();

    assert!(html.contains("what is in this window"), "no readout");
    // Every terrain, including the ones that are not there — a row reading zero
    // is usually the row somebody is trying to move off zero.
    for terrain in wgvb::Terrain::ALL {
        assert!(
            html.contains(terrain.name()),
            "{} is missing from the readout",
            terrain.name()
        );
    }
    // And the three ladders beside it.
    for band in wgvb::Elevation::ALL {
        assert!(html.contains(band.name()), "{} is missing", band.name());
    }
    assert!(html.contains("<caption>heat</caption>"));
    assert!(html.contains("<caption>moisture</caption>"));
}

#[test]
fn the_readout_counts_the_window_that_is_on_the_screen() {
    // Not a fixed window and not the whole world: what the numbers describe has
    // to be what the image beside them shows.
    let session = session();
    let window = Window::origin_of(SEED);
    let reply = get(&format!("/seed/{SEED_TEXT}?{}", window.query()), &session);

    let viewport = window.viewport().expect("a valid window");
    let (cols, rows) = viewport.tile_counts();
    let expected =
        wgvb_render::Distribution::of(&session.state().generator(SEED), viewport.coords());

    let html = reply.text();
    assert!(
        html.contains(&format!(
            "{} tiles, counted",
            u64::from(cols) * u64::from(rows)
        )),
        "the readout does not name the window's tile count"
    );
    // One terrain's count, spelled out, so the page cannot be showing somebody
    // else's window.
    let (terrain, count) = expected
        .terrain()
        .max_by_key(|(_, count)| *count)
        .expect("a window has a commonest terrain");
    assert!(count > 0);
    assert!(
        html.contains(&format!("{count}")),
        "the count of {} is not on the page",
        terrain.name()
    );
}

#[test]
fn a_page_whose_readout_would_cost_too_much_is_refused_like_an_image() {
    // The readout is a whole `Tile` per cell — seven evaluations — so the page
    // is no longer free and goes through the same budget the images do.
    let session = session();
    let window = Window::origin_of(SEED);
    let target = format!("/seed/{SEED_TEXT}?{}", window.query());
    assert_eq!(
        get(&target, &session).status,
        200,
        "the default window fits"
    );

    let tiny_budget = 1_000;
    let reply = answer(&target, false, "", &[], &session, tiny_budget);
    assert_eq!(reply.status, 400);
    assert!(reply.text().contains("budget"), "{}", reply.text());
}
