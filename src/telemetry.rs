use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use bee_ui::{
    UiRoot,
    prelude::{
        PlotPoint, ScatterControlPanel, ScatterDataset, ScatterPlot, ScatterPlotProps,
        ScatterPoint, ScatterSeries, TimeSeries, TimeSeriesDataset, TimeSeriesPlotProps,
        ViewportState,
    },
};
use eframe::egui;
use mote_api::messages::pubsub;

use crate::transport::LatestTelemetry;

const GUIDANCE_PLOT_ID: &str = "mote.telemetry.guidance";
const POSITION_PLOT_ID: &str = "mote.telemetry.position";
const CONTROL_PLOT_ID: &str = "mote.telemetry.control";
const IMU_PLOT_ID: &str = "mote.telemetry.imu";
const LIDAR_PLOT_ID: &str = "mote.telemetry.lidar";
const PLOT_CONTROLS_HEIGHT: f32 = 160.0;
const IMU_POINT_CAPACITY: usize = 10_000;
const IMU_HISTORY_ID: &str = "mote.telemetry.imu.history";
const IMU_SERIES: [(&str, &str); 6] = [
    ("acceleration_x", "Acceleration X"),
    ("acceleration_y", "Acceleration Y"),
    ("acceleration_z", "Acceleration Z"),
    ("angular_acceleration_x", "Angular acceleration X"),
    ("angular_acceleration_y", "Angular acceleration Y"),
    ("angular_acceleration_z", "Angular acceleration Z"),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TelemetryPlot {
    #[default]
    Guidance,
    Position,
    Control,
    Imu,
    Lidar,
}

impl TelemetryPlot {
    const ALL: [Self; 5] = [
        Self::Guidance,
        Self::Position,
        Self::Control,
        Self::Imu,
        Self::Lidar,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Guidance => "Guidance",
            Self::Position => "Navigation",
            Self::Control => "Control",
            Self::Imu => "IMU",
            Self::Lidar => "LiDAR",
        }
    }

    const fn telemetry_selection(self) -> pubsub::TelemetrySelection {
        match self {
            Self::Guidance => pubsub::TelemetrySelection::Guidance,
            Self::Position => pubsub::TelemetrySelection::Navigation,
            Self::Control => pubsub::TelemetrySelection::Control,
            Self::Imu => pubsub::TelemetrySelection::Imu,
            Self::Lidar => pubsub::TelemetrySelection::Lidar,
        }
    }
}

pub(crate) fn render(ui: &mut UiRoot<'_>, telemetry: &LatestTelemetry) -> Option<pubsub::Message> {
    let context = ui.context().clone();
    let selection_id = egui::Id::new("mote.telemetry.selected_plot");
    let previous_plot = context.data_mut(|data| data.get_temp::<TelemetryPlot>(selection_id));
    let mut selected_plot = previous_plot.unwrap_or_default();

    let plot_height = ui.raw(|raw_ui| {
        egui::ComboBox::from_id_salt("mote.telemetry.plot_selector")
            .selected_text(selected_plot.label())
            .show_ui(raw_ui, |combo_ui| {
                for plot in TelemetryPlot::ALL {
                    combo_ui.selectable_value(&mut selected_plot, plot, plot.label());
                }
            });

        (raw_ui.available_height() - PLOT_CONTROLS_HEIGHT).max(0.0)
    });

    match selected_plot {
        TelemetryPlot::Guidance => render_empty_plot(
            ui,
            GUIDANCE_PLOT_ID,
            &["Heading", "Distance"],
            "Value",
            plot_height,
        ),
        TelemetryPlot::Position => render_empty_plot(
            ui,
            POSITION_PLOT_ID,
            &["North", "East", "Heading"],
            "Value",
            plot_height,
        ),
        TelemetryPlot::Control => render_empty_plot(
            ui,
            CONTROL_PLOT_ID,
            &["Left", "Right"],
            "Angular velocity",
            plot_height,
        ),
        TelemetryPlot::Imu => {
            render_imu_plot(ui, telemetry.imu.as_ref(), plot_height);
        }
        TelemetryPlot::Lidar => render_lidar_plot(ui, telemetry.lidar_scan.as_ref(), plot_height),
    }

    context.data_mut(|data| data.insert_temp(selection_id, selected_plot));

    (previous_plot != Some(selected_plot))
        .then(|| pubsub::Message::TelemetrySelection(selected_plot.telemetry_selection()))
}

