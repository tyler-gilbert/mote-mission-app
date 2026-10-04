use bee_ui::{
    UiRoot,
    prelude::{Badge, Button, Card, HStack, Label, SvgIcon, Text},
};

pub(crate) fn render(ui: &mut UiRoot<'_>) {
    ui.label(Label::new("Settings").large());
    ui.text(Text::new("Adjust the app appearance and interface scale.").subtle());

    ui.card(Card::with_title("Appearance"), |ui| {
        ui.label(Label::new("Color theme").small());
        let theme = ui.context().theme();
        ui.hstack(HStack::new().wrap(true), |ui| {
            let light_button = if theme == eframe::egui::Theme::Light {
                Button::primary("Light").icon(SvgIcon::Sun)
            } else {
                Button::secondary("Light").icon(SvgIcon::Sun)
            };
            if ui.button(light_button).clicked() {
                ui.context().set_theme(eframe::egui::Theme::Light);
            }

            let dark_button = if theme == eframe::egui::Theme::Dark {
                Button::primary("Dark").icon(SvgIcon::Moon)
            } else {
                Button::secondary("Dark").icon(SvgIcon::Moon)
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
