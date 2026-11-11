//! Работа с файлами на SSH-сервере через системные `ssh` и `scp`.
//!
//! Используются OpenSSH-утилиты, которые уже есть в Windows: это проще и
//! надёжнее, чем встраивать SFTP-библиотеку, но требует POSIX-сервера
//! (`ls`, `head`) и наличия `ssh`/`scp` в `PATH`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::model::Server;
use crate::ssh::command;

/// Максимальный размер файла для текстового предпросмотра.
pub const PREVIEW_LIMIT: usize = 200 * 1024;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Элемент удалённого каталога.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEntry {
    /// Имя файла или каталога.
    pub name: String,
    /// Это каталог.
    pub is_dir: bool,
    /// Это символическая ссылка.
    pub is_symlink: bool,
    /// Размер в байтах.
    pub size: u64,
    /// Дата и время изменения в формате `long-iso`.
    pub modified: String,
}

/// Запрос, который окно файлов передаёт в приложение.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesRequest {
    /// Прочитать содержимое каталога.
    List {
        /// Идентификатор окна.
        window_id: u64,
        /// Идентификатор сервера.
        server_id: u64,
        /// Удалённый путь.
        path: String,
    },
    /// Скачать файл.
    Download {
        /// Идентификатор окна.
        window_id: u64,
        /// Идентификатор сервера.
        server_id: u64,
        /// Удалённый путь.
        remote_path: String,
        /// Открыть файл после скачивания.
        open_after: bool,
    },
    /// Показать текстовый предпросмотр.
    Preview {
        /// Идентификатор окна.
        window_id: u64,
        /// Идентификатор сервера.
        server_id: u64,
        /// Удалённый путь.
        remote_path: String,
    },
}

/// Ответ фоновой операции для окна файлов.
#[derive(Debug)]
pub enum FilesMessage {
    /// Результат чтения каталога.
    Listed {
        /// Идентификатор окна.
        window_id: u64,
        /// Элементы каталога или ошибка.
        result: Result<Vec<RemoteEntry>, String>,
    },
    /// Результат скачивания.
    Downloaded {
        /// Идентификатор окна.
        window_id: u64,
        /// Удалённый путь.
        remote_path: String,
        /// Локальный путь или ошибка.
        result: Result<PathBuf, String>,
        /// Открыть файл после скачивания.
        open_after: bool,
    },
    /// Результат предпросмотра.
    Previewed {
        /// Идентификатор окна.
        window_id: u64,
        /// Имя файла.
        name: String,
        /// Текст или ошибка.
        result: Result<String, String>,
    },
}

/// Читает список файлов удалённого каталога.
pub fn list_dir(server: &Server, path: &str) -> Result<Vec<RemoteEntry>, String> {
    let remote = format!(
        "LC_ALL=C ls -lA --time-style=long-iso -- {}",
        shell_quote(path)
    );
    let output = run_ssh(server, &remote)?;
    Ok(parse_ls_output(&output))
}

/// Скачивает файл в локальный каталог загрузок.
pub fn download(server: &Server, remote_path: &str) -> Result<PathBuf, String> {
    let file_name = remote_file_name(remote_path);
    let directory = download_directory(server);
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("Не удалось создать {}: {error}", directory.display()))?;
    let target = unique_path(&directory.join(file_name));
    let remote = format!("{}:{}", command::target(server), shell_quote(remote_path));

    let mut process = Command::new("scp");
    process.arg("-q");
    if server.port != crate::model::DEFAULT_PORT {
        process.arg("-P").arg(server.port.to_string());
    }
    let key = server.key_path.trim();
    if !key.is_empty() {
        process.arg("-i").arg(key);
    }
    let jump = server.jump.trim();
    if !jump.is_empty() {
        process.arg("-J").arg(jump);
    }
    process.arg(remote).arg(&target);
    hide_console(&mut process);

    let output = process
        .output()
        .map_err(|error| format!("Не удалось запустить scp: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let details = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("scp завершился с ошибкой: {details}"));
    }
    Ok(target)
}

