use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Clear, Paragraph};

use crate::RenderContext;

use super::{Button, ButtonEmphasis, ComponentId, ComponentTone, contains_point, tone_color};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageActionStatus {
    pub text: String,
    pub tone: ComponentTone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageAction {
    pub id: ComponentId,
    pub label: String,
    pub enabled: bool,
    pub emphasis: ButtonEmphasis,
    pub latched: bool,
    pub bracketed_label: bool,
}

impl PageAction {
    pub fn new(id: impl Into<ComponentId>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            enabled: true,
            emphasis: ButtonEmphasis::Normal,
            latched: false,
            bracketed_label: true,
        }
    }

    pub fn with_bracketed_label(mut self, bracketed: bool) -> Self {
        self.bracketed_label = bracketed;
        self
    }

    pub fn primary(mut self) -> Self {
        self.emphasis = ButtonEmphasis::Primary;
        self
    }

    pub fn danger(mut self) -> Self {
        self.emphasis = ButtonEmphasis::Danger;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.enabled = !disabled;
        self
    }

    pub fn latched(mut self, latched: bool) -> Self {
        self.latched = latched;
        self
    }
}

/// Renders at most 5 caller-supplied actions within 3 rows at the minimum supported width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageActionBar {
    pub page_id: ComponentId,
    pub status: Vec<PageActionStatus>,
    pub actions: Vec<PageAction>,
    pub selected: Option<ComponentId>,
}

impl PageActionBar {
    pub fn new(page_id: impl Into<ComponentId>) -> Self {
        Self {
            page_id: page_id.into(),
            status: Vec::new(),
            actions: Vec::new(),
            selected: None,
        }
    }

    pub fn with_status(mut self, status: Vec<PageActionStatus>) -> Self {
        self.status = status;
        self
    }

    pub fn with_actions(mut self, actions: Vec<PageAction>) -> Self {
        self.actions = actions;
        self
    }

    pub fn with_selected(mut self, selected: Option<ComponentId>) -> Self {
        self.selected = selected;
        self
    }

    pub fn height(&self, width: u16) -> u16 {
        let action_rows = self
            .action_rows(width)
            .iter()
            .map(|row| self.row_height(row, width))
            .sum::<u16>()
            .min(4);
        let status_capacity = 4u16.saturating_sub(action_rows);
        let status = (self.status.len() as u16).min(3).min(status_capacity);
        status.saturating_add(action_rows)
    }

    pub fn layout(&self, area: Rect) -> PageActionBarLayout {
        let wanted = self.height(area.width).min(area.height);
        let bar = Rect::new(
            area.x,
            area.bottom().saturating_sub(wanted),
            area.width,
            wanted,
        );
        let rows = self.action_rows(area.width);
        let action_height = rows
            .iter()
            .map(|row| self.row_height(row, area.width))
            .sum::<u16>()
            .min(bar.height);
        let status_capacity = 4usize.saturating_sub(usize::from(action_height));
        // Callers order status lines by priority, so keep the leading ones.
        let status_count =
            self.status
                .len()
                .min(3)
                .min(status_capacity)
                .min(usize::from(bar.height.saturating_sub(action_height))) as u16;
        let status = (0..status_count)
            .map(|index| Rect::new(bar.x, bar.y.saturating_add(index), bar.width, 1))
            .collect::<Vec<_>>();
        let mut actions = Vec::new();
        let mut bottom = bar.bottom();
        for row in &rows {
            let height = self
                .row_height(row, area.width)
                .min(bottom.saturating_sub(bar.y));
            if height == 0 {
                break;
            }
            let y = bottom - height;
            bottom = y;
            let mut right = bar.right();
            for &index in row {
                let width = self.action_width(index).min(bar.width);
                let x = right.saturating_sub(width);
                let action = &self.actions[index];
                actions.push(PageActionLayout {
                    index,
                    id: action.id.clone(),
                    area: Rect::new(x, y, width, height),
                    enabled: action.enabled,
                    emphasis: action.emphasis,
                });
                right = x.saturating_sub(1);
            }
        }
        PageActionBarLayout {
            area: bar,
            status,
            actions,
        }
    }

