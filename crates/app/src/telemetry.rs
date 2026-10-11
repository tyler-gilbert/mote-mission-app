use std::{collections::VecDeque, time::Instant};

use bee_ui::{
    UiRoot,
    prelude::{
        Button, Card, Col, HStack, PlotPoint, Row, TimeSeries, TimeSeriesDataset,
        TimeSeriesPlotProps, Variant, ViewportState,
    },
    theme::bootstrap_tokens::{ColorMode, card_component},
};

mod imu;
mod lidar;

use mote_api::messages::router;

use crate::{state, transport::WorkerSnapshot};

const GUIDANCE_PLOT_ID: &str = "mote.telemetry.guidance";
const POSITION_PLOT_ID: &str = "mote.telemetry.position";
const CONTROL_PLOT_ID: &str = "mote.telemetry.control";

const PLOT_CONTROLS_HEIGHT: f32 = 160.0;

const COUNTER_COLUMNS: usize = 2;
const COUNTER_COLUMN_GAP: f32 = 24.0;
const COUNTER_LABEL_VALUE_GAP: f32 = 12.0;

const STATUS_MAIN_SPAN: u8 = 8;
const STATUS_LOG_SPAN: u8 = 4;
const STATUS_GUTTER: f32 = 12.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TelemetryPlot {
    #[default]
    Guidance,
    Position,
    Control,
    Imu,
    Lidar,
    Status,
}

