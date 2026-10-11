use bee_ui::{
    UiRoot,
    prelude::{
        Alert, Badge, Button, Card, HStack, Label, ListGroup, ListGroupItem, RichText, Spinner,
        SvgIcon, Text, Variant,
    },
};

use crate::{
    state::{self, UiAction},
    transport::{ConnectionStatus, DiscoveredDevice},
};

#[derive(Default)]
pub(crate) struct Context {
    pub(crate) selected_device: Option<String>,
}

pub(crate) fn reconcile_selected_device(devices: &[DiscoveredDevice], context: &mut Context) {
    if context
        .selected_device
        .as_deref()
        .is_some_and(|fullname| find_selected_device(devices, Some(fullname)).is_none())
    {
        context.selected_device = None;
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

pub(crate) fn render(
    ui: &mut UiRoot<'_>,
    context: &mut Context,
    ui_state: &state::Context,
) -> Vec<UiAction> {
    let snapshot = &ui_state.snapshot;
    let last_error = ui_state.last_error.as_deref();
    let refreshing_devices = ui_state.refreshing_devices;
    let selected_device = context.selected_device.as_deref();
    let mut actions = Vec::new();

    let connection_text = RichText::new().text("Connection");
    ui.label(Label::new(connection_text).large());
    ui.text(Text::new("Discover and connect to Motes on your local network.").subtle());

    let selected = selected_device.and_then(|fullname| {
        snapshot
            .devices
            .iter()
            .find(|device| device.fullname == fullname)
    });
    let can_connect = can_connect(selected);
    let connected_uid = connected_device_uid(&snapshot.connection);

    ui.card(
        Card::with_title("Device discovery").variant(Variant::Secondary),
        |ui| {
            ui.hstack(HStack::new().wrap(true), |ui| {
                if ui
                    .button(
                        Button::primary("Refresh devices")
                            .icon(SvgIcon::ArrowClockwise)
                            .enabled(!refreshing_devices),
                    )
                    .clicked()
                {
                    actions.push(UiAction::Refresh);
                }
                if refreshing_devices {
                    ui.spinner(Spinner::new().size(18.0));
                }
                ui.label(Label::new(format!("{} discovered", snapshot.devices.len())).small());
            });
            ui.text(Text::new(snapshot.discovery_status.as_str()).subtle());

            if snapshot.devices.is_empty() {
                ui.alert(Alert::danger("No Mote services discovered yet."));
                ui.text(Text::new(
                    "Discovery uses the _mote-api._udp mDNS service on this LAN.",
                ));
            } else {
                let mut devices = ListGroup::new();
                for device in &snapshot.devices {
                    let selected = selected_device == Some(device.fullname.as_str());
                    let address = device.endpoint.map_or_else(
                        || "No IPv4 endpoint".to_owned(),
                        |endpoint| endpoint.to_string(),
                    );
                    devices = devices.item(
                        ListGroupItem::new(format!(
                            "{}  ·  {}  ·  {}\nHostname: {}  |  Service: {}",
                            device.uid, address, device.status, device.hostname, device.fullname
                        ))
                        .active(selected),
                    );
                }
                if let Some(index) = ui.list_group(devices).clicked_index()
                    && let Some(device) = snapshot.devices.get(index)
                {
                    actions.push(UiAction::SelectDevice(device.fullname.clone()));
                }
            }

            ui.hstack(HStack::new().wrap(true), |ui| {
                if ui
                    .button(
                        Button::primary("Connect")
                            .icon(SvgIcon::Plug)
                            .enabled(can_connect && connected_uid.is_none()),
                    )
                    .clicked()
                    && let Some(device) = selected
                {
                    actions.push(UiAction::Connect(device.fullname.clone()));
                }
                if ui
                    .button(
                        Button::secondary("Disconnect")
                            .icon(SvgIcon::XCircle)
                            .enabled(connected_uid.is_some()),
                    )
                    .clicked()
                {
                    actions.push(UiAction::Disconnect);
                }
                if let Some(device) = selected {
                    let variant = if connected_uid == Some(device.uid.as_str()) {
                        Variant::Success
                    } else {
                        Variant::Secondary
                    };
                    ui.badge(Badge::new(device.uid.clone()).variant(variant).as_pill());
                }
            });
        },
    );

    if let Some(error) = last_error {
        ui.alert(Alert::danger(error));
    }

    ui.text(
        Text::new(
            "The app keeps one UDP socket for the selected session, sends an immediate Ping, and pings once per second. A Pong is required to mark the connection live.",
        )
        .variant(Variant::Secondary),
    );

    actions
}

fn can_connect(selected: Option<&DiscoveredDevice>) -> bool {
    selected.is_some_and(|device| device.endpoint.is_some())
}

fn connected_device_uid(connection: &ConnectionStatus) -> Option<&str> {
    match connection {
        ConnectionStatus::Connected { uid, .. } => Some(uid),
        ConnectionStatus::Disconnected
        | ConnectionStatus::Connecting { .. }
        | ConnectionStatus::Error(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::*;

    fn device(endpoint: Option<SocketAddr>) -> DiscoveredDevice {
        DiscoveredDevice {
            fullname: "mote-1._mote-api._udp.local.".to_owned(),
            uid: "mote-1".to_owned(),
            hostname: "mote-1.local.".to_owned(),
            endpoint,
            status: "resolved".to_owned(),
        }
    }

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
        let mut context = Context {
            selected_device: Some("mote-1._mote-api._udp.local.".to_owned()),
        };

        reconcile_selected_device(&devices, &mut context);

        assert_eq!(context.selected_device, None);
    }

    #[test]
    fn selection_is_retained_while_device_remains_discovered() {
        let fullname = "mote-1._mote-api._udp.local.";
        let devices = [discovered_device(fullname)];
        let mut context = Context {
            selected_device: Some(fullname.to_owned()),
        };

        reconcile_selected_device(&devices, &mut context);

        assert_eq!(context.selected_device.as_deref(), Some(fullname));
    }

    #[test]
    fn selected_device_lookup_requires_a_current_discovery_entry() {
        let devices = [discovered_device("mote-1._mote-api._udp.local.")];

        assert!(find_selected_device(&devices, None).is_none());
        assert!(find_selected_device(&devices, Some("unknown")).is_none());
        assert_eq!(
            find_selected_device(&devices, Some("mote-1._mote-api._udp.local."))
                .map(|device| device.uid.as_str()),
            Some("mote-1")
        );
    }

    #[test]
    fn connect_requires_a_selected_device_with_a_resolved_endpoint() {
        let resolved = device(Some(SocketAddr::from(([127, 0, 0, 1], 7475))));
        let unresolved = device(None);

        assert!(!can_connect(None));
        assert!(!can_connect(Some(&unresolved)));
        assert!(can_connect(Some(&resolved)));
    }

    #[test]
    fn connected_device_uid_excludes_connecting_and_disconnected_states() {
        let endpoint = SocketAddr::from(([127, 0, 0, 1], 7475));
        let connected = ConnectionStatus::Connected {
            uid: "mote-1".to_owned(),
            endpoint,
            last_seen_ago_ms: 0,
        };
        let connecting = ConnectionStatus::Connecting {
            uid: "mote-1".to_owned(),
            endpoint,
        };

        assert_eq!(connected_device_uid(&connected), Some("mote-1"));
        assert_eq!(connected_device_uid(&connecting), None);
        assert_eq!(connected_device_uid(&ConnectionStatus::Disconnected), None);
        assert_eq!(
            connected_device_uid(&ConnectionStatus::Error("failed".to_owned())),
            None
        );
    }
}
