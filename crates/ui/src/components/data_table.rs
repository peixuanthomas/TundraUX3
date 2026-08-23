use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::buffer::CellWidth;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::RenderContext;

use super::{
    ComponentEvent, ComponentId, ComponentState, ComponentTone, InputEvent, Key, MouseButton,
    MouseKind, contains_point, tone_color,
};

/// A lightweight, component-owned table suitable for selectable data rows.
/// Column layout is deterministic and works with wide terminal glyphs because
/// Ratatui performs the final cell clipping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataTable {
    pub id: ComponentId,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub selected: Option<usize>,
    pub state: ComponentState,
    pub title: Option<String>,
    pub column_widths: Option<Vec<u16>>,
    pub viewport_start: usize,
    pub show_header: bool,
    pub row_tones: Vec<ComponentTone>,
    pub bordered: bool,
}

impl DataTable {
    pub fn new(
        id: impl Into<ComponentId>,
        headers: impl IntoIterator<Item = impl Into<String>>,
        rows: impl IntoIterator<Item = impl IntoIterator<Item = impl Into<String>>>,
    ) -> Self {
        let rows = rows
            .into_iter()
            .map(|row| row.into_iter().map(Into::into).collect())
            .collect::<Vec<Vec<String>>>();
        Self {
            id: id.into(),
            headers: headers.into_iter().map(Into::into).collect(),
            selected: (!rows.is_empty()).then_some(0),
            rows,
            state: ComponentState::default(),
            title: None,
            column_widths: None,
            viewport_start: 0,
            show_header: true,
            row_tones: Vec::new(),
            bordered: true,
        }
    }

    pub fn with_column_widths(mut self, widths: Vec<u16>) -> Self {
        self.column_widths = Some(widths);
        self
    }
    pub const fn with_viewport_start(mut self, start: usize) -> Self {
        self.viewport_start = start;
        self
    }
    pub const fn show_header(mut self, show: bool) -> Self {
        self.show_header = show;
        self
    }
    pub fn with_row_tones(mut self, tones: Vec<ComponentTone>) -> Self {
        self.row_tones = tones;
        self
    }
    pub const fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn handle_event(&mut self, event: InputEvent, area: Rect) -> ComponentEvent {
        match event {
            InputEvent::FocusGained => {
                self.state.focused = true;
                ComponentEvent::Consumed
            }
            InputEvent::FocusLost => {
                self.state.focused = false;
                ComponentEvent::Consumed
            }
            InputEvent::Key(key) if key.is_press_like() && self.state.focused => match key.key {
                Key::Up => self.select_offset(-1),
                Key::Down => self.select_offset(1),
                Key::Home => self.select_row(0),
                Key::End => self.select_row(self.rows.len().saturating_sub(1)),
                Key::Enter | Key::Space => self
                    .selected
                    .map(|row| {
                        ComponentEvent::Activated(ComponentId::from(format!(
                            "{}.{}",
                            self.id.as_str(),
                            row
                        )))
                    })
                    .unwrap_or(ComponentEvent::Consumed),
                _ => ComponentEvent::None,
            },
            InputEvent::Mouse(mouse)
                if matches!(
                    mouse.kind,
                    MouseKind::Down(MouseButton::Left) | MouseKind::Click(MouseButton::Left)
                ) =>
            {
                let row = self.row_at(area, mouse.column(), mouse.row());
                if let Some(row) = row {
                    self.state.focused = true;
                    self.select_row(row)
                } else {
                    ComponentEvent::None
                }
            }
            _ => ComponentEvent::None,
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        let tokens = context.theme;
        let mut block = Block::default()
            .style(Style::default().fg(tokens.text).bg(tokens.surface))
            .border_style(Style::default().fg(tokens.border).bg(tokens.surface));
        if self.bordered {
            block = block.borders(Borders::ALL);
        }
        if let Some(title) = &self.title {
            block = block.title(title.as_str());
        }
        let inner = block.inner(area);
        block.render(area, buffer);
        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let header_height = u16::from(self.show_header);
        if self.show_header {
            Paragraph::new(join_row(
                &self.headers,
                inner.width,
                self.column_widths.as_deref(),
            ))
            .alignment(HorizontalAlignment::Left)
            .style(
                Style::default()
                    .fg(tokens.muted)
                    .bg(tokens.raised)
                    .add_modifier(Modifier::BOLD),
            )
            .render(Rect::new(inner.x, inner.y, inner.width, 1), buffer);
        }
        for (index, row) in self
            .rows
            .iter()
            .enumerate()
            .skip(self.viewport_start)
            .take(inner.height.saturating_sub(header_height) as usize)
        {
            let selected = self.selected == Some(index);
            let style = if selected {
                Style::default()
                    .fg(if self.state.focused {
                        tokens.focus
                    } else {
                        tone_color(
                            self.row_tones.get(index).copied().unwrap_or_default(),
                            &context.compatibility_theme(),
                        )
                    })
                    .bg(tokens.accent_soft)
            } else {
                Style::default()
                    .fg(tone_color(
                        self.row_tones.get(index).copied().unwrap_or_default(),
                        &context.compatibility_theme(),
                    ))
                    .bg(tokens.surface)
            };
            Paragraph::new(join_row(row, inner.width, self.column_widths.as_deref()))
                .alignment(HorizontalAlignment::Left)
                .style(style)
                .render(
                    Rect::new(
                        inner.x,
                        inner
                            .y
                            .saturating_add(header_height)
                            .saturating_add((index - self.viewport_start) as u16),
                        inner.width,
                        1,
                    ),
                    buffer,
                );
        }
    }

    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        self.render(area, frame.buffer_mut(), context);
    }

    fn select_offset(&mut self, offset: isize) -> ComponentEvent {
        if self.rows.is_empty() {
            return ComponentEvent::Consumed;
        }
        let next = (self.selected.unwrap_or(0) as isize + offset)
            .clamp(0, self.rows.len().saturating_sub(1) as isize) as usize;
        self.select_row(next)
    }
    fn select_row(&mut self, row: usize) -> ComponentEvent {
        if self.rows.get(row).is_none() || self.selected == Some(row) {
            ComponentEvent::Consumed
        } else {
            self.selected = Some(row);
            ComponentEvent::Selected(self.id.clone(), row)
        }
    }
    fn row_at(&self, area: Rect, column: u16, row: u16) -> Option<usize> {
        if !contains_point(area, column, row) {
            return None;
        }
        row.checked_sub(
            area.y
                .saturating_add(u16::from(self.bordered) + u16::from(self.show_header)),
        )
        .map(|offset| self.viewport_start.saturating_add(usize::from(offset)))
        .filter(|index| *index < self.rows.len())
    }
}

fn join_row(cells: &[String], width: u16, explicit_widths: Option<&[u16]>) -> String {
    if cells.is_empty() {
        return String::new();
    }
    let separator = if explicit_widths.is_some() { "" } else { " " };
    let separators = if explicit_widths.is_some() {
        0
    } else {
        cells.len().saturating_sub(1)
    };
    let fallback = usize::from(width).saturating_sub(separators) / cells.len();
    cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let cell_width = explicit_widths
                .and_then(|widths| widths.get(index))
                .copied()
                .map(usize::from)
                .unwrap_or(fallback);
            let padding = cell_width.saturating_sub(usize::from(cell.cell_width()));
            format!("{cell}{}", " ".repeat(padding))
        })
        .collect::<Vec<_>>()
        .join(separator)
}
