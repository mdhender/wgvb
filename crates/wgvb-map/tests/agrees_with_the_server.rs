//! The CLI and the web viewer must render the same window the same way.
//!
//! `wgvb-map` and `wgvb-serve` are two front ends over one renderer, and the
//! only thing stopping them drifting is an assertion that says so. This is it:
//! the bytes `wgvb-map` writes to a file and the bytes `/seed/{s}/map.png`
//! returns for the same window, layer, and scale are the same bytes.
//!
//! The comparison is on PNG bytes rather than on decoded RGBA, which is the
//! opposite of what `DESIGN.md` section 29 asks of a *golden*. That is
//! deliberate and is not the same claim: a golden pins what the renderer draws
//! across `png` crate versions, and `crates/wgvb-render/tests/golden_image.rs`
//! already does that. This pins that two callers in one build produce one
//! result, and a byte comparison is the strongest available form of it.

use std::process::Command;

use wgvb::{Config, Coord};
use wgvb_render::{Layer, Viewport};
use wgvb_serve::{PNG, Source, View, reply, reply_from};
use wgvb_store::World;

/// One window, described the way each front end wants it.
struct Window {
    seed: u64,
    center: Coord,
    cols: u32,
    rows: u32,
    hex_radius: f32,
    layer: Layer,
}

/// Runs the real `wgvb-map` binary over a window and returns the PNG it wrote.
fn from_the_cli(window: &Window, directory: &std::path::Path) -> Vec<u8> {
    // `wgvb-map` takes the window's *first* tile; the viewer takes its center.
    // `Viewport::centered_on` is the one conversion, and using it here is the
    // point rather than a shortcut: a second copy of the offset arithmetic is
    // exactly what this test exists to prevent.
    let origin = Viewport::centered_on(window.center, window.cols, window.rows, window.hex_radius)
        .expect("a valid window")
        .origin();

    let out = directory.join(format!("{}-{}.png", window.seed, window.layer.name()));
    let status = Command::new(env!("CARGO_BIN_EXE_wgvb-map"))
        .args(["--seed", &window.seed.to_string()])
        .args(["--q", &origin.q().to_string()])
        .args(["--r", &origin.r().to_string()])
        .args(["--cols", &window.cols.to_string()])
        .args(["--rows", &window.rows.to_string()])
        .args(["--hex-radius", &window.hex_radius.to_string()])
        .args(["--layer", window.layer.name()])
        .args(["--out", out.to_str().expect("a utf-8 path")])
        .status()
        .expect("wgvb-map runs");
    assert!(status.success(), "wgvb-map exited with {status}");
    std::fs::read(&out).expect("wgvb-map wrote the file")
}

/// Asks the server for the same window.
fn from_the_server(window: &Window) -> Vec<u8> {
    let target = format!(
        "/seed/{:016x}/map.png?q={}&r={}&cols={}&rows={}&hex-radius={}&layer={}",
        window.seed,
        window.center.q(),
        window.center.r(),
        window.cols,
        window.rows,
        window.hex_radius,
        window.layer.name(),
    );

    // The URL the viewer would have built for this state, so the test cannot
    // agree with a spelling the page never emits.
    let (_, view) = View::parse(&target).expect("the target parses");
    assert_eq!(
        view.image_url(),
        target,
        "the viewer spells this URL differently"
    );

    let reply = reply(&target);
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(reply.content_type, PNG);
    reply.body
}

