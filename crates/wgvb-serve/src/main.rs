//! `wgvb-serve` — a local web viewer for one seed.
//!
//! See the crate documentation in `lib.rs` for the routes and the reasoning,
//! and `DESIGN.md` section 29 for why a viewer exists at all.
//!
//! This is not a mode of `wgvb-map`. A server drags in an HTTP stack, and the
//! CLI has no use for one; keeping them apart keeps `wgvb-map --help` honest
//! and keeps the diagnostic CLI buildable without a web server in the tree.

use std::process::ExitCode;

use clap::Parser;
use wgvb_serve::{DEFAULT_HOST, DEFAULT_PORT, Options, default_workers, is_loopback, serve};

/// Serve a browsable view of a WGVB seed over HTTP.
#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Interface to bind.
    ///
    /// The default is loopback on purpose: this viewer has no authentication
    /// and renders whatever window the caller asks for.
    #[arg(long, default_value = DEFAULT_HOST)]
    host: String,

    /// Port to bind.
    #[arg(long, default_value_t = DEFAULT_PORT)]
    port: u16,

    /// Worker threads. Zero means one per available core.
    #[arg(long, default_value_t = 0)]
    workers: usize,

    /// Do not print a line per request.
    #[arg(long)]
    quiet: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();

    if !is_loopback(&args.host) {
        eprintln!(
            "wgvb-serve: {} is not a loopback address. This is a diagnostic \
             viewer with no authentication.",
            args.host
        );
    }

    let options = Options {
        host: args.host,
        port: args.port,
        workers: if args.workers == 0 {
            default_workers()
        } else {
            args.workers
        },
        log: !args.quiet,
    };

    match serve(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("wgvb-serve: {error}");
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
    fn the_defaults_are_loopback() {
        let args = Args::try_parse_from(["wgvb-serve"]).expect("no arguments is valid");
        assert_eq!(args.host, DEFAULT_HOST);
        assert!(is_loopback(&args.host), "the default bind must be loopback");
        assert_eq!(args.port, DEFAULT_PORT);
        assert_eq!(args.workers, 0, "zero means one worker per core");
        assert!(!args.quiet);
    }

    #[test]
    fn the_documented_flags_all_parse() {
        let args = Args::try_parse_from([
            "wgvb-serve",
            "--host",
            "0.0.0.0",
            "--port",
            "9000",
            "--workers",
            "2",
            "--quiet",
        ])
        .expect("the documented command line parses");
        assert_eq!(args.host, "0.0.0.0");
        assert_eq!(args.port, 9000);
        assert_eq!(args.workers, 2);
        assert!(args.quiet);
        assert!(!is_loopback(&args.host), "0.0.0.0 is not loopback");
    }
}
