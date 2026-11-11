//! Окно встроенного терминала: отрисовка экрана vt100 и ввод с клавиатуры.

use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Vec2};

use crate::terminal::{TerminalSession, TerminalSize};
use crate::theme;

/// Открытое окно встроенного терминала.
pub struct TerminalWindow {
    /// Уникальный идентификатор окна.
    pub id: u64,
    /// Активная сессия.
    pub session: TerminalSession,
    /// Окно не закрыто пользователем.
    pub open: bool,
}

impl TerminalWindow {
    /// Создаёт окно для готовой сессии.
    pub fn new(id: u64, session: TerminalSession) -> Self {
        Self {
            id,
            session,
            open: true,
        }
    }

    /// Рисует содержимое окна (вызывается внутри `egui::Window`).
    pub fn show(&mut self, ui: &mut egui::Ui) {
        self.session.drain();

        ui.horizontal(|ui| {
            if let Some(error) = &self.session.error {
                ui.colored_label(theme::DANGER, error);
            } else if self.session.closed {
                ui.colored_label(theme::WARNING, "Сессия завершена");
            } else {
                ui.label(
                    egui::RichText::new(format!(
                        "{}×{}",
                        self.session.size().cols,
                        self.session.size().rows
                    ))
                    .small()
                    .weak(),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.session.screen().scrollback() > 0 && ui.button("Вниз").clicked() {
                    self.session.parser.screen_mut().set_scrollback(0);
                }
                if ui.button("Копировать экран").clicked() {
                    ui.ctx().copy_text(self.session.screen().contents());
                }
            });
        });

        let font = FontId::monospace(14.0);
        let (cell_width, row_height) = ui.fonts_mut(|fonts| {
            (
                fonts.glyph_width(&font, 'M').max(1.0),
                fonts.row_height(&font),
            )
        });

        let available = ui.available_size();
        let cols = ((available.x / cell_width).floor() as u16).clamp(20, 400);
        let rows = ((available.y / row_height).floor() as u16).clamp(4, 200);
        self.session.resize(TerminalSize { rows, cols });

        let widget_size = Vec2::new(cols as f32 * cell_width, rows as f32 * row_height);
        let (rect, response) = ui.allocate_exact_size(widget_size, Sense::click());
        if response.clicked() {
            response.request_focus();
        }

        if response.hovered() {
            let scroll_delta = ui.input(|input| input.smooth_scroll_delta.y);
            if scroll_delta.abs() > 0.1 {
                let screen = self.session.screen();
                let current = screen.scrollback() as isize;
                let maximum = screen.scrollback_len() as isize;
                let lines = (scroll_delta / row_height).round() as isize;
                let target = (current - lines).clamp(0, maximum) as usize;
                self.session.parser.screen_mut().set_scrollback(target);
            }
        }

        if response.has_focus() {
            let events = ui.input(|input| input.events.clone());
            for event in events {
                match event {
                    egui::Event::Text(text) if !text.is_empty() => {
                        self.session.write_text(&text);
                    }
                    egui::Event::Paste(text) => {
                        self.session.write_text(&text);
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if key == egui::Key::C && modifiers.ctrl && modifiers.shift {
                            ui.ctx().copy_text(self.session.screen().contents());
                            continue;
                        }
                        if let Some(bytes) = crate::terminal::key_to_bytes(key, modifiers) {
                            self.session.write_bytes(&bytes);
                        }
                    }
                    _ => {}
                }
            }
        }

        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 2.0, BACKGROUND);
        self.paint_screen(&painter, rect, &font, cell_width, row_height);

        if response.has_focus() && !self.session.closed && self.session.screen().scrollback() == 0 {
            let (cursor_row, cursor_col) = self.session.screen().cursor_position();
            let cell = self.session.screen().cell(cursor_row, cursor_col);
            let symbol = cell.map(|cell| cell.contents()).unwrap_or("");
            let position = Pos2::new(
                rect.left() + cursor_col as f32 * cell_width,
                rect.top() + cursor_row as f32 * row_height,
            );
            painter.rect_filled(
                Rect::from_min_size(position, Vec2::new(cell_width, row_height)),
                0.0,
                theme::ACCENT,
            );
            painter.text(
                position,
                Align2::LEFT_TOP,
                if symbol.is_empty() { " " } else { symbol },
                font.clone(),
                BACKGROUND,
            );
        }

