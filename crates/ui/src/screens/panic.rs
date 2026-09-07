use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

// Keep the crash screen usable even when the theme or asset files are broken.
const DEAD_PROGRAM: &str = r"     .-----------------.
     |                 |
     |     X     X     |
     |        _        |
     |     R.I.P.      |
     '--------+--------'
          ___|___
         /_______\";

/// A standalone crash page with no dependency on assets or authentication.
pub struct PanicScreen {
    text: String,
    scroll: usize,
    max_scroll: usize,
    page_height: usize,
}

impl PanicScreen {
    pub fn new(message: &str) -> Self {
        let message: String = message
            .chars()
            .filter(|ch| *ch == '\n' || *ch == '\t' || !ch.is_control())
            .collect();
        Self {
            text: format!(
                "{DEAD_PROGRAM}\n\nTundraUX3 has stopped.\nRestart the program or exit.\n\nError:\n{}",
                message.replace('\t', "    ")
            ),
            scroll: 0,
            max_scroll: 0,
            page_height: 1,
        }
    }

    pub fn scroll_up(&mut self, page: bool) {
        self.scroll = self
            .scroll
            .saturating_sub(if page { self.page_height } else { 1 });
    }

    pub fn scroll_down(&mut self, page: bool) {
        self.scroll = self
            .scroll
            .saturating_add(if page { self.page_height } else { 1 })
            .min(self.max_scroll);
    }

    pub fn scroll_to_start(&mut self) {
        self.scroll = 0;
    }

    pub fn scroll_to_end(&mut self) {
        self.scroll = self.max_scroll;
    }

    pub fn render(&mut self, frame: &mut Frame<'_>) {
        let area = frame.area();
        let style = Style::default().fg(Color::White).bg(Color::Blue);
        frame.render_widget(Block::default().style(style), area);
        if area.is_empty() {
            return;
        }
        let header = Rect::new(area.x, area.y, area.width, 1);
        frame.render_widget(Paragraph::new("PANIC - TundraUX3").style(style), header);
        let body = Rect::new(
            area.x,
            area.y.saturating_add(1),
            area.width,
            area.height.saturating_sub(2),
        );
        let lines = wrap_lines(&self.text, body.width);
        self.page_height = usize::from(body.height).max(1);
        self.max_scroll = lines.len().saturating_sub(self.page_height);
        self.scroll = self.scroll.min(self.max_scroll);
        let visible: Vec<Line<'_>> = lines
            .iter()
            .skip(self.scroll)
            .take(self.page_height)
            .map(|line| Line::raw(line.as_str()))
            .collect();
        frame.render_widget(Paragraph::new(visible).style(style), body);
        if area.height > 1 {
            let footer = Rect::new(area.x, area.bottom() - 1, area.width, 1);
            frame.render_widget(
                Paragraph::new("R: Restart | Q: Exit | Up/Down/PgUp/PgDn/Home/End: Scroll")
                    .style(style),
                footer,
            );
        }
    }
}

fn wrap_lines(text: &str, width: u16) -> Vec<String> {
    if width == 0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    for source in text.split('\n') {
        let span = Span::raw(source);
        let mut line = String::new();
        let mut used = 0;
        for grapheme in span.styled_graphemes(Style::default()) {
            let size = grapheme.symbol.width();
            if used + size > usize::from(width) && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push_str(grapheme.symbol);
            used += size;
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    fn draw(screen: &mut PanicScreen, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| screen.render(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn crash_page_shows_hardcoded_art_and_error_without_assets() {
        let mut screen = PanicScreen::new("Intentional panic: 文件读取失败");
        let rendered = draw(&mut screen, 100, 30);
        assert!(rendered.contains("R.I.P."));
        assert!(rendered.contains("X     X"));
        assert!(rendered.contains("Intentional panic:"));
        // The test backend includes the blank trailing cell of wide characters.
        assert!(rendered.replace(' ', "").contains("文件读取失败"));
        assert!(rendered.contains("R: Restart | Q: Exit"));
    }

    #[test]
    fn long_unicode_error_can_be_scrolled_and_resize_clamps_position() {
        let message = format!("{}\nLAST ERROR", "报错文本".repeat(100));
        let mut screen = PanicScreen::new(&message);
        draw(&mut screen, 24, 8);
        screen.scroll_to_end();
        assert!(draw(&mut screen, 24, 8).contains("LAST ERROR"));
        screen.scroll_up(true);
        assert!(screen.scroll < screen.max_scroll);
        draw(&mut screen, 120, 50);
        assert_eq!(screen.scroll, 0);
        for (width, height) in [(0, 0), (1, 1), (2, 3), (10, 5)] {
            draw(&mut screen, width, height);
        }
    }
}
