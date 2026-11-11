//! Общие элементы интерфейса.

pub mod editor;
pub mod files;
pub mod import;
pub mod keygen;
pub mod servers;
pub mod snippets;
pub mod terminal;
pub mod tunnels;

use eframe::egui;

use crate::model::Server;
use crate::probe::ProbeStatus;
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

/// `user@host:port` (или `host:port`, если пользователь не задан).
pub fn format_target(server: &Server) -> String {
    let user = server.user.trim();
    if user.is_empty() {
        format!("{}:{}", server.host.trim(), server.port)
    } else {
        format!("{user}@{}:{}", server.host.trim(), server.port)
    }
}

/// Индикатор статуса TCP-проверки: цветной кружок с подсказкой.
pub fn probe_indicator(ui: &mut egui::Ui, status: &ProbeStatus) -> egui::Response {
    let color = match status {
        ProbeStatus::Unknown => theme::MUTED,
        ProbeStatus::Checking => theme::WARNING,
        ProbeStatus::Online { .. } => theme::SUCCESS,
        ProbeStatus::Offline { .. } => theme::DANGER,
    };
    let response = ui.label(egui::RichText::new("●").color(color).size(14.0));
    match status {
        ProbeStatus::Unknown => response.on_hover_text("Проверка ещё не запускалась"),
        ProbeStatus::Checking => response.on_hover_text("Проверка выполняется…"),
        ProbeStatus::Online { rtt_ms } => {
            response.on_hover_text(format!("TCP-соединение установлено: {rtt_ms} мс"))
        }
        ProbeStatus::Offline { reason } => {
            response.on_hover_text(format!("TCP-соединение не установлено: {reason}"))
        }
    }
}
