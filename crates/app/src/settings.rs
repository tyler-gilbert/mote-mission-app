use std::{
    fs,
    time::{Duration, Instant},
};

use bee_ui::{
    UiRoot,
    prelude::{Alert, Badge, Button, Card, FormControl, HStack, Label, SvgIcon, Text, Variant},
};
use eframe::egui;
use mote_api::messages::router;

use crate::{control, state, transport::ConnectionStatus};

pub(crate) struct Context {
    settings_path: String,
    settings_preview: Option<router::Settings>,
    settings_result: Option<Result<String, String>>,
    send_disabled_until: Option<Instant>,
}

impl Context {
    pub(crate) fn new(settings_path: String) -> Self {
        let (settings_preview, settings_result) = if settings_path.trim().is_empty() {
            (None, None)
        } else {
            match load_settings(&settings_path) {
                Ok(settings) => (Some(settings), None),
                Err(error) => (None, Some(Err(error))),
            }
        };
        Self {
            settings_path,
            settings_preview,
            settings_result,
            send_disabled_until: None,
        }
    }

    pub(crate) fn global_center(&self) -> Option<(f64, f64)> {
        let reference_frame = &self.settings_preview.as_ref()?.reference_frame;
        let latitude: f32 = reference_frame.global_center.latitude.into();
        let longitude: f32 = reference_frame.global_center.longitude.into();
        let latitude = f64::from(latitude);
        let longitude = f64::from(longitude);
        if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
            return None;
        }
        Some((latitude, longitude))
    }

    pub(crate) fn global_diagonal_meters(&self) -> Option<f64> {
        let diagonal: f32 = self
            .settings_preview
            .as_ref()?
            .reference_frame
            .global_diagonal
            .into();
        (diagonal.is_finite() && diagonal > 0.0).then(|| f64::from(diagonal))
    }

    pub(crate) fn reference_frame(&self) -> Option<&router::ReferenceFrame> {
        self.settings_preview
            .as_ref()
            .map(|settings| &settings.reference_frame)
    }

    pub(crate) fn route(&self) -> Option<&router::Route> {
        self.settings_preview
            .as_ref()
            .map(|settings| &settings.route)
    }

    pub(crate) fn expected_hashes(&self) -> Option<(u32, u32)> {
        let settings = self.settings_preview.as_ref()?;
        Some((
            settings.reference_frame.hash(),
            router::hash_route(&settings.route),
        ))
    }
}