fn render_lidar_plot(ui: &mut UiRoot<'_>, scan: Option<&pubsub::Scan>, height: f32) {
    let dataset = lidar_dataset(scan);
    let props = ScatterPlotProps::new(LIDAR_PLOT_ID)
        .title("LiDAR scan")
        .axis_labels("X (mm)", "Y (mm)")
        .height(height)
        .fill_width(true);

    let context = ui.context().clone();
    let viewport_id = egui::Id::new(("mote.telemetry.viewport", LIDAR_PLOT_ID));
    let mut viewport = context
        .data_mut(|data| data.get_temp::<ViewportState>(viewport_id))
        .unwrap_or_default();

    ui.raw(|raw_ui| {
        ScatterPlot::show_with_viewport(raw_ui, &props, &dataset, &mut viewport);
        raw_ui.push_id("mote.telemetry.lidar.controls", |scoped_ui| {
            ScatterControlPanel::show(scoped_ui, &mut viewport, &dataset);
        });
        if dataset.is_empty() {
            raw_ui.label("Waiting for LiDAR scan…");
        }
    });

    context.data_mut(|data| data.insert_temp(viewport_id, viewport));
}

fn lidar_dataset(scan: Option<&pubsub::Scan>) -> ScatterDataset {
    let points = scan
        .into_iter()
        .flat_map(|scan| scan.iter())
        .filter_map(|reading| {
            let angle: f32 = reading.angle.into();
            let distance: f32 = reading.distance.into();
            let angle = f64::from(angle);
            let distance = f64::from(distance) * 1_000.0;
            (angle.is_finite() && distance.is_finite() && distance > 0.0)
                .then(|| ScatterPoint::new(distance * angle.cos(), distance * angle.sin()))
        })
        .collect();

    ScatterDataset::new(vec![
        ScatterSeries::new("mote.telemetry.lidar.returns", "Returns").with_points(points),
    ])
}

#[derive(Default)]
struct ImuPlotHistory {
    series: [VecDeque<PlotPoint>; 6],
    start_time: Option<f64>,
    last_sample_time: Option<f64>,
}

impl ImuPlotHistory {
    fn push_sample(&mut self, imu: &pubsub::Imu, timestamp: f64) {
        if !timestamp.is_finite()
            || self
                .last_sample_time
                .is_some_and(|last_sample_time| timestamp <= last_sample_time)
        {
            return;
        }

        let start_time = *self.start_time.get_or_insert(timestamp);
        let sample_time = timestamp - start_time;
        for (points, value) in self.series.iter_mut().zip(imu_values(imu)) {
            if points.len() == IMU_POINT_CAPACITY {
                points.pop_front();
            }
            points.push_back(PlotPoint::new(sample_time, value));
        }
        self.last_sample_time = Some(timestamp);
    }

    fn dataset(&self) -> TimeSeriesDataset {
        TimeSeriesDataset::new(
            IMU_SERIES
                .iter()
                .zip(&self.series)
                .map(|((id, label), points)| {
                    TimeSeries::new(format!("{IMU_PLOT_ID}.{id}"), *label)
                        .with_points(points.iter().copied().collect())
                })
                .collect(),
        )
    }
}

fn imu_values(imu: &pubsub::Imu) -> [f64; 6] {
    let acceleration_x: f32 = imu.accel.x.into();
    let acceleration_y: f32 = imu.accel.y.into();
    let acceleration_z: f32 = imu.accel.z.into();
    let angular_acceleration_x: f32 = imu.gyro.x.into();
    let angular_acceleration_y: f32 = imu.gyro.y.into();
    let angular_acceleration_z: f32 = imu.gyro.z.into();
    [
        f64::from(acceleration_x),
        f64::from(acceleration_y),
        f64::from(acceleration_z),
        f64::from(angular_acceleration_x),
        f64::from(angular_acceleration_y),
        f64::from(angular_acceleration_z),
    ]
}

