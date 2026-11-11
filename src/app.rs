//! Состояние приложения и обработка действий пользователя.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;

use eframe::egui;

use crate::model::{self, AppConfig, GroupFilter, Server};
use crate::ssh::{command, config_import, launch};
use crate::store;
use crate::ui::editor::{EditorAction, ServerEditor};
use crate::ui::import::{ImportAction, ImportDialog};

/// Вкладки приложения.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Tab {
    /// Список серверов.
    #[default]
    Servers,
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
    pub(crate) delete_confirm: Option<u64>,
    pub(crate) import: Option<ImportDialog>,
}

impl ManagerApp {
    /// Создаёт приложение, загружает конфигурацию и применяет оформление.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::theme::setup(&cc.egui_ctx);

        let loaded = store::load();
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
        if self.config.servers.len() != before {
            self.status = "Сервер удалён.".to_string();
        }
        self.persist();
    }

    /// Запускает SSH-сессию в терминале.
    pub(crate) fn connect(&mut self, id: u64) {
        let Some(server) = self
            .config
            .servers
            .iter()
            .find(|server| server.id == id)
            .cloned()
        else {
            return;
        };

        match launch::launch(&server) {
            Ok(launcher) => {
                self.status = format!("Сессия к {} запущена в {}.", server.name, launcher.label());
            }
            Err(error) => self.error_banner = Some(error),
        }
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

        if let Some(id) = self.delete_confirm {
            let name = self
                .config
                .servers
                .iter()
                .find(|server| server.id == id)
                .map(|server| server.name.clone())
                .unwrap_or_default();
            let mut decision: Option<bool> = None;

            let modal =
                egui::Modal::new(egui::Id::unique("delete-server-confirm")).show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.heading("Удалить сервер?");
                    ui.label(format!("Сервер «{name}» будет удалён из списка."));
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
                    self.delete_server(id);
                    self.delete_confirm = None;
                }
                Some(false) => self.delete_confirm = None,
                None => {}
            }
        }
    }
}

impl eframe::App for ManagerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar_ui(ui));
        egui::Panel::bottom("status-bar").show(ui, |ui| self.status_ui(ui));
        egui::Panel::left("groups-panel")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| self.groups_ui(ui));
        egui::CentralPanel::default().show(ui, |ui| self.servers_ui(ui));

        self.modals(&ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
