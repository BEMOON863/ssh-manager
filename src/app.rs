//! Состояние приложения и обработка действий пользователя.

use std::collections::{BTreeMap, HashMap};
use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;

use crate::model::{self, AppConfig, GroupFilter, Server, Snippet};
use crate::probe::{self, ProbeMessage, ProbeStatus};
use crate::remote_files::{self, FilesMessage, FilesRequest};
use crate::ssh::{command, config_import, keygen, launch};
use crate::store;
use crate::terminal::{TerminalSession, TerminalSize};
use crate::ui::editor::{EditorAction, ServerEditor};
use crate::ui::files::FilesWindow;
use crate::ui::import::{ImportAction, ImportDialog};
use crate::ui::keygen::{KeygenAction, KeygenDialog};
use crate::ui::snippets::{SnippetEditor, SnippetEditorAction};
use crate::ui::terminal::TerminalWindow;

/// Вкладки приложения.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    /// Список серверов.
    #[default]
    Servers,
    /// Сниппеты команд.
    Snippets,
    /// Обзор локальных туннелей.
    Tunnels,
}

/// Что именно удаляем после подтверждения.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteTarget {
    /// Сервер с указанным id.
    Server(u64),
    /// Сниппет с указанным id.
    Snippet(u64),
}

/// Действие, которое нужно подтвердить для Prod-сервера.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingAction {
    /// Внешний терминал (Windows Terminal / cmd.exe).
    External(u64),
    /// Встроенный терминал.
    Terminal(u64),
    /// Окно файлов на сервере.
    Files(u64),
}

/// Главное состояние GUI.
pub struct ManagerApp {
    pub(crate) config: AppConfig,
    pub(crate) data_path: PathBuf,
    pub(crate) status: String,
    pub(crate) error_banner: Option<String>,
    pub(crate) tab: Tab,
    pub(crate) search: String,
    pub(crate) group_filter: GroupFilter,
    pub(crate) editor: Option<ServerEditor>,
    pub(crate) delete_confirm: Option<DeleteTarget>,
    pub(crate) import: Option<ImportDialog>,
    pub(crate) probes: HashMap<u64, ProbeStatus>,
    probe_sender: Sender<ProbeMessage>,
    probe_receiver: Receiver<ProbeMessage>,
    pub(crate) pending_action: Option<PendingAction>,
    pub(crate) terminals: Vec<TerminalWindow>,
    next_terminal_id: u64,
    pub(crate) files_windows: Vec<FilesWindow>,
    files_sender: Sender<FilesMessage>,
    files_receiver: Receiver<FilesMessage>,
    next_files_id: u64,
    pub(crate) snippet_search: String,
    pub(crate) snippet_editor: Option<SnippetEditor>,
    pub(crate) keygen: Option<KeygenDialog>,
}

impl ManagerApp {
    /// Создаёт приложение, загружает конфигурацию и применяет оформление.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::setup(&cc.egui_ctx);

        let loaded = store::load();
        let (probe_sender, probe_receiver) = mpsc::channel();
        let (files_sender, files_receiver) = mpsc::channel();
        let mut app = Self {
            config: loaded.config,
            data_path: store::data_path(),
            status: String::new(),
            error_banner: loaded.warning,
            tab: Tab::default(),
            search: String::new(),
            group_filter: GroupFilter::All,
            editor: None,
            delete_confirm: None,
            import: None,
            probes: HashMap::new(),
            probe_sender,
            probe_receiver,
            pending_action: None,
            terminals: Vec::new(),
            next_terminal_id: 1,
            files_windows: Vec::new(),
            files_sender,
            files_receiver,
            next_files_id: 1,
            snippet_search: String::new(),
            snippet_editor: None,
            keygen: None,
        };

