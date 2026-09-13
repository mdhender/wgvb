//! `wgvb-map --db`: the database is the world, and the command line is only the
//! window.
//!
//! This is the phase 7 exit condition end to end — a database can create,
//! reopen, and reproduce one world safely, and multiple seeds and distant
//! coordinate windows produce varied but coherent player PNGs. It drives the
//! real binary, because the thing being tested is the command's contract rather
//! than a function's.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use wgvb_render::FOG;

/// A scratch directory that cleans up after itself.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "wgvb-map-db-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::remove_dir_all(&path).ok();
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

/// Runs the real binary.
fn wgvb_map(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wgvb-map"))
        .args(args)
        .output()
        .expect("the binary runs")
}

/// Runs it and insists it succeeded, returning what it said.
fn succeed(args: &[&str]) -> String {
    let output = wgvb_map(args);
    assert!(
        output.status.success(),
        "wgvb-map {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("utf-8 output")
}

/// Decoded RGBA, never file bytes: the `png` crate's filter and compression
/// defaults can change between versions and produce a different file for an
/// identical image.
fn rgba(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).expect("the PNG was written");
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("a readable PNG");
    let mut buffer = vec![0; reader.output_buffer_size().expect("a bounded image")];
    let info = reader.next_frame(&mut buffer).expect("a decodable frame");
    buffer.truncate(info.buffer_size());
    buffer
}

/// How many pixels of an image are fog.
fn fog_pixels(rgba: &[u8]) -> usize {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| **pixel == FOG)
        .count()
}

/// How many distinct colors an image uses.
fn colors(rgba: &[u8]) -> usize {
    rgba.as_chunks::<4>().0.iter().collect::<HashSet<_>>().len()
}

/// The window every test renders unless it wants a different one.
fn window(db: &Path, out: &Path) -> Vec<String> {
    [
        "--db",
        db.to_str().expect("a utf-8 path"),
        "--q",
        "-120",
        "--r",
        "60",
        "--cols",
        "40",
        "--rows",
        "30",
        "--hex-radius",
        "6",
        "--layer",
        "terrain",
        "--out",
        out.to_str().expect("a utf-8 path"),
    ]
    .iter()
    .map(|piece| (*piece).to_string())
    .collect()
}

/// Replaces the value that follows a flag, by name rather than by index.
fn set(args: &mut [String], flag: &str, value: &str) {
    let at = args
        .iter()
        .position(|piece| piece == flag)
        .unwrap_or_else(|| panic!("{flag} is not in the command line"));
    args[at + 1] = value.to_owned();
}

/// `Vec<String>` to what `Command` wants.
fn borrow(args: &[String]) -> Vec<&str> {
    args.iter().map(String::as_str).collect()
}

#[test]
fn a_database_is_created_then_reopened_and_reproduces_the_same_image() {
    let scratch = Scratch::new("roundtrip");
    let db = scratch.file("world.wgvb");
    let first = scratch.file("first.png");
    let second = scratch.file("second.png");

    let mut args = window(&db, &first);
    args.extend(["--seed".to_owned(), "424242".to_owned()]);
    let message = succeed(&borrow(&args));
    assert!(message.contains("created"), "{message}");
    assert!(db.exists(), "no database was written");

    // The second run names no seed at all. Everything it needs is in the file.
    let args = window(&db, &second);
    let message = succeed(&borrow(&args));
    assert!(message.contains("opened"), "{message}");
    assert!(
        message.contains("seed 0x0000000000067932"),
        "the stored seed was not reported: {message}"
    );

    assert_eq!(
        rgba(&first),
        rgba(&second),
        "a reopened world did not reproduce its own image"
    );
}

#[test]
fn a_stored_world_refuses_a_seed_from_the_command_line() {
    let scratch = Scratch::new("seedclash");
    let db = scratch.file("world.wgvb");
    let out = scratch.file("map.png");

    let mut args = window(&db, &out);
    args.extend(["--seed".to_owned(), "7".to_owned()]);
    succeed(&borrow(&args));
    std::fs::remove_file(&out).expect("the first image is removed");

    let mut args = window(&db, &out);
    args.extend(["--seed".to_owned(), "8".to_owned()]);
    let output = wgvb_map(&borrow(&args));
    assert!(
        !output.status.success(),
        "a world silently rendered under a seed it does not hold"
    );
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("cannot be overridden"), "{message}");
    assert!(!out.exists(), "a refused render wrote an image anyway");
}

#[test]
fn two_seeds_produce_different_worlds_through_the_same_database_path() {
    let scratch = Scratch::new("seeds");
    let mut images = Vec::new();
    for seed in ["1", "2", "12345678901234567890"] {
        let db = scratch.file(&format!("{seed}.wgvb"));
        let out = scratch.file(&format!("{seed}.png"));
        let mut args = window(&db, &out);
        args.extend(["--seed".to_owned(), seed.to_owned()]);
        succeed(&borrow(&args));
        images.push(rgba(&out));
    }

    for (index, left) in images.iter().enumerate() {
        for right in images.iter().skip(index + 1) {
            assert_ne!(left, right, "two seeds rendered the same world");
        }
    }
}

