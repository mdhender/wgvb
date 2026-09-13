//! `wgvb-map` — diagnostic and player-facing map renderer.
//!
//! See `DESIGN.md` section 29 for the command surface.
//!
//! # This phase renders from memory, not from a world
//!
//! The design's eventual command takes `--db world.wgvb`, and the database
//! supplies the seed, algorithm version, and effective configuration. Until
//! persistence exists, the same rendering code is driven by an explicitly
//! constructed in-memory generator, which is what `--seed` is for.
//!
//! **Output produced this way is diagnostic and does not represent a saved
//! world.** Nothing here writes a world file, and a world file opened later
//! will not reproduce these images unless it happens to carry the same seed and
//! configuration. Player-facing rendering always loads its effective
//! configuration from the database.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use wgvb::{Coord, Generator};
use wgvb_render::{Layer, Viewport, encode_png, render};

/// Render a bounded window of a WGVB world to a PNG.
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// World seed. Diagnostic only at this phase; a saved world will supply it.
    #[arg(long, default_value_t = 0)]
    seed: u64,

    /// Axial `q` of the window's first tile.
    #[arg(long, allow_negative_numbers = true)]
    q: i64,

    /// Axial `r` of the window's first tile.
    #[arg(long, allow_negative_numbers = true)]
    r: i64,

    /// Window width, in tiles.
    #[arg(long, default_value_t = 64)]
    cols: u32,

    /// Window height, in tiles.
    #[arg(long, default_value_t = 48)]
    rows: u32,

    /// Hex radius, in pixels.
    #[arg(long, default_value_t = 8.0)]
    hex_radius: f32,

    /// Scalar layer to draw.
    #[arg(long, default_value = "elevation-raw", value_parser = parse_layer)]
    layer: Layer,

    /// Where to write the PNG.
    #[arg(long)]
    out: PathBuf,
}

/// Parses a layer name, listing the alternatives on failure.
///
/// An unknown layer is an error rather than a silent fallback, for the same
/// reason an unknown configuration field is.
fn parse_layer(name: &str) -> Result<Layer, String> {
    Layer::parse(name).map_err(|_| {
        let known: Vec<&str> = Layer::ALL.iter().map(|layer| layer.name()).collect();
        format!(
            "unknown layer {name:?}; known layers are {}",
            known.join(", ")
        )
    })
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("wgvb-map: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Does the work, so `main` only chooses an exit code.
fn run(args: &Args) -> Result<String, Box<dyn std::error::Error>> {
    let generator = Generator::with_defaults(args.seed);
    let origin = Coord::new(args.q, args.r);
    let viewport = Viewport::new(origin, args.cols, args.rows, args.hex_radius)?;
    let image = render(&generator, &viewport, args.layer);
    let bytes = encode_png(&image)?;
    std::fs::write(&args.out, &bytes)?;

    let (width, height) = image.size();
    Ok(format!(
        "wrote {} ({width}x{height} px, {} tiles, layer {}, origin ({}, {}) from ({}, {}))",
        args.out.display(),
        u64::from(args.cols) * u64::from(args.rows),
        args.layer.name(),
        origin.q(),
        origin.r(),
        args.q,
        args.r,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn the_command_surface_is_well_formed() {
        Args::command().debug_assert();
    }

    #[test]
    fn the_documented_flags_all_parse() {
        let args = Args::try_parse_from([
            "wgvb-map",
            "--q",
            "-200",
            "--r",
            "-150",
            "--cols",
            "400",
            "--rows",
            "300",
            "--hex-radius",
            "8",
            "--layer",
            "continentalness",
            "--out",
            "map.png",
        ])
        .expect("the documented command line parses");
        assert_eq!(args.q, -200);
        assert_eq!(args.r, -150);
        assert_eq!(args.cols, 400);
        assert_eq!(args.rows, 300);
        assert_eq!(args.hex_radius, 8.0);
        assert_eq!(args.layer, Layer::Continentalness);
        assert_eq!(args.out, PathBuf::from("map.png"));
        assert_eq!(args.seed, 0, "the seed defaults rather than being required");
    }

    #[test]
    fn negative_coordinates_do_not_look_like_flags() {
        // `--q -200` must be a coordinate, not a parse error. Section 24:
        // negative coordinates are first-class.
        for (q, r) in [("-1", "-1"), ("-32767", "32767"), ("0", "-9999")] {
            let args = Args::try_parse_from(["wgvb-map", "--q", q, "--r", r, "--out", "m.png"])
                .expect("negative coordinates parse");
            assert_eq!(args.q, q.parse::<i64>().unwrap());
            assert_eq!(args.r, r.parse::<i64>().unwrap());
        }
    }

    #[test]
    fn every_layer_name_is_accepted() {
        for layer in Layer::ALL {
            let args = Args::try_parse_from([
                "wgvb-map",
                "--q",
                "0",
                "--r",
                "0",
                "--layer",
                layer.name(),
                "--out",
                "m.png",
            ])
            .expect("a known layer parses");
            assert_eq!(args.layer, layer);
        }
    }

    #[test]
    fn an_unknown_layer_is_rejected_and_lists_the_alternatives() {
        let error = Args::try_parse_from([
            "wgvb-map", "--q", "0", "--r", "0", "--layer", "terrain", "--out", "m.png",
        ])
        .expect_err("an unknown layer is rejected");
        let message = error.to_string();
        assert!(message.contains("terrain"), "{message}");
        assert!(message.contains("elevation-raw"), "{message}");
    }

    #[test]
    fn an_impossible_viewport_is_an_error_rather_than_a_panic() {
        let args = Args::try_parse_from([
            "wgvb-map", "--q", "0", "--r", "0", "--cols", "0", "--out", "m.png",
        ])
        .expect("the command line itself is valid");
        assert!(run(&args).is_err());
    }

    #[test]
    fn rendering_writes_a_png_that_decodes_to_the_requested_size() {
        let directory = std::env::temp_dir().join(format!(
            "wgvb-map-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).expect("a writable temporary directory");
        let out = directory.join("map.png");

        let args = Args::try_parse_from([
            "wgvb-map",
            "--seed",
            "7",
            "--q",
            "-25",
            "--r",
            "13",
            "--cols",
            "12",
            "--rows",
            "9",
            "--hex-radius",
            "5",
            "--layer",
            "elevation-raw",
            "--out",
            out.to_str().expect("a utf-8 path"),
        ])
        .expect("valid arguments");

        let message = run(&args).expect("rendering succeeds");
        assert!(message.contains("elevation-raw"), "{message}");

        let bytes = std::fs::read(&out).expect("the file was written");
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let reader = decoder.read_info().expect("a readable PNG");
        let info = reader.info();
        let viewport = Viewport::new(Coord::new(-25, 13), 12, 9, 5.0).expect("valid viewport");
        assert_eq!((info.width, info.height), viewport.image_size());

        std::fs::remove_dir_all(&directory).ok();
    }
}