pub(crate) fn render(
    ui: &mut UiRoot<'_>,
    context: &mut Context,
    control_context: &mut control::Context,
    ui_state: &mut state::Context,
) {
    ui.label(Label::new("Settings").large());
    let connected = matches!(
        ui_state.snapshot.connection,
        ConnectionStatus::Connected { .. }
    );
    let connected_uid = match &ui_state.snapshot.connection {
        ConnectionStatus::Connected { uid, .. } => Some(uid.clone()),
        _ => None,
    };

    ui.card(
            Card::with_title("Mission settings").variant(Variant::Secondary),
            |ui| {
        ui.text(Text::new(
            "Load a JSON settings file containing both the reference frame and route. Settings can be prepared while disconnected.",
        ));
        if ui
            .form_control(&mut context.settings_path, FormControl::new())
            .changed()
        {
            context.settings_result = None;
            context.settings_preview = load_settings(&context.settings_path).ok();
            ui_state.persistent_settings.mission_settings_path = context.settings_path.clone();
            if let Err(error) = ui_state.persistent_settings.save() {
                context.settings_result = Some(Err(error));
            }
            control_context.clear_plan();
        }

        let device_hashes = if connected {
            ui_state
                .snapshot
                .latest_telemetry
                .debug
                .as_ref()
                .map(|debug| (debug.reference_frame_hash, debug.route_hash))
        } else {
            None
        };

        let cooldown_remaining = context
            .send_disabled_until
            .map(|disabled_until| disabled_until.saturating_duration_since(Instant::now()))
            .unwrap_or_default();
        if !cooldown_remaining.is_zero() {
            ui.context().request_repaint_after(cooldown_remaining);
        }

        let send_clicked = ui.hstack(HStack::new().gap(4.0).wrap(true), |ui| {
            let clicked = ui
                .button(
                    Button::primary("Send settings")
                        .icon(SvgIcon::Send)
                        .enabled(connected && cooldown_remaining.is_zero()),
                )
                .clicked();
            let local_variant = if context.settings_preview.is_some() {
                Variant::Success
            } else {
                Variant::Secondary
            };
            ui.button(
                Button::new("Local")
                    .variant(local_variant)
                    .icon(SvgIcon::CheckCircleFill)
                    .disabled()
                    .hover_text("Settings loaded locally"),
            );
            if let Some((reference_frame_hash, route_hash)) = context.expected_hashes() {
                ui.badge(
                    Badge::new(format!("Reference frame: {reference_frame_hash:#010x}"))
                        .variant(hash_variant(
                            reference_frame_hash,
                            device_hashes.map(|hashes| hashes.0),
                        ))
                        .as_pill(),
                );
                ui.badge(
                    Badge::new(format!("Route: {route_hash:#010x}"))
                        .variant(hash_variant(route_hash, device_hashes.map(|hashes| hashes.1)))
                        .as_pill(),
                );
            }
            if !control_context.plan_confirmed()
                && let Some(Ok(message)) = context.settings_result.as_ref()
            {
                ui.text(Text::new(message.as_str()).subtle());
            }
            clicked
        });

        if send_clicked {
            context.send_disabled_until = Some(Instant::now() + Duration::from_secs(1));
            ui.context().request_repaint_after(Duration::from_secs(1));
            context.settings_result = Some(match load_settings(&context.settings_path) {
                Ok(settings) => {
                    context.settings_preview = Some(settings.clone());
                    control_context.queue_plan(settings);
                    Ok(format!(
                        "Plan queued for {}. Waiting for device confirmation.",
                        connected_uid.as_deref().unwrap_or("Mote")
                    ))
                }
                Err(error) => {
                    context.settings_preview = None;
                    control_context.clear_plan();
                    Err(error)
                }
            });
        }
        render_send_result(ui, context.settings_result.as_ref());
    },
    );

    ui.card(
        Card::with_title("Appearance").variant(Variant::Secondary),
        |ui| {
            ui.label(Label::new("Color theme").small());
            ui.hstack(HStack::new().wrap(true), |ui| {
                if ui
                    .button(
                        Button::new("Light")
                            .variant(Variant::Light)
                            .icon(SvgIcon::Sun),
                    )
                    .clicked()
                {
                    ui.context().set_theme(egui::Theme::Light);
                }

                if ui
                    .button(
                        Button::new("Dark")
                            .variant(Variant::Dark)
                            .icon(SvgIcon::Moon),
                    )
                    .clicked()
                {
                    ui.context().set_theme(egui::Theme::Dark);
                }
            });

            ui.label(Label::new("Interface zoom").small());
            let zoom = ui.context().zoom_factor();
            ui.hstack(HStack::new().wrap(true), |ui| {
                if ui
                    .button(
                        Button::secondary("Zoom -")
                            .icon(SvgIcon::ZoomOut)
                            .enabled(zoom > 0.5),
                    )
                    .clicked()
                {
                    ui.context().set_zoom_factor((zoom - 0.1).max(0.5));
                }
                ui.badge(Badge::secondary(format!("{:.0}%", zoom * 100.0)).as_pill());
                if ui
                    .button(
                        Button::secondary("Zoom +")
                            .icon(SvgIcon::ZoomIn)
                            .enabled(zoom < 3.0),
                    )
                    .clicked()
                {
                    ui.context().set_zoom_factor((zoom + 0.1).min(3.0));
                }
            });
            ui.text(Text::new("Zoom range: 50%–300%.").subtle());
        },
    );
}

fn load_settings(path: &str) -> Result<router::Settings, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err("Enter a path to a settings JSON file.".to_owned());
    }

    let json = fs::read_to_string(path)
        .map_err(|error| format!("Could not read settings file '{path}': {error}"))?;
    parse_settings(&json)
        .map_err(|error| format!("Could not parse settings file '{path}': {error}"))
}

fn parse_settings(input: &str) -> Result<router::Settings, serde_json::Error> {
    serde_json::from_str(input)
}

fn hash_variant(expected: u32, device: Option<u32>) -> Variant {
    match device {
        None => Variant::Secondary,
        Some(device) if device == expected => Variant::Success,
        Some(_) => Variant::Danger,
    }
}

fn render_send_result(ui: &mut UiRoot<'_>, result: Option<&Result<String, String>>) {
    if let Some(Err(error)) = result {
        ui.alert(Alert::danger(error.as_str()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_settings_with_reference_frame_and_route() {
        let settings = parse_settings(
            r#"{
                "reference_frame": {
                    "global_center": {"latitude": 37.081481615, "longitude": -121.5968056},
                    "global_diagonal": 20000.0,
                    "local_diagonal": 3.0,
                    "structure_side_wall_length": 0.2032
                },
                "route": [
                    {"latitude": 37.07924456, "longitude": -121.60339},
                    {"latitude": 37.07632934, "longitude": -121.60176305}
                ]
            }"#,
        )
        .unwrap();

        let reference_frame = &settings.reference_frame;
        assert!(reference_frame.global_diagonal == units::Length::new(20_000.0));
        assert!(reference_frame.local_diagonal == units::Length::new(3.0));
        assert!(reference_frame.structure_side_wall_length == units::Length::new(0.2032));
        assert_eq!(settings.route.len(), 2);
        assert!(settings.route[0].latitude == units::imperial::Degrees::new(37.079_243));
    }

    #[test]
    fn hash_variant_reflects_connected_device_hash() {
        assert_eq!(hash_variant(0x1234, None), Variant::Secondary);
        assert_eq!(hash_variant(0x1234, Some(0x1234)), Variant::Success);
        assert_eq!(hash_variant(0x1234, Some(0x4321)), Variant::Danger);
    }
}
