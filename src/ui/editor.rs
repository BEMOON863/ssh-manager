//! Модальное окно создания и редактирования сервера.

use eframe::egui;

use crate::model::{self, Server};
use crate::theme;

/// Состояние формы сервера.
pub struct ServerEditor {
    pub(crate) is_new: bool,
    pub(crate) draft: Server,
    pub(crate) error: Option<String>,
}

impl ServerEditor {
    /// Новая форма для сервера с заданным идентификатором.
    pub fn new(next_id: u64) -> Self {
        Self {
            is_new: true,
            draft: Server::new(next_id),
            error: None,
        }
    }

    /// Форма редактирования копии существующего сервера.
    pub fn edit(server: &Server) -> Self {
        Self {
            is_new: false,
            draft: server.clone(),
            error: None,
        }
    }
}

/// Результат работы окна редактирования.
pub enum EditorAction {
    /// Сохранить сервер.
    Save(Box<Server>),
    /// Закрыть без сохранения.
    Cancel,
}

/// Показывает модальное окно и возвращает действие пользователя.
pub fn show(ctx: &egui::Context, editor: &mut ServerEditor) -> Option<EditorAction> {
    let mut action = None;
    let title = if editor.is_new {
        "Новый сервер"
    } else {
        "Редактирование сервера"
    };

    let modal = egui::Modal::new(egui::Id::unique("server-editor")).show(ctx, |ui| {
        ui.set_width(580.0);
        ui.heading(title);
        ui.add_space(8.0);

        egui::Grid::new("server-editor-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Название");
                ui.text_edit_singleline(&mut editor.draft.name);
                ui.end_row();

                ui.label("Группа");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut editor.draft.group);
                    for preset in ["Prod", "Staging", "Dev", "Home"] {
                        if ui.small_button(preset).clicked() {
                            editor.draft.group = preset.to_string();
                        }
                    }
                });
                ui.end_row();

                ui.label("Хост / IP");
                ui.text_edit_singleline(&mut editor.draft.host);
                ui.end_row();

                ui.label("Порт");
                ui.add(egui::DragValue::new(&mut editor.draft.port).range(1..=65535));
                ui.end_row();

                ui.label("Пользователь");
                ui.text_edit_singleline(&mut editor.draft.user);
                ui.end_row();

                ui.label("SSH-ключ");
                ui.text_edit_singleline(&mut editor.draft.key_path);
                ui.end_row();

                ui.label("Jump-хост (-J)");
                ui.text_edit_singleline(&mut editor.draft.jump);
                ui.end_row();

                ui.label("Заметки");
                ui.add(
                    egui::TextEdit::multiline(&mut editor.draft.notes)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                ui.end_row();
            });

        ui.add_space(6.0);
        if let Some(error) = &editor.error {
            ui.colored_label(theme::DANGER, error);
            ui.add_space(4.0);
        }

        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new("Сохранить").fill(theme::SUCCESS))
                .clicked()
            {
                match model::validate_server(&editor.draft) {
                    Ok(()) => {
                        action = Some(EditorAction::Save(Box::new(editor.draft.clone())));
                    }
                    Err(message) => editor.error = Some(message),
                }
            }
            if ui.button("Отмена").clicked() {
                action = Some(EditorAction::Cancel);
            }
        });
    });

    if action.is_none() && modal.should_close() {
        action = Some(EditorAction::Cancel);
    }
    action
}
