//! Вкладка «Туннели»: обзор локальных пробросов по всем серверам.

use eframe::egui;

use crate::app::ManagerApp;
use crate::model::Server;
use crate::ssh::command;

enum TunnelRowAction {
    Connect(u64),
    Copy(u64),
}

impl ManagerApp {
    /// Центральная часть вкладки «Туннели».
    pub(crate) fn tunnels_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "Проброс задаётся в карточке сервера: «Изменить» → раздел «Пробросы (-L)».",
            )
            .small()
            .weak(),
        );
        ui.add_space(8.0);

        let servers: Vec<Server> = self
            .config
            .servers
            .iter()
            .filter(|server| !server.tunnels.is_empty())
            .cloned()
            .collect();

        if servers.is_empty() {
            ui.label(egui::RichText::new("Пока нет ни одного локального проброса.").weak());
            return;
        }

        let mut action: Option<TunnelRowAction> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for server in &servers {
                    ui.horizontal(|ui| {
                        crate::ui::group_badge(ui, &server.group);
                        ui.label(egui::RichText::new(&server.name).strong().size(15.0));
                        ui.label(
                            egui::RichText::new(crate::ui::format_target(server))
                                .monospace()
                                .weak(),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Копировать команду").clicked() {
                                action = Some(TunnelRowAction::Copy(server.id));
                            }
                            if ui.button("Подключиться").clicked() {
                                action = Some(TunnelRowAction::Connect(server.id));
                            }
                        });
                    });

                    for tunnel in &server.tunnels {
                        ui.label(
                            egui::RichText::new(format!("-L {}", command::tunnel_spec(tunnel)))
                                .monospace(),
                        );
                    }
                    ui.separator();
                }
            });

        if let Some(action) = action {
            match action {
                TunnelRowAction::Connect(id) => self.connect(id),
                TunnelRowAction::Copy(id) => self.copy_command(ui.ctx(), id),
            }
        }
    }
}
