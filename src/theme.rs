//! Оформление приложения: тёмная тема и цвета групп.

use eframe::egui::{self, Color32};

/// Акцентный цвет интерфейса.
pub const ACCENT: Color32 = Color32::from_rgb(96, 165, 250);
/// Цвет ошибок и Production-метки.
pub const DANGER: Color32 = Color32::from_rgb(239, 68, 68);
/// Цвет успешных действий (подключение, зелёная проверка).
pub const SUCCESS: Color32 = Color32::from_rgb(34, 197, 94);
/// Цвет предупреждений и Staging-метки.
pub const WARNING: Color32 = Color32::from_rgb(245, 158, 11);
/// Приглушённый цвет подписей.
pub const MUTED: Color32 = Color32::from_rgb(148, 163, 184);

/// Цвет группы: Prod — красный, Staging — янтарный, Dev — зелёный, Home — синий.
pub fn group_color(group: &str) -> Color32 {
    match group.trim().to_lowercase().as_str() {
        "prod" | "production" => DANGER,
        "staging" | "stage" => WARNING,
        "dev" | "development" => SUCCESS,
        "home" => ACCENT,
        _ => MUTED,
    }
}

/// Применяет тёмную тему и базовые отступы.
pub fn setup(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = Color32::from_rgb(15, 18, 25);
    visuals.window_fill = Color32::from_rgb(22, 26, 35);
    visuals.faint_bg_color = Color32::from_rgb(26, 31, 42);
    visuals.extreme_bg_color = Color32::from_rgb(11, 13, 18);
    visuals.hyperlink_color = ACCENT;
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 5.0);
    });
}
