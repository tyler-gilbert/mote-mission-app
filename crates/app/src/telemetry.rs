use std::{collections::VecDeque, time::Instant};

use bee_ui::{
    UiRoot,
    prelude::{
        DisclosureState, Dropdown, DropdownItem, PlotPoint, TimeSeries, TimeSeriesDataset,
        TimeSeriesPlotProps, ViewportState,
    },
};

mod imu;
mod lidar;

use crate::state;

const GUIDANCE_PLOT_ID: &str = "mote.telemetry.guidance";
const POSITION_PLOT_ID: &str = "mote.telemetry.position";
const CONTROL_PLOT_ID: &str = "mote.telemetry.control";

const PLOT_CONTROLS_HEIGHT: f32 = 160.0;

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
}

pub(crate) struct Context {
    selected_plot: Option<TelemetryPlot>,
    guidance: LivePlotContext,
    position: LivePlotContext,
    control: LivePlotContext,
    imu: imu::Context,
    lidar: lidar::Context,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            selected_plot: None,
            guidance: LivePlotContext::new(2),
            position: LivePlotContext::new(3),
            control: LivePlotContext::new(2),
            imu: imu::Context::default(),
            lidar: lidar::Context::default(),
        }
    }
}

pub(crate) fn render(ui: &mut UiRoot<'_>, context: &mut Context, ui_state: &mut state::Context) {
    let mut selected_plot = context.selected_plot.unwrap_or_default();

    let plot_selector =
        TelemetryPlot::ALL
            .iter()
            .fold(Dropdown::new(selected_plot.label()), |dropdown, plot| {
                dropdown.item(DropdownItem::new(plot.label()).active(*plot == selected_plot))
            });
    let clicked_index = ui
        .dropdown(
            DisclosureState::uncontrolled("mote.telemetry.plot_selector", false),
            plot_selector,
        )
        .clicked_index();

    if let Some(index) = clicked_index {
        selected_plot = TelemetryPlot::ALL[index];
    }

    let telemetry = &ui_state.snapshot.latest_telemetry;
    let plot_height = ui.raw(|raw_ui| (raw_ui.available_height() - PLOT_CONTROLS_HEIGHT).max(0.0));

    match selected_plot {
        TelemetryPlot::Guidance => render_live_plot(
            ui,
            &mut context.guidance,
            GUIDANCE_PLOT_ID,
            &[("heading", "Heading"), ("distance", "Distance")],
            telemetry.guidance.as_ref().map(|value| {
                [
                    quantity_value(value.heading),
                    quantity_value(value.distance),
                ]
            }),
            telemetry.guidance_received_at,
            "Guidance",
            "Value",
            plot_height,
        ),
        TelemetryPlot::Position => render_live_plot(
            ui,
            &mut context.position,
            POSITION_PLOT_ID,
            &[("north", "North"), ("east", "East"), ("heading", "Heading")],
            telemetry.position.as_ref().map(|value| {
                [
                    quantity_value(value.coordinate.latitude),
                    quantity_value(value.coordinate.longitude),
                    quantity_value(value.heading),
                ]
            }),
            telemetry.position_received_at,
            "Navigation",
            "Value",
            plot_height,
        ),
        TelemetryPlot::Control => render_live_plot(
            ui,
            &mut context.control,
            CONTROL_PLOT_ID,
            &[("left", "Left"), ("right", "Right")],
            telemetry.control.as_ref().map(|value| {
                [
                    quantity_value(value.motor_drive.left),
                    quantity_value(value.motor_drive.right),
                ]
            }),
            telemetry.control_received_at,
            "Control",
            "Angular velocity",
            plot_height,
        ),
        TelemetryPlot::Imu => imu::render(ui, &mut context.imu, ui_state, plot_height),
        TelemetryPlot::Lidar => {
            lidar::render(ui, &mut context.lidar, ui_state, plot_height);
        }
    }

    context.selected_plot = Some(selected_plot);
}

