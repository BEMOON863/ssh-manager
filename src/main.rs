//! Точка входа Ssh-manager.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1120.0, 720.0])
        .with_min_inner_size([880.0, 560.0]);

    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Ssh-manager",
        options,
        Box::new(|cc| Ok(Box::new(ssh_manager::app::ManagerApp::new(cc)))),
    )
}
