use crate::{
    connection, control, mission, settings,
    state::{self, UiAction},
    telemetry,
    transport::ConnectionStatus,
};
use bee_ui::{
    UiRoot,
    prelude::{Badge, Button, Container, HStack, SvgIcon},
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

pub(crate) struct Context {
    pub(crate) connection: connection::Context,
    pub(crate) control: control::Context,
    pub(crate) mission: mission::Context,
    pub(crate) telemetry: telemetry::Context,
    pub(crate) settings: settings::Context,
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
                    Button::primary(panel.title()).icon(panel.icon())
                } else {
                    Button::secondary(panel.title()).icon(panel.icon())
                };
                if ui.button(button).clicked() {
                    actions.push(UiAction::SelectPanel(panel));
                }
            }
            match &connection {
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
                actions.extend(connection::render(ui, &mut context.connection, ui_state));
            }
            ControlPanel::Control => {
                control::render(ui, &mut context.control, ui_state);
            }
            ControlPanel::Mission => mission::render(ui, &mut context.mission),
            ControlPanel::Telemetry => {
                telemetry::render(ui, &mut context.telemetry, ui_state);
            }
            ControlPanel::Settings => settings::render(ui, &mut context.settings),
        }
    });

    actions
}
