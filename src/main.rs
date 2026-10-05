mod connection;
mod control;
mod mission;
mod mote_app;
mod settings;
mod state;
mod telemetry;
mod transport;
mod ui;

use mote_app::MoteApp;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Mote")
            .with_inner_size([900.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Mote",
        options,
        Box::new(|creation_context| {
            egui_extras::install_image_loaders(&creation_context.egui_ctx);
            bee_ui::theme::install_default_theme(&creation_context.egui_ctx);
            Ok(Box::new(MoteApp::new()))
        }),
    )
}
