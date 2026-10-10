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

#[derive(Default)]
pub(crate) struct Context {
    settings_path: String,
    settings_preview: Option<router::Settings>,
    settings_result: Option<Result<String, String>>,
    send_disabled_until: Option<Instant>,
}

impl Context {
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
    ui.text(Text::new("Adjust the app appearance and interface scale.").subtle());

    let connected = matches!(
        ui_state.snapshot.connection,
        ConnectionStatus::Connected { .. }
    );
    let connected_uid = match &ui_state.snapshot.connection {
        ConnectionStatus::Connected { uid, .. } => Some(uid.clone()),
        _ => None,
    };

    ui.card(Card::with_title("Mission settings"), |ui| {
        ui.text(Text::new(
            "Load a JSON settings file containing both the reference frame and route. Settings can be prepared while disconnected.",
        ));
        ui.label(Label::new("Settings JSON file path").small());
        if ui
            .form_control(&mut context.settings_path, FormControl::new())
            .changed()
        {
            context.settings_result = None;
            context.settings_preview = load_settings(&context.settings_path).ok();
            control_context.clear_plan();
        }

        if let Some(settings) = context.settings_preview.as_ref() {
            ui.label(Label::new("Expected device hashes").small());
            ui.hstack(HStack::new().wrap(true), |ui| {
                ui.badge(
                    Badge::info(format!(
                        "Reference frame: {:#010x}",
                        settings.reference_frame.hash()
                    ))
                    .as_pill(),
                );
                ui.badge(
                    Badge::info(format!(
                        "Route: {:#010x}",
                        router::hash_route(&settings.route)
                    ))
                    .as_pill(),
                );
            });
            ui.text(Text::new("Compare these with the hashes reported by the device.").subtle());
        }

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
    });

    ui.card(Card::with_title("Appearance"), |ui| {
        ui.label(Label::new("Color theme").small());
        let theme = ui.context().theme();
        ui.hstack(HStack::new().wrap(true), |ui| {
            const LIGHT_ICON: SvgIcon = SvgIcon::Sun;
            let light_button = if theme == egui::Theme::Light {
                Button::primary("Light").icon(LIGHT_ICON)
            } else {
                Button::secondary("Light").icon(LIGHT_ICON)
            };
            if ui.button(light_button).clicked() {
                ui.context().set_theme(egui::Theme::Light);
            }

            const DARK_ICON: SvgIcon = SvgIcon::Moon;
            let dark_button = if theme == egui::Theme::Dark {
                Button::primary("Dark").icon(DARK_ICON)
            } else {
                Button::secondary("Dark").icon(DARK_ICON)
            };
            if ui.button(dark_button).clicked() {
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
            ui.badge(Badge::info(format!("{:.0}%", zoom * 100.0)).as_pill());
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
    });
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
                    "local_diagonal": 3.0
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
        assert_eq!(settings.route.len(), 2);
        assert!(settings.route[0].latitude == units::imperial::Degrees::new(37.07924456));
    }
}
