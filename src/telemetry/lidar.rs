use bee_ui::{
    UiRoot,
    prelude::{
        ScatterControlPanel, ScatterDataset, ScatterPlot, ScatterPlotProps, ScatterPoint,
        ScatterSeries, ViewportState,
    },
};
use eframe::egui;
use mote_api::messages::router;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crate::state;

const LIDAR_PLOT_ID: &str = "mote.telemetry.lidar";
const LIDAR_COLOR_MAX_AGE_SECONDS: f32 = 5.0;
const LIDAR_POINT_FADE_SECONDS: f32 = LIDAR_COLOR_MAX_AGE_SECONDS / 10.0;
const LIDAR_MIN_POINT_SIZE: f32 = 2.0;
const LIDAR_MAX_POINT_SIZE: f32 = 8.0;
const LIDAR_POINT_CAPACITY: usize = 10_000;

#[derive(Default)]
struct LidarPlotHistory {
    points: VecDeque<LidarHistoryPoint>,
    last_scan_time: Option<Instant>,
}

struct LidarHistoryPoint {
    x: f64,
    y: f64,
    size: f32,
    received_at: Instant,
}

impl LidarPlotHistory {
    fn push_scan(&mut self, scan: &router::Scan, timestamp: Instant) {
        if self
            .last_scan_time
            .is_some_and(|last_scan_time| timestamp <= last_scan_time)
        {
            return;
        }

        for reading in scan.iter() {
            let angle: f32 = reading.angle.into();
            let distance: f32 = reading.distance.into();
            let angle = f64::from(angle);
            let distance = f64::from(distance) * 1_000.0;
            if !angle.is_finite() || !distance.is_finite() || distance <= 0.0 {
                continue;
            }

            if self.points.len() == LIDAR_POINT_CAPACITY {
                self.points.pop_front();
            }
            self.points.push_back(LidarHistoryPoint {
                x: distance * angle.cos(),
                y: distance * angle.sin(),
                size: intensity_size(reading.quality),
                received_at: timestamp,
            });
        }
        self.last_scan_time = Some(timestamp);
    }

    fn dataset(&self, now: Instant) -> ScatterDataset {
        let points = self
            .points
            .iter()
            .map(|point| {
                let age = now.saturating_duration_since(point.received_at);
                let color = if Some(point.received_at) == self.last_scan_time {
                    age_color(age)
                } else {
                    historical_color(age)
                };
                ScatterPoint::new(point.x, point.y)
                    .size(point.size)
                    .color(color)
            })
            .collect();

        ScatterDataset::new(vec![
            ScatterSeries::new("mote.telemetry.lidar.returns", "Returns").with_points(points),
        ])
    }
}

#[derive(Default)]
pub(super) struct Context {
    history: LidarPlotHistory,
    viewport: ViewportState,
}

pub(super) fn render(
    ui: &mut UiRoot<'_>,
    context: &mut Context,
    ui_state: &state::Context,
    height: f32,
) {
    let telemetry = &ui_state.snapshot.latest_telemetry;
    if let (Some(scan), Some(received_at)) = (
        telemetry.lidar_scan.as_ref(),
        telemetry.lidar_scan_received_at,
    ) {
        context.history.push_scan(scan, received_at);
    }
    let dataset = context.history.dataset(Instant::now());
    let props = ScatterPlotProps::new(LIDAR_PLOT_ID)
        .title("LiDAR scan")
        .axis_labels("X (mm)", "Y (mm)")
        .height(height)
        .fill_width(true);

    ui.raw(|raw_ui| {
        ScatterPlot::show_with_viewport(raw_ui, &props, &dataset, &mut context.viewport);
        raw_ui.push_id("mote.telemetry.lidar.controls", |scoped_ui| {
            ScatterControlPanel::show(scoped_ui, &mut context.viewport, &dataset);
        });
        if dataset.is_empty() {
            raw_ui.label("Waiting for LiDAR scan…");
        }
    });
}

fn intensity_size(intensity: u8) -> f32 {
    let normalized = f32::from(intensity) / f32::from(u8::MAX);
    LIDAR_MIN_POINT_SIZE + normalized * (LIDAR_MAX_POINT_SIZE - LIDAR_MIN_POINT_SIZE)
}

fn point_alpha(age: Duration) -> u8 {
    let normalized_age = age.as_secs_f32() / LIDAR_POINT_FADE_SECONDS;
    (255.0 / (1.0 + normalized_age)).round() as u8
}

fn age_color(age: Duration) -> egui::Color32 {
    let normalized_age = (age.as_secs_f32() / LIDAR_COLOR_MAX_AGE_SECONDS).clamp(0.0, 1.0);
    let red = (normalized_age * 255.0).round() as u8;
    let blue = ((1.0 - normalized_age) * 255.0).round() as u8;
    egui::Color32::from_rgba_unmultiplied(red, 0, blue, point_alpha(age))
}

