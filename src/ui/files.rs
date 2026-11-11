//! Окно работы с файлами на SSH-сервере.

use eframe::egui;

use crate::remote_files::{
    FilesMessage, FilesRequest, RemoteEntry, format_size, join_remote, normalize_remote_path,
    parent_remote,
};
use crate::theme;

/// Текст, открытый в предпросмотре.
pub struct PreviewState {
    /// Имя файла.
    pub name: String,
    /// Содержимое.
    pub text: String,
}

/// Открытое окно файлов.
pub struct FilesWindow {
    /// Уникальный идентификатор окна.
    pub id: u64,
    /// Идентификатор сервера.
    pub server_id: u64,
    /// Имя сервера для заголовка.
    pub server_name: String,
    /// Окно не закрыто пользователем.
    pub open: bool,
    /// Текущий удалённый путь.
    pub path: String,
    /// Содержимое каталога.
    pub entries: Vec<RemoteEntry>,
    /// Идёт загрузка списка.
    pub loading: bool,
    /// Ошибка последней операции.
    pub error: Option<String>,
    /// Статус последней операции.
    pub status: String,
    /// Открытый текстовый предпросмотр.
    pub preview: Option<PreviewState>,
    request: Option<FilesRequest>,
}

impl FilesWindow {
    /// Создаёт окно для сервера и сразу запрашивает домашний каталог.
    pub fn new(id: u64, server_id: u64, server_name: String) -> Self {
        Self {
            id,
            server_id,
            server_name,
            open: true,
            path: ".".to_string(),
            entries: Vec::new(),
            loading: true,
            error: None,
            status: String::new(),
            preview: None,
            request: None,
        }
    }

    /// Запрашивает список текущего каталога.
    pub fn list(&mut self) {
        self.loading = true;
        self.error = None;
        self.request = Some(FilesRequest::List {
            window_id: self.id,
            server_id: self.server_id,
            path: normalize_remote_path(&self.path),
        });
    }

    /// Забирает накопившийся запрос для приложения.
    pub fn take_request(&mut self) -> Option<FilesRequest> {
        self.request.take()
    }

    /// Применяет результат фоновой операции.
    pub fn apply_message(&mut self, message: FilesMessage) {
        match message {
            FilesMessage::Listed { result, .. } => {
                self.loading = false;
                match result {
                    Ok(entries) => {
                        self.entries = entries;
                        self.error = None;
                    }
                    Err(error) => {
                        self.entries.clear();
                        self.error = Some(error);
                    }
                }
            }
            FilesMessage::Downloaded {
                result, open_after, ..
            } => {
                self.loading = false;
                match result {
                    Ok(path) => {
                        self.status = format!("Скачано: {}", path.display());
                        self.error = None;
                        if open_after && let Err(error) = crate::remote_files::open_local(&path) {
                            self.error = Some(error);
                        }
                    }
                    Err(error) => {
                        self.status.clear();
                        self.error = Some(error);
                    }
                }
            }
            FilesMessage::Previewed { name, result, .. } => {
                self.loading = false;
                match result {
                    Ok(text) => {
                        self.preview = Some(PreviewState { name, text });
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }
    }

    /// Рисует окно.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Путь:");
            ui.add(
                egui::TextEdit::singleline(&mut self.path)
                    .hint_text("/var/log")
                    .desired_width(f32::INFINITY),
            );
            if ui.button("Перейти").clicked() {
                self.list();
            }
            if ui.button("Вверх").clicked() {
                self.path = parent_remote(&self.path);
                self.list();
            }
            if ui.button("Обновить").clicked() {
                self.list();
            }
        });

        if self.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(egui::RichText::new("Загрузка…").small().weak());
            });
        }
        if !self.status.is_empty() {
            ui.label(egui::RichText::new(&self.status).small().weak());
        }
        if let Some(error) = &self.error {
            ui.colored_label(theme::DANGER, error);
        }
        ui.separator();

        if let Some(preview) = &mut self.preview {
            let mut close = false;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&preview.name).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Закрыть просмотр").clicked() {
                        close = true;
                    }
                });
            });
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut text = preview.text.clone();
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .desired_width(f32::INFINITY)
                            .desired_rows(18)
                            .interactive(false),
                    );
                });
            if close {
                self.preview = None;
            }
            return;
        }

        let mut action: Option<FileAction> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.entries.is_empty() && !self.loading {
                    ui.label(egui::RichText::new("Каталог пуст.").weak());
                    return;
                }
                for entry in &self.entries {
                    ui.horizontal(|ui| {
                        if entry.is_dir {
                            if ui
                                .selectable_label(
                                    false,
                                    egui::RichText::new(format!("{}/", entry.name))
                                        .strong()
                                        .color(theme::ACCENT),
                                )
                                .clicked()
                            {
                                action = Some(FileAction::Enter(entry.name.clone()));
                            }
                        } else {
                            ui.label(&entry.name);
                            if entry.is_symlink {
                                ui.label(egui::RichText::new("(ссылка)").small().weak());
                            }
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !entry.is_dir {
                                if ui.button("Скачать").clicked() {
                                    action = Some(FileAction::Download(entry.name.clone()));
                                }
                                if ui.button("Открыть").clicked() {
                                    action = Some(FileAction::Open(entry.name.clone()));
                                }
                                if ui.button("Просмотр").clicked() {
                                    action = Some(FileAction::Preview(entry.name.clone()));
                                }
                            }
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}   {}",
                                    entry.modified,
                                    format_size(entry.size)
                                ))
                                .small()
                                .weak(),
                            );
                        });
                    });
                    ui.separator();
                }
            });

        match action {
            Some(FileAction::Enter(name)) => {
                self.path = join_remote(&self.path, &name);
                self.list();
            }
            Some(FileAction::Download(name)) => {
                self.status = format!("Скачиваю {name}…");
                self.request = Some(FilesRequest::Download {
                    window_id: self.id,
                    server_id: self.server_id,
                    remote_path: join_remote(&self.path, &name),
                    open_after: false,
                });
            }
            Some(FileAction::Open(name)) => {
                self.status = format!("Скачиваю {name}…");
                self.request = Some(FilesRequest::Download {
                    window_id: self.id,
                    server_id: self.server_id,
                    remote_path: join_remote(&self.path, &name),
                    open_after: true,
                });
            }
            Some(FileAction::Preview(name)) => {
                self.loading = true;
                self.request = Some(FilesRequest::Preview {
                    window_id: self.id,
                    server_id: self.server_id,
                    remote_path: join_remote(&self.path, &name),
                });
            }
            None => {}
        }
    }
}

enum FileAction {
    Enter(String),
    Download(String),
    Open(String),
    Preview(String),
}
