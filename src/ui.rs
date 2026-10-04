use bee_ui::{
    UiRoot,
    prelude::{Button, Container, HStack, SvgIcon},
};

use crate::{connection, control, mission, settings, telemetry, transport::WorkerSnapshot};

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
            Self::Control => SvgIcon::Activity,
            Self::Mission => SvgIcon::RocketTakeoff,
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
    SetMode(bool),
    Drive {
        left_velocity_rad_per_s: f32,
        right_velocity_rad_per_s: f32,
    },
}

pub(crate) fn render(
    ui: &mut UiRoot<'_>,
    snapshot: &WorkerSnapshot,
    selected_device: Option<&str>,
    last_error: Option<&str>,
    active_panel: ControlPanel,
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
        });

        match active_panel {
            ControlPanel::Connection => {
                actions.extend(connection::render(
                    ui,
                    snapshot,
                    selected_device,
                    last_error,
                ));
            }
            ControlPanel::Control => {
                actions.extend(
                    control::render(ui, snapshot, last_error, mode_enabled, drive_velocity)
                        .into_iter()
                        .map(|action| match action {
                            control::ControlAction::SetMode(enabled) => UiAction::SetMode(enabled),
                            control::ControlAction::Drive {
                                left_velocity_rad_per_s,
                                right_velocity_rad_per_s,
                            } => UiAction::Drive {
                                left_velocity_rad_per_s,
                                right_velocity_rad_per_s,
                            },
                        }),
                );
            }
            ControlPanel::Mission => mission::render(ui),
            ControlPanel::Telemetry => telemetry::render(ui),
            ControlPanel::Settings => settings::render(ui),
        }
    });

    actions
}
