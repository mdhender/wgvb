//! The CLI and the tuner must render the same window the same way.
//!
//! The same assertion `agrees_with_the_server.rs` makes about `wgvb-serve`,
//! made about the second front end. #16 put it in writing: whatever the tuner
//! gains goes into `wgvb-render` or into the URL, never into a drawing path of
//! its own — and the only thing that keeps that true is a test that fails when
//! it stops being true.
//!
//! Two things are compared here that the viewer's agreement test cannot: the
//! grid rasterizer, which draws one pixel per hex, and the turn, which rotates
//! the sampled region by a sixth. Both arrived for the tuner and both live in
//! the renderer, which is the whole point.

use std::process::Command;

use wgvb::{Config, Seed};
use wgvb_render::Layer;
use wgvb_tune::{PNG, Session, Window, answer};

const SEED: Seed = 0x0123_4567_89ab_cdef;
const SEED_TEXT: &str = "0123456789abcdef";
const BUDGET: u64 = 64_000_000;

/// A temporary directory of this test's own.
fn directory(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wgvb-tune-agreement-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&path).expect("a writable temporary directory");
    path
}

/// Runs the real `wgvb-map` binary over a grid window.
fn grid_from_the_cli(
    center: (i64, i64),
    cols: u32,
    rows: u32,
    scale: u32,
    turn: u8,
    layer: Layer,
    directory: &std::path::Path,
) -> Vec<u8> {
    let out = directory.join(format!("grid-{}-{turn}.png", layer.name()));
    let status = Command::new(env!("CARGO_BIN_EXE_wgvb-map"))
        .args(["--seed", &SEED.to_string()])
        .args(["--q", &center.0.to_string()])
        .args(["--r", &center.1.to_string()])
        .args(["--cols", &cols.to_string()])
        .args(["--rows", &rows.to_string()])
        .args(["--grid", &scale.to_string()])
        .args(["--turn", &turn.to_string()])
        .args(["--layer", layer.name()])
        .args(["--out", out.to_str().expect("a utf-8 path")])
        .status()
        .expect("wgvb-map runs");
    assert!(status.success(), "wgvb-map exited with {status}");
    std::fs::read(&out).expect("wgvb-map wrote the file")
}

/// Asks the tuner for the same window.
fn from_the_tuner(target: &str) -> Vec<u8> {
    let session = Session::new(SEED, Config::default()).expect("the defaults are valid");
    let reply = answer(target, false, "", &[], &session, BUDGET);
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert_eq!(reply.content_type, PNG);
    reply.body
}

