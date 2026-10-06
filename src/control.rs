use bee_ui::{
    UiRoot,
    prelude::{Badge, Card, Col, FormSelect, FormSelectOption, Row, SpanIcon, SvgIcon, Text},
};
use eframe::egui;
use mote_api::messages::router;

use crate::{
    state,
    transport::{ConnectionStatus, LatestTelemetry, WorkerCommand},
};

const FORWARD_SPEED_RAD_PER_S: f32 = 1.5;
const TURN_SPEED_RAD_PER_S: f32 = 1.0;
const STOP: (f32, f32) = (0.0, 0.0);

#[derive(Default)]
pub(crate) struct Context {
    pub(crate) mode_enabled: bool,
    pub(crate) drive_velocity: (f32, f32),
}

pub(crate) fn render(ui: &mut UiRoot<'_>, context: &mut Context, ui_state: &mut state::Context) {
    let telemetry = &ui_state.snapshot.latest_telemetry;
    let connection = &ui_state.snapshot.connection;
    let last_error = ui_state.last_error.clone();
    let mode_enabled = context.mode_enabled;
    let drive_velocity = context.drive_velocity;
    let connected = matches!(connection, ConnectionStatus::Connected { .. });

    if let Some(error) = last_error.as_deref() {
        ui.raw(|raw_ui| raw_ui.colored_label(egui::Color32::LIGHT_RED, error));
    }

    let mut selected_mode = usize::from(mode_enabled);
    let (new_mode_enabled, pressed) = ui.card(Card::new().outline(), |ui| {
        let mut pressed = [false; 4];
        ui.row(Row::new().gutter_x(16.0).gutter_y(12.0), |ui| {
            ui.col(Col::new().xs(12).md(4), |ui| {
                ui.form_select(
                    &mut selected_mode,
                    FormSelect::new()
                        .option(FormSelectOption::new("Disabled"))
                        .option(FormSelectOption::new("Enabled"))
                        .width(220.0)
                        .disabled(!connected),
                );

                if connected {
                    ui.badge(if selected_mode != 0 {
                        Badge::success("Enabled").as_pill()
                    } else {
                        Badge::secondary("Disabled").as_pill()
                    });
                } else {
                    ui.badge(Badge::warning("Connect to a Mote to change mode").as_pill());
                }
                ui.text(Text::new("Hold a direction to move; release to stop.").subtle());
            });
            ui.col(Col::new().xs(12).md(8), |ui| {
                let enabled = connected && selected_mode != 0;
                pressed = ui.raw(|raw_ui| {
                    let mut pressed = [false; 4];
                    raw_ui.vertical_centered(|raw_ui| {
                        pressed[0] = directional_button(raw_ui, enabled, SvgIcon::ArrowUp, "Up");
                        raw_ui.horizontal(|row_ui| {
                            const BUTTON_GAP: f32 = 16.0;
                            row_ui.spacing_mut().item_spacing.x = 0.0;
                            let pair_width = 2.0 * 72.0 + BUTTON_GAP;
                            row_ui.add_space(
                                ((row_ui.available_width() - pair_width) * 0.5).max(0.0),
                            );
                            pressed[1] =
                                directional_button(row_ui, enabled, SvgIcon::ArrowLeft, "Left");
                            row_ui.add_space(BUTTON_GAP);
                            pressed[2] =
                                directional_button(row_ui, enabled, SvgIcon::ArrowRight, "Right");
                        });
                        pressed[3] =
                            directional_button(raw_ui, enabled, SvgIcon::ArrowDown, "Down");
                    });
                    pressed
                });
            });
        });
        (selected_mode != 0, pressed)
    });
    let mode_changed = new_mode_enabled != mode_enabled;

    let target_velocity = if connected && new_mode_enabled {
        let forward = ((if pressed[0] { 1.0 } else { 0.0 }) - (if pressed[3] { 1.0 } else { 0.0 }))
            * FORWARD_SPEED_RAD_PER_S;
        let turn = ((if pressed[2] { 1.0 } else { 0.0 }) - (if pressed[1] { 1.0 } else { 0.0 }))
            * TURN_SPEED_RAD_PER_S;
        (
            (forward + turn).clamp(-FORWARD_SPEED_RAD_PER_S, FORWARD_SPEED_RAD_PER_S),
            (forward - turn).clamp(-FORWARD_SPEED_RAD_PER_S, FORWARD_SPEED_RAD_PER_S),
        )
    } else {
        STOP
    };

    render_latest_telemetry(ui, telemetry);

    let drive_changed = target_velocity != drive_velocity;
    if mode_changed && !new_mode_enabled && drive_changed {
        send_message(
            context,
            ui_state,
            router::Message::MotorDrive(router::MotorDrive {
                left: units::AngularVelocity::new(target_velocity.0),
                right: units::AngularVelocity::new(target_velocity.1),
            }),
        );
    }
    if mode_changed {
        send_message(
            context,
            ui_state,
            if new_mode_enabled {
                router::Message::EnableControlMode
            } else {
                router::Message::DisableControlMode
            },
        );
    }
    if drive_changed && !(mode_changed && !new_mode_enabled) {
        send_message(
            context,
            ui_state,
            router::Message::MotorDrive(router::MotorDrive {
                left: units::AngularVelocity::new(target_velocity.0),
                right: units::AngularVelocity::new(target_velocity.1),
            }),
        );
    }
}

