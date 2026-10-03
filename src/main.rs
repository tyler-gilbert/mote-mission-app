mod transport;
mod ui;

use std::{sync::mpsc::TryRecvError, time::Duration};

use eframe::egui;
use transport::{
    ConnectionStatus, DiscoveredDevice, TransportWorker, WorkerCommand, WorkerSnapshot,
};
use ui::UiAction;

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
            bee_ui::theme::install_default_theme(&creation_context.egui_ctx);
            Ok(Box::new(MoteApp::new()))
        }),
    )
}

struct MoteApp {
    worker: TransportWorker,
    snapshot: WorkerSnapshot,
    selected_device: Option<String>,
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
            },
            selected_device: None,
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

        reconcile_selected_device(&self.snapshot.devices, &mut self.selected_device);
    }

    fn send_command(&mut self, command: WorkerCommand) {
        match self.worker.command(command) {
            Ok(()) => self.last_error = None,
            Err(error) => self.last_error = Some(error),
        }
    }

    fn selected_device(&self) -> Option<&DiscoveredDevice> {
        find_selected_device(&self.snapshot.devices, self.selected_device.as_deref())
    }

    fn apply_ui_action(&mut self, action: UiAction) {
        match action {
            UiAction::Refresh => self.send_command(WorkerCommand::Refresh),
            UiAction::SelectDevice(fullname) => self.selected_device = Some(fullname),
            UiAction::Connect(fullname) => {
                self.send_command(WorkerCommand::Connect(fullname));
            }
            UiAction::Disconnect => self.send_command(WorkerCommand::Disconnect),
        }
    }
}

fn find_selected_device<'a>(
    devices: &'a [DiscoveredDevice],
    selected_fullname: Option<&str>,
) -> Option<&'a DiscoveredDevice> {
    let selected_fullname = selected_fullname?;
    devices
        .iter()
        .find(|device| device.fullname == selected_fullname)
}

fn reconcile_selected_device(devices: &[DiscoveredDevice], selected: &mut Option<String>) {
    if selected
        .as_deref()
        .is_some_and(|fullname| find_selected_device(devices, Some(fullname)).is_none())
    {
        *selected = None;
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
            )
        });
        for action in actions {
            self.apply_ui_action(action);
        }

        context.request_repaint_after(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::*;

    fn discovered_device(fullname: &str) -> DiscoveredDevice {
        DiscoveredDevice {
            fullname: fullname.to_owned(),
            uid: "mote-1".to_owned(),
            hostname: "mote-1.local.".to_owned(),
            endpoint: Some(SocketAddr::from(([127, 0, 0, 1], 7475))),
            status: "resolved".to_owned(),
        }
    }

    #[test]
    fn selection_is_cleared_when_device_disappears() {
        let devices = [discovered_device("mote-2._mote-api._udp.local.")];
        let mut selected = Some("mote-1._mote-api._udp.local.".to_owned());

        reconcile_selected_device(&devices, &mut selected);

        assert_eq!(selected, None);
    }

    #[test]
    fn selection_is_retained_while_device_remains_discovered() {
        let fullname = "mote-1._mote-api._udp.local.";
        let devices = [discovered_device(fullname)];
        let mut selected = Some(fullname.to_owned());

        reconcile_selected_device(&devices, &mut selected);

        assert_eq!(selected.as_deref(), Some(fullname));
    }

    #[test]
    fn selected_device_lookup_requires_a_current_discovery_entry() {
        let device = discovered_device("mote-1._mote-api._udp.local.");
        let devices = [device];

        assert!(find_selected_device(&devices, None).is_none());
        assert!(find_selected_device(&devices, Some("unknown")).is_none());
        assert_eq!(
            find_selected_device(&devices, Some("mote-1._mote-api._udp.local."))
                .map(|device| device.uid.as_str()),
            Some("mote-1")
        );
    }
}
