//! Общие элементы интерфейса.

pub mod editor;
pub mod import;
pub mod servers;

use eframe::egui;

use crate::theme;

/// Цветная метка группы (Prod выделяется красным).
pub fn group_badge(ui: &mut egui::Ui, group: &str) {
    let group = group.trim();
    if group.is_empty() {
        ui.label(egui::RichText::new("без группы").weak().small());
        return;
    }
    let color = theme::group_color(group);
    ui.label(
        egui::RichText::new(format!("● {group}"))
            .color(color)
            .small()
            .strong(),
    );
}
