//! Доменные типы приложения и сериализация в `servers.json`.

use serde::{Deserialize, Serialize};

/// Порт SSH по умолчанию.
pub const DEFAULT_PORT: u16 = 22;

/// Текущая версия формата `servers.json`.
pub const CONFIG_VERSION: u32 = 1;

/// Имя группы, для которой включён Prod Guard.
pub const PROD_GROUP: &str = "Prod";

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_version() -> u32 {
    CONFIG_VERSION
}

/// Сервер, к которому можно подключиться.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub group: String,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub key_path: String,
    #[serde(default)]
    pub jump: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tunnels: Vec<Tunnel>,
}

impl Server {
    /// Пустой сервер с заданным идентификатором.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            name: String::new(),
            group: String::new(),
            host: String::new(),
            port: DEFAULT_PORT,
            user: String::new(),
            key_path: String::new(),
            jump: String::new(),
            notes: String::new(),
            tunnels: Vec::new(),
        }
    }
}

/// Локальный проброс портов (`-L local_port:remote_host:remote_port`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tunnel {
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
}

impl Tunnel {
    /// Пустой проброс для формы редактирования.
    pub fn new() -> Self {
        Self {
            local_port: 0,
            remote_host: String::new(),
            remote_port: 0,
        }
    }
}

impl Default for Tunnel {
    fn default() -> Self {
        Self::new()
    }
}

/// Сохранённая bash-команда.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    pub id: u64,
    pub name: String,
    pub command: String,
}

impl Snippet {
    /// Пустой сниппет с заданным идентификатором.
    pub fn new(id: u64) -> Self {
        Self {
            id,
            name: String::new(),
            command: String::new(),
        }
    }
}

/// Содержимое `servers.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub servers: Vec<Server>,
    #[serde(default)]
    pub snippets: Vec<Snippet>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            servers: Vec::new(),
            snippets: Vec::new(),
        }
    }
}

impl AppConfig {
    /// Конфигурация с набором стартовых сниппетов (для первого запуска).
    pub fn with_defaults() -> Self {
        Self {
            version: CONFIG_VERSION,
            servers: Vec::new(),
            snippets: default_snippets(),
        }
    }

    /// Следующий свободный идентификатор сервера.
    pub fn next_server_id(&self) -> u64 {
        self.servers.iter().map(|s| s.id).max().unwrap_or(0) + 1
    }

    /// Следующий свободный идентификатор сниппета.
    pub fn next_snippet_id(&self) -> u64 {
        self.snippets.iter().map(|s| s.id).max().unwrap_or(0) + 1
    }
}

/// Стартовый набор сниппетов.
pub fn default_snippets() -> Vec<Snippet> {
    vec![
        Snippet {
            id: 1,
            name: "htop".to_string(),
            command: "htop".to_string(),
        },
        Snippet {
            id: 2,
            name: "Свободное место".to_string(),
            command: "df -h".to_string(),
        },
        Snippet {
            id: 3,
            name: "Docker-контейнеры".to_string(),
            command: "docker ps".to_string(),
        },
    ]
}

/// Является ли группа Production-группой (регистронезависимо).
pub fn is_prod_group(group: &str) -> bool {
    group.trim().eq_ignore_ascii_case(PROD_GROUP)
}

/// Нужно ли подтверждение перед подключением (Prod Guard).
pub fn requires_prod_guard(server: &Server) -> bool {
    is_prod_group(&server.group)
}

/// Фильтр по группам в левой панели.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum GroupFilter {
    /// Показывать все серверы.
    #[default]
    All,
    /// Только серверы без группы.
    Ungrouped,
    /// Только серверы указанной группы.
    Named(String),
}

impl GroupFilter {
    /// Проверяет, попадает ли сервер под фильтр.
    pub fn matches(&self, server: &Server) -> bool {
        match self {
            Self::All => true,
            Self::Ungrouped => server.group.trim().is_empty(),
            Self::Named(group) => server.group.trim().eq_ignore_ascii_case(group.trim()),
        }
    }
}

/// Живой поиск по названию, хосту/IP и тегам (плюс пользователь).
pub fn server_matches(server: &Server, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    let haystack = format!(
        "{} {} {} {}",
        server.name, server.host, server.group, server.user
    )
    .to_lowercase();
    haystack.contains(&query)
}