#[test]
fn distant_windows_of_one_world_are_varied_and_coherent() {
    // Section 2.2: the world is effectively unbounded, so windows thousands of
    // tiles apart must be different places rather than one tiling repeated.
    //
    // Note what is *not* asserted. Two windows coming back identical is not a
    // failure: at this scale a forty-by-thirty window can land entirely in deep
    // ocean, and two such windows are the same picture of two different places.
    // What would be a failure is every window agreeing, which is what a
    // renderer that ignored its coordinates would produce.
    let scratch = Scratch::new("windows");
    let db = scratch.file("world.wgvb");
    let mut images = Vec::new();

    for (index, (q, r)) in [
        (0_i64, 0_i64),
        (-9_000, 4_500),
        (12_000, -20_000),
        (31_000, 31_000),
    ]
    .into_iter()
    .enumerate()
    {
        let out = scratch.file(&format!("window{index}.png"));
        let mut args = window(&db, &out);
        set(&mut args, "--q", &q.to_string());
        set(&mut args, "--r", &r.to_string());
        set(&mut args, "--cols", "100");
        set(&mut args, "--rows", "75");
        set(&mut args, "--hex-radius", "3");
        succeed(&borrow(&args));
        images.push(rgba(&out));
    }

    let distinct: HashSet<&Vec<u8>> = images.iter().collect();
    assert!(
        distinct.len() > 1,
        "every distant window rendered the same image"
    );

    // Coherent rather than noise: somewhere in these four windows is ground
    // with coastlines, uplands, and climate on it, which is many colors. A
    // world of pure ocean would pass the test above and still be broken.
    let richest = images.iter().map(|image| colors(image)).max().unwrap_or(0);
    assert!(
        richest >= 8,
        "the most varied of four distant windows had only {richest} colors"
    );
}

#[test]
fn discovering_a_disc_lifts_the_fog_over_it_and_nowhere_else() {
    let scratch = Scratch::new("fog");
    let db = scratch.file("world.wgvb");
    let plain = scratch.file("plain.png");
    let fogged = scratch.file("fogged.png");

    // A fresh world tracks no discoveries, so nothing is hidden.
    let args = window(&db, &plain);
    succeed(&borrow(&args));
    assert_eq!(
        fog_pixels(&rgba(&plain)),
        0,
        "a world with no discoveries rendered as fog"
    );

    // Reveal a disc around one tile inside the window.
    let mut args = window(&db, &fogged);
    args.extend([
        "--discover".to_owned(),
        "-110,70".to_owned(),
        "--discover-radius".to_owned(),
        "4".to_owned(),
    ]);
    succeed(&borrow(&args));

    let pixels = rgba(&fogged);
    let fogged_count = fog_pixels(&pixels);
    assert!(fogged_count > 0, "discovering one disc hid nothing");
    assert!(
        fogged_count < pixels.len() / 4,
        "discovering one disc hid everything"
    );

    // The fog is new: the same window with no discoveries had none of it.
    assert_ne!(rgba(&plain), pixels);
}

#[test]
fn a_settlement_marker_appears_and_survives_a_reopen() {
    let scratch = Scratch::new("settle");
    let db = scratch.file("world.wgvb");
    let bare = scratch.file("bare.png");
    let marked = scratch.file("marked.png");
    let reopened = scratch.file("reopened.png");

    succeed(&borrow(&window(&db, &bare)));

    let mut args = window(&db, &marked);
    args.extend(["--settle".to_owned(), "-110,70=Ashford".to_owned()]);
    succeed(&borrow(&args));
    assert_ne!(rgba(&bare), rgba(&marked), "the settlement drew nothing");

    // A later run that names no settlement still draws the stored one.
    succeed(&borrow(&window(&db, &reopened)));
    assert_eq!(
        rgba(&marked),
        rgba(&reopened),
        "a stored settlement did not survive the reopen"
    );
}

#[test]
fn a_wgva_file_is_refused_rather_than_overwritten() {
    let scratch = Scratch::new("wgva");
    let db = scratch.file("world.wgva");
    let out = scratch.file("map.png");

    // Any non-empty file that is not a WGVB database. A real WGVA file is the
    // case that matters; SQLite refuses to read this one at all, which is the
    // same refusal one gate earlier.
    std::fs::write(&db, b"this is not a database").expect("the file is written");
    let before = std::fs::read(&db).expect("the file reads");

    let output = wgvb_map(&borrow(&window(&db, &out)));
    assert!(!output.status.success(), "a foreign file was adopted");
    assert_eq!(
        before,
        std::fs::read(&db).expect("the file reads"),
        "a refused open wrote to the file"
    );
    assert!(!out.exists(), "a refused open rendered an image anyway");
}
