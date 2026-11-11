//! Генерация пары ключей ed25519 через системный `ssh-keygen`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Результат генерации новой пары ключей.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedKey {
    /// Путь к закрытому ключу.
    pub private_path: PathBuf,
    /// Путь к публичному ключу.
    pub public_path: PathBuf,
    /// Содержимое публичного ключа.
    pub public_key: String,
}

/// Путь по умолчанию: `%USERPROFILE%\.ssh\id_ed25519`.
pub fn default_key_path() -> String {
    home_dir()
        .map(|home| home.join(".ssh").join("id_ed25519").display().to_string())
        .unwrap_or_else(|| "id_ed25519".to_string())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn public_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.pub", path.display()))
}

/// Проверяет путь до запуска `ssh-keygen` (не пустой, файлов ещё нет).
pub fn validate_path(path: &str) -> Result<PathBuf, String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err("Укажите путь для нового ключа.".to_string());
    }
    let path = PathBuf::from(trimmed);
    if path.exists() {
        return Err(format!(
            "Файл {} уже существует. Укажите другой путь, чтобы не перезаписать ключ.",
            path.display()
        ));
    }
    let public = public_path(&path);
    if public.exists() {
        return Err(format!(
            "Файл {} уже существует. Укажите другой путь, чтобы не перезаписать ключ.",
            public.display()
        ));
    }
    Ok(path)
}

/// Генерирует ключ `ed25519` без парольной фразы.
pub fn generate(path: &str, comment: &str) -> Result<GeneratedKey, String> {
    let path = validate_path(path)?;

    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::create_dir_all(dir)
            .map_err(|error| format!("Не удалось создать каталог {}: {error}", dir.display()))?;
    }

    let mut command = Command::new("ssh-keygen.exe");
    command.args([
        "-t",
        "ed25519",
        "-f",
        &path.display().to_string(),
        "-N",
        "",
        "-C",
        comment.trim(),
    ]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let output = command.output().map_err(|error| {
        format!(
            "Не удалось запустить ssh-keygen: {error}. Проверьте, что установлен OpenSSH Client."
        )
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let details = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("ssh-keygen завершился с ошибкой: {details}"));
    }

    let public = public_path(&path);
    let public_key = fs::read_to_string(&public).map_err(|error| {
        format!(
            "Ключ создан, но публичную часть {} прочитать не удалось: {error}",
            public.display()
        )
    })?;

    Ok(GeneratedKey {
        private_path: path,
        public_path: public,
        public_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_path() -> PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-data")
            .join(format!("key-{}-{id}", std::process::id()))
            .join("id_ed25519")
    }

    #[test]
    fn rejects_empty_path() {
        assert!(validate_path("   ").is_err());
    }

    #[test]
    fn rejects_existing_private_key() {
        let path = unique_path();
        fs::create_dir_all(path.parent().expect("каталог")).expect("создание каталога");
        fs::write(&path, "existing").expect("запись");
        assert!(validate_path(&path.display().to_string()).is_err());
        let _ = fs::remove_dir_all(path.parent().expect("каталог"));
    }

    #[test]
    fn rejects_existing_public_key() {
        let path = unique_path();
        fs::create_dir_all(path.parent().expect("каталог")).expect("создание каталога");
        fs::write(public_path(&path), "existing").expect("запись");
        assert!(validate_path(&path.display().to_string()).is_err());
        let _ = fs::remove_dir_all(path.parent().expect("каталог"));
    }

    #[test]
    fn accepts_fresh_path() {
        let path = unique_path();
        let validated = validate_path(&path.display().to_string()).expect("свежий путь допустим");
        assert_eq!(validated, path);
        assert_eq!(
            public_path(&path)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("pub")
        );
    }

    #[test]
    fn default_key_path_points_to_ssh_directory() {
        let path = default_key_path();
        assert!(path.ends_with("id_ed25519"), "{path}");
    }
}