        if !response.has_focus() {
            ui.label(
                RichText::new("Кликните по терминалу, чтобы вводить команды")
                    .small()
                    .weak(),
            );
        }
    }

    fn paint_screen(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        font: &FontId,
        cell_width: f32,
        row_height: f32,
    ) {
        let screen = self.session.screen();
        let (rows, cols) = screen.size();

        for row in 0..rows.get() {
            let mut runs: Vec<Run> = Vec::new();
            for col in 0..cols.get() {
                let Some(cell) = screen.cell(row, col) else {
                    continue;
                };
                let fg = color_to_egui(cell.fgcolor(), FOREGROUND, cell.bold());
                let bg = color_to_egui(cell.bgcolor(), BACKGROUND, false);
                let (fg, bg) = if cell.inverse() { (bg, fg) } else { (fg, bg) };
                let text = cell.contents();
                let underline = cell.underline();

                match runs.last_mut() {
                    Some(run) if run.fg == fg && run.bg == bg && run.underline == underline => {
                        run.text.push_str(if text.is_empty() { " " } else { text });
                    }
                    _ => runs.push(Run {
                        col,
                        text: if text.is_empty() {
                            " ".to_string()
                        } else {
                            text.to_string()
                        },
                        fg,
                        bg,
                        underline,
                    }),
                }
            }

            let y = rect.top() + row as f32 * row_height;
            for run in runs {
                let x = rect.left() + run.col as f32 * cell_width;
                let width = run.text.chars().count() as f32 * cell_width;
                let run_rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, row_height));
                if run.bg != BACKGROUND {
                    painter.rect_filled(run_rect, 0.0, run.bg);
                }
                painter.text(
                    Pos2::new(x, y),
                    Align2::LEFT_TOP,
                    &run.text,
                    font.clone(),
                    run.fg,
                );
                if run.underline {
                    painter.line_segment(
                        [
                            Pos2::new(x, y + row_height - 1.5),
                            Pos2::new(x + width, y + row_height - 1.5),
                        ],
                        (1.0, run.fg),
                    );
                }
            }
        }
    }
}

struct Run {
    col: u16,
    text: String,
    fg: Color32,
    bg: Color32,
    underline: bool,
}

const BACKGROUND: Color32 = Color32::from_rgb(11, 13, 18);
const FOREGROUND: Color32 = Color32::from_rgb(216, 222, 233);

fn color_to_egui(color: vt100::Color, default: Color32, bold: bool) -> Color32 {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Rgb(red, green, blue) => Color32::from_rgb(red, green, blue),
        vt100::Color::Idx(index) => palette(index, bold),
    }
}

fn palette(index: u8, bold: bool) -> Color32 {
    const ANSI: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 49, 49),
        (13, 188, 121),
        (229, 229, 16),
        (36, 114, 200),
        (188, 63, 188),
        (17, 168, 205),
        (229, 229, 229),
        (102, 102, 102),
        (241, 76, 76),
        (35, 209, 139),
        (245, 245, 67),
        (59, 142, 234),
        (214, 112, 214),
        (41, 184, 219),
        (255, 255, 255),
    ];

    if index < 16 {
        let index = if bold && index < 8 { index + 8 } else { index };
        let (red, green, blue) = ANSI[index as usize];
        return Color32::from_rgb(red, green, blue);
    }
    if index < 232 {
        let cube = index - 16;
        let red = (cube / 36) % 6;
        let green = (cube / 6) % 6;
        let blue = cube % 6;
        let channel = |value: u8| -> u8 { if value == 0 { 0 } else { 55 + value * 40 } };
        return Color32::from_rgb(channel(red), channel(green), channel(blue));
    }
    let gray = 8 + (index - 232) * 10;
    Color32::from_rgb(gray, gray, gray)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_covers_ansi_and_grayscale() {
        assert_eq!(palette(1, false), Color32::from_rgb(205, 49, 49));
        assert_eq!(palette(1, true), Color32::from_rgb(241, 76, 76));
        let gray = palette(255, false);
        assert_eq!(gray.r(), gray.g());
    }
}