fn historical_color(age: Duration) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(0, 0, 0, point_alpha(age))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::LatestTelemetry;

    #[test]
    fn lidar_scan_pubsub_messages_update_the_shared_telemetry() {
        let mut scan = router::Scan::new();
        assert!(
            scan.push(router::ScanPoint {
                timestamp: units::Time::new(0.0),
                quality: 1,
                angle: units::PlaneAngle::new(0.0),
                distance: units::Length::new(1.0),
            })
            .is_ok()
        );

        let mut telemetry = LatestTelemetry::default();
        assert!(telemetry.update_router(router::Message::LidarScan(scan.clone())));
        let latest_scan = telemetry
            .lidar_scan
            .as_ref()
            .expect("LiDAR scan was stored");
        assert_eq!(latest_scan.len(), 1);
        assert_eq!(latest_scan[0].quality, scan[0].quality);
        assert!(telemetry.lidar_scan_received_at.is_some());
    }

    #[test]
    fn lidar_scan_points_are_added_to_the_scatter_dataset() {
        let scan = router::Scan::from_slice(&[
            router::ScanPoint {
                timestamp: units::Time::new(0.0),
                quality: 0,
                angle: units::PlaneAngle::new(0.0),
                distance: units::Length::new(1.0),
            },
            router::ScanPoint {
                timestamp: units::Time::new(0.0),
                quality: u8::MAX,
                angle: units::PlaneAngle::new(std::f32::consts::FRAC_PI_2),
                distance: units::Length::new(0.5),
            },
        ])
        .expect("scan fits in router::Scan");

        let mut history = LidarPlotHistory::default();
        let received_at = Instant::now();
        history.push_scan(&scan, received_at);
        let dataset = history.dataset(received_at);
        let points = &dataset.series[0].points;

        assert_eq!(points.len(), 2);
        assert_eq!(points[0].size, LIDAR_MIN_POINT_SIZE);
        assert_eq!(points[1].size, LIDAR_MAX_POINT_SIZE);
        assert_eq!(points[0].color, age_color(Duration::ZERO));
        let aged_dataset = history.dataset(received_at + Duration::from_secs(5));
        let aged_color = aged_dataset.series[0].points[0].color;
        assert_eq!(aged_color, age_color(Duration::from_secs(5)));
        assert_ne!(points[0].color, aged_color);
        assert!(aged_color.a() < points[0].color.a());
        let older_color = age_color(Duration::from_secs(10));
        assert!(older_color.a() < aged_color.a());
        assert_eq!(point_alpha(Duration::from_millis(500)), 128);
        assert!((points[0].x - 1_000.0).abs() < 1e-6);
        assert!(points[0].y.abs() < 1e-6);
        assert!(points[1].x.abs() < 1e-4);
        assert!((points[1].y - 500.0).abs() < 1e-4);
    }

    #[test]
    fn points_from_previous_scans_are_black_and_fade_faster() {
        let first_scan = router::Scan::from_slice(&[router::ScanPoint {
            timestamp: units::Time::new(0.0),
            quality: u8::MAX,
            angle: units::PlaneAngle::new(0.0),
            distance: units::Length::new(1.0),
        }])
        .expect("single point fits in router::Scan");
        let second_scan = router::Scan::from_slice(&[router::ScanPoint {
            timestamp: units::Time::new(0.0),
            quality: u8::MAX,
            angle: units::PlaneAngle::new(0.0),
            distance: units::Length::new(2.0),
        }])
        .expect("single point fits in router::Scan");
        let first_received_at = Instant::now();
        let latest_received_at = first_received_at + Duration::from_millis(500);

        let mut history = LidarPlotHistory::default();
        history.push_scan(&first_scan, first_received_at);
        history.push_scan(&second_scan, latest_received_at);

        let points = &history.dataset(latest_received_at).series[0].points;
        let previous_scan_color = points[0].color;
        let latest_scan_color = points[1].color;

        assert_eq!(previous_scan_color.r(), 0);
        assert_eq!(previous_scan_color.g(), 0);
        assert_eq!(previous_scan_color.b(), 0);
        assert_eq!(previous_scan_color.a(), 128);
        assert!(latest_scan_color.b() > latest_scan_color.r());
        assert!(previous_scan_color.a() < latest_scan_color.a());
    }

    #[test]
    fn lidar_scan_ignores_invalid_ranges_and_angles() {
        let scan = router::Scan::from_slice(&[
            router::ScanPoint {
                timestamp: units::Time::new(0.0),
                quality: 1,
                angle: units::PlaneAngle::new(f32::NAN),
                distance: units::Length::new(0.5),
            },
            router::ScanPoint {
                timestamp: units::Time::new(0.0),
                quality: 1,
                angle: units::PlaneAngle::new(0.0),
                distance: units::Length::new(0.0),
            },
            router::ScanPoint {
                timestamp: units::Time::new(0.0),
                quality: 1,
                angle: units::PlaneAngle::new(0.0),
                distance: units::Length::new(0.5),
            },
        ])
        .expect("scan fits in router::Scan");

        let mut history = LidarPlotHistory::default();
        history.push_scan(&scan, Instant::now());
        let dataset = history.dataset(Instant::now());

        assert_eq!(dataset.series[0].points.len(), 1);
    }

    #[test]
    fn lidar_history_keeps_the_latest_one_hundred_thousand_points() {
        let mut history = LidarPlotHistory::default();
        let start_time = Instant::now();

        for index in 1..=LIDAR_POINT_CAPACITY + 1 {
            let scan = router::Scan::from_slice(&[router::ScanPoint {
                timestamp: units::Time::new(0.0),
                quality: u8::MAX,
                angle: units::PlaneAngle::new(0.0),
                distance: units::Length::new(index as f32),
            }])
            .expect("single point fits in router::Scan");
            history.push_scan(&scan, start_time + Duration::from_secs(index as u64));
        }

        let dataset =
            history.dataset(start_time + Duration::from_secs((LIDAR_POINT_CAPACITY + 1) as u64));
        let points = &dataset.series[0].points;

        assert_eq!(points.len(), LIDAR_POINT_CAPACITY);
        assert_eq!(points[0].x, 2_000.0);
        assert_eq!(points[0].y, 0.0);
        assert_eq!(
            points[points.len() - 1].x,
            (LIDAR_POINT_CAPACITY + 1) as f64 * 1_000.0
        );
    }
}
