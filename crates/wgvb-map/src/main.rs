//! `wgvb-map` — diagnostic and player-facing map renderer.
//!
//! See `DESIGN.md` section 29 for the command surface.
//!
//! # Two ways to name a world, and only one of them is a world
//!
//! With `--db world.wgvb` the database supplies the seed, the algorithm
//! version, and the complete effective configuration. Creating a database
//! writes all three before anything is rendered; opening an existing one never
//! overrides them, and `--seed` naming a different seed than the file holds is
//! an error rather than a silent override. This is the player-facing path, and
//! it is the one that composes overlays.
//!
//! Without `--db`, the same rendering code is driven by an in-memory generator
//! built from `--seed` and the current program defaults. **Output produced that
//! way is diagnostic and does not represent a saved world.** Nothing is
//! written, and a world file opened later will not reproduce these images
//! unless it happens to carry the same seed and configuration.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use wgvb::{Config, Coord, Generator};
use wgvb_render::{Layer, Overlays, Viewport, encode_png, render_player};
use wgvb_store::{Bounds, World};

/// Render a bounded window of a WGVB world to a PNG.
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// World database. Created from `--seed` if it does not exist, and
    /// authoritative for the seed and configuration if it does.
    #[arg(long)]
    db: Option<PathBuf>,

    /// World seed. Used to create a world, or on its own for a diagnostic
    /// render. Naming a seed a `--db` already disagrees with is an error.
    #[arg(long)]
    seed: Option<u64>,

    /// Record a tile as seen, as `q,r`. Repeatable; needs `--db`.
    #[arg(long, value_name = "Q,R", allow_hyphen_values = true)]
    discover: Vec<String>,

    /// How far around each `--discover` tile to reveal, in hexes.
    #[arg(long, default_value_t = 0)]
    discover_radius: u32,

    /// Found or rename a settlement, as `q,r=name`. Repeatable; needs `--db`.
    #[arg(long, value_name = "Q,R=NAME", allow_hyphen_values = true)]
    settle: Vec<String>,

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
    #[arg(long, default_value = "elevation", value_parser = parse_layer)]
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
    let origin = Coord::new(args.q, args.r);
    let viewport = Viewport::new(origin, args.cols, args.rows, args.hex_radius)?;

    let (generator, overlays, provenance) = match &args.db {
        Some(path) => from_world(args, path, &viewport)?,
        None => (from_seed(args)?, Overlays::none(), diagnostic_note(args)),
    };

    let image = render_player(&generator, &viewport, args.layer, &overlays);
    let bytes = encode_png(&image)?;
    std::fs::write(&args.out, &bytes)?;

    let (width, height) = image.size();
    Ok(format!(
        "wrote {} ({width}x{height} px, {} tiles, layer {}, origin ({}, {}) from ({}, {})); {provenance}",
        args.out.display(),
        u64::from(args.cols) * u64::from(args.rows),
        args.layer.name(),
        origin.q(),
        origin.r(),
        args.q,
        args.r,
    ))
}

/// The diagnostic path: a generator from the command line and nothing stored.
fn from_seed(args: &Args) -> Result<Generator, Box<dyn std::error::Error>> {
    if !args.discover.is_empty() || !args.settle.is_empty() {
        return Err("--discover and --settle write player state, which needs --db".into());
    }
    Ok(Generator::with_defaults(args.seed.unwrap_or(0)))
}

/// What the diagnostic path should say about itself.
fn diagnostic_note(args: &Args) -> String {
    format!(
        "diagnostic render of seed {} with program defaults, not a saved world",
        args.seed.unwrap_or(0)
    )
}

