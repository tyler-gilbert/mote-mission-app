use std::{
    sync::mpsc::TryRecvError,
    time::{Duration, Instant},
};

use eframe::egui;
use mote_api::messages::host_to_mote;

use crate::{
    connection,
    state::{self, UiAction},
    transport::{self, ConnectionStatus, TransportWorker, WorkerCommand, WorkerSnapshot},
    ui::{self, ControlPanel},
};

const DEVICE_REFRESH_DURATION: Duration = Duration::from_secs(30);

pub(crate) struct MoteApp {
    ui_context: ui::Context,
    ui_state: state::Context,
}

impl MoteApp {
    pub(crate) fn new() -> Self {
        Self {
            ui_context: ui::Context {
                connection: connection::Context::default(),
                control: crate::control::Context::default(),
                mission: crate::mission::Context,
                telemetry: crate::telemetry::Context::default(),
                settings: crate::settings::Context::default(),
            },
            ui_state: state::Context {
                worker: TransportWorker::start(),
                device_refresh_until: None,
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
                last_error: None,
                active_panel: ControlPanel::Connection,
                refreshing_devices: false,
            },
        }
    }

    fn process_worker_events(&mut self) {
        loop {
            match self.ui_state.worker.try_snapshot() {
                Ok(snapshot) => self.ui_state.snapshot = snapshot,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.ui_state.last_error =
                        Some("Transport worker stopped unexpectedly".to_owned());
                    break;
                }
            }
        }

        connection::reconcile_selected_device(
            &self.ui_state.snapshot.devices,
            &mut self.ui_context.connection,
        );
        if !matches!(
            &self.ui_state.snapshot.connection,
            ConnectionStatus::Connected { .. }
        ) {
            self.ui_context.control.telemetry_selection_sent = false;
            self.ui_context.control.mode_enabled = false;
            self.ui_context.control.drive_velocity = (0.0, 0.0);
        }
    }

    fn send_command(&mut self, command: WorkerCommand) -> bool {
        let result = self.ui_state.worker.command(command);
        self.handle_send_result(result)
    }

    fn handle_send_result(&mut self, result: Result<(), String>) -> bool {
        match result {
            Ok(()) => {
                self.ui_state.last_error = None;
                true
            }
            Err(error) => {
                self.ui_state.last_error = Some(error);
                false
            }
        }
    }

    fn stop_drive(&mut self) {
        if self.ui_context.control.drive_velocity != (0.0, 0.0)
            && self.send_command(WorkerCommand::HostToMote(
                host_to_mote::Message::SetDriveBaseVelocity(host_to_mote::SetDriveBaseVelocity {
                    left_velocity_rad_per_s: 0.0,
                    right_velocity_rad_per_s: 0.0,
                }),
            ))
        {
            self.ui_context.control.drive_velocity = (0.0, 0.0);
        }
    }

    fn apply_ui_action(&mut self, action: UiAction) {
        match action {
            UiAction::SelectPanel(panel) => {
                self.ui_state.active_panel = panel;
            }
            UiAction::Refresh => {
                let result = self.ui_state.worker.refresh();
                if self.handle_send_result(result) {
                    self.ui_state.device_refresh_until =
                        Some(Instant::now() + DEVICE_REFRESH_DURATION);
                }
            }
            UiAction::SelectDevice(fullname) => {
                self.ui_context.connection.selected_device = Some(fullname);
            }
            UiAction::Connect(fullname) => {
                let result = self.ui_state.worker.connect(fullname);
                self.handle_send_result(result);
            }
            UiAction::Disconnect => {
                let result = self.ui_state.worker.disconnect();
                self.handle_send_result(result);
            }
        }
    }
}

impl eframe::App for MoteApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_worker_events();
        if self.ui_state.device_refresh_until.is_some_and(|deadline| {
            !self.ui_state.snapshot.devices.is_empty() || Instant::now() >= deadline
        }) {
            self.ui_state.device_refresh_until = None;
        }
        self.ui_state.refreshing_devices = self.ui_state.device_refresh_until.is_some();

        let actions = bee_ui::show_root(context, |ui| {
            ui::render(ui, &mut self.ui_context, &mut self.ui_state)
        });
        for action in actions {
            self.apply_ui_action(action);
        }
        if !self.ui_context.control.mode_enabled {
            self.stop_drive();
        }

        context.request_repaint_after(Duration::from_millis(100));
    }
}