        if loaded.first_run {
            app.status = format!("Первый запуск. Создаю {}", app.data_path.display());
            app.persist();
        } else {
            app.status = format!("Загружено серверов: {}.", app.config.servers.len());
        }
        app
    }

    /// Серверы с учётом поиска и фильтра по группе, отсортированные по имени.
    pub(crate) fn visible_servers(&self) -> Vec<Server> {
        let mut servers: Vec<Server> = self
            .config
            .servers
            .iter()
            .filter(|server| {
                self.group_filter.matches(server) && model::server_matches(server, &self.search)
            })
            .cloned()
            .collect();
        servers.sort_by_key(|server| (server.name.to_lowercase(), server.host.to_lowercase()));
        servers
    }

    /// Уникальные группы с количеством серверов (сортировка по алфавиту).
    pub(crate) fn group_counts(&self) -> Vec<(String, usize)> {
        let mut groups: BTreeMap<String, (String, usize)> = BTreeMap::new();
        for server in &self.config.servers {
            let group = server.group.trim();
            if group.is_empty() {
                continue;
            }
            let entry = groups
                .entry(group.to_lowercase())
                .or_insert_with(|| (group.to_string(), 0));
            entry.1 += 1;
        }
        groups.into_values().collect()
    }

    /// Открывает форму нового сервера.
    pub(crate) fn open_new_server(&mut self) {
        self.editor = Some(ServerEditor::new(self.config.next_server_id()));
    }

    /// Открывает форму редактирования сервера.
    pub(crate) fn open_editor(&mut self, id: u64) {
        if let Some(server) = self.config.servers.iter().find(|server| server.id == id) {
            self.editor = Some(ServerEditor::edit(server));
        }
    }

    /// Сохраняет сервер (вставка или замена по id) и пишет файл.
    fn save_server(&mut self, server: Server) {
        let is_new = !self
            .config
            .servers
            .iter()
            .any(|existing| existing.id == server.id);

        if is_new {
            self.config.servers.push(server);
            self.status = "Сервер добавлен.".to_string();
        } else if let Some(existing) = self
            .config
            .servers
            .iter_mut()
            .find(|existing| existing.id == server.id)
        {
            *existing = server;
            self.status = "Сервер обновлён.".to_string();
        }

        self.persist();
    }

    /// Удаляет сервер по идентификатору и пишет файл.
    fn delete_server(&mut self, id: u64) {
        let before = self.config.servers.len();
        self.config.servers.retain(|server| server.id != id);
        self.probes.remove(&id);
        self.terminals
            .retain(|terminal| terminal.session.server_id != id);
        self.files_windows.retain(|window| window.server_id != id);
        if self.config.servers.len() != before {
            self.status = "Сервер удалён.".to_string();
        }
        self.persist();
    }

    /// Запускает SSH-сессию во внешнем терминале (с подтверждением для Prod).
    pub(crate) fn launch_external(&mut self, id: u64) {
        if self.requires_prod_guard(id) {
            self.pending_action = Some(PendingAction::External(id));
            return;
        }
        self.launch_external_now(id);
    }

    /// Открывает встроенный терминал (с подтверждением для Prod).
    pub(crate) fn open_terminal(&mut self, ctx: &egui::Context, id: u64) {
        if self.requires_prod_guard(id) {
            self.pending_action = Some(PendingAction::Terminal(id));
            return;
        }
        self.open_terminal_now(ctx, id);
    }

    fn requires_prod_guard(&self, id: u64) -> bool {
        self.config
            .servers
            .iter()
            .any(|server| server.id == id && model::requires_prod_guard(server))
    }

    fn launch_external_now(&mut self, id: u64) {
        let Some(server) = self
            .config
            .servers
            .iter()
            .find(|server| server.id == id)
            .cloned()
        else {
            return;
        };
        self.launch_session(&server);
    }

    fn open_terminal_now(&mut self, ctx: &egui::Context, id: u64) {
        let Some(server) = self
            .config
            .servers
            .iter()
            .find(|server| server.id == id)
            .cloned()
        else {
            return;
        };

        match TerminalSession::spawn_ssh(&server, TerminalSize::normalized(30, 110), ctx) {
            Ok(session) => {
                let window_id = self.next_terminal_id;
                self.next_terminal_id += 1;
                self.terminals.push(TerminalWindow::new(window_id, session));
                self.status = format!("Открыт встроенный терминал: {}.", server.name);
            }
            Err(error) => self.error_banner = Some(error),
        }
    }

    fn launch_session(&mut self, server: &Server) {
        match launch::launch(server) {
            Ok(launcher) => {
                self.status = format!("Сессия к {} запущена в {}.", server.name, launcher.label());
            }
            Err(error) => self.error_banner = Some(error),
        }
    }

    /// Открывает окно файлов на сервере (с подтверждением для Prod).
    pub(crate) fn open_files(&mut self, ctx: &egui::Context, id: u64) {
        if self.requires_prod_guard(id) {
            self.pending_action = Some(PendingAction::Files(id));
            return;
        }
        self.open_files_now(ctx, id);
    }

    fn open_files_now(&mut self, ctx: &egui::Context, id: u64) {
        let Some(server) = self
            .config
            .servers
            .iter()
            .find(|server| server.id == id)
            .cloned()
        else {
            return;
        };

        let window_id = self.next_files_id;
        self.next_files_id += 1;
        let window = FilesWindow::new(window_id, id, server.name);
        let request = FilesRequest::List {
            window_id,
            server_id: id,
            path: window.path.clone(),
        };
        self.files_windows.push(window);
        self.spawn_files_request(ctx, request);
    }

    fn spawn_files_request(&mut self, ctx: &egui::Context, request: FilesRequest) {
        let server_id = match &request {
            FilesRequest::List { server_id, .. }
            | FilesRequest::Download { server_id, .. }
            | FilesRequest::Preview { server_id, .. } => *server_id,
        };
        let Some(server) = self
            .config
            .servers
            .iter()
            .find(|server| server.id == server_id)
            .cloned()
        else {
            return;
        };

        let sender = self.files_sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let message = match request {
                FilesRequest::List {
                    window_id, path, ..
                } => FilesMessage::Listed {
                    window_id,
                    result: remote_files::list_dir(&server, &path),
                },
                FilesRequest::Download {
                    window_id,
                    remote_path,
                    open_after,
                    ..
                } => FilesMessage::Downloaded {
                    window_id,
                    remote_path: remote_path.clone(),
                    result: remote_files::download(&server, &remote_path),
                    open_after,
                },
                FilesRequest::Preview {
                    window_id,
                    remote_path,
                    ..
                } => FilesMessage::Previewed {
                    window_id,
                    name: remote_files::remote_file_name(&remote_path),
                    result: remote_files::read_text(&server, &remote_path),
                },
            };
            let _ = sender.send(message);
            ctx.request_repaint();
        });
    }

    /// Копирует готовую SSH-команду в буфер обмена.
    pub(crate) fn copy_command(&mut self, ctx: &egui::Context, id: u64) {
        let Some(server) = self.config.servers.iter().find(|server| server.id == id) else {
            return;
        };
        let line = command::command_line(server);
        ctx.copy_text(line.clone());
        self.status = format!("Команда скопирована: {line}");
    }

    /// Открывает окно импорта из SSH-конфига.
    pub(crate) fn open_import(&mut self) {
        let Some(path) = config_import::default_config_path() else {
            self.error_banner =
                Some("Не удалось определить домашний каталог пользователя.".to_string());
            return;
        };

        match config_import::read_config(&path) {
            Ok(servers) => {
                if servers.is_empty() {
                    self.status = format!("В {} не найдено ни одного блока Host.", path.display());
                    return;
                }
                let duplicates = servers
                    .iter()
                    .map(|server| config_import::is_duplicate(&self.config.servers, server))
                    .collect();
                self.import = Some(ImportDialog::new(path, servers, duplicates));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.status = format!(
                    "Файл {} не найден. Добавьте сервер вручную или создайте SSH-конфиг.",
                    path.display()
                );
            }
            Err(error) => {
                self.error_banner =
                    Some(format!("Не удалось прочитать {}: {error}", path.display()));
            }
        }
    }

    /// Добавляет выбранные при импорте серверы, пропуская дубликаты.
    fn apply_import(&mut self, servers: Vec<Server>) {
        let mut next_id = self.config.next_server_id();
        let mut imported = 0usize;
        for mut server in servers {
            if config_import::is_duplicate(&self.config.servers, &server) {
                continue;
            }
            server.id = next_id;
            next_id += 1;
            imported += 1;
            self.config.servers.push(server);
        }
        self.status = format!("Импортировано серверов: {imported}.");
        self.persist();
    }

    /// Запускает TCP-проверку одного сервера.
    pub(crate) fn probe_one(&mut self, ctx: &egui::Context, id: u64) {
        let Some(server) = self
            .config
            .servers
            .iter()
            .find(|server| server.id == id)
            .cloned()
        else {
            return;
        };
        self.spawn_probe(ctx, &server);
    }

    /// Запускает TCP-проверку всех серверов.
    pub(crate) fn probe_all(&mut self, ctx: &egui::Context) {
        let servers = self.config.servers.clone();
        if servers.is_empty() {
            self.status = "Нет серверов для проверки.".to_string();
            return;
        }
        for server in &servers {
            self.spawn_probe(ctx, server);
        }
        self.status = format!("Проверяю {} серверов…", servers.len());
    }

    fn spawn_probe(&mut self, ctx: &egui::Context, server: &Server) {
        if matches!(self.probes.get(&server.id), Some(ProbeStatus::Checking)) {
            return;
        }
        self.probes.insert(server.id, ProbeStatus::Checking);

        let sender = self.probe_sender.clone();
        let ctx = ctx.clone();
        let server = server.clone();
        std::thread::spawn(move || {
            let result = probe::probe_host(&server.host, server.port, probe::PROBE_TIMEOUT);
            let _ = sender.send(ProbeMessage {
                server_id: server.id,
                result,
            });
            ctx.request_repaint();
        });
    }

    fn apply_probe(&mut self, message: ProbeMessage) {
        let status = match message.result {
            Ok(rtt_ms) => ProbeStatus::Online { rtt_ms },
            Err(reason) => ProbeStatus::Offline { reason },
        };
        self.probes.insert(message.server_id, status);
    }

    /// Открывает форму нового сниппета.
    pub(crate) fn open_new_snippet(&mut self) {
        self.snippet_editor = Some(SnippetEditor::new(self.config.next_snippet_id()));
    }

    /// Открывает форму редактирования сниппета.
    pub(crate) fn open_snippet_editor(&mut self, id: u64) {
        if let Some(snippet) = self.config.snippets.iter().find(|snippet| snippet.id == id) {
            self.snippet_editor = Some(SnippetEditor::edit(snippet));
        }
    }

    fn save_snippet(&mut self, snippet: Snippet) {
        let is_new = !self
            .config
            .snippets
            .iter()
            .any(|existing| existing.id == snippet.id);

        if is_new {
            self.config.snippets.push(snippet);
            self.status = "Сниппет добавлен.".to_string();
        } else if let Some(existing) = self
            .config
            .snippets
            .iter_mut()
            .find(|existing| existing.id == snippet.id)
        {
            *existing = snippet;
            self.status = "Сниппет обновлён.".to_string();
        }

        self.persist();
    }

    fn delete_snippet(&mut self, id: u64) {
        let before = self.config.snippets.len();
        self.config.snippets.retain(|snippet| snippet.id != id);
        if self.config.snippets.len() != before {
            self.status = "Сниппет удалён.".to_string();
        }
        self.persist();
    }

    /// Копирует команду сниппета в буфер обмена.
    pub(crate) fn copy_snippet(&mut self, ctx: &egui::Context, id: u64) {
        let Some(snippet) = self.config.snippets.iter().find(|snippet| snippet.id == id) else {
            return;
        };
        ctx.copy_text(snippet.command.clone());
        self.status = format!("Команда скопирована: {}", snippet.command);
    }

    /// Открывает окно генератора ключей.
    pub(crate) fn open_keygen(&mut self) {
        self.keygen = Some(KeygenDialog::new());
    }

    /// Пишет `servers.json`; ошибки показывает в баннере.
    fn persist(&mut self) {
        if let Err(error) = store::save_to(&self.data_path, &self.config) {
            self.error_banner = Some(format!(
                "Не удалось сохранить {}: {error}",
                self.data_path.display()
            ));
        }
    }

    /// Отрисовывает активные модальные окна.
    fn modals(&mut self, ctx: &egui::Context) {
        if let Some(mut editor) = self.editor.take() {
            match crate::ui::editor::show(ctx, &mut editor) {
                Some(EditorAction::Save(server)) => self.save_server(*server),
                Some(EditorAction::Cancel) => {}
                None => self.editor = Some(editor),
            }
        }

        if let Some(mut import) = self.import.take() {
            match crate::ui::import::show(ctx, &mut import) {
                Some(ImportAction::Accept(servers)) => self.apply_import(servers),
                Some(ImportAction::Cancel) => {}
                None => self.import = Some(import),
            }
        }

        if let Some(mut editor) = self.snippet_editor.take() {
            match crate::ui::snippets::show_editor(ctx, &mut editor) {
                Some(SnippetEditorAction::Save(snippet)) => self.save_snippet(snippet),
                Some(SnippetEditorAction::Cancel) => {}
                None => self.snippet_editor = Some(editor),
            }
        }

        if let Some(mut dialog) = self.keygen.take() {
            match crate::ui::keygen::show(ctx, &mut dialog) {
                Some(KeygenAction::Generate) => {
                    match keygen::generate(&dialog.path, &dialog.comment) {
                        Ok(generated) => {
                            self.status =
                                format!("Ключ создан: {}", generated.private_path.display());
                            dialog.result = Some(generated);
                            dialog.error = None;
                        }
                        Err(error) => dialog.error = Some(error),
                    }
                    self.keygen = Some(dialog);
                }
                Some(KeygenAction::Cancel) => {}
                None => self.keygen = Some(dialog),
            }
        }

        if let Some(target) = self.delete_confirm {
            let (title, description) = match target {
                DeleteTarget::Server(id) => {
                    let name = self
                        .config
                        .servers
                        .iter()
                        .find(|server| server.id == id)
                        .map(|server| server.name.clone())
                        .unwrap_or_default();
                    (
                        "Удалить сервер?",
                        format!("Сервер «{name}» будет удалён из списка."),
                    )
                }
                DeleteTarget::Snippet(id) => {
                    let name = self
                        .config
                        .snippets
                        .iter()
                        .find(|snippet| snippet.id == id)
                        .map(|snippet| snippet.name.clone())
                        .unwrap_or_default();
                    (
                        "Удалить сниппет?",
                        format!("Сниппет «{name}» будет удалён."),
                    )
                }
            };

            let mut decision: Option<bool> = None;
            let modal = egui::Modal::new(egui::Id::unique("delete-confirm")).show(ctx, |ui| {
                ui.set_width(400.0);
                ui.heading(title);
                ui.label(description);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new("Удалить").fill(crate::theme::DANGER))
                        .clicked()
                    {
                        decision = Some(true);
                    }
                    if ui.button("Отмена").clicked() {
                        decision = Some(false);
                    }
                });
            });

            if decision.is_none() && modal.should_close() {
                decision = Some(false);
            }
            match decision {
                Some(true) => {
                    match target {
                        DeleteTarget::Server(id) => self.delete_server(id),
                        DeleteTarget::Snippet(id) => self.delete_snippet(id),
                    }
                    self.delete_confirm = None;
                }
                Some(false) => self.delete_confirm = None,
                None => {}
            }
        }

        if let Some(action) = self.pending_action {
            let id = match action {
                PendingAction::External(id)
                | PendingAction::Terminal(id)
                | PendingAction::Files(id) => id,
            };
            let Some(server) = self
                .config
                .servers
                .iter()
                .find(|server| server.id == id)
                .cloned()
            else {
                self.pending_action = None;
                return;
            };

            let mut decision: Option<bool> = None;
            let modal = egui::Modal::new(egui::Id::unique("prod-guard")).show(ctx, |ui| {
                ui.set_width(480.0);
                ui.heading(egui::RichText::new("Prod Guard").color(crate::theme::DANGER));
                ui.add_space(6.0);
                ui.label(format!(
                    "Вы подключаетесь к Production-серверу «{}» ({}).",
                    server.name,
                    crate::ui::format_target(&server)
                ));
                ui.label("Продолжить?");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new("Продолжить").fill(crate::theme::DANGER))
                        .clicked()
                    {
                        decision = Some(true);
                    }
                    if ui.button("Отмена").clicked() {
                        decision = Some(false);
                    }
                });
            });

            if decision.is_none() && modal.should_close() {
                decision = Some(false);
            }
            match decision {
                Some(true) => {
                    self.pending_action = None;
                    match action {
                        PendingAction::External(id) => self.launch_external_now(id),
                        PendingAction::Terminal(id) => self.open_terminal_now(ctx, id),
                        PendingAction::Files(id) => self.open_files_now(ctx, id),
                    }
                }
                Some(false) => self.pending_action = None,
                None => {}
            }
        }
    }

    /// Рисует открытые окна встроенных терминалов.
    fn terminal_windows(&mut self, ctx: &egui::Context) {
        let mut close_index: Option<usize> = None;
        for (index, terminal) in self.terminals.iter_mut().enumerate() {
            let mut open = terminal.open;
            let title = terminal.session.title.clone();
            egui::Window::new(format!("Терминал — {title}"))
                .id(egui::Id::unique(("terminal-window", terminal.id)))
                .open(&mut open)
                .default_size([860.0, 540.0])
                .resizable(true)
                .show(ctx, |ui| terminal.show(ui));
            terminal.open = open;
            if !open {
                close_index = Some(index);
            }
        }
        if let Some(index) = close_index {
            self.terminals.remove(index);
        }
    }

    /// Рисует окна файлов и запускает фоновые операции.
    fn files_windows_ui(&mut self, ctx: &egui::Context) {
        let mut close_index: Option<usize> = None;
        for (index, window) in self.files_windows.iter_mut().enumerate() {
            let mut open = window.open;
            let title = format!("Файлы — {}", window.server_name);
            egui::Window::new(title)
                .id(egui::Id::unique(("files-window", window.id)))
                .open(&mut open)
                .default_size([880.0, 560.0])
                .resizable(true)
                .show(ctx, |ui| window.show(ui));
            window.open = open;
            if !open {
                close_index = Some(index);
            }
        }
        if let Some(index) = close_index {
            self.files_windows.remove(index);
        }

        let requests: Vec<FilesRequest> = self
            .files_windows
            .iter_mut()
            .filter_map(FilesWindow::take_request)
            .collect();
        for request in requests {
            self.spawn_files_request(ctx, request);
        }
    }

    fn apply_files_message(&mut self, message: FilesMessage) {
        let window_id = match &message {
            FilesMessage::Listed { window_id, .. }
            | FilesMessage::Downloaded { window_id, .. }
            | FilesMessage::Previewed { window_id, .. } => *window_id,
        };
        if let Some(window) = self
            .files_windows
            .iter_mut()
            .find(|window| window.id == window_id)
        {
            window.apply_message(message);
        }
    }
}