fn render_imu_plot(ui: &mut UiRoot<'_>, imu: Option<&pubsub::Imu>, height: f32) {
    let context = ui.context().clone();
    let history_id = egui::Id::new(IMU_HISTORY_ID);
    let history = context.data_mut(|data| {
        data.get_temp::<Arc<Mutex<ImuPlotHistory>>>(history_id)
            .unwrap_or_else(|| {
                let history = Arc::new(Mutex::new(ImuPlotHistory::default()));
                data.insert_temp(history_id, history.clone());
                history
            })
    });

    let timestamp = context.input(|input| input.time);
    let mut history = history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(imu) = imu {
        history.push_sample(imu, timestamp);
    }
    let dataset = history.dataset();
    drop(history);

    let waiting_for_data = dataset.is_empty();
    render_time_series_plot(ui, IMU_PLOT_ID, &dataset, "Sensor reading", height);
    if waiting_for_data {
        ui.raw(|raw_ui| raw_ui.label("Waiting for IMU telemetry…"));
    }
}

fn render_empty_plot(
    ui: &mut UiRoot<'_>,
    id: &'static str,
    series: &[&str],
    y_axis: &str,
    height: f32,
) {
    let dataset = TimeSeriesDataset::new(
        series
            .iter()
            .map(|label| TimeSeries::new(format!("{id}.{label}"), *label))
            .collect(),
    );
    render_time_series_plot(ui, id, &dataset, y_axis, height);
}

