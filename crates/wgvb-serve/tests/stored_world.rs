//! `wgvb-serve --db`: the database is the world, the route's seed is a check.
//!
//! `DESIGN.md` section 29.1 wrote this down before it existed. These are the
//! assertions that it is now true, and they go through [`reply_from`] rather
//! than through a socket for the same reason every other test in this crate
//! does: the routing, the rendering, and the error mapping are a pure function
//! of the target and the source.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use wgvb::{Config, Coord};
use wgvb_render::FOG;
use wgvb_serve::{PNG, Source, reply, reply_from};
use wgvb_store::World;

/// The seed every test here stores.
const SEED: u64 = 0x0123_4567_89ab_cdef;

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A directory that is removed when it goes out of scope.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "wgvb-serve-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).expect("a writable temporary directory");
        Scratch(path)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// A world on a scratch path, and a source serving it.
fn stored(label: &str) -> (Scratch, PathBuf) {
    let scratch = Scratch::new(label);
    let path = scratch.file("world.wgvb");
    let world = World::open_or_create(&path, SEED, &Config::default()).expect("a fresh world");
    assert!(world.was_created());
    drop(world);
    (scratch, path)
}

/// A source over a stored world.
fn source(path: &Path) -> Source {
    Source::open(Some(path)).expect("the world opens")
}

/// The page route for a center, at a window small enough to be cheap.
fn page_url(seed: u64, center: Coord) -> String {
    format!(
        "/seed/{seed:016x}?q={}&r={}&cols=11&rows=9&hex-radius=5&layer=terrain",
        center.q(),
        center.r()
    )
}

/// The image route for the same window.
fn image_url(seed: u64, center: Coord) -> String {
    format!(
        "/seed/{seed:016x}/map.png?q={}&r={}&cols=11&rows=9&hex-radius=5&layer=terrain",
        center.q(),
        center.r()
    )
}

#[test]
fn a_stored_world_says_it_is_one_and_a_diagnostic_render_says_it_is_not() {
    let (_scratch, path) = stored("provenance");
    let center = Coord::new(0, 0);

    let saved = reply_from(&page_url(SEED, center), &source(&path));
    assert_eq!(saved.status, 200, "{}", saved.text());
    assert!(
        saved.text().contains("This is a saved world."),
        "a world-backed page did not say so"
    );
    assert!(
        !saved.text().contains("This is not a saved world."),
        "a world-backed page called itself diagnostic"
    );
    assert!(
        saved.text().contains("algorithm version 5"),
        "the page did not name the stored algorithm version"
    );

    let diagnostic = reply(&page_url(SEED, center));
    assert!(
        diagnostic.text().contains("This is not a saved world."),
        "a seed-only page called itself saved"
    );
}

#[test]
fn the_seed_in_the_route_is_a_check_rather_than_the_source() {
    // The sentence section 29.1 wrote, asserted. A server holding one world
    // does not have the seed that was asked for, so this is a 404 — and the
    // message names the seed it does have, because the fix is a link away.
    let (_scratch, path) = stored("check");
    let source = source(&path);

    let wrong = reply_from(&page_url(SEED + 1, Coord::ORIGIN), &source);
    assert_eq!(wrong.status, 404, "{}", wrong.text());
    assert!(
        wrong.text().contains(&format!("{SEED:016x}")),
        "the refusal did not name the seed this server holds: {}",
        wrong.text()
    );

    let right = reply_from(&page_url(SEED, Coord::ORIGIN), &source);
    assert_eq!(right.status, 200, "{}", right.text());
}

#[test]
fn a_world_backed_image_carries_no_etag_and_a_diagnostic_one_does() {
    // A strong validator promises the bytes are a pure function of what it
    // names. A player image also depends on overlays, which are mutable state
    // with no version anywhere, so the honest answer is no tag at all.
    let (_scratch, path) = stored("etag");
    let center = Coord::new(3, -4);

    let saved = reply_from(&image_url(SEED, center), &source(&path));
    assert_eq!(saved.status, 200);
    assert_eq!(saved.content_type, PNG);
    assert_eq!(
        saved.etag, None,
        "a world-backed image promised its bytes would not change"
    );

    let diagnostic = reply(&image_url(SEED, center));
    assert!(
        diagnostic.etag.is_some(),
        "a diagnostic image lost its strong validator"
    );

    // The page keeps its tag either way: it does not draw overlays. It must
    // still distinguish the two configurations behind it.
    let saved_page = reply_from(&page_url(SEED, center), &source(&path));
    assert!(saved_page.etag.is_some(), "the page lost its validator");
}

