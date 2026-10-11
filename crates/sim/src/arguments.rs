use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
    /// Run a test vector of known positions through guidance against a route.
    Guidance {
        /// Settings JSON containing the route (for example, san_martin_pattern.json).
        route: PathBuf,
        /// Test vector JSON of positions and expected guidance.
        vector: PathBuf,
    },
    /// Run a test vector of guidance inputs through control.
    Control {
        /// Settings JSON containing the reference frame (for example, san_martin_pattern.json).
        settings: PathBuf,
        /// Test vector JSON of guidance inputs and expected wheel speeds.
        vector: PathBuf,
    },
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
        Harness::Guidance { route, vector } => runner::guidance_vector(&route, &vector),
        Harness::Control { settings, vector } => runner::control_vector(&settings, &vector),
    }
}
