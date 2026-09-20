use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use unicode_width::UnicodeWidthStr;

// Keep the crash screen usable even when the theme or asset files are broken.
const DEAD_PROGRAM: &str = r"     .-----------------------------.
     |                             |
     |    \   /           \   /    |
     |      X               X      |
     |    /   \           /   \    |
     |                             |
     |           .----.            |
     |         .'      '.          |
     |        /          \         |
     |                             |
     '-----------------------------'
    /       ________________        \
   /_______/________________\________\";

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
            text: format!("{DEAD_PROGRAM}\n\n{}", message.replace('\t', "    ")),
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
        let style = Style::default().fg(Color::White).bg(Color::Black);
        frame.render_widget(Block::default().style(style), area);
        if area.is_empty() {
            return;
        }
        let header = Rect::new(area.x, area.y, area.width, 1);
        frame.render_widget(
            Paragraph::new(i18n::tr!("ui-panic-title")).style(style),
            header,
        );
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
            let help = if self.max_scroll > 0 {
                i18n::tr!("ui-panic-help-scroll")
            } else {
                i18n::tr!("ui-panic-help")
            };
            frame.render_widget(Paragraph::new(help).style(style), footer);
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
#[path = "../../tests/unit/screens/panic/tests.rs"]
mod tests;
