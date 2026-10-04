mod connection;
mod control;
mod mission;
mod settings;
mod telemetry;
mod transport;
mod ui;

use std::{
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};

use eframe::egui;
use mote_api::messages::{host_to_mote, pubsub};
use transport::{ConnectionStatus, TransportWorker, WorkerCommand, WorkerSnapshot};
use ui::{ControlPanel, UiAction};

const DEVICE_REFRESH_DURATION: Duration = Duration::from_secs(30);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Mote")
            .with_inner_size([900.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Mote",
        options,
        Box::new(|creation_context| {
            egui_extras::install_image_loaders(&creation_context.egui_ctx);
            bee_ui::theme::install_default_theme(&creation_context.egui_ctx);
            Ok(Box::new(MoteApp::new()))
        }),
    )
}

struct MoteApp {
    worker: TransportWorker,
    snapshot: WorkerSnapshot,
    selected_device: Option<String>,
    active_panel: ControlPanel,
    control_panel_setup_sent: bool,
    telemetry_panel_setup_sent: bool,
    telemetry_selection: pubsub::TelemetrySelection,
    device_refresh_until: Option<Instant>,
    control_mode_enabled: bool,
    drive_velocity: (f32, f32),
    last_error: Option<String>,
}

impl MoteApp {
    fn new() -> Self {
        Self {
            worker: TransportWorker::start(),
            snapshot: WorkerSnapshot {
                devices: Vec::new(),
                discovery_status: "Starting mDNS discovery…".to_owned(),
                connection: ConnectionStatus::Disconnected,
                incoming_messages: 0,
                malformed_packets: 0,
                version_mismatches: 0,
                last_protocol_error: None,
                latest_telemetry: transport::LatestTelemetry::default(),
            },
            selected_device: None,
            active_panel: ControlPanel::Connection,
            control_panel_setup_sent: false,
            telemetry_panel_setup_sent: false,
            telemetry_selection: pubsub::TelemetrySelection::Guidance,
            device_refresh_until: None,
            control_mode_enabled: false,
            drive_velocity: (0.0, 0.0),
            last_error: None,
        }
    }

    fn process_worker_events(&mut self) {
        loop {
            match self.worker.try_snapshot() {
                Ok(snapshot) => self.snapshot = snapshot,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.last_error = Some("Transport worker stopped unexpectedly".to_owned());
                    break;
                }
            }
        }

        connection::reconcile_selected_device(&self.snapshot.devices, &mut self.selected_device);
        if !matches!(
            &self.snapshot.connection,
            ConnectionStatus::Connected { .. }
        ) {
            self.control_panel_setup_sent = false;
            self.telemetry_panel_setup_sent = false;
            self.control_mode_enabled = false;
            self.drive_velocity = (0.0, 0.0);
        }
    }

    fn send_command(&mut self, command: WorkerCommand) -> bool {
        let result = self.worker.command(command);
        self.handle_send_result(result)
    }

    fn handle_send_result(&mut self, result: Result<(), String>) -> bool {
        match result {
            Ok(()) => {
                self.last_error = None;
                true
            }
            Err(error) => {
                self.last_error = Some(error);
                false
            }
        }
    }

    fn send_pubsub_message(&mut self, message: pubsub::Message) -> bool {
        let mode_enabled = match &message {
            pubsub::Message::EnableControlMode => Some(true),
            pubsub::Message::DisableControlMode => Some(false),
            _ => None,
        };
        let drive_velocity = match &message {
            pubsub::Message::Control(control) => Some((control.left.into(), control.right.into())),
            _ => None,
        };
        let sent = self.send_command(WorkerCommand::PubSub(message));
        if sent {
            if let Some(enabled) = mode_enabled {
                self.control_mode_enabled = enabled;
            }
            if let Some(velocity) = drive_velocity {
                self.drive_velocity = velocity;
            }
        }
        sent
    }

    fn stop_drive(&mut self) {
        if self.drive_velocity != (0.0, 0.0)
            && self.send_command(WorkerCommand::HostToMote(
                host_to_mote::Message::SetDriveBaseVelocity(host_to_mote::SetDriveBaseVelocity {
                    left_velocity_rad_per_s: 0.0,
                    right_velocity_rad_per_s: 0.0,
                }),
            ))
        {
            self.drive_velocity = (0.0, 0.0);
        }
    }

    fn apply_ui_action(&mut self, action: UiAction) {
        match action {
            UiAction::SelectPanel(panel) => {
                if panel != ControlPanel::Control {
                    self.control_panel_setup_sent = false;
                }
                if panel != ControlPanel::Telemetry {
                    self.telemetry_panel_setup_sent = false;
                }
                self.active_panel = panel;
            }
            UiAction::Refresh => {
                let result = self.worker.refresh();
                if self.handle_send_result(result) {
                    self.device_refresh_until = Some(Instant::now() + DEVICE_REFRESH_DURATION);
                }
            }
            UiAction::SelectDevice(fullname) => self.selected_device = Some(fullname),
            UiAction::Connect(fullname) => {
                let result = self.worker.connect(fullname);
                self.handle_send_result(result);
            }
            UiAction::Disconnect => {
                let result = self.worker.disconnect();
                self.handle_send_result(result);
            }
            UiAction::PubSub(message) => {
                let is_telemetry_selection =
                    matches!(&message, pubsub::Message::TelemetrySelection(_));
                if let pubsub::Message::TelemetrySelection(selection) = &message {
                    self.telemetry_selection = selection.clone();
                    self.telemetry_panel_setup_sent = false;
                }
                let sent = self.send_pubsub_message(message);
                if self.active_panel == ControlPanel::Telemetry && is_telemetry_selection {
                    self.telemetry_panel_setup_sent = sent;
                }
            }
        }
    }
}

impl eframe::App for MoteApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_worker_events();
        if self
            .device_refresh_until
            .is_some_and(|deadline| !self.snapshot.devices.is_empty() || Instant::now() >= deadline)
        {
            self.device_refresh_until = None;
        }
        let refreshing_devices = self.device_refresh_until.is_some();

        let actions = bee_ui::show_root(context, |ui| {
            ui::render(
                ui,
                &self.snapshot,
                self.selected_device.as_deref(),
                self.last_error.as_deref(),
                self.active_panel,
                refreshing_devices,
                self.control_mode_enabled,
                self.drive_velocity,
            )
        });
        for action in actions {
            self.apply_ui_action(action);
        }
        if self.active_panel == ControlPanel::Control
            && !self.control_panel_setup_sent
            && matches!(
                &self.snapshot.connection,
                ConnectionStatus::Connected { .. }
            )
        {
            let enable_sent = self.send_pubsub_message(pubsub::Message::EnableControlMode);
            let telemetry_sent = self.send_pubsub_message(pubsub::Message::TelemetrySelection(
                pubsub::TelemetrySelection::RemoteControl,
            ));
            self.control_panel_setup_sent = enable_sent && telemetry_sent;
        }
        if self.active_panel == ControlPanel::Telemetry
            && !self.telemetry_panel_setup_sent
            && matches!(
                &self.snapshot.connection,
                ConnectionStatus::Connected { .. }
            )
        {
            self.telemetry_panel_setup_sent = self.send_pubsub_message(
                pubsub::Message::TelemetrySelection(self.telemetry_selection.clone()),
            );
        }
        if self.active_panel != ControlPanel::Control || !self.control_mode_enabled {
            self.stop_drive();
        }

        context.request_repaint_after(Duration::from_millis(100));
    }
}
