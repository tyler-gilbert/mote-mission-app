use bee_ui::{
    UiRoot,
    prelude::{
        AxisZoomMode, FollowMode, ScatterControlPanel, ScatterDataset, ScatterPlot,
        ScatterPlotProps, ScatterPoint, ScatterSeries, ViewportState,
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
const LIDAR_POINT_COLOR: egui::Color32 = egui::Color32::from_rgb(0, 0, 255);
const LIDAR_POINT_LIFETIME: Duration = Duration::from_secs(1);
const LIDAR_MIN_POINT_SIZE: f32 = 2.0;
const LIDAR_MAX_POINT_SIZE: f32 = 8.0;
const LIDAR_POINT_CAPACITY: usize = 10_000;
/// The reference view shows the local diagonal plus 20% margin.
const LIDAR_REFERENCE_VIEW_SCALE: f64 = 1.2;
/// Matches the 5% per-side padding used by the scatter plot's own fit modes.
const LIDAR_FIT_PADDING: f64 = 0.05;
const LIDAR_MIN_FIT_SPAN: f64 = 100.0;
const MILLIMETERS_PER_METER: f64 = 1_000.0;
const SPAN_TOLERANCE: f64 = 1e-6;

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

        for reading in scan.point_cloud.iter() {
            let angle: f32 = reading.angle.into();
            let distance: f32 = reading.distance.into();
            let angle = f64::from(angle);
            let distance = f64::from(distance) * MILLIMETERS_PER_METER;
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

    /// Drops points that have fully faded out. Points are stored in arrival order.
    fn prune(&mut self, now: Instant) {
        while self.points.front().is_some_and(|point| {
            now.saturating_duration_since(point.received_at) >= LIDAR_POINT_LIFETIME
        }) {
            self.points.pop_front();
        }
    }

    fn dataset(&self, now: Instant) -> ScatterDataset {
        let points = self
            .points
            .iter()
            .filter_map(|point| {
                let age = now.saturating_duration_since(point.received_at);
                (age < LIDAR_POINT_LIFETIME).then(|| {
                    ScatterPoint::new(point.x, point.y)
                        .size(point.size)
                        .color(faded_color(age))
                })
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
    reference_frame_hash: Option<u32>,
    axis_span: Option<f64>,
}

impl Context {
    /// Resets the view to a square around the sensor whenever a new reference frame is loaded.
    fn apply_reference_frame(&mut self, reference_frame: Option<&router::ReferenceFrame>) {
        let Some(reference_frame) = reference_frame else {
            return;
        };
        let hash = reference_frame.hash();
        if self.reference_frame_hash == Some(hash) {
            return;
        }
        self.reference_frame_hash = Some(hash);
        if let Some(span) = reference_view_span(reference_frame) {
            set_square_view(&mut self.viewport, (0.0, 0.0), span);
            self.axis_span = Some(span);
        }
    }

    /// Keeps the X and Y spans identical. Fit modes act as a one-shot square fit to the data.
    fn constrain_viewport(&mut self, dataset: &ScatterDataset) {
        // Equal spans are enforced here; egui's data aspect lock would fight them.
        self.viewport.lock_aspect_ratio = false;
        if (self.viewport.x_mode != AxisZoomMode::Manual
            || self.viewport.y_mode != AxisZoomMode::Manual)
            && let Some(bounds) = dataset_bounds(dataset)
        {
            fit_square_to_bounds(&mut self.viewport, bounds);
            self.axis_span = Some(self.viewport.x_span());
        }
        self.axis_span = Some(equalize_axis_spans(&mut self.viewport, self.axis_span));
    }
}

pub(super) fn render(
    ui: &mut UiRoot<'_>,
    context: &mut Context,
    ui_state: &state::Context,
    reference_frame: Option<&router::ReferenceFrame>,
    height: f32,
) {
    let telemetry = &ui_state.snapshot.latest_telemetry;
    if let (Some(scan), Some(received_at)) = (
        telemetry.lidar_scan.as_ref(),
        telemetry.lidar_scan_received_at,
    ) {
        context.history.push_scan(scan, received_at);
    }
    let now = Instant::now();
    context.history.prune(now);
    let dataset = context.history.dataset(now);
    context.apply_reference_frame(reference_frame);
    context.constrain_viewport(&dataset);

    // A square plot keeps equal X and Y spans geometrically undistorted.
    let side = ui.raw(|raw_ui| raw_ui.available_width()).min(height);
    let props = ScatterPlotProps::new(LIDAR_PLOT_ID)
        .axis_labels("X (mm)", "Y (mm)")
        .size(side, side);

    ui.raw(|raw_ui| {
        ScatterPlot::show_with_viewport(raw_ui, &props, &dataset, &mut context.viewport);
        raw_ui.push_id("mote.telemetry.lidar.controls", |scoped_ui| {
            ScatterControlPanel::show(scoped_ui, &mut context.viewport, &dataset);
        });
        if dataset.is_empty() {
            raw_ui.label("Waiting for LiDAR scan…");
        }
    });
    context.constrain_viewport(&dataset);
}

fn intensity_size(intensity: u8) -> f32 {
    let normalized = f32::from(intensity) / f32::from(u8::MAX);
    LIDAR_MIN_POINT_SIZE + normalized * (LIDAR_MAX_POINT_SIZE - LIDAR_MIN_POINT_SIZE)
}

fn point_alpha(age: Duration) -> u8 {
    let remaining = 1.0 - age.as_secs_f32() / LIDAR_POINT_LIFETIME.as_secs_f32();
    (remaining.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn faded_color(age: Duration) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        LIDAR_POINT_COLOR.r(),
        LIDAR_POINT_COLOR.g(),
        LIDAR_POINT_COLOR.b(),
        point_alpha(age),
    )
}

/// Side length (mm) of a square view whose diagonal is the local diagonal plus 20%.
fn reference_view_span(reference_frame: &router::ReferenceFrame) -> Option<f64> {
    let diagonal: f32 = reference_frame.local_diagonal.into();
    let diagonal = f64::from(diagonal);
    (diagonal.is_finite() && diagonal > 0.0).then(|| {
        diagonal * MILLIMETERS_PER_METER * LIDAR_REFERENCE_VIEW_SCALE / std::f64::consts::SQRT_2
    })
}

fn set_square_view(viewport: &mut ViewportState, center: (f64, f64), span: f64) {
    viewport.x_mode = AxisZoomMode::Manual;
    viewport.y_mode = AxisZoomMode::Manual;
    viewport.follow = FollowMode::Free;
    set_axis_spans(viewport, center, span);
}

fn set_axis_spans(viewport: &mut ViewportState, (x_center, y_center): (f64, f64), span: f64) {
    let half_span = span * 0.5;
    viewport.x_min = x_center - half_span;
    viewport.x_max = x_center + half_span;
    viewport.y_min = y_center - half_span;
    viewport.y_max = y_center + half_span;
    viewport.x_window_size = span;
}

fn fit_square_to_bounds(viewport: &mut ViewportState, (x_min, x_max, y_min, y_max): Bounds) {
    let span = (x_max - x_min).max(y_max - y_min) * (1.0 + 2.0 * LIDAR_FIT_PADDING);
    let center = ((x_min + x_max) * 0.5, (y_min + y_max) * 0.5);
    set_square_view(viewport, center, span.max(LIDAR_MIN_FIT_SPAN));
}

/// Makes both axes use the same span, following whichever axis the user changed last.
fn equalize_axis_spans(viewport: &mut ViewportState, previous_span: Option<f64>) -> f64 {
    let (x_min, x_max, y_min, y_max) = viewport.normalized_bounds();
    let x_span = x_max - x_min;
    let y_span = y_max - y_min;
    let span = match previous_span {
        Some(previous) if spans_match(x_span, previous) => y_span,
        Some(previous) if spans_match(y_span, previous) => x_span,
        _ => x_span.max(y_span),
    };
    if !spans_match(x_span, span) || !spans_match(y_span, span) {
        set_axis_spans(
            viewport,
            ((x_min + x_max) * 0.5, (y_min + y_max) * 0.5),
            span,
        );
    }
    span
}

fn spans_match(a: f64, b: f64) -> bool {
    (a - b).abs() <= SPAN_TOLERANCE * a.abs().max(b.abs()).max(1.0)
}

type Bounds = (f64, f64, f64, f64);

fn dataset_bounds(dataset: &ScatterDataset) -> Option<Bounds> {
    dataset
        .series
        .iter()
        .flat_map(|series| &series.points)
        .filter(|point| point.x.is_finite() && point.y.is_finite())
        .fold(None, |bounds, point| {
            let (x_min, x_max, y_min, y_max) =
                bounds.unwrap_or((point.x, point.x, point.y, point.y));
            Some((
                x_min.min(point.x),
                x_max.max(point.x),
                y_min.min(point.y),
                y_max.max(point.y),
            ))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::LatestTelemetry;

    fn scan(points: &[router::ScanPoint]) -> router::Scan {
        router::Scan {
            timestamp: units::Time::new(0.0),
            point_cloud: router::PointCloud::from_slice(points).expect("scan fits in router::Scan"),
        }
    }

    fn reference_frame(local_diagonal: f32) -> router::ReferenceFrame {
        router::ReferenceFrame {
            local_diagonal: units::Length::new(local_diagonal),
            ..router::ReferenceFrame::default()
        }
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn lidar_scan_pubsub_messages_update_the_shared_telemetry() {
        let scan = scan(&[router::ScanPoint {
            quality: 1,
            angle: units::PlaneAngle::new(0.0),
            distance: units::Length::new(1.0),
        }]);

        let mut telemetry = LatestTelemetry::default();
        assert!(telemetry.update_router(router::Message::LidarScan(scan.clone())));
        let latest_scan = telemetry
            .lidar_scan
            .as_ref()
            .expect("LiDAR scan was stored");
        assert_eq!(latest_scan.point_cloud.len(), 1);
        assert_eq!(
            latest_scan.point_cloud[0].quality,
            scan.point_cloud[0].quality
        );
        assert!(telemetry.lidar_scan_received_at.is_some());
    }

    #[test]
    fn lidar_scan_points_are_added_to_the_scatter_dataset() {
        let scan = scan(&[
            router::ScanPoint {
                quality: 0,
                angle: units::PlaneAngle::new(0.0),
                distance: units::Length::new(1.0),
            },
            router::ScanPoint {
                quality: u8::MAX,
                angle: units::PlaneAngle::new(std::f32::consts::FRAC_PI_2),
                distance: units::Length::new(0.5),
            },
        ]);

        let mut history = LidarPlotHistory::default();
        let received_at = Instant::now();
        history.push_scan(&scan, received_at);
        let dataset = history.dataset(received_at);
        let points = &dataset.series[0].points;

        assert_eq!(points.len(), 2);
        assert_eq!(points[0].size, LIDAR_MIN_POINT_SIZE);
        assert_eq!(points[1].size, LIDAR_MAX_POINT_SIZE);
        assert_eq!(points[0].color, LIDAR_POINT_COLOR);
        assert!((points[0].x - 1_000.0).abs() < 1e-6);
        assert!(points[0].y.abs() < 1e-6);
        assert!(points[1].x.abs() < 1e-4);
        assert!((points[1].y - 500.0).abs() < 1e-4);
    }

    #[test]
    fn points_fade_in_the_same_color_and_disappear_within_one_second() {
        let first_scan = scan(&[router::ScanPoint {
            quality: u8::MAX,
            angle: units::PlaneAngle::new(0.0),
            distance: units::Length::new(1.0),
        }]);
        let second_scan = scan(&[router::ScanPoint {
            quality: u8::MAX,
            angle: units::PlaneAngle::new(0.0),
            distance: units::Length::new(2.0),
        }]);
        let first_received_at = Instant::now();
        let latest_received_at = first_received_at + Duration::from_millis(500);

        let mut history = LidarPlotHistory::default();
        history.push_scan(&first_scan, first_received_at);
        history.push_scan(&second_scan, latest_received_at);

        let points = &history.dataset(latest_received_at).series[0].points;
        let previous_scan_color = points[0].color;
        let latest_scan_color = points[1].color;
        assert_eq!(latest_scan_color, LIDAR_POINT_COLOR);
        assert_eq!(
            previous_scan_color,
            egui::Color32::from_rgba_unmultiplied(0, 0, 255, 128)
        );

        let expired_at = first_received_at + LIDAR_POINT_LIFETIME;
        assert_eq!(point_alpha(LIDAR_POINT_LIFETIME), 0);
        assert_eq!(history.dataset(expired_at).series[0].points.len(), 1);
        history.prune(expired_at);
        assert_eq!(history.points.len(), 1);
        history.prune(latest_received_at + LIDAR_POINT_LIFETIME);
        assert!(history.points.is_empty());
    }

    #[test]
    fn lidar_scan_ignores_invalid_ranges_and_angles() {
        let scan = scan(&[
            router::ScanPoint {
                quality: 1,
                angle: units::PlaneAngle::new(f32::NAN),
                distance: units::Length::new(0.5),
            },
            router::ScanPoint {
                quality: 1,
                angle: units::PlaneAngle::new(0.0),
                distance: units::Length::new(0.0),
            },
            router::ScanPoint {
                quality: 1,
                angle: units::PlaneAngle::new(0.0),
                distance: units::Length::new(0.5),
            },
        ]);

        let mut history = LidarPlotHistory::default();
        let received_at = Instant::now();
        history.push_scan(&scan, received_at);
        let dataset = history.dataset(received_at);

        assert_eq!(dataset.series[0].points.len(), 1);
    }

    #[test]
    fn lidar_history_keeps_the_latest_points_up_to_capacity() {
        let mut history = LidarPlotHistory::default();
        let start_time = Instant::now();

        for index in 1..=LIDAR_POINT_CAPACITY + 1 {
            let scan = scan(&[router::ScanPoint {
                quality: u8::MAX,
                angle: units::PlaneAngle::new(0.0),
                distance: units::Length::new(index as f32),
            }]);
            history.push_scan(&scan, start_time + Duration::from_micros(index as u64));
        }

        let dataset =
            history.dataset(start_time + Duration::from_micros((LIDAR_POINT_CAPACITY + 1) as u64));
        let points = &dataset.series[0].points;

        assert_eq!(points.len(), LIDAR_POINT_CAPACITY);
        assert_eq!(points[0].x, 2_000.0);
        assert_eq!(points[0].y, 0.0);
        assert_eq!(
            points[points.len() - 1].x,
            (LIDAR_POINT_CAPACITY + 1) as f64 * 1_000.0
        );
    }

    #[test]
    fn reference_frame_update_sets_square_view_from_local_diagonal() {
        let mut context = Context::default();
        context.apply_reference_frame(Some(&reference_frame(3.0)));

        // 3 m diagonal + 20% = 3.6 m diagonal, so each side is 3600 / sqrt(2) mm.
        let span = 3_600.0 / std::f64::consts::SQRT_2;
        let viewport = context.viewport;
        assert_eq!(viewport.x_mode, AxisZoomMode::Manual);
        assert_eq!(viewport.y_mode, AxisZoomMode::Manual);
        assert_close(viewport.x_min, -span / 2.0);
        assert_close(viewport.x_max, span / 2.0);
        assert_close(viewport.y_min, -span / 2.0);
        assert_close(viewport.y_max, span / 2.0);
        assert_close(viewport.x_span().hypot(viewport.y_span()), 3_600.0);

        // The same frame does not reset a user-adjusted view; a new frame does.
        set_axis_spans(&mut context.viewport, (0.0, 0.0), 500.0);
        context.apply_reference_frame(Some(&reference_frame(3.0)));
        assert_close(context.viewport.x_span(), 500.0);
        context.apply_reference_frame(Some(&reference_frame(5.0)));
        assert_close(
            context.viewport.x_span(),
            6_000.0 / std::f64::consts::SQRT_2,
        );
    }

    #[test]
    fn axis_spans_follow_the_axis_that_changed() {
        let mut viewport = ViewportState::default();
        set_square_view(&mut viewport, (0.0, 0.0), 1_000.0);

        viewport.y_min = -1_000.0;
        viewport.y_max = 1_000.0;
        let span = equalize_axis_spans(&mut viewport, Some(1_000.0));
        assert_close(span, 2_000.0);
        assert_close(viewport.x_span(), 2_000.0);
        assert_close(viewport.y_span(), 2_000.0);

        viewport.x_min = 0.0;
        viewport.x_max = 500.0;
        let span = equalize_axis_spans(&mut viewport, Some(2_000.0));
        assert_close(span, 500.0);
        assert_close(viewport.y_span(), 500.0);
        assert_close(viewport.y_min, -250.0);
        assert_close(viewport.x_min, 0.0);
    }

    #[test]
    fn fit_modes_fit_a_square_around_the_data_and_return_to_manual() {
        let dataset =
            ScatterDataset::new(vec![ScatterSeries::new("returns", "Returns").with_points(
                vec![
                    ScatterPoint::new(-1_000.0, 0.0),
                    ScatterPoint::new(1_000.0, 500.0),
                ],
            )]);
        let mut context = Context::default();
        context.constrain_viewport(&dataset);

        let viewport = context.viewport;
        assert_eq!(viewport.x_mode, AxisZoomMode::Manual);
        assert_eq!(viewport.y_mode, AxisZoomMode::Manual);
        assert_close(viewport.x_span(), 2_200.0);
        assert_close(viewport.y_span(), 2_200.0);
        assert_close(viewport.y_min + viewport.y_max, 500.0);
    }
}
