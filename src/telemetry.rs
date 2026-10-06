use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    io::{self, BufWriter, Write},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use bee_ui::{
    UiRoot,
    prelude::{
        Button, DisclosureState, Dropdown, DropdownItem, HStack, PlotPoint, SvgIcon, TimeSeries,
        TimeSeriesDataset, TimeSeriesPlotProps, ViewportState,
    },
};
use mote_api::messages::router;
mod imu;
mod lidar;

use crate::{
    state,
    transport::{ConnectionStatus, WorkerCommand},
};

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

    const fn telemetry_selection(self) -> router::TelemetrySelection {
        match self {
            Self::Guidance => router::TelemetrySelection::Guidance,
            Self::Position => router::TelemetrySelection::Navigation,
            Self::Control => router::TelemetrySelection::Control,
            Self::Imu => router::TelemetrySelection::Imu,
            Self::Lidar => router::TelemetrySelection::Lidar,
        }
    }

    const fn file_type(self) -> &'static str {
        match self {
            Self::Guidance => "guidance",
            Self::Position => "navigation",
            Self::Control => "control",
            Self::Imu => "imu",
            Self::Lidar => "lidar",
        }
    }
}

struct TelemetryRecorder {
    plot: TelemetryPlot,
    writer: BufWriter<File>,
    last_sample_time: Option<Instant>,
}

impl TelemetryRecorder {
    fn create(plot: TelemetryPlot) -> io::Result<Self> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_millis();
        let filename = format!("{}_{}.data", plot.file_type(), timestamp);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(filename)?;

        Ok(Self {
            plot,
            writer: BufWriter::new(file),
            last_sample_time: None,
        })
    }

    fn write_sample(&mut self, received_at: Instant, message: &router::Message) -> io::Result<()> {
        if self
            .last_sample_time
            .is_some_and(|last_sample_time| received_at <= last_sample_time)
        {
            return Ok(());
        }

        let encoded = postcard::to_allocvec(message).map_err(io::Error::other)?;
        let encoded_len = u32::try_from(encoded.len())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        // Frame concatenated Postcard messages so a reader can decode them sequentially.
        self.writer.write_all(&encoded_len.to_le_bytes())?;
        self.writer.write_all(&encoded)?;
        self.writer.flush()?;
        self.last_sample_time = Some(received_at);
        Ok(())
    }

    fn finish(mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

fn selected_sample(
    plot: TelemetryPlot,
    telemetry: &crate::transport::LatestTelemetry,
) -> Option<(Instant, router::Message)> {
    match plot {
        TelemetryPlot::Guidance => Some((
            telemetry.guidance_received_at?,
            router::Message::Guidance(telemetry.guidance.as_ref()?.clone()),
        )),
        TelemetryPlot::Position => Some((
            telemetry.position_received_at?,
            router::Message::Position(telemetry.position.as_ref()?.clone()),
        )),
        TelemetryPlot::Control => Some((
            telemetry.control_received_at?,
            router::Message::Control(telemetry.control.as_ref()?.clone()),
        )),
        TelemetryPlot::Imu => Some((
            telemetry.imu_received_at?,
            router::Message::Imu(telemetry.imu.as_ref()?.clone()),
        )),
        TelemetryPlot::Lidar => Some((
            telemetry.lidar_scan_received_at?,
            router::Message::LidarScan(telemetry.lidar_scan.as_ref()?.clone()),
        )),
    }
}

pub(crate) struct Context {
    selected_plot: Option<TelemetryPlot>,
    guidance: LivePlotContext,
    position: LivePlotContext,
    control: LivePlotContext,
    imu: imu::Context,
    lidar: lidar::Context,
    recorder: Option<TelemetryRecorder>,
    recording_error: Option<String>,
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
            recorder: None,
            recording_error: None,
        }
    }
}

