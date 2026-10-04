mod connection;
mod control;
mod mission;
mod settings;
mod telemetry;
mod transport;
mod ui;

use std::{sync::mpsc::TryRecvError, time::Duration};

use eframe::egui;
use transport::{ConnectionStatus, TransportWorker, WorkerCommand, WorkerSnapshot};
use ui::{ControlPanel, UiAction};

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
            self.control_mode_enabled = false;
            self.drive_velocity = (0.0, 0.0);
        }
    }

    fn send_command(&mut self, command: WorkerCommand) -> bool {
        match self.worker.command(command) {
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

    fn stop_drive(&mut self) {
        if self.drive_velocity != (0.0, 0.0)
            && self.send_command(WorkerCommand::SetDriveVelocity {
                left_velocity_rad_per_s: 0.0,
                right_velocity_rad_per_s: 0.0,
            })
        {
            self.drive_velocity = (0.0, 0.0);
        }
    }

    fn apply_ui_action(&mut self, action: UiAction) {
        match action {
            UiAction::SelectPanel(panel) => self.active_panel = panel,
            UiAction::Refresh => {
                self.send_command(WorkerCommand::Refresh);
            }
            UiAction::SelectDevice(fullname) => self.selected_device = Some(fullname),
            UiAction::Connect(fullname) => {
                self.send_command(WorkerCommand::Connect(fullname));
            }
            UiAction::Disconnect => {
                self.send_command(WorkerCommand::Disconnect);
            }
            UiAction::SetMode(enabled) => {
                if self.send_command(WorkerCommand::SetControlMode(enabled)) {
                    self.control_mode_enabled = enabled;
                }
            }
            UiAction::Drive {
                left_velocity_rad_per_s,
                right_velocity_rad_per_s,
            } => {
                if self.send_command(WorkerCommand::SetDriveVelocity {
                    left_velocity_rad_per_s,
                    right_velocity_rad_per_s,
                }) {
                    self.drive_velocity = (left_velocity_rad_per_s, right_velocity_rad_per_s);
                }
            }
        }
    }
}

impl eframe::App for MoteApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_worker_events();

        let actions = bee_ui::show_root(context, |ui| {
            ui::render(
                ui,
                &self.snapshot,
                self.selected_device.as_deref(),
                self.last_error.as_deref(),
                self.active_panel,
                self.control_mode_enabled,
                self.drive_velocity,
            )
        });
        for action in actions {
            self.apply_ui_action(action);
        }
        if self.active_panel != ControlPanel::Control || !self.control_mode_enabled {
            self.stop_drive();
        }

        context.request_repaint_after(Duration::from_millis(100));
    }
}