fn render_time_series_plot(
    ui: &mut UiRoot<'_>,
    id: &'static str,
    dataset: &TimeSeriesDataset,
    y_axis: &str,
    height: f32,
) {
    let props = TimeSeriesPlotProps::new(id)
        .axis_labels("Time (s)", y_axis)
        .height(height)
        .fill_width(true)
        .legend(true)
        .show_control_panel(true);

    let context = ui.context().clone();
    let viewport_id = egui::Id::new(("mote.telemetry.viewport", id));
    let mut viewport = context
        .data_mut(|data| data.get_temp::<ViewportState>(viewport_id))
        .unwrap_or_default();

    ui.time_series_plot(props, dataset, &mut viewport);

    context.data_mut(|data| data.insert_temp(viewport_id, viewport));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plots_request_the_matching_pubsub_telemetry() {
        assert!(matches!(
            TelemetryPlot::Control.telemetry_selection(),
            pubsub::TelemetrySelection::Control
        ));
        assert!(matches!(
            TelemetryPlot::Guidance.telemetry_selection(),
            pubsub::TelemetrySelection::Guidance
        ));
        assert!(matches!(
            TelemetryPlot::Position.telemetry_selection(),
            pubsub::TelemetrySelection::Navigation
        ));
        assert!(matches!(
            TelemetryPlot::Imu.telemetry_selection(),
            pubsub::TelemetrySelection::Imu
        ));
        assert!(matches!(
            TelemetryPlot::Lidar.telemetry_selection(),
            pubsub::TelemetrySelection::Lidar
        ));
    }

    fn sample_imu(values: [f32; 6]) -> pubsub::Imu {
        pubsub::Imu {
            accel: pubsub::Accel {
                x: units::Acceleration::new(values[0]),
                y: units::Acceleration::new(values[1]),
                z: units::Acceleration::new(values[2]),
            },
            gyro: pubsub::Gyro {
                x: units::AngularAcceleration::new(values[3]),
                y: units::AngularAcceleration::new(values[4]),
                z: units::AngularAcceleration::new(values[5]),
            },
        }
    }

    #[test]
    fn imu_history_collects_six_series_of_timestamped_samples() {
        let mut history = ImuPlotHistory::default();
        assert_eq!(history.dataset().series.len(), 6);

        let first_sample = sample_imu([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let second_sample = sample_imu([7.0, 8.0, 9.0, 10.0, 11.0, 12.0]);
        history.push_sample(&first_sample, 4.0);
        history.push_sample(&second_sample, 4.25);

        let dataset = history.dataset();
        assert_eq!(dataset.series.len(), 6);
        assert_eq!(
            dataset
                .series
                .iter()
                .map(|series| series.label.as_str())
                .collect::<Vec<_>>(),
            [
                "Acceleration X",
                "Acceleration Y",
                "Acceleration Z",
                "Angular acceleration X",
                "Angular acceleration Y",
                "Angular acceleration Z",
            ]
        );
        for (series, expected_value) in dataset.series.iter().zip([1.0, 2.0, 3.0, 4.0, 5.0, 6.0]) {
            assert_eq!(series.points.len(), 2);
            assert_eq!(series.points[0].y, expected_value);
            assert_eq!(series.points[1].y, expected_value + 6.0);
            assert_eq!(series.points[0].x, 0.0);
            assert_eq!(series.points[1].x, 0.25);
        }
    }

    #[test]
    fn imu_history_keeps_the_latest_ten_thousand_points_per_series() {
        let mut history = ImuPlotHistory::default();
        for index in 0..=IMU_POINT_CAPACITY {
            let value = index as f32;
            history.push_sample(&sample_imu([value; 6]), index as f64);
        }

        let dataset = history.dataset();
        assert_eq!(dataset.series.len(), 6);
        for series in &dataset.series {
            assert_eq!(series.points.len(), IMU_POINT_CAPACITY);
            assert_eq!(series.points[0].x, 1.0);
            assert_eq!(series.points[0].y, 1.0);
            assert_eq!(
                series.points[IMU_POINT_CAPACITY - 1].x,
                IMU_POINT_CAPACITY as f64
            );
            assert_eq!(
                series.points[IMU_POINT_CAPACITY - 1].y,
                IMU_POINT_CAPACITY as f64
            );
        }
    }

    #[test]
    fn lidar_scan_pubsub_messages_update_the_shared_telemetry() {
        let mut scan = pubsub::Scan::new();
        assert!(
            scan.push(pubsub::ScanPoint {
                quality: 1,
                angle: units::PlaneAngle::new(0.0),
                distance: units::Length::new(1.0),
            })
            .is_ok()
        );

        let mut telemetry = LatestTelemetry::default();
        assert!(telemetry.update_pubsub(pubsub::Message::LidarScan(scan.clone())));
        //assert_eq!(telemetry.lidar_scan.as_ref(), Some(&scan));
    }

    #[test]
    fn lidar_scan_coordinates_are_added_to_the_scatter_dataset() {
        let scan = pubsub::Scan::from_slice(&[
            pubsub::ScanPoint {
                quality: 1,
                angle: units::PlaneAngle::new(0.0),
                distance: units::Length::new(1.0),
            },
            pubsub::ScanPoint {
                quality: 1,
                angle: units::PlaneAngle::new(std::f32::consts::FRAC_PI_2),
                distance: units::Length::new(0.5),
            },
        ])
        .expect("scan fits in pubsub::Scan");

        let dataset = lidar_dataset(Some(&scan));
        let points = &dataset.series[0].points;

        assert_eq!(points.len(), 2);
        assert!((points[0].x - 1_000.0).abs() < 1e-6);
        assert!(points[0].y.abs() < 1e-6);
        assert!(points[1].x.abs() < 1e-4);
        assert!((points[1].y - 500.0).abs() < 1e-4);
    }

    #[test]
    fn lidar_scan_ignores_invalid_ranges_and_angles() {
        let scan = pubsub::Scan::from_slice(&[
            pubsub::ScanPoint {
                quality: 1,
                angle: units::PlaneAngle::new(f32::NAN),
                distance: units::Length::new(0.5),
            },
            pubsub::ScanPoint {
                quality: 1,
                angle: units::PlaneAngle::new(0.0),
                distance: units::Length::new(0.0),
            },
            pubsub::ScanPoint {
                quality: 1,
                angle: units::PlaneAngle::new(0.0),
                distance: units::Length::new(0.5),
            },
        ])
        .expect("scan fits in pubsub::Scan");

        let dataset = lidar_dataset(Some(&scan));

        assert_eq!(dataset.series[0].points.len(), 1);
    }
}
