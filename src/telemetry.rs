use bee_ui::{
    UiRoot,
    prelude::{TimeSeries, TimeSeriesDataset, TimeSeriesPlotProps, ViewportState},
};
use eframe::egui;

const GUIDANCE_PLOT_ID: &str = "mote.telemetry.guidance";
const POSITION_PLOT_ID: &str = "mote.telemetry.position";
const CONTROL_PLOT_ID: &str = "mote.telemetry.control";
const IMU_PLOT_ID: &str = "mote.telemetry.imu";
const PLOT_CONTROLS_HEIGHT: f32 = 160.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TelemetryPlot {
    #[default]
    Guidance,
    Position,
    Control,
    Imu,
}

impl TelemetryPlot {
    const ALL: [Self; 4] = [Self::Guidance, Self::Position, Self::Control, Self::Imu];

    const fn label(self) -> &'static str {
        match self {
            Self::Guidance => "Guidance",
            Self::Position => "Position",
            Self::Control => "Control",
            Self::Imu => "Inertial Measurement Unit (IMU)",
        }
    }
}

pub(crate) fn render(ui: &mut UiRoot<'_>) {
    let context = ui.context().clone();
    let selection_id = egui::Id::new("mote.telemetry.selected_plot");
    let mut selected_plot = context
        .data_mut(|data| data.get_temp::<TelemetryPlot>(selection_id))
        .unwrap_or_default();

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

    let (id, series, y_axis) = match selected_plot {
        TelemetryPlot::Guidance => (GUIDANCE_PLOT_ID, &["Heading", "Distance"][..], "Value"),
        TelemetryPlot::Position => (POSITION_PLOT_ID, &["North", "East", "Heading"][..], "Value"),
        TelemetryPlot::Control => (CONTROL_PLOT_ID, &["Left", "Right"][..], "Angular velocity"),
        TelemetryPlot::Imu => (
            IMU_PLOT_ID,
            &[
                "Acceleration X",
                "Acceleration Y",
                "Acceleration Z",
                "Angular acceleration X",
                "Angular acceleration Y",
                "Angular acceleration Z",
            ][..],
            "Sensor reading",
        ),
    };
    render_empty_plot(ui, id, series, y_axis, plot_height);

    context.data_mut(|data| data.insert_temp(selection_id, selected_plot));
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

    ui.time_series_plot(props, &dataset, &mut viewport);

    context.data_mut(|data| data.insert_temp(viewport_id, viewport));
}