impl eframe::App for ManagerApp {
    fn logic(&mut self, _ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut updates = 0usize;
        while let Ok(message) = self.probe_receiver.try_recv() {
            self.apply_probe(message);
            updates += 1;
        }
        if updates > 0 {
            self.status = format!("Обновлены результаты проверки: {updates}.");
        }
        while let Ok(message) = self.files_receiver.try_recv() {
            self.apply_files_message(message);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar_ui(ui));
        egui::Panel::bottom("status-bar").show(ui, |ui| self.status_ui(ui));
        egui::Panel::left("groups-panel")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| self.groups_ui(ui));
        egui::CentralPanel::default().show(ui, |ui| match self.tab {
            Tab::Servers => self.servers_ui(ui),
            Tab::Snippets => self.snippets_ui(ui),
            Tab::Tunnels => self.tunnels_ui(ui),
        });

        self.modals(&ctx);
        self.terminal_windows(&ctx);
        self.files_windows_ui(&ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::ProbeStatus;
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempData {
        dir: PathBuf,
    }

    impl TempData {
        fn new() -> Self {
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("test-data")
                .join(format!("app-{}-{id}", std::process::id()));
            fs::create_dir_all(&dir).expect("временный каталог");
            Self { dir }
        }

        fn app(&self, servers: Vec<Server>) -> ManagerApp {
            let (probe_sender, probe_receiver) = mpsc::channel();
            let (files_sender, files_receiver) = mpsc::channel();
            ManagerApp {
                config: AppConfig {
                    servers,
                    ..AppConfig::default()
                },
                data_path: self.dir.join("servers.json"),
                status: String::new(),
                error_banner: None,
                tab: Tab::Servers,
                search: String::new(),
                group_filter: GroupFilter::All,
                editor: None,
                delete_confirm: None,
                import: None,
                probes: HashMap::new(),
                probe_sender,
                probe_receiver,
                pending_action: None,
                terminals: Vec::new(),
                next_terminal_id: 1,
                files_windows: Vec::new(),
                files_sender,
                files_receiver,
                next_files_id: 1,
                snippet_search: String::new(),
                snippet_editor: None,
                keygen: None,
            }
        }
    }

    impl Drop for TempData {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn server(id: u64, name: &str, host: &str, group: &str) -> Server {
        let mut server = Server::new(id);
        server.name = name.to_string();
        server.host = host.to_string();
        server.user = "root".to_string();
        server.group = group.to_string();
        server
    }

    #[test]
    fn search_and_group_filter_narrow_the_list() {
        let data = TempData::new();
        let mut app = data.app(vec![
            server(1, "Web", "10.0.0.1", "Prod"),
            server(2, "DB", "10.0.0.2", "Staging"),
            server(3, "Home NAS", "192.168.1.5", ""),
        ]);

        assert_eq!(app.visible_servers().len(), 3);

        app.search = "10.0.0.2".to_string();
        assert_eq!(app.visible_servers().len(), 1);
        assert_eq!(app.visible_servers()[0].name, "DB");

        app.search.clear();
        app.group_filter = GroupFilter::Named("prod".to_string());
        assert_eq!(app.visible_servers().len(), 1);
        assert_eq!(app.visible_servers()[0].name, "Web");

        app.group_filter = GroupFilter::Ungrouped;
        assert_eq!(app.visible_servers().len(), 1);
        assert_eq!(app.visible_servers()[0].name, "Home NAS");
    }

    #[test]
    fn groups_are_sorted_and_counted() {
        let data = TempData::new();
        let app = data.app(vec![
            server(1, "a", "h1", "Prod"),
            server(2, "b", "h2", "home"),
            server(3, "c", "h3", "Prod"),
            server(4, "d", "h4", ""),
        ]);

        assert_eq!(
            app.group_counts(),
            vec![("home".to_string(), 1), ("Prod".to_string(), 2)]
        );
    }

    #[test]
    fn saving_and_deleting_write_servers_json() {
        let data = TempData::new();
        let mut app = data.app(Vec::new());

        app.save_server(server(1, "Web", "10.0.0.1", "Prod"));
        assert!(app.data_path.exists());
        let loaded = store::load_from(&app.data_path);
        assert_eq!(loaded.config.servers.len(), 1);
        assert_eq!(loaded.config.servers[0].name, "Web");

        app.save_server(server(1, "Web 2", "10.0.0.1", "Prod"));
        let loaded = store::load_from(&app.data_path);
        assert_eq!(loaded.config.servers.len(), 1);
        assert_eq!(loaded.config.servers[0].name, "Web 2");

        app.delete_server(1);
        assert!(store::load_from(&app.data_path).config.servers.is_empty());
    }

    #[test]
    fn import_assigns_ids_and_skips_duplicates() {
        let data = TempData::new();
        let mut app = data.app(vec![server(4, "existing", "10.0.0.1", "")]);

        let duplicate = server(0, "dup", "10.0.0.1", "");
        let fresh = server(0, "fresh", "10.0.0.2", "Prod");

        app.apply_import(vec![duplicate, fresh]);

        let names: Vec<&str> = app
            .config
            .servers
            .iter()
            .map(|server| server.name.as_str())
            .collect();
        assert_eq!(names, vec!["existing", "fresh"]);

        let fresh = app
            .config
            .servers
            .iter()
            .find(|server| server.name == "fresh")
            .expect("fresh должен быть импортирован");
        assert_eq!(fresh.id, 5, "id продолжается после максимального");
        assert!(
            store::load_from(&app.data_path)
                .config
                .servers
                .iter()
                .any(|server| server.name == "fresh")
        );
    }

    #[test]
    fn prod_connect_requires_confirmation() {
        let data = TempData::new();
        let mut app = data.app(vec![server(1, "prod-web", "10.0.0.1", "Prod")]);

        app.launch_external(1);
        assert_eq!(app.pending_action, Some(PendingAction::External(1)));

        let ctx = egui::Context::default();
        app.open_terminal(&ctx, 1);
        assert_eq!(app.pending_action, Some(PendingAction::Terminal(1)));

        app.open_files(&ctx, 1);
        assert_eq!(app.pending_action, Some(PendingAction::Files(1)));
    }

    #[test]
    fn probe_results_update_status_map() {
        let data = TempData::new();
        let mut app = data.app(vec![server(1, "web", "10.0.0.1", "")]);

        app.apply_probe(ProbeMessage {
            server_id: 1,
            result: Ok(17),
        });
        assert_eq!(
            app.probes.get(&1),
            Some(&ProbeStatus::Online { rtt_ms: 17 })
        );

        app.apply_probe(ProbeMessage {
            server_id: 1,
            result: Err("нет маршрута".to_string()),
        });
        assert!(matches!(
            app.probes.get(&1),
            Some(ProbeStatus::Offline { .. })
        ));
    }

    #[test]
    fn snippets_are_saved_and_deleted() {
        let data = TempData::new();
        let mut app = data.app(Vec::new());

        app.save_snippet(Snippet {
            id: 1,
            name: "Логи".to_string(),
            command: "tail -f /var/log/syslog".to_string(),
        });

        let loaded = store::load_from(&app.data_path);
        assert_eq!(loaded.config.snippets.len(), 1);
        assert_eq!(loaded.config.snippets[0].command, "tail -f /var/log/syslog");

        app.delete_snippet(1);
        assert!(store::load_from(&app.data_path).config.snippets.is_empty());
    }
}
