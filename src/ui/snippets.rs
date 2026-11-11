//! Вкладка «Сниппеты» и окно редактирования сниппета.

use eframe::egui;

use crate::app::{DeleteTarget, ManagerApp};
use crate::model::Snippet;
use crate::theme;

enum SnippetRowAction {
    Copy(u64),
    Edit(u64),
    Delete(u64),
}

impl ManagerApp {
    /// Центральная часть вкладки «Сниппеты».
    pub(crate) fn snippets_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.snippet_search)
                    .hint_text("Поиск по названию или команде…")
                    .desired_width(f32::INFINITY),
            );
            if ui.button("+ Сниппет").clicked() {
                self.open_new_snippet();
            }
        });
        ui.add_space(6.0);

        let search = self.snippet_search.to_lowercase();
        let snippets: Vec<Snippet> = self
            .config
            .snippets
            .iter()
            .filter(|snippet| {
                search.is_empty()
                    || snippet.name.to_lowercase().contains(&search)
                    || snippet.command.to_lowercase().contains(&search)
            })
            .cloned()
            .collect();

        let mut action: Option<SnippetRowAction> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if snippets.is_empty() {
                    ui.add_space(20.0);
                    ui.label(
                        egui::RichText::new(
                            "Сниппетов нет. Нажмите «+ Сниппет», чтобы добавить команду.",
                        )
                        .weak(),
                    );
                    return;
                }

                for snippet in &snippets {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&snippet.name).strong());
                        ui.label(egui::RichText::new(&snippet.command).monospace().weak());

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Удалить").clicked() {
                                action = Some(SnippetRowAction::Delete(snippet.id));
                            }
                            if ui.button("Изменить").clicked() {
                                action = Some(SnippetRowAction::Edit(snippet.id));
                            }
                            let copy = egui::Button::new(
                                egui::RichText::new("Копировать")
                                    .color(egui::Color32::from_rgb(6, 20, 12)),
                            )
                            .fill(theme::SUCCESS);
                            if ui.add(copy).clicked() {
                                action = Some(SnippetRowAction::Copy(snippet.id));
                            }
                        });
                    });
                    ui.separator();
                }
            });

        if let Some(action) = action {
            match action {
                SnippetRowAction::Copy(id) => self.copy_snippet(ui.ctx(), id),
                SnippetRowAction::Edit(id) => self.open_snippet_editor(id),
                SnippetRowAction::Delete(id) => {
                    self.delete_confirm = Some(DeleteTarget::Snippet(id));
                }
            }
        }
    }
}

/// Состояние формы сниппета.
pub struct SnippetEditor {
    pub(crate) is_new: bool,
    pub(crate) draft: Snippet,
    pub(crate) error: Option<String>,
}

impl SnippetEditor {
    /// Новая форма для сниппета с заданным идентификатором.
    pub fn new(next_id: u64) -> Self {
        Self {
            is_new: true,
            draft: Snippet::new(next_id),
            error: None,
        }
    }

    /// Форма редактирования копии существующего сниппета.
    pub fn edit(snippet: &Snippet) -> Self {
        Self {
            is_new: false,
            draft: snippet.clone(),
            error: None,
        }
    }
}

/// Результат работы окна сниппета.
pub enum SnippetEditorAction {
    /// Сохранить сниппет.
    Save(Snippet),
    /// Закрыть без сохранения.
    Cancel,
}

/// Показывает модальное окно сниппета и возвращает действие пользователя.
pub fn show_editor(ctx: &egui::Context, editor: &mut SnippetEditor) -> Option<SnippetEditorAction> {
    let mut action = None;
    let title = if editor.is_new {
        "Новый сниппет"
    } else {
        "Редактирование сниппета"
    };

    let modal = egui::Modal::new(egui::Id::unique("snippet-editor")).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.heading(title);
        ui.add_space(8.0);

        egui::Grid::new("snippet-editor-grid")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Название");
                ui.text_edit_singleline(&mut editor.draft.name);
                ui.end_row();

                ui.label("Команда");
                ui.add(
                    egui::TextEdit::multiline(&mut editor.draft.command)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("например, docker ps"),
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
                let name = editor.draft.name.trim();
                let command = editor.draft.command.trim();
                if name.is_empty() {
                    editor.error = Some("Укажите название сниппета.".to_string());
                } else if command.is_empty() {
                    editor.error = Some("Укажите команду.".to_string());
                } else {
                    editor.draft.name = name.to_string();
                    editor.draft.command = command.to_string();
                    action = Some(SnippetEditorAction::Save(editor.draft.clone()));
                }
            }
            if ui.button("Отмена").clicked() {
                action = Some(SnippetEditorAction::Cancel);
            }
        });
    });

    if action.is_none() && modal.should_close() {
        action = Some(SnippetEditorAction::Cancel);
    }
    action
}
