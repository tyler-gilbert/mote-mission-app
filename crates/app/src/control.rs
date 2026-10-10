use std::{
    fs::{File, OpenOptions},
    io::{self, BufWriter, Write},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use bee_ui::{
    UiRoot,
    prelude::{Button, HStack, SpanIcon, SvgIcon, Variant},
};
use eframe::egui;
use mote_api::messages::router;

use crate::{
    state,
    transport::{ConnectionStatus, WorkerCommand},
};

const FORWARD_SPEED_RAD_PER_S: f32 = 1.5;
const TURN_SPEED_RAD_PER_S: f32 = 1.0;
const STOP: (f32, f32) = (0.0, 0.0);

const TELEMETRY_MODES: [(&str, router::TelemetrySelection); 9] = [
    ("None          ", router::TelemetrySelection::None),
    ("Control       ", router::TelemetrySelection::Control),
    ("Guidance      ", router::TelemetrySelection::Guidance),
    ("Navigation    ", router::TelemetrySelection::Navigation),
    ("IMU           ", router::TelemetrySelection::Imu),
    ("Remote Control", router::TelemetrySelection::RemoteControl),
    ("LiDAR         ", router::TelemetrySelection::Lidar),
    (
        "Position Inputs",
        router::TelemetrySelection::PositionInputs,
    ),
    ("All           ", router::TelemetrySelection::All),
];

pub(crate) struct Context {
    pub(crate) mode_enabled: bool,
    pub(crate) drive_velocity: (f32, f32),
    pub(crate) telemetry_selection: router::TelemetrySelection,
    pub(crate) telemetry_selection_sent: bool,
    recorder: Option<TelemetryRecorder>,
    recording_error: Option<String>,
    plan: Option<PlanTransfer>,
}

impl Context {
    pub(crate) fn queue_plan(&mut self, settings: router::Settings) {
        self.plan = Some(PlanTransfer::new(settings));
    }

    pub(crate) fn clear_plan(&mut self) {
        self.plan = None;
    }

    pub(crate) fn plan_confirmed(&self) -> bool {
        self.plan
            .as_ref()
            .is_some_and(|plan| plan.reference_confirmed && plan.route_confirmed)
    }
}

impl Default for Context {
    fn default() -> Self {
        Self {
            mode_enabled: false,
            drive_velocity: STOP,
            telemetry_selection: router::TelemetrySelection::RemoteControl,
            telemetry_selection_sent: false,
            recorder: None,
            recording_error: None,
            plan: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlanMessageKind {
    ReferenceFrame,
    Route,
}

struct PlanTransfer {
    settings: router::Settings,
    reference_frame_hash: u32,
    route_hash: u32,
    reference_sent: bool,
    reference_confirmed: bool,
    route_sent: bool,
    route_confirmed: bool,
    connected_uid: Option<String>,
}

impl PlanTransfer {
    fn new(settings: router::Settings) -> Self {
        let reference_frame_hash = settings.reference_frame.hash();
        let route_hash = router::hash_route(&settings.route);
        Self {
            settings,
            reference_frame_hash,
            route_hash,
            reference_sent: false,
            reference_confirmed: false,
            route_sent: false,
            route_confirmed: false,
            connected_uid: None,
        }
    }

    fn reset_progress(&mut self) {
        self.reference_sent = false;
        self.reference_confirmed = false;
        self.route_sent = false;
        self.route_confirmed = false;
    }

    fn next_message(
        &mut self,
        debug_hashes: Option<(u32, u32)>,
    ) -> Option<(PlanMessageKind, router::Message)> {
        if !self.reference_confirmed
            && debug_hashes
                .is_some_and(|(reference_hash, _)| reference_hash == self.reference_frame_hash)
        {
            self.reference_confirmed = true;
        }
        if !self.route_confirmed
            && debug_hashes.is_some_and(|(_, route_hash)| route_hash == self.route_hash)
        {
            self.route_confirmed = true;
        }

        if !self.reference_confirmed {
            if self.reference_sent {
                return None;
            }
            return Some((
                PlanMessageKind::ReferenceFrame,
                router::Message::ReferenceFrame(self.settings.reference_frame.clone()),
            ));
        }

        if !self.route_confirmed {
            if self.route_sent {
                return None;
            }
            return Some((
                PlanMessageKind::Route,
                router::Message::Route(self.settings.route.clone()),
            ));
        }

        None
    }

    fn mark_queued(&mut self, kind: PlanMessageKind) {
        match kind {
            PlanMessageKind::ReferenceFrame => self.reference_sent = true,
            PlanMessageKind::Route => self.route_sent = true,
        }
    }
}

struct TelemetryRecorder {
    writer: BufWriter<File>,
    last_sample_times: [Option<Instant>; 5],
}

impl TelemetryRecorder {
    fn create() -> io::Result<Self> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_millis();
        let filename = format!("telemetry_{timestamp}.data");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(filename)?;

        Ok(Self {
            writer: BufWriter::new(file),
            // Do not include telemetry received before recording started.
            last_sample_times: [Some(Instant::now()); 5],
        })
    }

    fn write_sample(
        &mut self,
        index: usize,
        received_at: Instant,
        message: &router::Message,
    ) -> io::Result<()> {
        if self.last_sample_times[index].is_some_and(|last| received_at <= last) {
            return Ok(());
        }

        let encoded = postcard::to_allocvec(message).map_err(io::Error::other)?;
        let encoded_len = u32::try_from(encoded.len())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        // Frame concatenated Postcard messages so a reader can decode them sequentially.
        self.writer.write_all(&encoded_len.to_le_bytes())?;
        self.writer.write_all(&encoded)?;
        self.writer.flush()?;
        self.last_sample_times[index] = Some(received_at);
        Ok(())
    }

    fn finish(mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

pub(crate) fn render(
    ui: &mut UiRoot<'_>,
    context: &mut Context,
    expected_hashes: Option<(u32, u32)>,
    ui_state: &mut state::Context,
) {
    let connected = matches!(
        ui_state.snapshot.connection,
        ConnectionStatus::Connected { .. }
    );
    let selected_mode = telemetry_mode_index(&context.telemetry_selection);
    let recording = context.recorder.is_some();
    let connected_uid = match &ui_state.snapshot.connection {
        ConnectionStatus::Connected { uid, .. } => Some(uid.clone()),
        _ => None,
    };
    process_plan(context, ui_state, connected_uid.as_deref());
    let received_hashes = ui_state
        .snapshot
        .latest_telemetry
        .debug
        .as_ref()
        .map(|debug| (debug.reference_frame_hash, debug.route_hash));

    let (remote_control_clicked, pressed, mode_changed, record_clicked) =
        ui.hstack(HStack::new().gap(4.0), |ui| {
            let _ = ui.raw(|raw_ui| {
                raw_ui
                    .add(SpanIcon::new(SvgIcon::Joystick).size(16.0).egui_image(None))
                    .on_hover_text("Select the telemetry stream requested from the Mote")
            });

            let remote_control_button = if context.mode_enabled {
                Button::success("")
                    .icon(SvgIcon::ToggleOn)
                    .hover_text("Disable remote control")
            } else {
                Button::secondary("")
                    .icon(SvgIcon::ToggleOff)
                    .hover_text("Enable remote control")
            };
            let remote_control_clicked = ui
                .button(remote_control_button.enabled(connected))
                .clicked();

            let pressed = ui.raw(|raw_ui| {
                raw_ui
                    .horizontal(|raw_ui| {
                        raw_ui.spacing_mut().item_spacing.x = 2.0;
                        [
                            (SvgIcon::ArrowLeft, "Drive left"),
                            (SvgIcon::ArrowUp, "Drive forward"),
                            (SvgIcon::ArrowDown, "Drive backward"),
                            (SvgIcon::ArrowRight, "Drive right"),
                        ]
                        .into_iter()
                        .map(|(icon, tooltip)| {
                            directional_button(
                                raw_ui,
                                connected && context.mode_enabled,
                                icon,
                                tooltip,
                            )
                        })
                        .collect::<Vec<_>>()
                    })
                    .inner
            });

            let _ = ui.raw(|raw_ui| {
                raw_ui
                    .add(SpanIcon::new(SvgIcon::Activity).size(16.0).egui_image(None))
                    .on_hover_text("Select the telemetry stream requested from the Mote")
            });

            let mut selected_mode = selected_mode;
            let mode_changed = ui.raw(|raw_ui| {
                raw_ui
                    .add_enabled_ui(connected, |raw_ui| {
                        let before = selected_mode;
                        egui::ComboBox::from_id_salt("mote.control.telemetry_mode")
                            .selected_text(TELEMETRY_MODES[selected_mode].0)
                            .show_ui(raw_ui, |raw_ui| {
                                for (index, (label, _)) in TELEMETRY_MODES.iter().enumerate() {
                                    raw_ui.selectable_value(&mut selected_mode, index, *label);
                                }
                            });
                        selected_mode != before
                    })
                    .inner
            });

            let record_button = if recording {
                Button::warning("")
                    .icon(SvgIcon::StopCircleFill)
                    .hover_text("Stop recording telemetry")
            } else {
                Button::danger("")
                    .icon(SvgIcon::RecordCircleFill)
                    .hover_text("Record telemetry to telemetry_<timestamp>.data")
            };
            let record_clicked = ui
                .button(record_button.enabled(connected || recording))
                .clicked();

            render_plan_status(ui, expected_hashes, received_hashes);

            (
                remote_control_clicked,
                pressed,
                (selected_mode, mode_changed),
                record_clicked,
            )
        });

    if remote_control_clicked {
        let enabling = !context.mode_enabled;
        if enabling {
            send_message(context, ui_state, router::Message::EnableControlMode);
        } else {
            let stopped = context.drive_velocity == STOP
                || send_message(
                    context,
                    ui_state,
                    router::Message::MotorDrive(router::MotorDrive {
                        left: units::AngularVelocity::new(0.0),
                        right: units::AngularVelocity::new(0.0),
                    }),
                );
            if stopped {
                send_message(context, ui_state, router::Message::DisableControlMode);
            }
        }
    }

    if mode_changed.1 {
        context.telemetry_selection = TELEMETRY_MODES[mode_changed.0].1.clone();
        context.telemetry_selection_sent = false;
    }
    if !connected {
        context.telemetry_selection_sent = false;
    } else if !context.telemetry_selection_sent
        && send_message(
            context,
            ui_state,
            router::Message::TelemetrySelection(context.telemetry_selection.clone()),
        )
    {
        context.telemetry_selection_sent = true;
    }

    if record_clicked {
        if recording {
            stop_recording(context);
        } else {
            start_recording(context);
        }
    }

    let mode_enabled = connected && context.mode_enabled;
    let target_velocity = if mode_enabled {
        let forward = (pressed[1] as i8 - pressed[2] as i8) as f32 * FORWARD_SPEED_RAD_PER_S;
        let turn = (pressed[3] as i8 - pressed[0] as i8) as f32 * TURN_SPEED_RAD_PER_S;
        (
            (forward + turn).clamp(-FORWARD_SPEED_RAD_PER_S, FORWARD_SPEED_RAD_PER_S),
            (forward - turn).clamp(-FORWARD_SPEED_RAD_PER_S, FORWARD_SPEED_RAD_PER_S),
        )
    } else {
        STOP
    };

    let drive_changed = target_velocity != context.drive_velocity;
    if drive_changed {
        send_message(
            context,
            ui_state,
            router::Message::MotorDrive(router::MotorDrive {
                left: units::AngularVelocity::new(target_velocity.0),
                right: units::AngularVelocity::new(target_velocity.1),
            }),
        );
    }

    record_telemetry(context, ui_state);
    render_errors(ui, ui_state, context);
}

fn process_plan(context: &mut Context, ui_state: &mut state::Context, connected_uid: Option<&str>) {
    let Some(plan) = context.plan.as_mut() else {
        return;
    };

    match connected_uid {
        Some(uid) if plan.connected_uid.as_deref() != Some(uid) => {
            plan.connected_uid = Some(uid.to_owned());
            plan.reset_progress();
        }
        None if plan.connected_uid.take().is_some() => plan.reset_progress(),
        _ => {}
    }

    let Some(uid) = connected_uid else {
        return;
    };
    let debug_hashes = ui_state
        .snapshot
        .latest_telemetry
        .debug
        .as_ref()
        .map(|debug| (debug.reference_frame_hash, debug.route_hash));
    let Some((kind, message)) = plan.next_message(debug_hashes) else {
        return;
    };

    if send_message(context, ui_state, message) {
        if let Some(plan) = context.plan.as_mut() {
            plan.mark_queued(kind);
        }
    } else {
        ui_state.last_error = ui_state
            .last_error
            .take()
            .map(|error| format!("Could not send plan to {uid}: {error}"));
    }
}

fn render_plan_status(
    ui: &mut UiRoot<'_>,
    expected_hashes: Option<(u32, u32)>,
    received_hashes: Option<(u32, u32)>,
) {
    let statuses = [
        (
            "Reference frame",
            SvgIcon::AspectRatio,
            expected_hashes.map(|hashes| hashes.0),
            received_hashes.map(|hashes| hashes.0),
        ),
        (
            "Route",
            SvgIcon::Map,
            expected_hashes.map(|hashes| hashes.1),
            received_hashes.map(|hashes| hashes.1),
        ),
    ];
    for (label, icon, expected_hash, received_hash) in statuses {
        let confirmed = expected_hash
            .zip(received_hash)
            .is_some_and(|(expected, received)| expected == received);
        let tooltip = match (expected_hash, received_hash) {
            (Some(expected), Some(_)) if confirmed => {
                format!("{label} ready — local and device hashes match: {expected:#010x}")
            }
            (Some(expected), Some(received)) => {
                format!("{label} hash mismatch — local: {expected:#010x}; device: {received:#010x}")
            }
            (Some(expected), None) => {
                format!("Waiting for the device {label} hash — expected: {expected:#010x}")
            }
            (None, Some(received)) => {
                format!("No local settings to compare — device {label} hash: {received:#010x}")
            }
            (None, None) => format!("No local {label} hash or device hash available"),
        };
        let variant = if confirmed {
            Variant::Success
        } else {
            Variant::Secondary
        };
        ui.button(Button::new("").variant(variant).icon(icon).disabled())
            .on_disabled_hover_text(tooltip);
    }
}

fn send_message(
    context: &mut Context,
    ui_state: &mut state::Context,
    message: router::Message,
) -> bool {
    match ui_state
        .worker
        .command(WorkerCommand::PubSub(message.clone()))
    {
        Ok(()) => ui_state.last_error = None,
        Err(error) => {
            ui_state.last_error = Some(error);
            return false;
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
    true
}

fn start_recording(context: &mut Context) {
    match TelemetryRecorder::create() {
        Ok(recorder) => {
            context.recorder = Some(recorder);
            context.recording_error = None;
        }
        Err(error) => {
            context.recording_error = Some(format!("Could not start telemetry recording: {error}"));
        }
    }
}

fn stop_recording(context: &mut Context) {
    context.recording_error = None;
    if let Some(recorder) = context.recorder.take()
        && let Err(error) = recorder.finish()
    {
        context.recording_error = Some(format!("Could not finish telemetry recording: {error}"));
    }
}

fn record_telemetry(context: &mut Context, ui_state: &mut state::Context) {
    let Some(recorder) = context.recorder.as_mut() else {
        return;
    };

    let telemetry = &ui_state.snapshot.latest_telemetry;
    let samples = [
        (
            0,
            TelemetryKind::Guidance,
            telemetry
                .guidance_received_at
                .zip(telemetry.guidance.as_ref())
                .map(|(received_at, value)| {
                    (received_at, router::Message::Guidance(value.clone()))
                }),
        ),
        (
            1,
            TelemetryKind::Navigation,
            telemetry
                .position_received_at
                .zip(telemetry.position.as_ref())
                .map(|(received_at, value)| {
                    (received_at, router::Message::Position(value.clone()))
                }),
        ),
        (
            2,
            TelemetryKind::Control,
            telemetry
                .control_received_at
                .zip(telemetry.control.as_ref())
                .map(|(received_at, value)| (received_at, router::Message::Control(value.clone()))),
        ),
        (
            3,
            TelemetryKind::Imu,
            telemetry
                .imu_received_at
                .zip(telemetry.imu.as_ref())
                .map(|(received_at, value)| (received_at, router::Message::Imu(value.clone()))),
        ),
        (
            4,
            TelemetryKind::Lidar,
            telemetry
                .lidar_scan_received_at
                .zip(telemetry.lidar_scan.as_ref())
                .map(|(received_at, value)| {
                    (received_at, router::Message::LidarScan(value.clone()))
                }),
        ),
    ];

    let write_error = samples
        .into_iter()
        .filter(|(_, kind, _)| selection_includes(&context.telemetry_selection, *kind))
        .filter_map(|(index, _, sample)| {
            sample.map(|(received_at, message)| (index, received_at, message))
        })
        .find_map(|(index, received_at, message)| {
            recorder.write_sample(index, received_at, &message).err()
        });

    if let Some(error) = write_error {
        context.recorder = None;
        context.recording_error = Some(format!("Could not write telemetry recording: {error}"));
    }
}

#[derive(Clone, Copy)]
enum TelemetryKind {
    Control,
    Guidance,
    Navigation,
    Imu,
    Lidar,
}

fn selection_includes(selection: &router::TelemetrySelection, kind: TelemetryKind) -> bool {
    match selection {
        router::TelemetrySelection::None => false,
        router::TelemetrySelection::Control => matches!(kind, TelemetryKind::Control),
        router::TelemetrySelection::Guidance => matches!(kind, TelemetryKind::Guidance),
        router::TelemetrySelection::Navigation => matches!(kind, TelemetryKind::Navigation),
        router::TelemetrySelection::Imu => matches!(kind, TelemetryKind::Imu),
        router::TelemetrySelection::RemoteControl => matches!(
            kind,
            TelemetryKind::Control
                | TelemetryKind::Guidance
                | TelemetryKind::Navigation
                | TelemetryKind::Imu
        ),
        router::TelemetrySelection::Lidar => matches!(kind, TelemetryKind::Lidar),
        router::TelemetrySelection::PositionInputs => {
            matches!(kind, TelemetryKind::Imu | TelemetryKind::Lidar)
        }
        router::TelemetrySelection::All => true,
    }
}

fn telemetry_mode_index(selection: &router::TelemetrySelection) -> usize {
    TELEMETRY_MODES
        .iter()
        .position(|(_, candidate)| candidate == selection)
        .unwrap_or_default()
}

fn directional_button(
    ui: &mut egui::Ui,
    enabled: bool,
    icon: SvgIcon,
    tooltip: &'static str,
) -> bool {
    let image = SpanIcon::new(icon).size(17.0).egui_image(None);
    ui.add_enabled(
        enabled,
        egui::Button::image(image)
            .image_tint_follows_text_color(true)
            .min_size(egui::vec2(30.0, 28.0)),
    )
    .on_hover_text(tooltip)
    .is_pointer_button_down_on()
}

fn render_errors(ui: &mut UiRoot<'_>, ui_state: &state::Context, context: &Context) {
    if let Some(error) = ui_state
        .last_error
        .as_deref()
        .or(context.recording_error.as_deref())
    {
        ui.button(
            Button::danger("")
                .icon(SvgIcon::ExclamationTriangleFill)
                .hover_text(error),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_settings() -> router::Settings {
        router::Settings {
            reference_frame: router::ReferenceFrame::default(),
            route: router::Route::new(),
        }
    }

    #[test]
    fn plan_sends_reference_once_and_waits_for_its_hash() {
        let mut plan = PlanTransfer::new(empty_settings());

        let (kind, message) = plan.next_message(None).unwrap();
        assert_eq!(kind, PlanMessageKind::ReferenceFrame);
        assert!(matches!(message, router::Message::ReferenceFrame(_)));
        plan.mark_queued(kind);

        for _ in 0..3 {
            assert!(plan.next_message(None).is_none());
        }
        assert!(plan.reference_sent);
        assert!(!plan.reference_confirmed);
    }

    #[test]
    fn matching_reference_hash_sends_route_once_and_waits_for_its_hash() {
        let mut plan = PlanTransfer::new(empty_settings());
        let (kind, _) = plan.next_message(None).unwrap();
        plan.mark_queued(kind);

        let route_mismatch = (plan.reference_frame_hash, plan.route_hash.wrapping_add(1));
        let (kind, message) = plan.next_message(Some(route_mismatch)).unwrap();
        assert_eq!(kind, PlanMessageKind::Route);
        assert!(matches!(message, router::Message::Route(_)));
        assert!(plan.reference_confirmed);
        plan.mark_queued(kind);

        for _ in 0..3 {
            assert!(plan.next_message(Some(route_mismatch)).is_none());
        }
        assert!(plan.route_sent);
        assert!(!plan.route_confirmed);
    }

    #[test]
    fn plan_waits_for_reference_hash_before_sending_route_and_confirms_each_hash() {
        let mut plan = PlanTransfer::new(empty_settings());
        let hashes = (plan.reference_frame_hash, plan.route_hash);
        let mismatched_hashes = (
            plan.reference_frame_hash.wrapping_add(1),
            plan.route_hash.wrapping_add(1),
        );

        let (kind, message) = plan.next_message(Some(mismatched_hashes)).unwrap();
        assert_eq!(kind, PlanMessageKind::ReferenceFrame);
        assert!(matches!(message, router::Message::ReferenceFrame(_)));
        plan.mark_queued(kind);

        let route_mismatch = (plan.reference_frame_hash, plan.route_hash.wrapping_add(1));
        let (kind, message) = plan.next_message(Some(route_mismatch)).unwrap();
        assert_eq!(kind, PlanMessageKind::Route);
        assert!(matches!(message, router::Message::Route(_)));
        assert!(plan.reference_confirmed);
        assert!(!plan.route_confirmed);
        plan.mark_queued(kind);

        assert!(plan.next_message(Some(hashes)).is_none());
        assert!(plan.route_confirmed);
    }

    #[test]
    fn matching_device_hashes_confirm_plan_without_resending_it() {
        let mut plan = PlanTransfer::new(empty_settings());
        let hashes = (plan.reference_frame_hash, plan.route_hash);

        assert!(plan.next_message(Some(hashes)).is_none());
        assert!(plan.reference_confirmed);
        assert!(plan.route_confirmed);
        assert!(!plan.reference_sent);
        assert!(!plan.route_sent);
    }

    #[test]
    fn telemetry_modes_cover_the_router_selection_enum() {
        assert_eq!(TELEMETRY_MODES.len(), 9);
        for (index, (label, selection)) in TELEMETRY_MODES.iter().enumerate() {
            assert!(!label.is_empty());
            assert_eq!(telemetry_mode_index(selection), index);
        }
    }

    #[test]
    fn remote_control_records_control_and_navigation_inputs_but_not_lidar() {
        assert!(selection_includes(
            &router::TelemetrySelection::RemoteControl,
            TelemetryKind::Control
        ));
        assert!(selection_includes(
            &router::TelemetrySelection::RemoteControl,
            TelemetryKind::Guidance
        ));
        assert!(!selection_includes(
            &router::TelemetrySelection::RemoteControl,
            TelemetryKind::Lidar
        ));
    }
}
