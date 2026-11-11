//! Импорт серверов из стандартного `%USERPROFILE%\.ssh\config`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::{DEFAULT_PORT, Server};

/// Путь к стандартному SSH-конфигу (`%USERPROFILE%\.ssh\config` или `~/.ssh/config`).
pub fn default_config_path() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join(".ssh").join("config"))
}

/// Читает и разбирает SSH-конфиг по пути.
pub fn read_config(path: &Path) -> io::Result<Vec<Server>> {
    let text = fs::read_to_string(path)?;
    Ok(parse_ssh_config(&text))
}

/// Разбирает текст SSH-конфига.
///
/// Возвращает серверы с `id == 0` (идентификаторы и группы выдаёт приложение)
/// и `notes == ""`. Блоки `Host` только с шаблонами (`*`, `?`) и блоки `Match`
/// пропускаются. Группа берётся из комментария вида `# tag: Prod`.
pub fn parse_ssh_config(text: &str) -> Vec<Server> {
    let mut servers: Vec<Server> = Vec::new();
    let mut current: Option<Block> = None;
    let mut pending_tag: Option<String> = None;
    let mut in_match = false;

    for raw_line in text.lines() {
        let (body, comment) = split_comment(raw_line);
        let tag = comment.and_then(extract_tag);
        let body = body.trim();

        if body.is_empty() {
            if let Some(tag) = tag {
                match current.as_mut() {
                    Some(block) => block.tag = Some(tag),
                    None => pending_tag = Some(tag),
                }
            }
            continue;
        }

        let (directive, value) = split_directive(body);
        match directive.as_str() {
            "host" => {
                if let Some(block) = current.take() {
                    push_block(block, &mut servers);
                }
                in_match = false;
                current = Some(Block {
                    patterns: parse_patterns(value),
                    tag: tag.clone().or_else(|| pending_tag.take()),
                    ..Block::default()
                });
            }
            "match" => {
                in_match = true;
            }
            _ if in_match => {}
            "hostname" => {
                if let Some(block) = current.as_mut() {
                    block.hostname = Some(unquote(value));
                }
            }
            "user" => {
                if let Some(block) = current.as_mut() {
                    block.user = Some(unquote(value));
                }
            }
            "port" => {
                if let Some(block) = current.as_mut() {
                    block.port = value.trim().parse::<u16>().ok();
                }
            }
            "identityfile" => {
                if let Some(block) = current.as_mut() {
                    block.identity_file = Some(unquote(value));
                }
            }
            "proxyjump" => {
                if let Some(block) = current.as_mut() {
                    block.jump = Some(unquote(value));
                }
            }
            _ => {}
        }

        if let Some(tag) = tag
            && directive != "host"
            && let Some(block) = current.as_mut()
        {
            block.tag = Some(tag);
        }
    }

    if let Some(block) = current.take() {
        push_block(block, &mut servers);
    }

    deduplicate(&mut servers);
    servers
}

/// Уже есть сервер с тем же хостом, портом и пользователем?
pub fn is_duplicate(existing: &[Server], candidate: &Server) -> bool {
    existing.iter().any(|server| {
        server
            .host
            .trim()
            .eq_ignore_ascii_case(candidate.host.trim())
            && server.port == candidate.port
            && server
                .user
                .trim()
                .eq_ignore_ascii_case(candidate.user.trim())
    })
}

#[derive(Debug, Default)]
struct Block {
    patterns: Vec<String>,
    hostname: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<String>,
    jump: Option<String>,
    tag: Option<String>,
}

impl Block {
    fn into_server(self) -> Option<Server> {
        let name = self
            .patterns
            .iter()
            .find(|pattern| !pattern.contains('*') && !pattern.contains('?'))?
            .clone();
        if name.is_empty() {
            return None;
        }

        Some(Server {
            id: 0,
            name,
            group: self.tag.unwrap_or_default().trim().to_string(),
            host: self.hostname.unwrap_or_default(),
            port: self.port.unwrap_or(DEFAULT_PORT),
            user: self.user.unwrap_or_default(),
            key_path: self.identity_file.unwrap_or_default(),
            jump: self.jump.unwrap_or_default(),
            notes: String::new(),
            tunnels: Vec::new(),
        })
    }
}

fn push_block(block: Block, servers: &mut Vec<Server>) {
    if let Some(mut server) = block.into_server() {
        if server.host.trim().is_empty() {
            server.host = server.name.clone();
        }
        servers.push(server);
    }
}

fn deduplicate(servers: &mut Vec<Server>) {
    let mut unique: Vec<Server> = Vec::with_capacity(servers.len());
    for server in servers.drain(..) {
        if !is_duplicate(&unique, &server) {
            unique.push(server);
        }
    }
    *servers = unique;
}

/// Отделяет комментарий `# ...` от строки, не заглядывая внутрь кавычек.
fn split_comment(line: &str) -> (&str, Option<&str>) {
    let mut in_quotes = false;
    for (index, character) in line.char_indices() {
        match character {
            '"' => in_quotes = !in_quotes,
            '#' if !in_quotes => return (&line[..index], Some(&line[index + 1..])),
            _ => {}
        }
    }
    (line, None)
}

