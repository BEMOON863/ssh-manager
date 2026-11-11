//! Модальное окно импорта серверов из `~/.ssh/config`.

use std::path::PathBuf;

use eframe::egui;

use crate::model::Server;
use crate::theme;

/// Данные окна импорта.
pub struct ImportDialog {
    pub(crate) path: PathBuf,
    pub(crate) servers: Vec<Server>,
    pub(crate) selected: Vec<bool>,
    pub(crate) duplicates: Vec<bool>,
}

impl ImportDialog {
    /// Создаёт окно; дубликаты сразу сняты с выбора.
    pub fn new(path: PathBuf, servers: Vec<Server>, duplicates: Vec<bool>) -> Self {
        let selected = duplicates.iter().map(|duplicate| !duplicate).collect();
        Self {
            path,
            servers,
            selected,
            duplicates,
        }
    }
}

/// Результат работы окна импорта.
pub enum ImportAction {
    /// Импортировать выбранные серверы.
    Accept(Vec<Server>),
    /// Закрыть окно.
    Cancel,
}

/// Показывает окно импорта и возвращает действие пользователя.
pub fn show(ctx: &egui::Context, dialog: &mut ImportDialog) -> Option<ImportAction> {
    let mut action = None;
    let ImportDialog {
        path,
        servers,
        selected,
        duplicates,
    } = dialog;

    let modal = egui::Modal::new(egui::Id::unique("import-ssh-config")).show(ctx, |ui| {
        ui.set_width(640.0);
        ui.heading("Импорт из SSH Config");
        ui.label(
            egui::RichText::new(path.display().to_string())
                .monospace()
                .weak(),
        );
        ui.add_space(4.0);

        let duplicate_count = duplicates.iter().filter(|duplicate| **duplicate).count();
        ui.label(format!(
            "Найдено хостов: {}. Уже есть в списке: {duplicate_count}.",
            servers.len()
        ));
        ui.add_space(4.0);

        egui::ScrollArea::vertical()
            .max_height(320.0)
            .show(ui, |ui| {
                for (index, server) in servers.iter().enumerate() {
                    let duplicate = duplicates[index];
                    ui.horizontal(|ui| {
                        ui.add_enabled(!duplicate, egui::Checkbox::new(&mut selected[index], ""));
                        ui.label(egui::RichText::new(&server.name).strong());
                        ui.label(
                            egui::RichText::new(format!(
                                "{}@{}:{}",
                                server.user.trim(),
                                server.host.trim(),
                                server.port
                            ))
                            .monospace()
                            .weak(),
                        );
                        if !server.group.trim().is_empty() {
                            let color = theme::group_color(&server.group);
                            ui.label(
                                egui::RichText::new(format!("● {}", server.group.trim()))
                                    .color(color)
                                    .small(),
                            );
                        }
                        if duplicate {
                            ui.label(
                                egui::RichText::new("уже есть")
                                    .color(theme::WARNING)
                                    .small(),
                            );
                        }
                    });
                }
            });

        ui.add_space(6.0);
        ui.separator();
        ui.horizontal(|ui| {
            let selected_count = selected.iter().filter(|chosen| **chosen).count();
            if ui
                .add_enabled(
                    selected_count > 0,
                    egui::Button::new(format!("Импортировать ({selected_count})")),
                )
                .clicked()
            {
                let chosen = servers
                    .iter()
                    .zip(selected.iter())
                    .filter(|(_, chosen)| **chosen)
                    .map(|(server, _)| server.clone())
                    .collect();
                action = Some(ImportAction::Accept(chosen));
            }
            if ui.button("Отмена").clicked() {
                action = Some(ImportAction::Cancel);
            }
        });
    });

    if action.is_none() && modal.should_close() {
        action = Some(ImportAction::Cancel);
    }
    action
}