/// The player path: the database is authoritative for everything about the
/// world, and the command line is authoritative only for the window.
fn from_world(
    args: &Args,
    path: &std::path::Path,
    viewport: &Viewport,
) -> Result<(Generator, Overlays, String), Box<dyn std::error::Error>> {
    let world = World::open_or_create(path, args.seed.unwrap_or(0), &Config::default())?;

    // A stored world never takes a seed from the command line. Saying so is
    // the point: silently rendering seed 3's terrain for a file that holds
    // seed 7 would be the persistence layer lying about which world this is.
    if !world.was_created()
        && let Some(seed) = args.seed
        && seed != world.seed()
    {
        return Err(format!(
            "{} holds seed {}, not the {seed} given on the command line; \
             a stored world's seed cannot be overridden",
            path.display(),
            world.seed(),
        )
        .into());
    }

    apply_overlay_writes(args, &world)?;

    // One range scan over the smallest box holding the window's tiles. A
    // superset is correct — an overlay outside the window is never drawn — and
    // a wrapped window has no box smaller than this one.
    let bounds = Bounds::containing(window_coords(viewport)).unwrap_or_else(Bounds::everywhere);
    let overlays = Overlays::new(
        world.discoveries_in(&bounds)?,
        world
            .settlements_in(&bounds)?
            .into_iter()
            .map(|settlement| (settlement.coord, settlement.name))
            .collect(),
    );

    let provenance = format!(
        "{} {} (seed {:#018x}, algorithm {}, config {})",
        if world.was_created() {
            "created"
        } else {
            "opened"
        },
        path.display(),
        world.seed(),
        world.algorithm_version(),
        short_fingerprint(world.fingerprint()),
    );
    Ok((world.generator()?, overlays, provenance))
}

/// Applies every `--discover` and `--settle` before the window is read back.
fn apply_overlay_writes(args: &Args, world: &World) -> Result<(), Box<dyn std::error::Error>> {
    for spec in &args.discover {
        let center = parse_coord(spec)?;
        world.discover(&disc(center, args.discover_radius))?;
    }
    for spec in &args.settle {
        let (coord, name) = parse_settlement(spec)?;
        world.settle(coord, name)?;
    }
    Ok(())
}

/// Every tile the viewport will draw.
///
/// An iterator rather than a `Vec`: the only caller folds it into a bounding
/// box, and a four-hundred-by-three-hundred window is a hundred and twenty
/// thousand coordinates to allocate for a running minimum and maximum.
fn window_coords(viewport: &Viewport) -> impl Iterator<Item = Coord> + '_ {
    let (cols, rows) = viewport.tile_counts();
    (0..cols).flat_map(move |col| (0..rows).map(move |row| viewport.coord_at(col, row)))
}

/// Every tile within `radius` of a center, including the center.
///
/// Plain `i64` arithmetic over the axial rhombus, clipped to the hexagonal
/// range: `Coord::new` normalizes each result, so a disc that runs off the edge
/// of the canonical map wraps rather than being lost. `wgvb-map` does not
/// depend on `hexx` and should not start: this is nine lines of integer work.
fn disc(center: Coord, radius: u32) -> Vec<Coord> {
    let radius = i64::from(radius);
    let mut coords = Vec::new();
    for dq in -radius..=radius {
        let lo = (-radius).max(-dq - radius);
        let hi = radius.min(-dq + radius);
        for dr in lo..=hi {
            coords.push(Coord::new(
                i64::from(center.q()) + dq,
                i64::from(center.r()) + dr,
            ));
        }
    }
    coords
}

/// Parses `q,r`.
fn parse_coord(spec: &str) -> Result<Coord, String> {
    let (q, r) = spec
        .split_once(',')
        .ok_or_else(|| format!("expected a coordinate as `q,r`, got {spec:?}"))?;
    let q: i64 = q
        .trim()
        .parse()
        .map_err(|_| format!("{q:?} is not an axial q"))?;
    let r: i64 = r
        .trim()
        .parse()
        .map_err(|_| format!("{r:?} is not an axial r"))?;
    Ok(Coord::new(q, r))
}

/// Parses `q,r=name`.
fn parse_settlement(spec: &str) -> Result<(Coord, &str), String> {
    let (coord, name) = spec
        .split_once('=')
        .ok_or_else(|| format!("expected a settlement as `q,r=name`, got {spec:?}"))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(format!("a settlement needs a name, got {spec:?}"));
    }
    Ok((parse_coord(coord)?, name))
}