const PLOT_POINT_CAPACITY: usize = 10_000;

#[derive(Default)]
struct LivePlotContext {
    history: LivePlotHistory,
    viewport: ViewportState,
}

impl LivePlotContext {
    fn new(series_count: usize) -> Self {
        Self {
            history: LivePlotHistory::new(series_count),
            ..Self::default()
        }
    }
}

#[derive(Default)]
struct LivePlotHistory {
    series: Vec<VecDeque<PlotPoint>>,
    start_time: Option<Instant>,
    last_sample_time: Option<Instant>,
}

impl LivePlotHistory {
    fn new(series_count: usize) -> Self {
        Self {
            series: (0..series_count).map(|_| VecDeque::new()).collect(),
            ..Self::default()
        }
    }

    fn push_sample(&mut self, timestamp: Instant, values: &[f64]) {
        if self.series.len() != values.len()
            || self
                .last_sample_time
                .is_some_and(|last_sample_time| timestamp <= last_sample_time)
        {
            return;
        }

        let start_time = *self.start_time.get_or_insert(timestamp);
        let sample_time = timestamp.duration_since(start_time).as_secs_f64();
        for (points, value) in self.series.iter_mut().zip(values) {
            if !value.is_finite() {
                continue;
            }
            if points.len() == PLOT_POINT_CAPACITY {
                points.pop_front();
            }
            points.push_back(PlotPoint::new(sample_time, *value));
        }
        self.last_sample_time = Some(timestamp);
    }

    fn dataset(&self, id: &str, series: &[(&str, &str)]) -> TimeSeriesDataset {
        TimeSeriesDataset::new(
            series
                .iter()
                .zip(&self.series)
                .map(|((series_id, label), points)| {
                    TimeSeries::new(format!("{id}.{series_id}"), *label)
                        .with_points(points.iter().copied().collect())
                })
                .collect(),
        )
    }
}

fn quantity_value(quantity: impl Into<f32>) -> f64 {
    f64::from(quantity.into())
}

fn render_live_plot<const SERIES_COUNT: usize>(
    ui: &mut UiRoot<'_>,
    context: &mut LivePlotContext,
    id: &'static str,
    series: &[(&str, &str)],
    values: Option<[f64; SERIES_COUNT]>,
    received_at: Option<Instant>,
    waiting_for: &str,
    y_axis: &str,
    height: f32,
) {
    if let (Some(values), Some(received_at)) = (values, received_at) {
        context.history.push_sample(received_at, &values);
    }

    let dataset = context.history.dataset(id, series);
    let waiting_for_data = dataset.is_empty();
    render_time_series_plot(ui, &mut context.viewport, id, &dataset, y_axis, height);
    if waiting_for_data {
        ui.raw(|raw_ui| raw_ui.label(format!("Waiting for {waiting_for} telemetry…")));
    }
}

pub(super) fn render_time_series_plot(
    ui: &mut UiRoot<'_>,
    viewport: &mut ViewportState,
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

    ui.time_series_plot(props, dataset, viewport);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn live_plot_history_uses_arrival_times_and_ignores_repeated_snapshots() {
        let mut history = LivePlotHistory::new(2);
        let start_time = Instant::now();
        history.push_sample(start_time, &[1.0, 2.0]);
        history.push_sample(start_time + Duration::from_millis(250), &[3.0, 4.0]);
        history.push_sample(start_time + Duration::from_millis(250), &[5.0, 6.0]);

        let dataset = history.dataset(
            "mote.telemetry.test",
            &[("left", "Left"), ("right", "Right")],
        );
        assert_eq!(dataset.series.len(), 2);
        assert_eq!(dataset.series[0].points.len(), 2);
        assert_eq!(dataset.series[0].points[0], PlotPoint::new(0.0, 1.0));
        assert_eq!(dataset.series[0].points[1], PlotPoint::new(0.25, 3.0));
        assert_eq!(dataset.series[1].points[1], PlotPoint::new(0.25, 4.0));
    }
}
