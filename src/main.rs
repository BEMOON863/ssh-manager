//! Точка входа Ssh-manager.

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 720.0])
            .with_min_inner_size([880.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Ssh-manager",
        options,
        Box::new(|cc| Ok(Box::new(ssh_manager::app::ManagerApp::new(cc)))),
    )
}
