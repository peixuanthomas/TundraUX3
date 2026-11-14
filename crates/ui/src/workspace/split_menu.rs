use ratatui::{Frame, layout::Rect, style::Style, widgets::Clear};

use crate::components::{Button, ComponentState, terminal_width};
use crate::screens::shell::fit_cell;
use crate::{RenderContext, automation};

use super::{WorkspaceAction, WorkspaceSplitMode, WorkspaceTaskbarItem, panel::one_line};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceSplitMenuViewModel {
    pub mode: WorkspaceSplitMode,
    pub windows: Vec<WorkspaceTaskbarItem>,
    pub focused: Option<WorkspaceAction>,
    pub hovered: Option<WorkspaceAction>,
    pub pressed: Option<WorkspaceAction>,
    pub scroll: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSplitMenu {
    pub area: Rect,
    pub list: Rect,
    pub hits: Vec<(WorkspaceAction, Rect)>,
    pub scroll: usize,
    pub max_scroll: usize,
    items: Vec<SplitMenuItem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SplitMenuItem {
    action: WorkspaceAction,
    area: Rect,
    label: String,
    name: String,
    enabled: bool,
}

impl WorkspaceSplitMenu {
    pub fn new(available: Rect, anchor: Rect, model: &WorkspaceSplitMenuViewModel) -> Self {
        let fixed = [
            (
                WorkspaceAction::SetMode(WorkspaceSplitMode::LeftRight),
                i18n::tr!("ui-workspace-left-right"),
            ),
            (
                WorkspaceAction::SetMode(WorkspaceSplitMode::TopBottom),
                i18n::tr!("ui-workspace-top-bottom"),
            ),
            (
                WorkspaceAction::SetMode(WorkspaceSplitMode::Grid),
                i18n::tr!("ui-workspace-grid"),
            ),
            (WorkspaceAction::Add, i18n::tr!("ui-workspace-add")),
            (WorkspaceAction::Dismiss, i18n::tr!("ui-workspace-dismiss")),
        ];
        let desired_width = fixed
            .iter()
            .map(|(_, label)| terminal_width(label))
            .chain(
                model
                    .windows
                    .iter()
                    .map(|window| terminal_width(&window.title) + 2),
            )
            .max()
            .unwrap_or_default();
        let width = (desired_width + 4).clamp(18, 38) as u16;
        let width = width.min(available.width);
        let fixed_rows = fixed.len() as u16;
        let requested = fixed_rows + model.windows.len().min(6) as u16;
        let height = requested.min(available.height);
        let below = anchor.bottom().min(available.bottom()).max(available.y);
        let y = if height <= available.bottom() - below {
            below
        } else {
            anchor.y.saturating_sub(height).max(available.y)
        };
        let y = y.min(available.bottom() - height);
        let x = anchor.x.min(available.right() - width).max(available.x);
        let area = Rect::new(x, y, width, height);
        let list = Rect::new(
            x,
            y + fixed_rows.min(height),
            width,
            height.saturating_sub(fixed_rows),
        );
        let max_scroll = model.windows.len().saturating_sub(usize::from(list.height));
        let scroll = model.scroll.min(max_scroll);
        let arrows = max_scroll > 0 && list.height > 0 && list.width >= 5;
        let list_width = list.width - if arrows { 3 } else { 0 };
        let mut items = Vec::new();
        if width > 0 {
            for (index, (action, label)) in fixed.into_iter().take(usize::from(height)).enumerate()
            {
                items.push(SplitMenuItem {
                    action,
                    area: Rect::new(x, y + index as u16, width, 1),
                    name: label.clone(),
                    label,
                    enabled: true,
                });
            }
            for (index, window) in model
                .windows
                .iter()
                .skip(scroll)
                .take(usize::from(list.height))
                .enumerate()
            {
                let title = one_line(&window.title);
                let marker = window.marker();
                items.push(SplitMenuItem {
                    action: WorkspaceAction::Show(window.id),
                    area: Rect::new(x, list.y + index as u16, list_width, 1),
                    name: format!("{} · {}", i18n::tr!("ui-workspace-show"), window.label()),
                    label: format!("{marker}{title}"),
                    enabled: !window.closing,
                });
            }
        }
        if arrows {
            let scroll_x = list.right() - 3;
            let (up, down) = if list.height == 1 {
                (
                    Rect::new(scroll_x, list.y, 1, 1),
                    Rect::new(scroll_x + 1, list.y, 2, 1),
                )
            } else {
                (
                    Rect::new(scroll_x, list.y, 3, 1),
                    Rect::new(scroll_x, list.bottom() - 1, 3, 1),
                )
            };
            items.extend([
                SplitMenuItem {
                    action: WorkspaceAction::ScrollUp,
                    area: up,
                    name: i18n::tr!("ui-workspace-scroll-up"),
                    label: "↑".into(),
                    enabled: scroll > 0,
                },
                SplitMenuItem {
                    action: WorkspaceAction::ScrollDown,
                    area: down,
                    name: i18n::tr!("ui-workspace-scroll-down"),
                    label: "↓".into(),
                    enabled: scroll < max_scroll,
                },
            ]);
        }
        let hits = items
            .iter()
            .filter(|item| item.enabled)
            .map(|item| (item.action, item.area))
            .collect();
        Self {
            area,
            list,
            hits,
            scroll,
            max_scroll,
            items,
        }
    }

    pub fn hit_at(&self, point: (u16, u16)) -> Option<WorkspaceAction> {
        self.hits
            .iter()
            .find(|(_, area)| area.contains(point.into()))
            .map(|(action, _)| *action)
    }
}

pub fn render_workspace_split_menu(
    frame: &mut Frame<'_>,
    menu: &WorkspaceSplitMenu,
    model: &WorkspaceSplitMenuViewModel,
    context: &RenderContext,
) {
    let title = i18n::tr!("ui-workspace-split");
    let _scope = automation::scope("split-menu", "menu", &title, menu.area);
    frame.render_widget(Clear, menu.area);
    frame
        .buffer_mut()
        .set_style(menu.area, Style::default().bg(context.theme.raised));
    let mut theme = context.compatibility_theme();
    theme.buttons = None;
    for item in &menu.items {
        let label = fit_cell(
            &item.label,
            usize::from(item.area.width.saturating_sub(2).max(1)),
        );
        let mut button = Button::new(item.action.id(), label.trim_end())
            .with_centered_label(true)
            .with_capture_name(&item.name)
            .with_background(context.theme.raised)
            .with_current(item.action == WorkspaceAction::SetMode(model.mode));
        button.state = ComponentState::default()
            .disabled(!item.enabled)
            .focused(model.focused == Some(item.action))
            .hovered(model.hovered == Some(item.action))
            .active(model.pressed == Some(item.action));
        button.render_borderless_frame(frame, item.area, &theme);
    }
}