pub(crate) fn render(ui: &mut UiRoot<'_>, context: &mut Context, ui_state: &mut state::Context) {
    let previous_plot = context.selected_plot;
    let mut selected_plot = previous_plot.unwrap_or_default();

    let plot_selector =
        TelemetryPlot::ALL
            .iter()
            .fold(Dropdown::new(selected_plot.label()), |dropdown, plot| {
                dropdown.item(DropdownItem::new(plot.label()).active(*plot == selected_plot))
            });
    let (clicked_index, record_clicked) = ui.hstack(HStack::new(), |ui| {
        let clicked_index = ui
            .dropdown(
                DisclosureState::uncontrolled("mote.telemetry.plot_selector", false),
                plot_selector,
            )
            .clicked_index();
        let record_button = if ui_state.is_record_telemetry {
            Button::warning("Stop")
                .icon(SvgIcon::SquareFill)
                .hover_text("Stop recording telemetry")
        } else {
            Button::danger("Record")
                .icon(SvgIcon::CircleFill)
                .hover_text("Record telemetry to a log file")
        };
        (clicked_index, ui.button(record_button).clicked())
    });

    if let Some(index) = clicked_index {
        selected_plot = TelemetryPlot::ALL[index];
    }

    if record_clicked {
        if ui_state.is_record_telemetry {
            ui_state.is_record_telemetry = false;
            context.recording_error = None;
            if let Some(recorder) = context.recorder.take()
                && let Err(error) = recorder.finish()
            {
                context.recording_error = Some(format!("Could not finish telemetry log: {error}"));
            }
        } else {
            match TelemetryRecorder::create(selected_plot) {
                Ok(recorder) => {
                    context.recorder = Some(recorder);
                    context.recording_error = None;
                    ui_state.is_record_telemetry = true;
                }
                Err(error) => {
                    context.recording_error =
                        Some(format!("Could not start telemetry log: {error}"));
                }
            }
        }
    }

    if previous_plot != Some(selected_plot) {
        let selection = selected_plot.telemetry_selection();
        ui_state.telemetry_selection = selection.clone();
        ui_state.telemetry_panel_setup_sent = false;

        if matches!(
            ui_state.snapshot.connection,
            ConnectionStatus::Connected { .. }
        ) {
            match ui_state.worker.command(WorkerCommand::PubSub(
                router::Message::TelemetrySelection(selection),
            )) {
                Ok(()) => {
                    ui_state.telemetry_panel_setup_sent = true;
                    ui_state.last_error = None;
                }
                Err(error) => ui_state.last_error = Some(error),
            }
        }
    }

    record_active_telemetry(context, selected_plot, ui_state);
    if let Some(error) = context.recording_error.as_deref() {
        ui.raw(|raw_ui| raw_ui.colored_label(eframe::egui::Color32::RED, error));
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

fn record_active_telemetry(
    context: &mut Context,
    plot: TelemetryPlot,
    ui_state: &mut state::Context,
) {
    if !ui_state.is_record_telemetry {
        return;
    }

    if context
        .recorder
        .as_ref()
        .is_none_or(|recorder| recorder.plot != plot)
    {
        if let Some(recorder) = context.recorder.take()
            && let Err(error) = recorder.finish()
        {
            context.recording_error = Some(format!("Could not finish telemetry log: {error}"));
        }

        match TelemetryRecorder::create(plot) {
            Ok(recorder) => context.recorder = Some(recorder),
            Err(error) => {
                ui_state.is_record_telemetry = false;
                context.recording_error = Some(format!("Could not start telemetry log: {error}"));
                return;
            }
        }
    }

    if let Some((received_at, message)) = selected_sample(plot, &ui_state.snapshot.latest_telemetry)
        && let Some(recorder) = context.recorder.as_mut()
        && let Err(error) = recorder.write_sample(received_at, &message)
    {
        ui_state.is_record_telemetry = false;
        context.recorder = None;
        context.recording_error = Some(format!("Could not write telemetry log: {error}"));
    }
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

    #[test]
    fn plots_request_the_matching_pubsub_telemetry() {
        assert!(matches!(
            TelemetryPlot::Control.telemetry_selection(),
            router::TelemetrySelection::Control
        ));
        assert!(matches!(
            TelemetryPlot::Guidance.telemetry_selection(),
            router::TelemetrySelection::Guidance
        ));
        assert!(matches!(
            TelemetryPlot::Position.telemetry_selection(),
            router::TelemetrySelection::Navigation
        ));
        assert!(matches!(
            TelemetryPlot::Imu.telemetry_selection(),
            router::TelemetrySelection::Imu
        ));
        assert!(matches!(
            TelemetryPlot::Lidar.telemetry_selection(),
            router::TelemetrySelection::Lidar
        ));
    }
}