fn send_message(context: &mut Context, ui_state: &mut state::Context, message: router::Message) {
    match ui_state
        .worker
        .command(WorkerCommand::PubSub(message.clone()))
    {
        Ok(()) => ui_state.last_error = None,
        Err(error) => {
            ui_state.last_error = Some(error);
            return;
        }
    }

    match message {
        router::Message::EnableControlMode => context.mode_enabled = true,
        router::Message::DisableControlMode => context.mode_enabled = false,
        router::Message::MotorDrive(motor_drive) => {
            context.drive_velocity = (motor_drive.left.into(), motor_drive.right.into());
        }
        _ => {}
    }
}

fn directional_button(
    ui: &mut egui::Ui,
    enabled: bool,
    icon: SvgIcon,
    direction: &'static str,
) -> bool {
    let image = SpanIcon::new(icon).size(28.0).egui_image(None);
    ui.add_enabled(
        enabled,
        egui::Button::image(image)
            .image_tint_follows_text_color(true)
            .min_size(egui::vec2(72.0, 64.0)),
    )
    .on_hover_text(direction)
    .is_pointer_button_down_on()
}

fn render_latest_telemetry(ui: &mut UiRoot<'_>, telemetry: &LatestTelemetry) {
    ui.card(Card::new().outline(), |ui| {
        ui.raw(|raw_ui| {
            raw_ui.columns(4, |columns| {
                let guidance = telemetry.guidance.as_ref();
                render_section(
                    &mut columns[0],
                    "Guidance",
                    "mote.control.guidance",
                    &[
                        (
                            "heading",
                            guidance.map(|value| format_quantity(value.heading)),
                            "rad",
                        ),
                        (
                            "distance",
                            guidance.map(|value| format_quantity(value.distance)),
                            "m",
                        ),
                    ],
                );

                let position = telemetry.position.as_ref();
                render_section(
                    &mut columns[1],
                    "Position",
                    "mote.control.position",
                    &[
                        (
                            "north",
                            position.map(|value| format_quantity(value.coordinate.latitude)),
                            "m",
                        ),
                        (
                            "east",
                            position.map(|value| format_quantity(value.coordinate.longitude)),
                            "m",
                        ),
                        (
                            "heading",
                            position.map(|value| format_quantity(value.heading)),
                            "deg",
                        ),
                    ],
                );

                let control = telemetry.control.as_ref();
                render_section(
                    &mut columns[2],
                    "Control output",
                    "mote.control.output",
                    &[
                        (
                            "left",
                            control.map(|value| format_quantity(value.motor_drive.left)),
                            "rad/s",
                        ),
                        (
                            "right",
                            control.map(|value| format_quantity(value.motor_drive.right)),
                            "rad/s",
                        ),
                    ],
                );

                let imu = telemetry.imu.as_ref();
                render_section(
                    &mut columns[3],
                    "IMU",
                    "mote.control.imu",
                    &[
                        (
                            "accel.x",
                            imu.map(|value| format_quantity(value.accel.x)),
                            "m/s²",
                        ),
                        (
                            "accel.y",
                            imu.map(|value| format_quantity(value.accel.y)),
                            "m/s²",
                        ),
                        (
                            "accel.z",
                            imu.map(|value| format_quantity(value.accel.z)),
                            "m/s²",
                        ),
                        (
                            "gyro.x",
                            imu.map(|value| format_quantity(value.gyro.x)),
                            "rad/s²",
                        ),
                        (
                            "gyro.y",
                            imu.map(|value| format_quantity(value.gyro.y)),
                            "rad/s²",
                        ),
                        (
                            "gyro.z",
                            imu.map(|value| format_quantity(value.gyro.z)),
                            "rad/s²",
                        ),
                    ],
                );
            });
        });
    });
}

fn render_section(
    ui: &mut egui::Ui,
    title: &str,
    id: &'static str,
    values: &[(&str, Option<String>, &str)],
) {
    ui.group(|ui| {
        ui.heading(title);
        let horizontal_spacing = ui.spacing().item_spacing.x;
        let column_width = ((ui.available_width() - 2.0 * horizontal_spacing) / 3.0).max(0.0);
        egui::Grid::new(id)
            .min_col_width(column_width)
            .striped(true)
            .show(ui, |ui| {
                ui.strong("Field");
                ui.strong("Value");
                ui.strong("Unit");
                ui.end_row();

                for (name, value, unit) in values {
                    ui.label(*name);
                    if let Some(value) = value {
                        ui.strong(value);
                    } else {
                        ui.weak("—");
                    }
                    ui.weak(*unit);
                    ui.end_row();
                }
            });
    });
    ui.add_space(8.0);
}

fn format_quantity(quantity: impl Into<f32>) -> String {
    format!("{:.3}", quantity.into())
}
