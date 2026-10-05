use bee_ui::{
    UiRoot,
    prelude::{Badge, Button, Card, HStack, Label, SvgIcon, Text},
};

#[derive(Default)]
pub(crate) struct Context;

pub(crate) fn render(ui: &mut UiRoot<'_>, _context: &mut Context) {
    ui.label(Label::new("Settings").large());
    ui.text(Text::new("Adjust the app appearance and interface scale.").subtle());

    ui.card(Card::with_title("Appearance"), |ui| {
        ui.label(Label::new("Color theme").small());
        let theme = ui.context().theme();
        ui.hstack(HStack::new().wrap(true), |ui| {
            const LIGHT_ICON: SvgIcon = SvgIcon::Sun;
            let light_button = if theme == eframe::egui::Theme::Light {
                Button::primary("Light").icon(LIGHT_ICON)
            } else {
                Button::secondary("Light").icon(LIGHT_ICON)
            };
            if ui.button(light_button).clicked() {
                ui.context().set_theme(eframe::egui::Theme::Light);
            }

            const DARK_ICON: SvgIcon = SvgIcon::Moon;
            let dark_button = if theme == eframe::egui::Theme::Dark {
                Button::primary("Dark").icon(DARK_ICON)
            } else {
                Button::secondary("Dark").icon(DARK_ICON)
            };
            if ui.button(dark_button).clicked() {
                ui.context().set_theme(eframe::egui::Theme::Dark);
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