/// Читает начало текстового файла для предпросмотра.
pub fn read_text(server: &Server, remote_path: &str) -> Result<String, String> {
    let remote = format!("head -c {} -- {}", PREVIEW_LIMIT, shell_quote(remote_path));
    let output = run_ssh(server, &remote)?;
    Ok(output)
}

/// Открывает локальный файл приложением по умолчанию.
pub fn open_local(path: &Path) -> Result<(), String> {
    let mut process = Command::new("cmd.exe");
    process.args(["/c", "start", ""]).arg(path);
    hide_console(&mut process);
    process
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Не удалось открыть {}: {error}", path.display()))
}

fn run_ssh(server: &Server, remote_command: &str) -> Result<String, String> {
    let mut process = Command::new("ssh");
    process.args(command::build_args(server));
    process.arg(remote_command);
    process
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut process);

    let output = process
        .output()
        .map_err(|error| format!("Не удалось запустить ssh: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let details = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("ssh завершился с ошибкой: {details}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn hide_console(process: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        process.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    {
        let _ = process;
    }
}

/// Каталог загрузок: `%USERPROFILE%\Downloads\Ssh-manager\<сервер>`.
pub fn download_directory(server: &Server) -> PathBuf {
    let server_name = sanitize_name(&server.name);
    downloads_root().join(server_name)
}

fn downloads_root() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Downloads")
        .join("Ssh-manager")
}

/// Имя файла из удалённого пути.
pub fn remote_file_name(remote_path: &str) -> String {
    let trimmed = remote_path.trim_end_matches('/');
    let name = trimmed.rsplit('/').next().unwrap_or(trimmed);
    if name.is_empty() || name == "." || name == ".." {
        "file".to_string()
    } else {
        sanitize_name(name)
    }
}

/// Убирает из имени недопустимые для Windows символы.
pub fn sanitize_name(name: &str) -> String {
    let mut result: String = name
        .chars()
        .map(|character| {
            if matches!(
                character,
                '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
            ) || character.is_control()
            {
                '_'
            } else {
                character
            }
        })
        .collect();
    result = result
        .trim_matches(|character| character == ' ' || character == '.')
        .to_string();
    while result.contains("  ") {
        result = result.replace("  ", " ");
    }
    if result.is_empty() {
        "file".to_string()
    } else {
        result
    }
}

/// Возвращает свободное имя файла, добавляя « (1)», « (2)» и т.д.
pub fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let extension = path
        .extension()
        .map(|value| format!(".{}", value.to_string_lossy()))
        .unwrap_or_default();
    for index in 1..10_000 {
        let candidate = parent.join(format!("{stem} ({index}){extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    path.to_path_buf()
}

/// Квотирует строку для удалённого POSIX-shell.
pub fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "._-/".contains(character))
    {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Разбирает вывод `ls -lA --time-style=long-iso`.
pub fn parse_ls_output(output: &str) -> Vec<RemoteEntry> {
    let mut entries = Vec::new();
    for line in output.lines() {
        let line = line.trim_end();
        if line.is_empty() || line.starts_with("total ") {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 8 {
            continue;
        }
        let mode = fields[0];
        let is_dir = mode.starts_with('d');
        let is_symlink = mode.starts_with('l');
        let size = fields[4].parse::<u64>().unwrap_or(0);
        let modified = format!("{} {}", fields[5], fields[6]);
        let mut name = fields[7..].join(" ");
        if is_symlink && let Some((left, _)) = name.split_once(" -> ") {
            name = left.to_string();
        }
        if is_dir || is_symlink {
            name.truncate(name.trim_end().len());
        }
        if name.is_empty() {
            continue;
        }
        entries.push(RemoteEntry {
            name,
            is_dir,
            is_symlink,
            size,
            modified,
        });
    }
    entries.sort_by_key(|entry| (!entry.is_dir, entry.name.to_lowercase()));
    entries
}

/// Человекочитаемый размер файла.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[0])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// Проверяет, что путь не пытается выйти за пределы каталога.
pub fn normalize_remote_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return ".".to_string();
    }
    trimmed.to_string()
}

/// Присоединяет имя к удалённому пути.
pub fn join_remote(base: &str, name: &str) -> String {
    let base = base.trim();
    if base.is_empty() || base == "." {
        name.to_string()
    } else if base == "/" {
        format!("/{name}")
    } else {
        format!("{}/{}", base.trim_end_matches('/'), name)
    }
}

/// Родительский удалённый путь.
pub fn parent_remote(path: &str) -> String {
    let trimmed = path.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." || trimmed == "/" {
        return ".".to_string();
    }
    match trimmed.rsplit_once('/') {
        Some(("", _)) => "/".to_string(),
        Some((parent, _)) if !parent.is_empty() => parent.to_string(),
        _ => ".".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const LISTING: &str = "total 20\n\
drwxr-xr-x 2 deploy deploy 4096 2026-11-11 23:10 logs\n\
-rw-r--r-- 1 deploy deploy 1024 2026-11-11 23:05 app.conf\n\
-rw-r--r-- 1 deploy deploy 2048 2026-11-10 20:00 my notes.txt\n\
lrwxrwxrwx 1 deploy deploy 5 2026-11-09 10:00 current -> release\n\
-rw-r--r-- 1 deploy deploy 42 2026-11-08 09:00 привет.txt\n";

    #[test]
    fn parses_listing_with_dirs_files_and_symlinks() {
        let entries = parse_ls_output(LISTING);
        assert_eq!(entries.len(), 5);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "logs");

        let find = |name: &str| entries.iter().find(|entry| entry.name == name).expect(name);
        assert_eq!(find("app.conf").size, 1024);
        assert_eq!(find("my notes.txt").size, 2048);
        assert!(find("current").is_symlink);
        assert!(!find("current").is_dir);
        assert_eq!(find("привет.txt").name, "привет.txt");
    }

    #[test]
    fn quotes_remote_paths() {
        assert_eq!(shell_quote("/var/log"), "/var/log");
        assert_eq!(shell_quote("my file.txt"), "'my file.txt'");
        assert_eq!(shell_quote("it's.txt"), "'it'\\''s.txt'");
    }

    #[test]
    fn sanitizes_and_resolves_local_names() {
        assert_eq!(sanitize_name("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(remote_file_name("/var/log/"), "log");
        assert_eq!(remote_file_name("/"), "file");
        assert_eq!(sanitize_name("  ...  "), "file");
    }

    #[test]
    fn unique_path_appends_suffix() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join(format!("downloads-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("каталог");
        let first = dir.join("file.txt");
        fs::write(&first, "x").expect("файл");
        let second = unique_path(&first);
        assert_eq!(
            second.file_name().and_then(|n| n.to_str()),
            Some("file (1).txt")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(format_size(512), "512 Б");
        assert_eq!(format_size(2048), "2.0 КБ");
        assert_eq!(format_size(1024 * 1024), "1.0 МБ");
    }

    #[test]
    fn normalizes_paths() {
        assert_eq!(normalize_remote_path("  "), ".");
        assert_eq!(normalize_remote_path("/var/log"), "/var/log");
    }

    #[test]
    fn joins_and_walks_paths() {
        assert_eq!(join_remote(".", "logs"), "logs");
        assert_eq!(join_remote("/", "etc"), "/etc");
        assert_eq!(join_remote("/var/log", "app"), "/var/log/app");
        assert_eq!(parent_remote("logs"), ".");
        assert_eq!(parent_remote("/var/log"), "/var");
        assert_eq!(parent_remote("/var"), "/");
        assert_eq!(parent_remote("/"), ".");
        assert_eq!(parent_remote("."), ".");
    }

    #[test]
    fn download_directory_uses_server_name() {
        let mut server = Server::new(1);
        server.name = "prod/web".to_string();
        let directory = download_directory(&server);
        assert!(directory.ends_with(Path::new("Downloads").join("Ssh-manager").join("prod_web")));
    }
}
