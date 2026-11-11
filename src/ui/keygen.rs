//! Окно генератора ключей ed25519.

use eframe::egui;

use crate::ssh::keygen::{self, GeneratedKey};
use crate::theme;

/// Состояние окна генератора ключей.
pub struct KeygenDialog {
    pub(crate) path: String,
    pub(crate) comment: String,
    pub(crate) error: Option<String>,
    pub(crate) result: Option<GeneratedKey>,
}

impl KeygenDialog {
    /// Создаёт окно с путём по умолчанию `~/.ssh/id_ed25519`.
    pub fn new() -> Self {
        Self {
            path: keygen::default_key_path(),
            comment: "Ssh-manager".to_string(),
            error: None,
            result: None,
        }
    }
}

impl Default for KeygenDialog {
    fn default() -> Self {
        Self::new()
    }
}

/// Результат работы окна генератора.
pub enum KeygenAction {
    /// Сгенерировать ключ.
    Generate,
    /// Закрыть окно.
    Cancel,
}

/// Показывает окно генератора ключей.
pub fn show(ctx: &egui::Context, dialog: &mut KeygenDialog) -> Option<KeygenAction> {
    let mut action = None;

    let modal = egui::Modal::new(egui::Id::unique("keygen-dialog")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.heading("Генератор ключей ed25519");
        ui.label(
            egui::RichText::new("ssh-keygen -t ed25519")
                .monospace()
                .weak(),
        );
        ui.add_space(6.0);

        if let Some(result) = &dialog.result {
            ui.colored_label(theme::SUCCESS, "Ключ создан.");
            ui.label(format!("Закрытый ключ: {}", result.private_path.display()));
            ui.label(format!("Публичный ключ: {}", result.public_path.display()));
            ui.add_space(4.0);
            ui.label("Публичный ключ:");
            egui::ScrollArea::both().max_height(96.0).show(ui, |ui| {
                ui.label(egui::RichText::new(&result.public_key).monospace().small());
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                if ui.button("Скопировать публичный ключ").clicked() {
                    ctx.copy_text(result.public_key.clone());
                }
                if ui.button("Закрыть").clicked() {
                    action = Some(KeygenAction::Cancel);
                }
            });
        } else {
            egui::Grid::new("keygen-grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Путь к ключу");
                    ui.text_edit_singleline(&mut dialog.path);
                    ui.end_row();

                    ui.label("Комментарий (-C)");
                    ui.text_edit_singleline(&mut dialog.comment);
                    ui.end_row();
                });

            ui.label(
                egui::RichText::new("Парольная фраза не задаётся: ключ создаётся без пароля.")
                    .small()
                    .weak(),
            );
            ui.add_space(4.0);

            if let Some(error) = &dialog.error {
                ui.colored_label(theme::DANGER, error);
                ui.add_space(4.0);
            }

            ui.horizontal(|ui| {
                if ui
                    .add(egui::Button::new("Сгенерировать").fill(theme::SUCCESS))
                    .clicked()
                {
                    action = Some(KeygenAction::Generate);
                }
                if ui.button("Отмена").clicked() {
                    action = Some(KeygenAction::Cancel);
                }
            });
        }
    });

    if action.is_none() && modal.should_close() {
        action = Some(KeygenAction::Cancel);
    }
    action
}