impl TelemetryPlot {
    const ALL: [Self; 6] = [
        Self::Guidance,
        Self::Position,
        Self::Control,
        Self::Imu,
        Self::Lidar,
        Self::Status,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Guidance => "Guidance",
            Self::Position => "Navigation",
            Self::Control => "Control",
            Self::Imu => "IMU",
            Self::Lidar => "LiDAR",
            Self::Status => "Status",
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

pub(crate) fn render(
    ui: &mut UiRoot<'_>,
    context: &mut Context,
    ui_state: &mut state::Context,
    reference_frame: Option<&router::ReferenceFrame>,
) {
    let mut selected_plot = context.selected_plot.unwrap_or_default();

    ui.hstack(HStack::new().wrap(true), |ui| {
        for plot in TelemetryPlot::ALL {
            let variant = if plot == selected_plot {
                Variant::Primary
            } else {
                Variant::Secondary
            };
            if ui
                .button(Button::new(plot.label()).variant(variant))
                .clicked()
            {
                selected_plot = plot;
            }
        }
    });

    let telemetry = &ui_state.snapshot.latest_telemetry;
    let plot_height = ui.raw(|raw_ui| (raw_ui.available_height() - PLOT_CONTROLS_HEIGHT).max(0.0));

    match selected_plot {
        TelemetryPlot::Guidance => render_live_plot(
            ui,
            &mut context.guidance,
            LivePlotOptions {
                id: GUIDANCE_PLOT_ID,
                series: &[("heading", "Heading"), ("distance", "Distance")],
                values: telemetry.guidance.as_ref().map(|value| {
                    [
                        quantity_value(value.heading),
                        quantity_value(value.distance),
                    ]
                }),
                received_at: telemetry.guidance_received_at,
                waiting_for: "Guidance",
                y_axis: "Value",
                height: plot_height,
            },
        ),
        TelemetryPlot::Position => render_live_plot(
            ui,
            &mut context.position,
            LivePlotOptions {
                id: POSITION_PLOT_ID,
                series: &[("north", "North"), ("east", "East"), ("heading", "Heading")],
                values: telemetry.position.as_ref().map(|value| {
                    [
                        quantity_value(value.coordinate.latitude),
                        quantity_value(value.coordinate.longitude),
                        quantity_value(value.heading),
                    ]
                }),
                received_at: telemetry.position_received_at,
                waiting_for: "Navigation",
                y_axis: "Value",
                height: plot_height,
            },
        ),
        TelemetryPlot::Control => render_live_plot(
            ui,
            &mut context.control,
            LivePlotOptions {
                id: CONTROL_PLOT_ID,
                series: &[("left", "Left"), ("right", "Right")],
                values: telemetry.control.as_ref().map(|value| {
                    [
                        quantity_value(value.motor_drive.left),
                        quantity_value(value.motor_drive.right),
                    ]
                }),
                received_at: telemetry.control_received_at,
                waiting_for: "Control",
                y_axis: "Angular velocity",
                height: plot_height,
            },
        ),
        TelemetryPlot::Imu => imu::render(ui, &mut context.imu, ui_state, plot_height),
        TelemetryPlot::Lidar => {
            lidar::render(
                ui,
                &mut context.lidar,
                ui_state,
                reference_frame,
                plot_height,
            );
        }
        TelemetryPlot::Status => render_debug_dashboard(ui, &ui_state.snapshot),
    }

    context.selected_plot = Some(selected_plot);
}

fn render_debug_dashboard(ui: &mut UiRoot<'_>, snapshot: &WorkerSnapshot) {
    let telemetry = &snapshot.latest_telemetry;
    ui.row(
        Row::new()
            .gutter_x(STATUS_GUTTER)
            .id_salt("mote.telemetry.debug.row"),
        |ui| {
            ui.col(Col::new().xs(STATUS_MAIN_SPAN), |ui| {
                render_packet_status(ui, telemetry);
                render_counters(ui, telemetry);
            });
            ui.col(Col::new().xs(STATUS_LOG_SPAN), |ui| {
                render_log(ui, telemetry);
            });
        },
    );
}

fn status_card(title: &'static str) -> Card {
    Card::with_title(title).variant(Variant::Secondary)
}

/// Height available to the scroll area of a `fill_height` card.
///
/// The card adds its bottom padding after the body, so reserve that (and the item spacing before
/// it) to keep the card's bottom edge inside the space the card was given.
fn fill_card_scroll_height(raw_ui: &eframe::egui::Ui) -> f32 {
    let card_bottom_padding = f32::from(card_component(ColorMode::default()).spacer_y);
    (raw_ui.available_height() - card_bottom_padding - raw_ui.spacing().item_spacing.y).max(0.0)
}

fn render_packet_status(ui: &mut UiRoot<'_>, telemetry: &crate::transport::LatestTelemetry) {
    ui.card(status_card("Packet status"), |ui| {
        ui.raw(|raw_ui| {
            raw_ui.horizontal_wrapped(|raw_ui| {
                raw_ui.vertical(|raw_ui| {
                    raw_ui.label("Current sequence");
                    raw_ui.heading(
                        telemetry
                            .debug
                            .as_ref()
                            .map_or_else(|| "—".to_owned(), |debug| debug.sequence.to_string()),
                    );
                });
                raw_ui.add_space(24.0);
                raw_ui.vertical(|raw_ui| {
                    raw_ui.label("Packets lost");
                    raw_ui.heading(telemetry.debug_packets_lost.to_string());
                });
                raw_ui.add_space(24.0);
                raw_ui.vertical(|raw_ui| {
                    raw_ui.label("Frame reference hash");
                    raw_ui.heading(telemetry.debug.as_ref().map_or_else(
                        || "—".to_owned(),
                        |debug| format!("{:#010x}", debug.reference_frame_hash),
                    ));
                });
                raw_ui.add_space(24.0);
                raw_ui.vertical(|raw_ui| {
                    raw_ui.label("Route hash");
                    raw_ui.heading(telemetry.debug.as_ref().map_or_else(
                        || "—".to_owned(),
                        |debug| format!("{:#010x}", debug.route_hash),
                    ));
                });
            });
        });
    });
}

fn render_counters(ui: &mut UiRoot<'_>, telemetry: &crate::transport::LatestTelemetry) {
    ui.card(status_card("Counters").fill_height(), |ui| {
        ui.raw(|raw_ui| {
            eframe::egui::ScrollArea::vertical()
                .id_salt("mote.telemetry.debug.counters.scroll")
                .auto_shrink([false, false])
                .max_height(fill_card_scroll_height(raw_ui))
                .show(raw_ui, |raw_ui| {
                    let Some(debug) = telemetry.debug.as_ref() else {
                        raw_ui.label("Waiting for debug counters…");
                        return;
                    };
                    let rows = counter_rows(&debug.counters);
                    let row_spacing = raw_ui.spacing().item_spacing.y;
                    let column_width = ((raw_ui.available_width()
                        - COUNTER_COLUMN_GAP * (COUNTER_COLUMNS - 1) as f32)
                        / COUNTER_COLUMNS as f32)
                        .max(0.0);
                    let row_height = raw_ui.spacing().interact_size.y;
                    eframe::egui::Grid::new("mote.telemetry.debug.counters")
                        .num_columns(COUNTER_COLUMNS)
                        .spacing([COUNTER_COLUMN_GAP, row_spacing])
                        .min_col_width(column_width)
                        .max_col_width(column_width)
                        .striped(true)
                        .show(raw_ui, |raw_ui| {
                            for row in &rows {
                                for (key, value) in row {
                                    // Size each cell to its column so long counter names
                                    // truncate instead of widening the card. The value is
                                    // pinned to the right edge and the name fills the rest
                                    // from the left edge.
                                    raw_ui.allocate_ui_with_layout(
                                        eframe::egui::vec2(column_width, row_height),
                                        eframe::egui::Layout::right_to_left(
                                            eframe::egui::Align::Center,
                                        ),
                                        |cell_ui| {
                                            cell_ui.label(value);
                                            cell_ui.add_space(COUNTER_LABEL_VALUE_GAP);
                                            cell_ui.with_layout(
                                                eframe::egui::Layout::left_to_right(
                                                    eframe::egui::Align::Center,
                                                ),
                                                |label_ui| {
                                                    label_ui.add(
                                                        eframe::egui::Label::new(
                                                            eframe::egui::RichText::new(key)
                                                                .strong(),
                                                        )
                                                        .truncate(),
                                                    );
                                                },
                                            );
                                        },
                                    );
                                }
                                raw_ui.end_row();
                            }
                        });
                });
        });
    });
}

fn render_log(ui: &mut UiRoot<'_>, telemetry: &crate::transport::LatestTelemetry) {
    ui.card(status_card("Log").fill_height(), |ui| {
        ui.raw(|raw_ui| {
            eframe::egui::ScrollArea::vertical()
                .id_salt("mote.telemetry.debug.log.scroll")
                .auto_shrink([false, false])
                .max_height(fill_card_scroll_height(raw_ui))
                .stick_to_bottom(true)
                .show(raw_ui, |raw_ui| {
                    if telemetry.debug_logs.is_empty() {
                        raw_ui.label("Waiting for log messages…");
                    } else {
                        for line in &telemetry.debug_logs {
                            raw_ui.label(line);
                        }
                    }
                });
        });
    });
}

fn counter_rows(counters: &router::StatusCounters) -> Vec<Vec<(String, String)>> {
    let Ok(serde_json::Value::Object(counters)) = serde_json::to_value(counters) else {
        return Vec::new();
    };
    let entries: Vec<_> = counters
        .into_iter()
        .map(|(key, value)| (key, value.to_string()))
        .collect();
    entries.chunks(COUNTER_COLUMNS).map(<[_]>::to_vec).collect()
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

struct LivePlotOptions<const SERIES_COUNT: usize> {
    id: &'static str,
    series: &'static [(&'static str, &'static str)],
    values: Option<[f64; SERIES_COUNT]>,
    received_at: Option<Instant>,
    waiting_for: &'static str,
    y_axis: &'static str,
    height: f32,
}

fn render_live_plot<const SERIES_COUNT: usize>(
    ui: &mut UiRoot<'_>,
    context: &mut LivePlotContext,
    options: LivePlotOptions<SERIES_COUNT>,
) {
    if let (Some(values), Some(received_at)) = (options.values, options.received_at) {
        context.history.push_sample(received_at, &values);
    }

    let dataset = context.history.dataset(options.id, options.series);
    let waiting_for_data = dataset.is_empty();
    render_time_series_plot(
        ui,
        &mut context.viewport,
        options.id,
        &dataset,
        options.y_axis,
        options.height,
    );
    if waiting_for_data {
        ui.raw(|raw_ui| raw_ui.label(format!("Waiting for {} telemetry…", options.waiting_for)));
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
    fn status_cards_use_the_secondary_variant() {
        let card = status_card("Log").fill_height();
        assert_eq!(card.variant, Variant::Secondary);
        assert!(card.fill_height);
    }

    #[test]
    fn counter_rows_group_counters_into_two_columns() {
        let counters = router::StatusCounters {
            lidar_scan: 7,
            ..router::StatusCounters::default()
        };
        let rows = counter_rows(&counters);

        let total: usize = rows.iter().map(Vec::len).sum();
        assert_eq!(total, 10);
        assert_eq!(rows.len(), 5);
        assert!(rows.iter().all(|row| row.len() == COUNTER_COLUMNS));
        assert!(
            rows.iter()
                .flatten()
                .any(|(key, value)| key == "lidar_scan" && value == "7")
        );
    }
}
