//! `wgvb-tune` — a local web instrument for tuning a WGVB configuration.
//!
//! See the crate documentation in `lib.rs` for the routes and the reasoning,
//! and `DESIGN.md` section 29.3 for why this is a second tool rather than a
//! mode of `wgvb-serve`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use wgvb_tune::{
    DEFAULT_HOST, DEFAULT_PORT, DEFAULT_SEED, Options, default_workers, is_loopback, route, serve,
};

/// Tune a WGVB configuration and watch what it does to the world.
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Seed to land on, in hexadecimal. Every other seed is still reachable by
    /// typing it, because the route is where a seed comes from.
    #[arg(long, default_value_t = format!("{DEFAULT_SEED:016x}"))]
    seed: String,

    /// Configuration file to start from. Without one, this binary's defaults.
    ///
    /// This is the file the configuration tab downloads, so a session can be
    /// picked up where the last one left off.
    #[arg(long)]
    config: Option<PathBuf>,

    /// Interface to bind.
    ///
    /// Loopback by default and deliberately so: this instrument has no
    /// authentication, renders whatever window the caller asks for, and accepts
    /// a POST that changes what it draws.
    #[arg(long, default_value = DEFAULT_HOST)]
    host: String,

    /// Port to bind. One above the viewer's, so both can run at once.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,

    /// Worker threads. Zero means one per available core.
    #[arg(long, default_value_t = 0)]
    workers: usize,

    /// Generator evaluations one request may cost.
    ///
    /// Counted in evaluations rather than tiles because `relief`, `climate`,
    /// and `terrain` cost seven apiece. Raise it to measure a larger window.
    #[arg(long, default_value_t = route::DEFAULT_BUDGET)]
    budget: u64,

    /// Do not print a line per request.
    ///
    /// Those lines are the measurement: each one says how many tiles a window
    /// drew and how long generating them took.
    #[arg(long)]
    quiet: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();

    let seed = match u64::from_str_radix(args.seed.trim_start_matches("0x"), 16) {
        Ok(seed) => seed,
        Err(_) => {
            eprintln!(
                "wgvb-tune: {:?} is not a seed; a seed is hexadecimal digits, as \
                 everywhere else in this repository",
                args.seed
            );
            return ExitCode::FAILURE;
        }
    };

    if !is_loopback(&args.host) {
        eprintln!(
            "wgvb-tune: {} is not a loopback address. This is a diagnostic \
             instrument with no authentication that accepts configuration changes.",
            args.host
        );
    }

    let options = Options {
        seed,
        config: args.config,
        host: args.host,
        port: args.port,
        workers: if args.workers == 0 {
            default_workers()
        } else {
            args.workers
        },
        budget: args.budget,
        log: !args.quiet,
    };

    match serve(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("wgvb-tune: {error}");
            ExitCode::FAILURE
        }
    }
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
    fn the_defaults_are_loopback_and_the_golden_seed() {
        let args = Args::try_parse_from(["wgvb-tune"]).expect("no arguments is valid");
        assert_eq!(args.host, DEFAULT_HOST);
        assert!(is_loopback(&args.host), "the default bind must be loopback");
        assert_eq!(args.port, DEFAULT_PORT);
        assert_eq!(args.seed, format!("{DEFAULT_SEED:016x}"));
        assert_eq!(args.config, None);
        assert_eq!(args.budget, route::DEFAULT_BUDGET);
        assert!(!args.quiet);
    }

    #[test]
    fn the_two_servers_do_not_want_the_same_port() {
        // Comparing two configurations is two servers and alt-tab, which only
        // works if they do not fight over a port by default.
        assert_ne!(DEFAULT_PORT, wgvb_serve_default_port());
    }

    /// The viewer's default port, written out rather than depended on: this
    /// crate does not and should not link the viewer.
    const fn wgvb_serve_default_port() -> u16 {
        8080
    }

    #[test]
    fn a_seed_is_hexadecimal_with_or_without_a_prefix() {
        for text in ["0123456789abcdef", "0x0123456789abcdef", "FEEDFACE"] {
            let parsed = u64::from_str_radix(text.trim_start_matches("0x"), 16);
            assert!(parsed.is_ok(), "{text} should parse");
        }
        assert!(u64::from_str_radix("nonsense", 16).is_err());
    }
}
