use crate::{
    connection, control, mission, settings,
    state::{self, UiAction},
    telemetry,
    transport::ConnectionStatus,
};
use bee_ui::{
    UiRoot,
    prelude::{Button, Container, HStack, SvgIcon},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ControlPanel {
    Connection,
    Mission,
    Telemetry,
    Settings,
}

impl ControlPanel {
    const ALL: [Self; 4] = [
        Self::Connection,
        Self::Mission,
        Self::Telemetry,
        Self::Settings,
    ];

    const fn title(self) -> &'static str {
        match self {
            Self::Connection => "Connection",
            Self::Mission => "Mission",
            Self::Telemetry => "Telemetry",
            Self::Settings => "Settings",
        }
    }

    const fn icon(self) -> SvgIcon {
        match self {
            Self::Connection => SvgIcon::HddNetwork,
            Self::Mission => SvgIcon::AirplaneFill,
            Self::Telemetry => SvgIcon::Activity,
            Self::Settings => SvgIcon::Gear,
        }
    }
}

pub(crate) struct Context {
    pub(crate) connection: connection::Context,
    pub(crate) control: control::Context,
    pub(crate) mission: mission::Context,
    pub(crate) telemetry: telemetry::Context,
    pub(crate) settings: settings::Context,
}

pub(crate) fn render_connection_status(ui: &mut UiRoot<'_>, connection: &ConnectionStatus) {
    let (icon, tooltip, variant) = match connection {
        ConnectionStatus::Disconnected => (
            SvgIcon::WifiOff,
            "Disconnected from a Mote".to_owned(),
            bee_ui::prelude::Variant::Secondary,
        ),
        ConnectionStatus::Connecting { uid, .. } => (
            SvgIcon::Wifi,
            format!("Connecting to {uid}"),
            bee_ui::prelude::Variant::Warning,
        ),
        ConnectionStatus::Connected { uid, .. } => (
            SvgIcon::Wifi,
            format!("Connected to {uid}"),
            bee_ui::prelude::Variant::Success,
        ),
        ConnectionStatus::Error(error) => (
            SvgIcon::WifiOff,
            format!("Connection error: {error}"),
            bee_ui::prelude::Variant::Danger,
        ),
    };
    ui.button(
        Button::new("")
            .variant(variant)
            .icon(icon)
            .disabled()
            .hover_text(tooltip),
    );
}

pub(crate) fn render(
    ui: &mut UiRoot<'_>,
    context: &mut Context,
    ui_state: &mut state::Context,
) -> Vec<UiAction> {
    let connection = ui_state.snapshot.connection.clone();
    let active_panel = ui_state.active_panel;
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
                    Button::primary("")
                        .icon(panel.icon())
                        .hover_text(panel.title())
                } else {
                    Button::secondary("")
                        .icon(panel.icon())
                        .hover_text(panel.title())
                };
                if ui.button(button).clicked() {
                    actions.push(UiAction::SelectPanel(panel));
                }
            }
            control::render(ui, &mut context.control, ui_state);
            render_connection_status(ui, &connection);
        });

        match active_panel {
            ControlPanel::Connection => {
                actions.extend(connection::render(ui, &mut context.connection, ui_state));
            }
            ControlPanel::Mission => mission::render(ui, &mut context.mission, &context.settings),
            ControlPanel::Telemetry => {
                telemetry::render(ui, &mut context.telemetry, ui_state);
            }
            ControlPanel::Settings => settings::render(ui, &mut context.settings, ui_state),
        }
    });

    actions
}
