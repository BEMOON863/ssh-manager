//! Чтение и запись `servers.json` рядом с приложением.

use crate::model::AppConfig;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Имя файла конфигурации.
pub const DATA_FILE: &str = "servers.json";

/// Переменная окружения для переопределения каталога данных (удобно для тестов).
pub const DATA_DIR_ENV: &str = "AAA_DATA_DIR";

/// Каталог данных: каталог exe либо `AAA_DATA_DIR`.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV) {
        return PathBuf::from(dir);
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Полный путь к `servers.json`.
pub fn data_path() -> PathBuf {
    data_dir().join(DATA_FILE)
}

/// Результат загрузки конфигурации.
#[derive(Debug)]
pub struct Loaded {
    /// Загруженная (или стартовая) конфигурация.
    pub config: AppConfig,
    /// Файла не было — первый запуск.
    pub first_run: bool,
    /// Некритичная проблема, которую нужно показать пользователю.
    pub warning: Option<String>,
}

/// Загружает `servers.json` из каталога данных.
pub fn load() -> Loaded {
    load_from(&data_path())
}

/// Сохраняет `servers.json` в каталог данных.
pub fn save(config: &AppConfig) -> io::Result<()> {
    save_to(&data_path(), config)
}

/// Загружает конфигурацию из конкретного пути (для тестов и переопределений).
pub fn load_from(path: &Path) -> Loaded {
    match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<AppConfig>(&text) {
            Ok(config) => Loaded {
                config,
                first_run: false,
                warning: None,
            },
            Err(err) => {
                let backup = path.with_extension("json.bak");
                let warning = match fs::copy(path, &backup) {
                    Ok(_) => format!(
                        "Файл {} повреждён: {err}. Резервная копия сохранена в {}.",
                        path.display(),
                        backup.display()
                    ),
                    Err(copy_err) => format!(
                        "Файл {} повреждён: {err}. Резервную копию создать не удалось: {copy_err}.",
                        path.display()
                    ),
                };
                Loaded {
                    config: AppConfig::with_defaults(),
                    first_run: false,
                    warning: Some(warning),
                }
            }
        },
        Err(err) if err.kind() == io::ErrorKind::NotFound => Loaded {
            config: AppConfig::with_defaults(),
            first_run: true,
            warning: None,
        },
        Err(err) => Loaded {
            config: AppConfig::with_defaults(),
            first_run: true,
            warning: Some(format!("Не удалось прочитать {}: {err}.", path.display())),
        },
    }
}

/// Атомарно сохраняет конфигурацию по указанному пути.
pub fn save_to(path: &Path, config: &AppConfig) -> io::Result<()> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::create_dir_all(dir)?;
    }

    let json = serde_json::to_string_pretty(config).map_err(io::Error::other)?;

    let tmp = path.with_extension("json.tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(json.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppConfig, CONFIG_VERSION, Server, Snippet};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("test-data")
                .join(format!("store-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).expect("временный каталог");
            Self { path }
        }

        fn file(&self) -> PathBuf {
            self.path.join(DATA_FILE)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn sample_config() -> AppConfig {
        AppConfig {
            version: CONFIG_VERSION,
            servers: vec![Server {
                id: 1,
                name: "web".to_string(),
                group: "Prod".to_string(),
                host: "10.1.2.3".to_string(),
                port: 22,
                ..Server::new(1)
            }],
            snippets: vec![Snippet {
                id: 1,
                name: "htop".to_string(),
                command: "htop".to_string(),
            }],
        }
    }

    #[test]
    fn missing_file_is_first_run_with_defaults() {
        let dir = TestDir::new();
        let loaded = load_from(&dir.file());
        assert!(loaded.first_run);
        assert!(loaded.warning.is_none());
        assert!(loaded.config.servers.is_empty());
        assert!(!loaded.config.snippets.is_empty());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = TestDir::new();
        let config = sample_config();
        save_to(&dir.file(), &config).expect("запись");

        assert!(dir.file().exists());
        assert!(!dir.file().with_extension("json.tmp").exists());

        let loaded = load_from(&dir.file());
        assert!(!loaded.first_run);
        assert!(loaded.warning.is_none());
        assert_eq!(loaded.config, config);
    }

    #[test]
    fn corrupt_file_is_backed_up_and_replaced_with_defaults() {
        let dir = TestDir::new();
        fs::write(dir.file(), "{ это не json").expect("подготовка файла");

        let loaded = load_from(&dir.file());
        assert!(!loaded.first_run);
        assert!(loaded.warning.is_some());
        assert!(loaded.config.servers.is_empty());

        let backup = dir.file().with_extension("json.bak");
        assert!(backup.exists(), "должна появиться резервная копия");
        assert_eq!(
            fs::read_to_string(backup).expect("чтение копии"),
            "{ это не json"
        );
    }

    #[test]
    fn save_creates_missing_directories() {
        let dir = TestDir::new();
        let nested = dir.path.join("nested").join("data").join(DATA_FILE);
        save_to(&nested, &sample_config()).expect("запись во вложенный каталог");
        assert!(nested.exists());
    }
}