#[test]
fn the_tuner_and_the_command_line_draw_identical_grids() {
    let directory = directory("grid");

    // Every turn, because the turn is the newest arithmetic here and the one
    // most likely to be reimplemented by hand in a front end.
    for turn in 0..6_u8 {
        let window = Window::origin_of(SEED)
            .with_grid(101, 81)
            .with_turn(turn)
            .with_scale(2)
            .with_layer(Layer::Terrain)
            .centered_on(wgvb::Coord::new(-87, 6543));

        let target = format!("/seed/{SEED_TEXT}/grid.png?{}", window.query());
        // The URL the page would have built for this state, so the test cannot
        // agree with a spelling the tuner never emits.
        let (_, parsed) = Window::parse(&target).expect("the target parses");
        assert_eq!(parsed, window, "the tuner spells this URL differently");

        assert_eq!(
            grid_from_the_cli((-87, 6543), 101, 81, 2, turn, Layer::Terrain, &directory),
            from_the_tuner(&target),
            "the CLI and the tuner disagree about a grid at turn {turn}"
        );
    }

    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn the_tuner_and_the_command_line_draw_identical_hex_windows() {
    // The hex tab goes through `render_player` with no overlays, which is what
    // `wgvb-map` without `--db` does, so the two must agree there too — turn
    // included.
    let directory = directory("hex");

    for turn in [0_u8, 3, 5] {
        let center = wgvb::Coord::new(-87, 6543);
        let window = Window::origin_of(SEED)
            .with_turn(turn)
            .with_layer(Layer::Elevation)
            .centered_on(center);

        // `wgvb-map` takes the window's first tile; the tuner takes its center.
        // `Viewport::centered_on` is the one conversion, and using it here is
        // the point rather than a shortcut.
        let origin = window.viewport().expect("a valid window").origin();

        let out = directory.join(format!("hex-{turn}.png"));
        let status = Command::new(env!("CARGO_BIN_EXE_wgvb-map"))
            .args(["--seed", &SEED.to_string()])
            .args(["--q", &origin.q().to_string()])
            .args(["--r", &origin.r().to_string()])
            .args(["--cols", &window.view.cols.to_string()])
            .args(["--rows", &window.view.rows.to_string()])
            .args(["--hex-radius", &window.view.hex_radius.to_string()])
            .args(["--turn", &turn.to_string()])
            .args(["--layer", window.view.layer.name()])
            .args(["--out", out.to_str().expect("a utf-8 path")])
            .status()
            .expect("wgvb-map runs");
        assert!(status.success(), "wgvb-map exited with {status}");

        let target = format!("/seed/{SEED_TEXT}/map.png?{}", window.query());
        assert_eq!(
            std::fs::read(&out).expect("wgvb-map wrote the file"),
            from_the_tuner(&target),
            "the CLI and the tuner disagree about a hex window at turn {turn}"
        );
    }

    std::fs::remove_dir_all(&directory).ok();
}

#[test]
fn a_tuned_configuration_renders_the_same_in_both() {
    // The configuration is the one thing the tuner holds that the CLI does not,
    // and `--config` is the bridge: what the tuner downloads, the CLI draws.
    // If that file lost so much as a low bit, these two images would differ.
    let directory = directory("config");

    // A wavelength rather than a threshold, and one with a long binary
    // expansion. A threshold moves which tiles are called what and can leave a
    // window of open ocean looking identical; a wavelength moves the field
    // itself, so the picture has to change — and the odd decimal is there so
    // that a file which rounded the value would be caught here as well as in
    // `wgvb-config`'s own round-trip test.
    let tuned = wgvb_config::with_field(&Config::default(), "local_wavelength_miles", "97.3125")
        .expect("a valid wavelength");
    let path = directory.join("tuned.toml");
    std::fs::write(&path, wgvb_config::to_toml(&tuned).expect("it serializes"))
        .expect("the file is written");

    let center = wgvb::Coord::new(-87, 6543);
    // The elevation layer, which draws the scalar field itself. A *terrain*
    // window can absorb a changed wavelength without moving a pixel — this
    // window is open ocean deep enough that nothing near it reclassifies —
    // and a test that could not tell an applied configuration from an ignored
    // one would be worse than no test.
    let window = Window::origin_of(SEED)
        .with_layer(Layer::Elevation)
        .centered_on(center);
    let origin = window.viewport().expect("a valid window").origin();

    let out = directory.join("cli.png");
    let status = Command::new(env!("CARGO_BIN_EXE_wgvb-map"))
        .args(["--seed", &SEED.to_string()])
        .args(["--q", &origin.q().to_string()])
        .args(["--r", &origin.r().to_string()])
        .args(["--cols", &window.view.cols.to_string()])
        .args(["--rows", &window.view.rows.to_string()])
        .args(["--hex-radius", &window.view.hex_radius.to_string()])
        .args(["--layer", window.view.layer.name()])
        .args(["--config", path.to_str().expect("a utf-8 path")])
        .args(["--out", out.to_str().expect("a utf-8 path")])
        .status()
        .expect("wgvb-map runs");
    assert!(status.success(), "wgvb-map exited with {status}");

    let session = Session::new(SEED, tuned).expect("the tuned configuration is valid");
    let target = format!("/seed/{SEED_TEXT}/map.png?{}", window.query());
    let reply = answer(&target, false, "", &[], &session, BUDGET);
    assert_eq!(reply.status, 200, "{}", reply.text());

    assert_eq!(
        std::fs::read(&out).expect("wgvb-map wrote the file"),
        reply.body,
        "the CLI and the tuner disagree about a tuned configuration"
    );

    // And the configuration is actually doing something, which two front ends
    // that both ignored it would not prove.
    let plain = Session::new(SEED, Config::default()).expect("valid");
    assert_ne!(
        answer(&target, false, "", &[], &plain, BUDGET).body,
        reply.body,
        "the tuned configuration drew the same image as the defaults"
    );

    std::fs::remove_dir_all(&directory).ok();
}