#[test]
fn the_server_and_the_command_line_render_identical_bytes() {
    let directory = std::env::temp_dir().join(format!(
        "wgvb-agreement-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("a writable temporary directory");

    // Every layer, at a center that is nowhere near the origin, on an odd
    // window, so the conversion from center to first tile is doing work.
    for layer in Layer::ALL {
        let window = Window {
            seed: 0x0123_4567_89ab_cdef,
            center: Coord::new(-87, 6543),
            cols: 21,
            rows: 15,
            hex_radius: 6.0,
            layer,
        };
        assert_eq!(
            from_the_cli(&window, &directory),
            from_the_server(&window),
            "the CLI and the server disagree about the {} layer",
            layer.name()
        );
    }

    // And a window that wraps a world edge, where the center-to-origin
    // conversion crosses the seam.
    let window = Window {
        seed: 7,
        center: Coord::new(32_767, 0),
        cols: 11,
        rows: 9,
        hex_radius: 4.0,
        layer: Layer::Elevation,
    };
    assert_eq!(
        from_the_cli(&window, &directory),
        from_the_server(&window),
        "the CLI and the server disagree across a wrapped edge"
    );

    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn the_two_front_ends_compose_the_same_overlays() {
    // Phase 7 gave both front ends a second input — what the player has seen
    // and built — and a second input is a second chance to drift. The CLI
    // computes the overlay window from `Viewport::coords` and a `Bounds`, and
    // so does the server; this is the assertion that they get the same answer,
    // through a window with fog in it and a settlement on the edge of the
    // explored area so both composition rules are exercised.
    let directory = std::env::temp_dir().join(format!(
        "wgvb-agreement-db-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).expect("a writable temporary directory");
    let db = directory.join("world.wgvb");

    let window = Window {
        seed: 0x0123_4567_89ab_cdef,
        center: Coord::new(-87, 6543),
        cols: 21,
        rows: 15,
        hex_radius: 6.0,
        layer: Layer::Terrain,
    };

    {
        let world =
            World::open_or_create(&db, window.seed, &Config::default()).expect("a fresh world");
        // A short line of discovered tiles across the middle, so most of the
        // window is fog and some of it is not.
        let seen: Vec<Coord> = (-3..=3)
            .map(|d| {
                Coord::new(
                    i64::from(window.center.q()) + d,
                    i64::from(window.center.r()),
                )
            })
            .collect();
        world.discover(&seen).expect("the write runs");
        // One settlement on explored ground and one out in the fog: the two
        // rules are "fog hides terrain" and "fog does not hide what the player
        // built", and an image missing either would not match.
        world
            .settle(window.center, "Ashford")
            .expect("the write runs");
        world
            .settle(Coord::new(-80, 6547), "Longwatch")
            .expect("the write runs");
    }

    let origin = Viewport::centered_on(window.center, window.cols, window.rows, window.hex_radius)
        .expect("a valid window")
        .origin();
    let out = directory.join("cli.png");
    let status = Command::new(env!("CARGO_BIN_EXE_wgvb-map"))
        .args(["--db", db.to_str().expect("a utf-8 path")])
        .args(["--q", &origin.q().to_string()])
        .args(["--r", &origin.r().to_string()])
        .args(["--cols", &window.cols.to_string()])
        .args(["--rows", &window.rows.to_string()])
        .args(["--hex-radius", &window.hex_radius.to_string()])
        .args(["--layer", window.layer.name()])
        .args(["--out", out.to_str().expect("a utf-8 path")])
        .status()
        .expect("wgvb-map runs");
    assert!(status.success(), "wgvb-map exited with {status}");
    let from_cli = std::fs::read(&out).expect("wgvb-map wrote the file");

    let source = Source::open(Some(&db)).expect("the world opens");
    let target = format!(
        "/seed/{:016x}/map.png?q={}&r={}&cols={}&rows={}&hex-radius={}&layer={}",
        window.seed,
        window.center.q(),
        window.center.r(),
        window.cols,
        window.rows,
        window.hex_radius,
        window.layer.name(),
    );
    let reply = reply_from(&target, &source);
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(reply.content_type, PNG);

    assert_eq!(
        from_cli, reply.body,
        "the CLI and the server disagree about a world with overlays"
    );

    // And the overlays are actually in the picture. Two front ends that both
    // ignored them would agree perfectly and prove nothing.
    let plain = from_the_server(&window);
    assert_ne!(
        plain, reply.body,
        "the composed image is identical to the unexplored one"
    );

    std::fs::remove_dir_all(&directory).ok();
}