/// Проверка обязательных полей перед сохранением.
pub fn validate_server(server: &Server) -> Result<(), String> {
    if server.name.trim().is_empty() {
        return Err("Укажите название сервера.".to_string());
    }
    if server.host.trim().is_empty() {
        return Err("Укажите хост или IP-адрес.".to_string());
    }
    if server.port == 0 {
        return Err("Порт должен быть в диапазоне 1-65535.".to_string());
    }
    for (index, tunnel) in server.tunnels.iter().enumerate() {
        if tunnel.local_port == 0 {
            return Err(format!("Проброс #{}: укажите локальный порт.", index + 1));
        }
        if tunnel.remote_host.trim().is_empty() {
            return Err(format!("Проброс #{}: укажите удалённый хост.", index + 1));
        }
        if tunnel.remote_port == 0 {
            return Err(format!("Проброс #{}: укажите удалённый порт.", index + 1));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(name: &str, host: &str) -> Server {
        Server {
            id: 1,
            name: name.to_string(),
            host: host.to_string(),
            ..Server::new(1)
        }
    }

    #[test]
    fn missing_fields_get_defaults() {
        let parsed: Server = serde_json::from_str(r#"{"id":7,"name":"web","host":"10.0.0.1"}"#)
            .expect("валидный JSON");
        assert_eq!(parsed.id, 7);
        assert_eq!(parsed.port, DEFAULT_PORT);
        assert!(parsed.group.is_empty());
        assert!(parsed.user.is_empty());
        assert!(parsed.key_path.is_empty());
        assert!(parsed.jump.is_empty());
        assert!(parsed.notes.is_empty());
        assert!(parsed.tunnels.is_empty());
    }

    #[test]
    fn config_roundtrip() {
        let config = AppConfig {
            version: CONFIG_VERSION,
            servers: vec![{
                let mut s = server("web", "example.com");
                s.group = "Prod".to_string();
                s.port = 2222;
                s.tunnels = vec![Tunnel {
                    local_port: 8080,
                    remote_host: "127.0.0.1".to_string(),
                    remote_port: 80,
                }];
                s
            }],
            snippets: default_snippets(),
        };
        let json = serde_json::to_string(&config).expect("сериализация");
        let restored: AppConfig = serde_json::from_str(&json).expect("десериализация");
        assert_eq!(config, restored);
    }

    #[test]
    fn defaults_include_three_snippets() {
        let config = AppConfig::with_defaults();
        assert_eq!(config.snippets.len(), 3);
        assert!(config.snippets.iter().any(|s| s.command == "htop"));
        assert!(config.snippets.iter().any(|s| s.command == "df -h"));
        assert!(config.snippets.iter().any(|s| s.command == "docker ps"));
    }

    #[test]
    fn next_ids_start_at_one_and_avoid_collisions() {
        let config = AppConfig::default();
        assert_eq!(config.next_server_id(), 1);
        assert_eq!(config.next_snippet_id(), 1);

        let mut first = server("a", "h");
        first.id = 1;
        let mut second = server("b", "h");
        second.id = 2;
        let config = AppConfig {
            version: CONFIG_VERSION,
            servers: vec![first, second],
            snippets: vec![Snippet::new(4)],
        };
        assert_eq!(config.next_server_id(), 3);
        assert_eq!(config.next_snippet_id(), 5);
    }

    #[test]
    fn prod_detection_is_case_insensitive() {
        let mut s = server("db", "db.example.com");
        s.group = "Prod".to_string();
        assert!(requires_prod_guard(&s));
        s.group = " prod ".to_string();
        assert!(requires_prod_guard(&s));
        s.group = "Production".to_string();
        assert!(!requires_prod_guard(&s));
        s.group = "Staging".to_string();
        assert!(!requires_prod_guard(&s));
    }

    #[test]
    fn search_matches_name_host_and_group() {
        let mut s = server("Web frontend", "10.0.0.5");
        s.group = "Staging".to_string();
        s.user = "deploy".to_string();

        assert!(server_matches(&s, "web"));
        assert!(server_matches(&s, "10.0.0"));
        assert!(server_matches(&s, "staging"));
        assert!(server_matches(&s, "deploy"));
        assert!(server_matches(&s, "  "));
        assert!(!server_matches(&s, "prod"));
    }

    #[test]
    fn group_filter_matches_expected_servers() {
        let mut prod = server("a", "h");
        prod.group = "Prod".to_string();
        let bare = server("b", "h");

        assert!(GroupFilter::All.matches(&prod));
        assert!(GroupFilter::All.matches(&bare));
        assert!(GroupFilter::Named("prod".to_string()).matches(&prod));
        assert!(!GroupFilter::Named("prod".to_string()).matches(&bare));
        assert!(GroupFilter::Ungrouped.matches(&bare));
        assert!(!GroupFilter::Ungrouped.matches(&prod));
    }

    #[test]
    fn validation_rejects_incomplete_servers() {
        assert!(validate_server(&server("web", "10.0.0.1")).is_ok());

        let mut no_name = server("web", "10.0.0.1");
        no_name.name = "  ".to_string();
        assert!(validate_server(&no_name).is_err());

        let mut no_host = server("web", "10.0.0.1");
        no_host.host = String::new();
        assert!(validate_server(&no_host).is_err());

        let mut bad_port = server("web", "10.0.0.1");
        bad_port.port = 0;
        assert!(validate_server(&bad_port).is_err());

        let mut bad_tunnel = server("web", "10.0.0.1");
        bad_tunnel.tunnels = vec![Tunnel::new()];
        assert!(validate_server(&bad_tunnel).is_err());

        let mut good_tunnel = server("web", "10.0.0.1");
        good_tunnel.tunnels = vec![Tunnel {
            local_port: 8080,
            remote_host: "db".to_string(),
            remote_port: 5432,
        }];
        assert!(validate_server(&good_tunnel).is_ok());
    }
}
