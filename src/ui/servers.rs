//! Вкладка «Серверы»: панель групп, поиск, список и действия.

use eframe::egui;

use crate::app::{DeleteTarget, ManagerApp, Tab};
use crate::model::{GroupFilter, Server};
use crate::probe::ProbeStatus;
use crate::theme;
use crate::ui::{format_target, group_badge, probe_indicator};

enum RowAction {
    Connect(u64),
    Probe(u64),
    Copy(u64),
    Edit(u64),
    Delete(u64),
}

impl ManagerApp {
    /// Верхняя панель: название, вкладки и кнопки действий.
    pub(crate) fn toolbar_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Ssh-manager");
            ui.add_space(16.0);

            for (tab, label) in [
                (Tab::Servers, "Серверы"),
                (Tab::Snippets, "Сниппеты"),
                (Tab::Tunnels, "Туннели"),
            ] {
                if ui.selectable_label(self.tab == tab, label).clicked() {
                    self.tab = tab;
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Генератор ключей").clicked() {
                    self.open_keygen();
                }

                if self.tab == Tab::Servers {
                    if ui.button("+ Новый сервер").clicked() {
                        self.open_new_server();
                    }
                    if ui.button("Импорт из SSH Config").clicked() {
                        self.open_import();
                    }
                    if ui.button("Проверить все").clicked() {
                        self.probe_all(ui.ctx());
                    }
                }
            });
        });
    }

    /// Левая панель с группами и количеством серверов.
    pub(crate) fn groups_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.label(egui::RichText::new("ГРУППЫ").small().weak());
        ui.add_space(2.0);

        let total = self.config.servers.len();
        if ui
            .selectable_label(
                self.group_filter == GroupFilter::All,
                format!("Все серверы ({total})"),
            )
            .clicked()
        {
            self.group_filter = GroupFilter::All;
        }

        let ungrouped = self
            .config
            .servers
            .iter()
            .filter(|server| server.group.trim().is_empty())
            .count();
        if ungrouped > 0
            && ui
                .selectable_label(
                    self.group_filter == GroupFilter::Ungrouped,
                    format!("● без группы ({ungrouped})"),
                )
                .clicked()
        {
            self.group_filter = GroupFilter::Ungrouped;
        }

        for (group, count) in self.group_counts() {
            let color = theme::group_color(&group);
            let selected = self.group_filter == GroupFilter::Named(group.clone());
            let text = egui::RichText::new(format!("● {group} ({count})")).color(color);
            if ui.selectable_label(selected, text).clicked() {
                self.group_filter = GroupFilter::Named(group);
            }
        }

        ui.add_space(12.0);
        ui.separator();
        if ui.button("Сбросить фильтры").clicked() {
            self.group_filter = GroupFilter::All;
            self.search.clear();
        }
    }

    /// Центральная часть: поиск, счётчик и список серверов.
    pub(crate) fn servers_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Поиск по названию, IP, группе или пользователю…")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);

        let rows: Vec<(Server, ProbeStatus)> = self
            .visible_servers()
            .into_iter()
            .map(|server| {
                let status = self.probes.get(&server.id).cloned().unwrap_or_default();
                (server, status)
            })
            .collect();

        ui.label(
            egui::RichText::new(format!(
                "Показано: {} из {}",
                rows.len(),
                self.config.servers.len()
            ))
            .small()
            .weak(),
        );
        ui.add_space(4.0);

        let mut action: Option<RowAction> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if rows.is_empty() {
                    ui.add_space(24.0);
                    ui.label(
                        egui::RichText::new(
                            "Список пуст. Нажмите «+ Новый сервер» или «Импорт из SSH Config».",
                        )
                        .weak(),
                    );
                    return;
                }

                for (server, status) in &rows {
                    ui.horizontal(|ui| {
                        probe_indicator(ui, status);
                        group_badge(ui, &server.group);
                        ui.label(egui::RichText::new(&server.name).strong().size(15.0));
                        ui.label(
                            egui::RichText::new(format_target(server))
                                .monospace()
                                .weak(),
                        );
                        ui.label(egui::RichText::new(status.short_label()).small().weak());

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Удалить").clicked() {
                                action = Some(RowAction::Delete(server.id));
                            }
                            if ui.button("Изменить").clicked() {
                                action = Some(RowAction::Edit(server.id));
                            }
                            if ui.button("Копировать").clicked() {
                                action = Some(RowAction::Copy(server.id));
                            }
                            if ui.button("Проверить").clicked() {
                                action = Some(RowAction::Probe(server.id));
                            }
                            let connect = egui::Button::new(
                                egui::RichText::new("Подключиться")
                                    .color(egui::Color32::from_rgb(6, 20, 12)),
                            )
                            .fill(theme::SUCCESS);
                            if ui.add(connect).clicked() {
                                action = Some(RowAction::Connect(server.id));
                            }
                        });
                    });

                    if !server.notes.trim().is_empty() {
                        ui.label(egui::RichText::new(server.notes.trim()).small().weak());
                    }
                    ui.separator();
                }
            });

        if let Some(action) = action {
            self.handle_row_action(ui.ctx(), action);
        }
    }

    /// Нижняя строка состояния.
    pub(crate) fn status_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(banner) = self.error_banner.clone() {
                ui.colored_label(theme::DANGER, banner);
                if ui.small_button("Скрыть").clicked() {
                    self.error_banner = None;
                }
            } else {
                ui.label(egui::RichText::new(&self.status).small().weak());
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(self.data_path.display().to_string())
                        .small()
                        .weak(),
                );
            });
        });
    }

    fn handle_row_action(&mut self, ctx: &egui::Context, action: RowAction) {
        match action {
            RowAction::Connect(id) => self.connect(id),
            RowAction::Probe(id) => self.probe_one(ctx, id),
            RowAction::Copy(id) => self.copy_command(ctx, id),
            RowAction::Edit(id) => self.open_editor(id),
            RowAction::Delete(id) => self.delete_confirm = Some(DeleteTarget::Server(id)),
        }
    }
}
