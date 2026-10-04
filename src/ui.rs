use bee_ui::{
    UiRoot,
    prelude::{Badge, Button, Container, HStack, SvgIcon},
};
use mote_api::messages::pubsub;

use crate::{
    connection, control, mission, settings, telemetry,
    transport::{ConnectionStatus, WorkerSnapshot},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlPanel {
    Connection,
    Control,
    Mission,
    Telemetry,
    Settings,
}

impl ControlPanel {
    const ALL: [Self; 5] = [
        Self::Connection,
        Self::Control,
        Self::Mission,
        Self::Telemetry,
        Self::Settings,
    ];

    const fn title(self) -> &'static str {
        match self {
            Self::Connection => "Connection",
            Self::Control => "Control",
            Self::Mission => "Mission",
            Self::Telemetry => "Telemetry",
            Self::Settings => "Settings",
        }
    }

    const fn icon(self) -> SvgIcon {
        match self {
            Self::Connection => SvgIcon::HddNetwork,
            Self::Control => SvgIcon::Joystick,
            Self::Mission => SvgIcon::AirplaneFill,
            Self::Telemetry => SvgIcon::Activity,
            Self::Settings => SvgIcon::Gear,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum UiAction {
    SelectPanel(ControlPanel),
    Refresh,
    SelectDevice(String),
    Connect(String),
    Disconnect,
    PubSub(pubsub::Message),
}

pub(crate) fn render(
    ui: &mut UiRoot<'_>,
    snapshot: &WorkerSnapshot,
    selected_device: Option<&str>,
    last_error: Option<&str>,
    active_panel: ControlPanel,
    refreshing_devices: bool,
    mode_enabled: bool,
    drive_velocity: (f32, f32),
) -> Vec<UiAction> {
    let mut actions = Vec::new();
    let mut container = Container::fluid().props().fill_height();
    if !matches!(
        active_panel,
        ControlPanel::Mission | ControlPanel::Telemetry
    ) {
        container = container.scroll_y();
    }

    ui.container(container, |ui| {
        ui.hstack(HStack::new().wrap(true), |ui| {
            for panel in ControlPanel::ALL {
                let button = if panel == active_panel {
                    Button::primary(panel.title()).icon(panel.icon())
                } else {
                    Button::secondary(panel.title()).icon(panel.icon())
                };
                if ui.button(button).clicked() {
                    actions.push(UiAction::SelectPanel(panel));
                }
            }
            match &snapshot.connection {
                ConnectionStatus::Disconnected => {
                    ui.badge(Badge::secondary("Disconnected").as_pill());
                }
                ConnectionStatus::Connecting { uid, .. } => {
                    ui.badge(Badge::warning(format!("Connecting to {uid}")).as_pill());
                }
                ConnectionStatus::Connected { uid, .. } => {
                    ui.badge(Badge::success(format!("Connected to {uid}")).as_pill());
                }
                ConnectionStatus::Error(_) => {
                    ui.badge(Badge::danger("Connection error").as_pill());
                }
            }
        });

        match active_panel {
            ControlPanel::Connection => {
                actions.extend(connection::render(
                    ui,
                    snapshot,
                    selected_device,
                    last_error,
                    refreshing_devices,
                ));
            }
            ControlPanel::Control => {
                actions.extend(
                    control::render(
                        ui,
                        &snapshot.latest_telemetry,
                        &snapshot.connection,
                        last_error,
                        mode_enabled,
                        drive_velocity,
                    )
                    .into_iter()
                    .map(UiAction::PubSub),
                );
            }
            ControlPanel::Mission => mission::render(ui),
            ControlPanel::Telemetry => {
                if let Some(message) = telemetry::render(ui, &snapshot.latest_telemetry) {
                    actions.push(UiAction::PubSub(message));
                }
            }
            ControlPanel::Settings => settings::render(ui),
        }
    });

    actions
}
