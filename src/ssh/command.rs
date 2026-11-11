//! Построение команды `ssh` из параметров сервера.

use crate::model::{Server, Tunnel};

/// Пользователь@хост (или только хост, если пользователь не задан).
pub fn target(server: &Server) -> String {
    let host = server.host.trim();
    let user = server.user.trim();
    if user.is_empty() {
        host.to_string()
    } else {
        format!("{user}@{host}")
    }
}

/// Аргументы для `ssh` (без самого имени программы).
pub fn build_args(server: &Server) -> Vec<String> {
    let mut args = Vec::new();

    if server.port != crate::model::DEFAULT_PORT {
        args.push("-p".to_string());
        args.push(server.port.to_string());
    }

    let key = server.key_path.trim();
    if !key.is_empty() {
        args.push("-i".to_string());
        args.push(key.to_string());
    }

    let jump = server.jump.trim();
    if !jump.is_empty() {
        args.push("-J".to_string());
        args.push(jump.to_string());
    }

    for tunnel in &server.tunnels {
        args.push("-L".to_string());
        args.push(tunnel_spec(tunnel));
    }

    args.push(target(server));
    args
}

/// Спецификация проброса для `-L`: `local_port:remote_host:remote_port`.
pub fn tunnel_spec(tunnel: &Tunnel) -> String {
    format!(
        "{}:{}:{}",
        tunnel.local_port,
        tunnel.remote_host.trim(),
        tunnel.remote_port
    )
}

/// Полная команда в виде строки (для показа и копирования в буфер обмена).
pub fn command_line(server: &Server) -> String {
    let mut parts = Vec::with_capacity(server.tunnels.len() + 5);
    parts.push("ssh".to_string());
    for arg in build_args(server) {
        parts.push(quote_arg(&arg));
    }
    parts.join(" ")
}

/// Квотирует аргумент по правилам командной строки Windows.
///
/// Простые аргументы остаются как есть; аргументы с пробелами или кавычками
/// заключаются в двойные кавычки.
pub fn quote_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".to_string();
    }
    if !arg.chars().any(|c| c.is_whitespace() || c == '"') {
        return arg.to_string();
    }
    let escaped = arg.replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DEFAULT_PORT, Server, Tunnel};

    fn server() -> Server {
        let mut server = Server::new(1);
        server.name = "web".to_string();
        server.host = "10.0.0.5".to_string();
        server.user = "root".to_string();
        server
    }

    #[test]
    fn minimal_server_uses_default_port_and_no_flags() {
        let server = server();
        assert_eq!(build_args(&server), vec!["root@10.0.0.5"]);
        assert_eq!(command_line(&server), "ssh root@10.0.0.5");
    }

    #[test]
    fn empty_user_targets_host_only() {
        let mut server = server();
        server.user = "  ".to_string();
        assert_eq!(target(&server), "10.0.0.5");
        assert_eq!(build_args(&server), vec!["10.0.0.5"]);
    }

    #[test]
    fn full_command_contains_port_key_jump_and_tunnels() {
        let mut server = server();
        server.port = 2222;
        server.key_path = r"C:\Users\me\.ssh\id_ed25519".to_string();
        server.jump = "bastion@jump.example.com".to_string();
        server.tunnels = vec![
            Tunnel {
                local_port: 8080,
                remote_host: "127.0.0.1".to_string(),
                remote_port: 80,
            },
            Tunnel {
                local_port: 5432,
                remote_host: "db.internal".to_string(),
                remote_port: 5432,
            },
        ];

        assert_eq!(
            build_args(&server),
            vec![
                "-p",
                "2222",
                "-i",
                r"C:\Users\me\.ssh\id_ed25519",
                "-J",
                "bastion@jump.example.com",
                "-L",
                "8080:127.0.0.1:80",
                "-L",
                "5432:db.internal:5432",
                "root@10.0.0.5",
            ]
        );
    }

    #[test]
    fn default_port_is_not_passed_explicitly() {
        let mut server = server();
        server.port = DEFAULT_PORT;
        assert_eq!(build_args(&server), vec!["root@10.0.0.5"]);
    }

    #[test]
    fn command_line_quotes_paths_with_spaces() {
        let mut server = server();
        server.key_path = r"C:\Users\my user\.ssh\id_ed25519".to_string();
        let line = command_line(&server);
        assert_eq!(
            line,
            r#"ssh -i "C:\Users\my user\.ssh\id_ed25519" root@10.0.0.5"#
        );
    }

    #[test]
    fn quote_arg_handles_simple_empty_and_quoted_values() {
        assert_eq!(quote_arg("htop"), "htop");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(quote_arg("a b"), "\"a b\"");
        assert_eq!(quote_arg("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn target_ignores_surrounding_whitespace() {
        let mut server = server();
        server.user = "  deploy ".to_string();
        server.host = " example.com ".to_string();
        assert_eq!(target(&server), "deploy@example.com");
        assert_eq!(build_args(&server), vec!["deploy@example.com"]);
    }

    #[test]
    fn tunnel_spec_formats_all_parts() {
        let tunnel = Tunnel {
            local_port: 8080,
            remote_host: " 127.0.0.1 ".to_string(),
            remote_port: 80,
        };
        assert_eq!(tunnel_spec(&tunnel), "8080:127.0.0.1:80");
    }
}