/// Ищет в комментарии метку вида `tag: Prod`.
fn extract_tag(comment: &str) -> Option<String> {
    let lower = comment.to_lowercase();
    let index = lower.find("tag:")?;
    let value = comment[index + 4..].trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// Делит строку на директиву и значение.
fn split_directive(line: &str) -> (String, &str) {
    match line.find(char::is_whitespace) {
        Some(index) => (line[..index].to_lowercase(), line[index..].trim()),
        None => (line.to_lowercase(), ""),
    }
}

/// Убирает обрамляющие кавычки, если они есть.
fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

/// Список шаблонов из строки `Host`.
fn parse_patterns(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .map(unquote)
        .filter(|pattern| !pattern.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# Основной конфиг
Host web
    HostName 10.0.0.5
    User deploy
    Port 2222
    IdentityFile ~/.ssh/id_ed25519

Host db prod-db
    HostName db.example.com
    User admin
    ProxyJump bastion.example.com

Host *
    User default
"#;

    #[test]
    fn parses_hosts_and_skips_wildcard_blocks() {
        let servers = parse_ssh_config(SAMPLE);
        assert_eq!(servers.len(), 2);

        assert_eq!(servers[0].name, "web");
        assert_eq!(servers[0].host, "10.0.0.5");
        assert_eq!(servers[0].user, "deploy");
        assert_eq!(servers[0].port, 2222);
        assert_eq!(servers[0].key_path, "~/.ssh/id_ed25519");

        assert_eq!(servers[1].name, "db");
        assert_eq!(servers[1].host, "db.example.com");
        assert_eq!(servers[1].user, "admin");
        assert_eq!(servers[1].port, DEFAULT_PORT);
        assert_eq!(servers[1].jump, "bastion.example.com");
    }

    #[test]
    fn directive_names_are_case_insensitive() {
        let text = "Host box\n HOSTNAME box.example.com\n uSeR root\n PoRt 2200\n IDENTITYFILE \"C:\\Users\\me\\.ssh\\custom key\"\n";
        let servers = parse_ssh_config(text);
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].host, "box.example.com");
        assert_eq!(servers[0].user, "root");
        assert_eq!(servers[0].port, 2200);
        assert_eq!(servers[0].key_path, r"C:\Users\me\.ssh\custom key");
    }

    #[test]
    fn host_without_hostname_uses_pattern_as_host() {
        let servers = parse_ssh_config("Host home\n  User root\n");
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].host, "home");
    }

    #[test]
    fn tags_are_read_from_comments() {
        let text = "# tag: Prod\nHost prod-web\n  HostName 10.1.1.1\n\nHost dev-web\n  HostName 10.1.1.2\n  # TAG: Staging\n";
        let servers = parse_ssh_config(text);
        assert_eq!(servers[0].group, "Prod");
        assert_eq!(servers[1].group, "Staging");
    }

    #[test]
    fn match_blocks_are_ignored_until_next_host() {
        let text = "Host web\n  HostName 10.0.0.5\nMatch host *.example.com\n  User ignored\nHost db\n  HostName 10.0.0.6\n";
        let servers = parse_ssh_config(text);
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].user, "");
        assert_eq!(servers[1].name, "db");
    }

    #[test]
    fn invalid_port_falls_back_to_default() {
        let servers = parse_ssh_config("Host web\n  HostName 10.0.0.5\n  Port 99999\n");
        assert_eq!(servers[0].port, DEFAULT_PORT);
    }

    #[test]
    fn duplicates_are_removed() {
        let text =
            "Host a\n  HostName 10.0.0.5\n  User root\nHost b\n  HostName 10.0.0.5\n  User root\n";
        let servers = parse_ssh_config(text);
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "a");
    }

    #[test]
    fn comments_outside_host_blocks_are_ignored() {
        let servers = parse_ssh_config(
            "# просто комментарий\nHost web\n  # ещё комментарий\n  HostName 10.0.0.5\n",
        );
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].host, "10.0.0.5");
        assert_eq!(servers[0].group, "");
    }

    #[test]
    fn quoted_values_lose_quotes() {
        let servers = parse_ssh_config("Host web\n  HostName \"web.example.com\"\n");
        assert_eq!(servers[0].host, "web.example.com");
    }

    #[test]
    fn default_config_path_points_to_ssh_config() {
        let path = default_config_path().expect("USERPROFILE или HOME должны быть заданы");
        assert!(path.ends_with(Path::new(".ssh").join("config")), "{path:?}");
    }

    #[test]
    fn duplicate_detection_ignores_user_case_and_whitespace() {
        let mut existing = Server::new(1);
        existing.host = "10.0.0.5".to_string();
        existing.user = "Root".to_string();
        existing.port = 22;

        let mut candidate = Server::new(2);
        candidate.host = " 10.0.0.5 ".to_string();
        candidate.user = "root".to_string();
        candidate.port = 22;

        assert!(is_duplicate(&[existing], &candidate));
    }
}
