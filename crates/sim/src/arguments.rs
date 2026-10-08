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
    /// Replay Postcard IMU and LiDAR inputs through navigation.
    Navigation {
        /// Postcard IMU input file.
        #[arg(long)]
        imu: PathBuf,
        /// Postcard LiDAR scan input file.
        #[arg(long)]
        lidar: PathBuf,
    },
    /// Convert a recorded LiDAR telemetry file to JSON.
    ConvertLidar {
        /// Input .data file (for example, lidar_1791341524014.data).
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
        Harness::Navigation { imu, lidar } => runner::navigation(&lidar),
        Harness::ConvertLidar { input, output } => {
            runner::convert_lidar_to_json(&input, output.as_deref())
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
