//! Запуск SSH-сессии в новом окне Windows Terminal с фолбэком на `cmd.exe`.

use std::io;
use std::process::{Command, Stdio};

use crate::model::Server;
use crate::ssh::command::{build_args, command_line};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;

/// Каким терминалом удалось запустить сессию.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launcher {
    /// Новое окно Windows Terminal.
    WindowsTerminal,
    /// Новое окно `cmd.exe` (Windows Terminal не найден).
    CommandPrompt,
}

impl Launcher {
    /// Человекочитаемое имя терминала для статусной строки.
    pub fn label(self) -> &'static str {
        match self {
            Self::WindowsTerminal => "Windows Terminal",
            Self::CommandPrompt => "cmd.exe",
        }
    }
}

/// Запускает `ssh` в новом окне терминала.
///
/// Сначала пробует Windows Terminal, при ошибке — `cmd.exe`.
pub fn launch(server: &Server) -> Result<Launcher, String> {
    match launch_windows_terminal(server) {
        Ok(()) => Ok(Launcher::WindowsTerminal),
        Err(wt_error) => match launch_command_prompt(server) {
            Ok(()) => Ok(Launcher::CommandPrompt),
            Err(cmd_error) => Err(format!(
                "Не удалось запустить терминал. Windows Terminal: {wt_error}. cmd.exe: {cmd_error}."
            )),
        },
    }
}

/// Аргументы `wt.exe`: принудительно новое окно, новая вкладка и команда ssh.
fn windows_terminal_args(server: &Server) -> Vec<String> {
    let mut args = vec![
        "-w".to_string(),
        "-1".to_string(),
        "new-tab".to_string(),
        "ssh".to_string(),
    ];
    args.extend(build_args(server));
    args
}

fn launch_windows_terminal(server: &Server) -> io::Result<()> {
    let mut command = Command::new("wt.exe");
    command.args(windows_terminal_args(server));
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    set_creation_flags(&mut command, CREATE_NO_WINDOW);
    command.spawn().map(|_| ())
}

fn launch_command_prompt(server: &Server) -> io::Result<()> {
    let mut command = Command::new("cmd.exe");
    command.args(["/D", "/K", &command_line(server)]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    set_creation_flags(&mut command, CREATE_NEW_CONSOLE);
    command.spawn().map(|_| ())
}

fn set_creation_flags(command: &mut Command, flags: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(flags);
    }
    #[cfg(not(windows))]
    {
        let _ = (command, flags);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_terminal_args_force_new_window_and_pass_ssh_command() {
        let mut server = Server::new(1);
        server.name = "web".to_string();
        server.host = "10.0.0.5".to_string();
        server.user = "root".to_string();
        server.port = 2222;

        assert_eq!(
            windows_terminal_args(&server),
            vec!["-w", "-1", "new-tab", "ssh", "-p", "2222", "root@10.0.0.5"]
        );
    }

    #[test]
    fn launcher_labels_are_human_readable() {
        assert_eq!(Launcher::WindowsTerminal.label(), "Windows Terminal");
        assert_eq!(Launcher::CommandPrompt.label(), "cmd.exe");
    }
}
