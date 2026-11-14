use ratatui::{Frame, layout::Rect, style::Style};
use workspace_protocol::SessionId;

use crate::components::{Button, ComponentState, terminal_width};
use crate::screens::shell::fit_cell;
use crate::{RenderContext, automation};

use super::panel::one_line;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceTaskbarItem {
    pub id: SessionId,
    pub title: String,
    pub focused: bool,
    pub minimized: bool,
    pub closing: bool,
    pub unresponsive: bool,
}

impl WorkspaceTaskbarItem {
    fn status_marker(&self) -> &'static str {
        if self.unresponsive {
            "! "
        } else if self.closing {
            "… "
        } else {
            ""
        }
    }

    pub(super) fn marker(&self) -> &'static str {
        if self.minimized && !self.unresponsive && !self.closing {
            "− "
        } else {
            self.status_marker()
        }
    }

    pub(super) fn label(&self) -> String {
        let title = one_line(&self.title);
        let status = if self.unresponsive {
            i18n::tr!("ui-workspace-unresponsive")
        } else if self.closing {
            i18n::tr!("ui-workspace-closing")
        } else if self.minimized {
            i18n::tr!("ui-workspace-backgrounded")
        } else {
            return title;
        };
        format!("{title} · {status}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTaskbarAction {
    Activate(SessionId),
    Close(SessionId),
    ScrollLeft,
    ScrollRight,
}

impl WorkspaceTaskbarAction {
    pub fn id(self) -> String {
        match self {
            Self::Activate(id) => format!("workspace.taskbar.{id}"),
            Self::Close(id) => format!("workspace.taskbar.{id}.close"),
            Self::ScrollLeft => "workspace.taskbar.previous".into(),
            Self::ScrollRight => "workspace.taskbar.next".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceTaskbar {
    pub area: Rect,
    pub scroll: usize,
    pub max_scroll: usize,
    pub hits: Vec<(WorkspaceTaskbarAction, Rect)>,
    arrow_width: u16,
}

impl WorkspaceTaskbar {
    pub fn new(area: Rect, items: &[WorkspaceTaskbarItem], scroll: usize) -> Self {
        let widths = items
            .iter()
            .map(|item| {
                let marker_width = terminal_width(item.status_marker());
                (terminal_width(&one_line(&item.title)) + marker_width + 4).clamp(8, 28) as u16
            })
            .collect::<Vec<_>>();
        let total = widths
            .iter()
            .map(|width| usize::from(*width) + 1)
            .sum::<usize>()
            .saturating_sub(1);
        let overflow = total > usize::from(area.width);
        let arrow_width = if overflow && items.len() > 1 {
            3.min(area.width.saturating_sub(1) / 2)
        } else {
            0
        };
        let item_width = area.width - arrow_width * 2;
        let max_scroll = if overflow {
            let mut remaining = item_width;
            let mut last = widths.len().saturating_sub(1);
            remaining = remaining.saturating_sub(widths.last().copied().unwrap_or(0));
            while last > 0 && widths[last - 1] < remaining {
                last -= 1;
                remaining -= widths[last] + 1;
            }
            last
        } else {
            0
        };
        let scroll = scroll.min(max_scroll);
        let mut hits = Vec::new();
        if !area.is_empty() {
            if arrow_width > 0 && scroll > 0 {
                hits.push((
                    WorkspaceTaskbarAction::ScrollLeft,
                    Rect::new(area.x, area.y, arrow_width, 1),
                ));
            }
            let mut x = area.x + arrow_width;
            let right = area.right() - arrow_width;
            for (item, width) in items.iter().zip(widths).skip(scroll) {
                let remaining = right.saturating_sub(x);
                let width = if x == area.x + arrow_width {
                    width.min(remaining)
                } else {
                    width
                };
                if width == 0 || width > remaining {
                    break;
                }
                let close_width = 2.min(width - 1);
                if close_width > 0 {
                    hits.push((
                        WorkspaceTaskbarAction::Close(item.id),
                        Rect::new(x, area.y, close_width, 1),
                    ));
                }
                hits.push((
                    WorkspaceTaskbarAction::Activate(item.id),
                    Rect::new(x + close_width, area.y, width - close_width, 1),
                ));
                x = x.saturating_add(width + 1);
            }
            if arrow_width > 0 && scroll < max_scroll {
                hits.push((
                    WorkspaceTaskbarAction::ScrollRight,
                    Rect::new(right, area.y, arrow_width, 1),
                ));
            }
        }
        Self {
            area,
            scroll,
            max_scroll,
            hits,
            arrow_width,
        }
    }

    pub fn hit_at(&self, point: (u16, u16)) -> Option<WorkspaceTaskbarAction> {
        self.hits
            .iter()
            .find(|(_, area)| area.contains(point.into()))
            .map(|(action, _)| *action)
    }
}

pub fn render_workspace_taskbar(
    frame: &mut Frame<'_>,
    taskbar: &WorkspaceTaskbar,
    items: &[WorkspaceTaskbarItem],
    hovered: Option<WorkspaceTaskbarAction>,
    pressed: Option<WorkspaceTaskbarAction>,
    context: &RenderContext,
) {
    use WorkspaceTaskbarAction::*;
    if taskbar.area.is_empty() || items.is_empty() {
        return;
    }
    let title = i18n::tr!("ui-workspace-taskbar");
    let _scope = automation::scope("taskbar", "navigation", &title, taskbar.area);
    frame
        .buffer_mut()
        .set_style(taskbar.area, Style::default().bg(context.theme.surface));
    let mut theme = context.compatibility_theme();
    theme.buttons = None;
    for &(action, area) in &taskbar.hits {
        let id = match action {
            Activate(id) | Close(id) => id,
            _ => continue,
        };
        let Some(item) = items.iter().find(|item| item.id == id) else {
            continue;
        };
        let mut button = if matches!(action, Close(_)) {
            Button::new(action.id(), if area.width > 1 { " −" } else { "−" })
                .with_capture_name(i18n::tr!(
                    "ui-workspace-close-window",
                    window = one_line(&item.title)
                ))
                .with_emphasis(crate::components::ButtonEmphasis::Danger)
        } else {
            let title = format!("{}{}", item.status_marker(), one_line(&item.title));
            let label = if area.width <= 2 {
                fit_cell(&title, usize::from(area.width))
            } else {
                format!(" {}", fit_cell(&title, usize::from(area.width - 2)))
            };
            Button::new(action.id(), label).with_capture_name(item.label())
        }
        .with_label_alignment(ratatui::layout::HorizontalAlignment::Left)
        .with_background(context.theme.raised)
        .with_current(item.focused);
        button.set_disabled(matches!(action, Close(_)) && item.closing);
        button.state.hovered = hovered == Some(action);
        button.state.active = pressed == Some(action);
        button.render_borderless_frame(frame, area, &theme);
    }
    if taskbar.arrow_width > 0 {
        for (action, label, name, x, enabled) in [
            (
                ScrollLeft,
                "‹",
                i18n::tr!("ui-workspace-scroll-left"),
                taskbar.area.x,
                taskbar.scroll > 0,
            ),
            (
                ScrollRight,
                "›",
                i18n::tr!("ui-workspace-scroll-right"),
                taskbar.area.right() - taskbar.arrow_width,
                taskbar.scroll < taskbar.max_scroll,
            ),
        ] {
            let mut button = Button::new(action.id(), label)
                .with_centered_label(true)
                .with_capture_name(name);
            button.state = ComponentState::default()
                .disabled(!enabled)
                .hovered(hovered == Some(action))
                .active(pressed == Some(action));
            button.render_borderless_frame(
                frame,
                Rect::new(x, taskbar.area.y, taskbar.arrow_width, 1),
                &theme,
            );
        }
    }
}
