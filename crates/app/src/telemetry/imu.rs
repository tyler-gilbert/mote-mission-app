use std::{collections::VecDeque, time::Instant};

use bee_ui::{
    UiRoot,
    prelude::{PlotPoint, TimeSeries, TimeSeriesDataset},
};

use mote_api::messages::router;

use crate::state;

use super::render_time_series_plot;

const IMU_PLOT_ID: &str = "mote.telemetry.imu";
const IMU_POINT_CAPACITY: usize = 10_000;

const IMU_SERIES: [(&str, &str); 6] = [
    ("acceleration_x", "Acceleration X"),
    ("acceleration_y", "Acceleration Y"),
    ("acceleration_z", "Acceleration Z"),
    ("angular_acceleration_x", "Angular acceleration X"),
    ("angular_acceleration_y", "Angular acceleration Y"),
    ("angular_acceleration_z", "Angular acceleration Z"),
];

#[derive(Default)]
struct ImuPlotHistory {
    series: [VecDeque<PlotPoint>; 6],
    start_time: Option<Instant>,
    last_sample_time: Option<Instant>,
}

impl ImuPlotHistory {
    fn push_sample(&mut self, imu: &router::Imu, timestamp: Instant) {
        if self
            .last_sample_time
            .is_some_and(|last_sample_time| timestamp <= last_sample_time)
        {
            return;
        }

        let start_time = *self.start_time.get_or_insert(timestamp);
        let sample_time = timestamp.duration_since(start_time).as_secs_f64();
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

fn imu_values(imu: &router::Imu) -> [f64; 6] {
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

#[derive(Default)]
pub(super) struct Context {
    history: ImuPlotHistory,
    viewport: bee_ui::prelude::ViewportState,
}

pub(super) fn render(
    ui: &mut UiRoot<'_>,
    context: &mut Context,
    ui_state: &state::Context,
    height: f32,
) {
    let telemetry = &ui_state.snapshot.latest_telemetry;
    if let (Some(imu), Some(received_at)) = (telemetry.imu.as_ref(), telemetry.imu_received_at) {
        context.history.push_sample(imu, received_at);
    }
    let dataset = context.history.dataset();

    let waiting_for_data = dataset.is_empty();
    render_time_series_plot(
        ui,
        &mut context.viewport,
        IMU_PLOT_ID,
        &dataset,
        "Sensor reading",
        height,
    );
    if waiting_for_data {
        ui.raw(|raw_ui| raw_ui.label("Waiting for IMU telemetry…"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sample_imu(values: [f32; 6]) -> router::Imu {
        router::Imu {
            timestamp: units::Time::new(0.0),
            accel: router::Accel {
                x: units::Acceleration::new(values[0]),
                y: units::Acceleration::new(values[1]),
                z: units::Acceleration::new(values[2]),
            },
            gyro: router::Gyro {
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
        let start_time = Instant::now();
        history.push_sample(&first_sample, start_time);
        history.push_sample(&second_sample, start_time + Duration::from_millis(250));

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
        let start_time = Instant::now();
        for index in 0..=IMU_POINT_CAPACITY {
            let value = index as f32;
            history.push_sample(
                &sample_imu([value; 6]),
                start_time + Duration::from_secs(index as u64),
            );
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
}
