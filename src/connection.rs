use bee_ui::{
    UiRoot,
    prelude::{
        Alert, Badge, Button, Card, HStack, Label, ListGroup, ListGroupItem, Spinner, SvgIcon,
        Text, Variant,
    },
};

use crate::{
    transport::{ConnectionStatus, DiscoveredDevice, WorkerSnapshot},
    ui::UiAction,
};

pub(crate) fn reconcile_selected_device(
    devices: &[DiscoveredDevice],
    selected: &mut Option<String>,
) {
    if selected
        .as_deref()
        .is_some_and(|fullname| find_selected_device(devices, Some(fullname)).is_none())
    {
        *selected = None;
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
    snapshot: &WorkerSnapshot,
    selected_device: Option<&str>,
    last_error: Option<&str>,
    refreshing_devices: bool,
) -> Vec<UiAction> {
    let mut actions = Vec::new();

    ui.label(Label::new("Connection").large());
    ui.text(Text::new("Discover and connect to Motes on your local network.").subtle());

    ui.card(Card::with_title("Device discovery"), |ui| {
        ui.hstack(HStack::new().wrap(true), |ui| {
            if ui
                .button(
                    Button::secondary("Refresh devices")
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
            ui.alert(Alert::info("No Mote services discovered yet."));
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
    });

    let selected = selected_device.and_then(|fullname| {
        snapshot
            .devices
            .iter()
            .find(|device| device.fullname == fullname)
    });
    let can_connect = can_connect(selected);

    ui.card(Card::new().title("Selected device"), |ui| {
        ui.hstack(HStack::new().wrap(true), |ui| {
            if ui
                .button(
                    Button::primary("Connect")
                        .icon(SvgIcon::Plug)
                        .enabled(can_connect),
                )
                .clicked()
                && let Some(device) = selected
            {
                actions.push(UiAction::Connect(device.fullname.clone()));
            }
            if ui
                .button(Button::secondary("Disconnect").icon(SvgIcon::XCircle))
                .clicked()
            {
                actions.push(UiAction::Disconnect);
            }
            if let Some(device) = selected {
                ui.badge(Badge::info(format!("Selected: {}", device.uid)).as_pill());
            } else {
                ui.text(Text::new("Select a discovered Mote to connect.").subtle());
            }
        });
    });

    ui.card(Card::with_title("Connection status"), |ui| {
        match &snapshot.connection {
            ConnectionStatus::Disconnected => {
                ui.badge(Badge::secondary("Disconnected").as_pill());
            }
            ConnectionStatus::Connecting { uid, endpoint } => {
                ui.badge(Badge::warning("Connecting").as_pill());
                ui.alert(Alert::warning(format!(
                    "Connecting to {uid} at {endpoint}; waiting for protocol Pong…"
                )));
            }
            ConnectionStatus::Connected {
                uid,
                endpoint,
                last_seen_ago_ms,
            } => {
                ui.badge(Badge::success("Connected").as_pill());
                ui.text(Text::new(format!("Connected to {uid} at {endpoint}")));
                ui.text(Text::new(format!(
                    "Last Mote message: {:.1} seconds ago",
                    *last_seen_ago_ms as f64 / 1000.0
                )));
            }
            ConnectionStatus::Error(error) => {
                ui.badge(Badge::danger("Connection error").as_pill());
                ui.alert(Alert::danger(format!("Connection error: {error}")));
            }
        }

        ui.text(Text::new(format!(
            "Decoded incoming messages: {}",
            snapshot.incoming_messages
        )));
        ui.text(Text::new(format!(
            "Malformed/protocol packets: {} malformed, {} version mismatch",
            snapshot.malformed_packets, snapshot.version_mismatches
        )));
        if let Some(error) = &snapshot.last_protocol_error {
            ui.alert(Alert::danger(format!("Protocol: {error}")));
        }
    });

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
}