    pub fn render(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        context: &RenderContext,
    ) -> PageActionBarLayout {
        let layout = self.layout(area);
        let theme = context.compatibility_theme();
        frame.render_widget(Clear, layout.area);
        frame.render_widget(Paragraph::new("").style(theme.body_style()), layout.area);
        for (status, row) in self.status.iter().zip(&layout.status) {
            let style = Style::default()
                .fg(tone_color(status.tone, &theme))
                .bg(theme.background);
            frame.render_widget(Paragraph::new(status.text.clone()).style(style), *row);
        }
        for action_layout in &layout.actions {
            let action = &self.actions[action_layout.index];
            let mut button = Button::new(
                if self.page_id.as_str().is_empty() {
                    action.id.clone()
                } else {
                    format!("{}.{}", self.page_id, action.id).into()
                },
                self.action_label(action_layout.index, action_layout.area.width)
                    .join("\n"),
            )
            .with_bracketed_label(false)
            .with_emphasis(action.emphasis)
            .with_latched(action.latched)
            .with_focus_fill(Some(context.theme.accent_soft));
            button.set_disabled(!action.enabled);
            button.state.selected = self.selected.as_ref() == Some(&action.id);
            button.render_borderless_frame(frame, action_layout.area, &theme);
        }
        layout
    }

    pub fn hit(&self, layout: &PageActionBarLayout, x: u16, y: u16) -> Option<PageActionHit> {
        layout
            .actions
            .iter()
            .find(|action| action.area.width > 0 && contains_point(action.area, x, y))
            .map(|action| PageActionHit {
                id: action.id.clone(),
                enabled: action.enabled,
            })
    }

    fn action_rows(&self, width: u16) -> Vec<Vec<usize>> {
        if self.actions.is_empty() {
            return Vec::new();
        }
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut used = 0u16;
        for index in 0..self.actions.len() {
            let action_width = self.action_width(index).min(width.max(1));
            if !row.is_empty() && used.saturating_add(1).saturating_add(action_width) > width {
                rows.push(row);
                row = Vec::new();
                used = 0;
            }
            if row.is_empty() {
                used = action_width;
            } else {
                used = used.saturating_add(1).saturating_add(action_width);
            }
            row.push(index);
        }
        if !row.is_empty() {
            rows.push(row);
        }
        debug_assert!(
            rows.len() <= 3 || width < 48,
            "PageActionBar accepts at most 5 actions fitting 3 rows at the minimum width"
        );
        rows.truncate(3);
        rows
    }

    fn action_label(&self, index: usize, width: u16) -> Vec<String> {
        let action = &self.actions[index];
        let label = action.label.trim();
        let text = if action.bracketed_label && !(label.starts_with('[') && label.ends_with(']')) {
            format!("[{label}]")
        } else {
            action.label.clone()
        };
        crate::management_wrapped_lines(&text, width)
    }

    fn row_height(&self, row: &[usize], width: u16) -> u16 {
        row.iter()
            .map(|index| self.action_label(*index, width).len() as u16)
            .max()
            .unwrap_or(0)
    }

    fn action_width(&self, index: usize) -> u16 {
        let button = Button::new("measure", self.actions[index].label.as_str())
            .with_bracketed_label(self.actions[index].bracketed_label);
        u16::try_from(button.rendered_label_width()).unwrap_or(u16::MAX)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageActionBarLayout {
    pub area: Rect,
    pub status: Vec<Rect>,
    pub actions: Vec<PageActionLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageActionLayout {
    pub index: usize,
    pub id: ComponentId,
    pub area: Rect,
    pub enabled: bool,
    pub emphasis: ButtonEmphasis,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageActionHit {
    pub id: ComponentId,
    pub enabled: bool,
}

#[cfg(test)]
#[path = "../../tests/unit/components/page_action_bar/tests.rs"]
mod tests;
