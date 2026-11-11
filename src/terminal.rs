//! Встроенный терминал: псевдотерминал Windows + разбор вывода vt100.

use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::model::Server;

/// Сколько строк истории хранить в терминале.
const SCROLLBACK_LINES: usize = 2_000;

/// Размер терминала в ячейках.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalSize {
    pub rows: u16,
    pub cols: u16,
}

impl TerminalSize {
    /// Ограничивает размер разумными значениями.
    pub fn normalized(rows: u16, cols: u16) -> Self {
        Self {
            rows: rows.clamp(4, 200),
            cols: cols.clamp(20, 400),
        }
    }
}

/// Событие из фонового потока чтения псевдотерминала.
#[derive(Debug)]
pub enum TerminalEvent {
    /// Очередная порция вывода.
    Output(Vec<u8>),
    /// Процесс завершился или поток чтения закрыт.
    Closed,
}

/// Активная терминальная сессия.
pub struct TerminalSession {
    /// Название окна.
    pub title: String,
    /// Идентификатор сервера.
    pub server_id: u64,
    /// Состояние экрана, которое обновляет поток чтения.
    pub parser: vt100::Parser,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    receiver: Receiver<TerminalEvent>,
    size: TerminalSize,
    /// Процесс завершился.
    pub closed: bool,
    /// Ошибка ввода/вывода.
    pub error: Option<String>,
}

impl TerminalSession {
    /// Запускает `ssh` с параметрами сервера в псевдотерминале.
    pub fn spawn_ssh(
        server: &Server,
        size: TerminalSize,
        ctx: &egui::Context,
    ) -> Result<Self, String> {
        let mut command = CommandBuilder::new("ssh");
        for arg in crate::ssh::command::build_args(server) {
            command.arg(arg);
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        Self::spawn_program(server.id, server.name.clone(), command, size, ctx)
    }

    /// Запускает произвольную команду в псевдотерминале (используется и в тестах).
    pub fn spawn_program(
        server_id: u64,
        title: String,
        command: CommandBuilder,
        size: TerminalSize,
        ctx: &egui::Context,
    ) -> Result<Self, String> {
        let size = TerminalSize::normalized(size.rows, size.cols);
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("Не удалось создать псевдотерминал: {error}"))?;

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("Не удалось запустить ssh: {error}"))?;
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| format!("Не удалось читать вывод терминала: {error}"))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| format!("Не удалось получить ввод терминала: {error}"))?;

        let (sender, receiver) = mpsc::channel();
        spawn_reader(reader, sender, ctx.clone());

        let parser = vt100::Parser::new(
            NonZeroU16::new(size.rows).unwrap_or(NonZeroU16::MIN),
            NonZeroU16::new(size.cols).unwrap_or(NonZeroU16::MIN),
            SCROLLBACK_LINES,
        );

        Ok(Self {
            title,
            server_id,
            parser,
            master: pair.master,
            writer,
            child: Some(child),
            receiver,
            size,
            closed: false,
            error: None,
        })
    }

    /// Забирает накопившийся вывод и обновляет состояние экрана.
    pub fn drain(&mut self) {
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                TerminalEvent::Output(bytes) => {
                    // ConPTY при старте спрашивает позицию курсора (ESC[6n) и без
                    // ответа не отдаёт дальнейший вывод.
                    if bytes.windows(4).any(|window| window == b"\x1b[6n") {
                        self.write_bytes(b"\x1b[1;1R");
                    }
                    self.parser.process(&bytes);
                }
                TerminalEvent::Closed => self.closed = true,
            }
        }
    }

    /// Отправляет байты в терминал.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        if self.closed || bytes.is_empty() {
            return;
        }
        let result = self
            .writer
            .write_all(bytes)
            .and_then(|()| self.writer.flush());
        if let Err(error) = result {
            self.error = Some(format!("Не удалось отправить ввод: {error}"));
            self.closed = true;
        }
    }

    /// Отправляет текст в терминал.
    pub fn write_text(&mut self, text: &str) {
        self.write_bytes(text.as_bytes());
    }

    /// Меняет размер псевдотерминала и экрана.
    pub fn resize(&mut self, size: TerminalSize) {
        let size = TerminalSize::normalized(size.rows, size.cols);
        if size == self.size {
            return;
        }
        self.size = size;
        let _ = self.master.resize(PtySize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        if let (Some(rows), Some(cols)) = (NonZeroU16::new(size.rows), NonZeroU16::new(size.cols)) {
            self.parser.set_size(rows, cols);
        }
    }

    /// Текущий размер терминала.
    pub fn size(&self) -> TerminalSize {
        self.size
    }

    /// Возвращает экран для отрисовки.
    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        // Закрытие псевдотерминала завершает дочерний процесс; явный kill может
        // заблокироваться, поэтому выполняем его в отдельном потоке.
        if let Some(mut child) = self.child.take() {
            std::thread::spawn(move || {
                let _ = child.kill();
            });
        }
    }
}

fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    sender: Sender<TerminalEvent>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    let _ = sender.send(TerminalEvent::Closed);
                    ctx.request_repaint();
                    break;
                }
                Ok(read) => {
                    let _ = sender.send(TerminalEvent::Output(buffer[..read].to_vec()));
                    ctx.request_repaint();
                }
                Err(_) => {
                    let _ = sender.send(TerminalEvent::Closed);
                    ctx.request_repaint();
                    break;
                }
            }
        }
    });
}