#[test]
fn the_etag_names_the_configuration_fingerprint() {
    // The hole the `etag` function used to carry a warning about. Two pages of
    // the same URL under different configurations must not share a tag.
    let scratch = Scratch::new("fingerprint");
    let normal = scratch.file("normal.wgvb");
    let tilted = scratch.file("tilted.wgvb");
    World::open_or_create(&normal, SEED, &Config::default()).expect("a fresh world");
    World::open_or_create(
        &tilted,
        SEED,
        &Config {
            sea_level: 0.125,
            ..Config::default()
        },
    )
    .expect("a fresh world");

    let target = page_url(SEED, Coord::ORIGIN);
    let one = reply_from(&target, &source(&normal)).etag;
    let other = reply_from(&target, &source(&tilted)).etag;
    assert!(one.is_some() && other.is_some());
    assert_ne!(
        one, other,
        "two configurations behind one URL shared a strong ETag"
    );
}

#[test]
fn discovering_a_tile_changes_what_the_next_request_draws() {
    // Overlays are read fresh per request, which is the whole reason the image
    // has no tag. Explore, ask again, see the difference.
    let (_scratch, path) = stored("fog");
    let source = source(&path);
    let center = Coord::new(0, 0);

    let before = reply_from(&image_url(SEED, center), &source).body;
    assert_eq!(fog_pixels(&before), 0, "a fresh world rendered as fog");

    let world = World::open(&path).expect("the world reopens");
    world.discover(&[center]).expect("the write runs");
    drop(world);

    let after = reply_from(&image_url(SEED, center), &source).body;
    assert_ne!(before, after, "exploring the world changed nothing");
    assert!(
        fog_pixels(&after) > 0,
        "one discovery did not switch fog on for the rest of the window"
    );
}

#[test]
fn the_page_reports_the_fog_and_the_settlement_under_the_center() {
    let (_scratch, path) = stored("readout");
    let center = Coord::new(-2, 5);

    // Nothing recorded: no overlay rows at all, because a server with no
    // overlays has no opinion about them.
    let bare = reply_from(&page_url(SEED, center), &source(&path));
    assert!(
        !bare.text().contains("<dt>fog</dt>"),
        "an empty world had fog"
    );

    let world = World::open(&path).expect("the world reopens");
    world.discover(&[center]).expect("the write runs");
    world.settle(center, "Ashford").expect("the write runs");
    drop(world);

    let page = reply_from(&page_url(SEED, center), &source(&path));
    let text = page.text();
    assert!(text.contains("<dt>fog</dt>"), "{text}");
    assert!(text.contains("this tile has been seen"), "{text}");
    assert!(text.contains("Ashford"), "{text}");
    assert!(
        text.contains("1 discovered tile,"),
        "the window counts read wrong: {text}"
    );
}

#[test]
fn a_settlement_name_is_escaped_on_the_page() {
    // The one value on this page that a person wrote rather than a parser
    // produced. Everything else is a number or a `&'static str` from the core
    // crate, which is why nothing else is escaped.
    let (_scratch, path) = stored("escape");
    let center = Coord::new(1, 1);

    let world = World::open(&path).expect("the world reopens");
    world
        .settle(center, "<script>alert('x')</script>")
        .expect("the write runs");
    drop(world);

    let text = reply_from(&page_url(SEED, center), &source(&path))
        .text()
        .to_owned();
    assert!(
        !text.contains("<script>"),
        "a settlement name reached the page as markup"
    );
    assert!(text.contains("&lt;script&gt;"), "{text}");
}

#[test]
fn a_database_that_is_not_a_world_is_a_startup_failure() {
    // Section 27.5's gates, reached through the server. A world this binary
    // cannot read must stop the server starting rather than turn every request
    // into a 500.
    let scratch = Scratch::new("foreign");
    let path = scratch.file("not-a-world.wgvb");
    std::fs::write(&path, b"this is not a database").expect("the file is written");

    assert!(
        Source::open(Some(&path)).is_err(),
        "a foreign file was adopted as a world"
    );
}

#[test]
fn one_source_is_opened_for_every_worker() {
    // `World` is `Send` and not `Sync`, so section 27.7's arrangement here is
    // one connection per thread. The count is the assertion.
    let (_scratch, path) = stored("workers");
    let sources = Source::open_all(Some(&path), 4).expect("four worlds open");
    assert_eq!(sources.len(), 4);
    for source in &sources {
        assert_eq!(source.world().expect("a stored world").seed(), SEED);
    }

    // And zero workers still gets one source, because a server with no worker
    // to hand it to would never have started.
    assert_eq!(
        Source::open_all(None, 0).expect("a default source").len(),
        1
    );
}

/// How many pixels of a PNG body are fog.
fn fog_pixels(body: &[u8]) -> usize {
    let decoder = png::Decoder::new(std::io::Cursor::new(body));
    let mut reader = decoder.read_info().expect("a readable PNG");
    let mut buffer = vec![0; reader.output_buffer_size().expect("a bounded image")];
    let info = reader.next_frame(&mut buffer).expect("a decodable frame");
    buffer.truncate(info.buffer_size());
    buffer
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| **pixel == FOG)
        .count()
}
