use std::path::PathBuf;

use clap::{Parser, Subcommand};
use mote_api::messages::router;

use crate::runner;

#[derive(Debug, Parser)]
#[command(about = "Run Mote work-module test harnesses")]
struct Arguments {
    #[command(subcommand)]
    harness: Harness,
}

#[derive(Debug, Subcommand)]
enum Harness {
    /// Replay one or more canonical LiDAR recordings through navigation.
    Navigation {
        /// A canonical .data recording or its .json representation.
        #[arg(required = true, num_args = 1..)]
        inputs: Vec<PathBuf>,
        /// Optional JSON-lines output for per-revolution observations and poses.
        #[arg(long)]
        dump: Option<PathBuf>,
    },
    /// Convert a recorded telemetry file to JSON.
    ConvertTelemetry {
        /// Input .data file (for example, telemetry_1791430559418.data).
        input: PathBuf,
        /// Output JSON path (defaults to the input path with a .json extension).
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Invoke guidance with an empty route and the default position.
    Guidance,
    /// Invoke control without a guidance update.
    Control,
}

pub fn execute() -> anyhow::Result<()> {
    match Arguments::parse().harness {
        Harness::Navigation { inputs, dump } if inputs.len() == 1 && dump.is_none() => {
            runner::navigation(&inputs[0])
        }
        Harness::Navigation { inputs, dump } => runner::navigation_paths(&inputs, dump.as_deref()),
        Harness::ConvertTelemetry { input, output } => {
            runner::convert_telemetry_to_json(&input, output.as_deref())
        }
        Harness::Guidance => {
            let position = router::Position::default();
            let route = router::Route::new();
            let timestamp = position.timestamp;
            let result = runner::guidance(timestamp, &position, &route);
            println!(
                "Guidance harness completed; produced output: {}.",
                result.is_some()
            );
            Ok(())
        }
        Harness::Control => {
            let result = runner::control(router::Position::default().timestamp, None);
            println!(
                "Control harness completed; produced output: {}.",
                result.is_some()
            );
            Ok(())
        }
    }
}