/// The first four bytes of a fingerprint, for a line a human reads.
///
/// Enough to notice that two renders came from different configurations, and
/// short enough not to bury the rest of the message. Nothing compares worlds by
/// this prefix; the store compares all thirty-two bytes.
fn short_fingerprint(fingerprint: &[u8; 32]) -> String {
    fingerprint[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
        assert_eq!(
            args.seed, None,
            "the seed is optional rather than required, and a diagnostic render \
             without one uses zero"
        );
        assert_eq!(args.db, None, "a diagnostic render needs no database");
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
            "wgvb-map", "--q", "0", "--r", "0", "--layer", "swamps", "--out", "m.png",
        ])
        .expect_err("an unknown layer is rejected");
        let message = error.to_string();
        assert!(message.contains("swamps"), "{message}");
        assert!(message.contains("elevation-raw"), "{message}");
        assert!(message.contains("relief"), "{message}");
        assert!(message.contains("terrain"), "{message}");
    }

    #[test]
    fn a_coordinate_specification_parses_and_rejects() {
        assert_eq!(parse_coord("3,-4").unwrap(), Coord::new(3, -4));
        assert_eq!(parse_coord(" -12 , 30 ").unwrap(), Coord::new(-12, 30));
        assert!(parse_coord("3").is_err());
        assert!(parse_coord("three,four").is_err());
        assert!(parse_coord("3,").is_err());
    }

    #[test]
    fn a_settlement_specification_parses_and_rejects() {
        let (coord, name) = parse_settlement("-5,6=Ashford Keep").unwrap();
        assert_eq!(coord, Coord::new(-5, 6));
        assert_eq!(name, "Ashford Keep");
        assert!(parse_settlement("-5,6").is_err(), "a name is required");
        assert!(
            parse_settlement("-5,6=").is_err(),
            "an empty name is not one"
        );
        assert!(parse_settlement("nowhere=Ashford").is_err());
    }

    #[test]
    fn a_disc_is_the_hexagonal_neighborhood_of_its_center() {
        // 1 + 3*R*(R+1) tiles, the same count as the world itself at its own
        // radius. Derived from the formula rather than from what the loop does.
        for radius in 0..5_u32 {
            let center = Coord::new(-3, 9);
            let tiles = disc(center, radius);
            let r = i64::from(radius);
            assert_eq!(
                tiles.len(),
                usize::try_from(1 + 3 * r * (r + 1)).expect("a small disc"),
                "radius {radius}"
            );
            assert!(tiles.contains(&center));
        }

        // Every tile of a radius-1 disc is the center or one of its neighbors.
        let center = Coord::new(0, 0);
        let mut expected: Vec<Coord> = (0..6).map(|d| center.neighbor(d)).collect();
        expected.push(center);
        expected.sort_unstable();
        let mut found = disc(center, 1);
        found.sort_unstable();
        assert_eq!(found, expected);
    }

    #[test]
    fn overlay_flags_without_a_database_are_an_error_rather_than_a_silent_no_op() {
        let args = Args::try_parse_from([
            "wgvb-map",
            "--q",
            "0",
            "--r",
            "0",
            "--discover",
            "1,1",
            "--out",
            "m.png",
        ])
        .expect("the command line itself is valid");
        assert!(
            run(&args).is_err(),
            "player state was written nowhere and the command said it succeeded"
        );
    }

    #[test]
    fn negative_overlay_coordinates_survive_the_command_line() {
        let args = Args::try_parse_from([
            "wgvb-map",
            "--q",
            "0",
            "--r",
            "0",
            "--discover",
            "-5,-7",
            "--settle",
            "-1,-2=South",
            "--out",
            "m.png",
        ])
        .expect("negative overlay coordinates parse");
        assert_eq!(args.discover, ["-5,-7"]);
        assert_eq!(args.settle, ["-1,-2=South"]);
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
            "elevation",
            "--out",
            out.to_str().expect("a utf-8 path"),
        ])
        .expect("valid arguments");

        let message = run(&args).expect("rendering succeeds");
        assert!(message.contains("elevation"), "{message}");

        let bytes = std::fs::read(&out).expect("the file was written");
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let reader = decoder.read_info().expect("a readable PNG");
        let info = reader.info();
        let viewport = Viewport::new(Coord::new(-25, 13), 12, 9, 5.0).expect("valid viewport");
        assert_eq!((info.width, info.height), viewport.image_size());

        std::fs::remove_dir_all(&directory).ok();
    }
}