/// Преобразует нажатие клавиши в байты для терминала.
pub fn key_to_bytes(key: egui::Key, modifiers: egui::Modifiers) -> Option<Vec<u8>> {
    let control = modifiers.ctrl || modifiers.command;

    let with_modifier = |sequence: &str, modifier: Option<u8>| -> Vec<u8> {
        match modifier {
            Some(code) if code != 1 => format!("\x1b[1;{code}{sequence}").into_bytes(),
            _ => format!("\x1b[{sequence}").into_bytes(),
        }
    };
    let tilde = |number: u8, modifier: Option<u8>| -> Vec<u8> {
        match modifier {
            Some(code) if code != 1 => format!("\x1b[{number};{code}~").into_bytes(),
            _ => format!("\x1b[{number}~").into_bytes(),
        }
    };

    let modifier = modifier_code(modifiers);
    match key {
        egui::Key::Enter => Some(b"\r".to_vec()),
        egui::Key::Tab => Some(b"\t".to_vec()),
        egui::Key::Backspace => Some(vec![0x7f]),
        egui::Key::Escape => Some(vec![0x1b]),
        egui::Key::ArrowUp => Some(with_modifier("A", modifier)),
        egui::Key::ArrowDown => Some(with_modifier("B", modifier)),
        egui::Key::ArrowRight => Some(with_modifier("C", modifier)),
        egui::Key::ArrowLeft => Some(with_modifier("D", modifier)),
        egui::Key::Home => Some(with_modifier("H", modifier)),
        egui::Key::End => Some(with_modifier("F", modifier)),
        egui::Key::Insert => Some(tilde(2, modifier)),
        egui::Key::Delete => Some(tilde(3, modifier)),
        egui::Key::PageUp => Some(tilde(5, modifier)),
        egui::Key::PageDown => Some(tilde(6, modifier)),
        egui::Key::F1 => Some(function_key('P', modifier)),
        egui::Key::F2 => Some(function_key('Q', modifier)),
        egui::Key::F3 => Some(function_key('R', modifier)),
        egui::Key::F4 => Some(function_key('S', modifier)),
        egui::Key::F5 => Some(tilde(15, modifier)),
        egui::Key::F6 => Some(tilde(17, modifier)),
        egui::Key::F7 => Some(tilde(18, modifier)),
        egui::Key::F8 => Some(tilde(19, modifier)),
        egui::Key::F9 => Some(tilde(20, modifier)),
        egui::Key::F10 => Some(tilde(21, modifier)),
        egui::Key::F11 => Some(tilde(23, modifier)),
        egui::Key::F12 => Some(tilde(24, modifier)),
        _ if control => control_code(key, modifiers),
        _ => None,
    }
}

fn function_key(letter: char, modifier: Option<u8>) -> Vec<u8> {
    match modifier {
        Some(code) if code != 1 => format!("\x1b[1;{code}{letter}").into_bytes(),
        _ => format!("\x1bO{letter}").into_bytes(),
    }
}

fn modifier_code(modifiers: egui::Modifiers) -> Option<u8> {
    let mut code = 1u8;
    if modifiers.shift {
        code += 1;
    }
    if modifiers.alt {
        code += 2;
    }
    if modifiers.ctrl || modifiers.command {
        code += 4;
    }
    if code == 1 { None } else { Some(code) }
}

fn control_code(key: egui::Key, modifiers: egui::Modifiers) -> Option<Vec<u8>> {
    if modifiers.alt {
        return None;
    }
    let name = key.name();
    if name.len() == 1 {
        let character = name.chars().next()?;
        if character.is_ascii_alphabetic() {
            return Some(vec![character.to_ascii_uppercase() as u8 - b'A' + 1]);
        }
        if character == '[' {
            return Some(vec![0x1b]);
        }
        if character == '\\' {
            return Some(vec![0x1c]);
        }
        if character == ']' {
            return Some(vec![0x1d]);
        }
    }
    if key == egui::Key::Space {
        return Some(vec![0]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl() -> egui::Modifiers {
        egui::Modifiers::CTRL
    }

    #[test]
    fn maps_basic_keys() {
        assert_eq!(
            key_to_bytes(egui::Key::Enter, egui::Modifiers::NONE),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            key_to_bytes(egui::Key::Backspace, egui::Modifiers::NONE),
            Some(vec![0x7f])
        );
        assert_eq!(
            key_to_bytes(egui::Key::ArrowUp, egui::Modifiers::NONE),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_to_bytes(egui::Key::Delete, egui::Modifiers::NONE),
            Some(b"\x1b[3~".to_vec())
        );
        assert_eq!(
            key_to_bytes(egui::Key::F1, egui::Modifiers::NONE),
            Some(b"\x1bOP".to_vec())
        );
    }

    #[test]
    fn maps_ctrl_combinations() {
        assert_eq!(key_to_bytes(egui::Key::C, ctrl()), Some(vec![0x03]));
        assert_eq!(key_to_bytes(egui::Key::D, ctrl()), Some(vec![0x04]));
        assert_eq!(key_to_bytes(egui::Key::Space, ctrl()), Some(vec![0]));
    }

    #[test]
    fn plain_letters_are_left_to_text_events() {
        assert_eq!(key_to_bytes(egui::Key::A, egui::Modifiers::NONE), None);
    }

    #[test]
    fn pty_echo_reaches_vt100_screen() {
        let ctx = egui::Context::default();
        let mut command = CommandBuilder::new("cmd.exe");
        command.arg("/c");
        command.arg("echo ssh-manager-terminal-test");
        let mut session = TerminalSession::spawn_program(
            1,
            "test".to_string(),
            command,
            TerminalSize::normalized(24, 80),
            &ctx,
        )
        .expect("псевдотерминал должен запускаться");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut found = false;
        while std::time::Instant::now() < deadline {
            session.drain();
            if session
                .screen()
                .contents()
                .contains("ssh-manager-terminal-test")
            {
                found = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(found, "вывод команды должен появиться на экране терминала");
    }
}
