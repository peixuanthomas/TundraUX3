use super::{layout::*, model::*};
use crate::components::{
    Button, ComponentState, DataTable, Dialog, DialogAction, EmptyState, List, ListItem,
    MetricCard, Scrollbar, Surface,
};
use crate::screens::diagnostics::{
    render_diagnostics_content, render_diagnostics_footer, render_diagnostics_header,
    render_diagnostics_repair_dialog,
};
use crate::screens::shell::{fit_cell, render_compact_home, render_status, render_top};
use crate::{RenderContext, ShellChromeViewModel, ShellLayout, TundraTheme, compute_shell_layout};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph};

pub fn render_system_status(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SystemStatusViewModel,
    theme: &TundraTheme,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_system_status_contextual(frame, area, chrome, model, &context)
}
pub fn render_system_status_contextual(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SystemStatusViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    match compute_shell_layout(area) {
        ShellLayout::Compact(c) => render_compact_home(frame, c, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_main(frame, main, model, context);
            render_status(frame, status, chrome, theme)
        }
    }
}
fn render_main(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &SystemStatusViewModel,
    context: &RenderContext,
) {
    let l = system_status_layout(main, model);
    Surface::new()
        .titled("System Status")
        .bordered(true)
        .render_frame(frame, l.panel, context);
    match model.route {
        SystemStatusRoute::Dashboard => render_dashboard(frame, &l, model, context),
        SystemStatusRoute::Detail(d) => render_detail(frame, &l, model, d, context),
    }
}
fn render_dashboard(
    frame: &mut Frame<'_>,
    l: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    let updated = if model.dashboard.updated.is_empty() {
        String::new()
    } else {
        format!("Updated {}", model.dashboard.updated)
    };
    let width = usize::from(l.header.width);
    let left = "Dashboard";
    let gap = width.saturating_sub(left.len() + updated.len());
    frame.render_widget(
        Paragraph::new(format!("{left}{}{updated}", " ".repeat(gap))).style(theme.title_style()),
        l.header,
    );
    Surface::new()
        .bordered(true)
        .render_frame(frame, l.content_panel, context);
    if l.empty_canvas {
        EmptyState::new("Dashboard needs more room")
            .detail("Increase the terminal height to show metric cards.")
            .render_frame(frame, l.canvas, context)
    } else {
        let widgets = model.dashboard.widgets(l.profile);
        for g in &l.widgets {
            if let Some(vm) = widgets.iter().find(|w| w.kind == g.kind) {
                let mut card = MetricCard::new(vm);
                card.state = ComponentState::default()
                    .selected(model.dashboard.selected == Some(g.kind))
                    .focused(model.dashboard.selected == Some(g.kind));
                card.render_frame(frame, g.area, context)
            }
        }
        if let Some(a) = l.scrollbar {
            let max = model
                .dashboard
                .widgets(l.profile)
                .iter()
                .map(|w| w.row.saturating_add(w.size.rows()))
                .max()
                .unwrap_or(0);
            Scrollbar::new(
                max as usize,
                l.visible_row_end.saturating_sub(l.visible_row_start) as usize,
                l.visible_row_start as usize,
            )
            .render_frame(frame, a, context)
        }
    }
    let hint = model
        .dashboard
        .feedback
        .as_deref()
        .unwrap_or(if model.dashboard.editing {
            "Arrows Move · Enter Select · Esc Cancel"
        } else {
            "E Edit · Enter Details · R Refresh · Esc Home"
        });
    let action_left = if model.dashboard.editing {
        l.add_button.x
    } else {
        l.edit_button.x
    };
    let help = Rect::new(
        l.footer.x,
        l.footer.y,
        action_left.saturating_sub(l.footer.x),
        1,
    );
    frame.render_widget(
        Paragraph::new(fit_cell(hint, usize::from(help.width))).style(theme.muted_style()),
        help,
    );
    if model.dashboard.editing {
        button(
            frame,
            l.add_button,
            "Add",
            model.dashboard.actions.add_disabled,
            theme,
        );
        button(
            frame,
            l.size_button,
            "Size",
            model.dashboard.actions.size_disabled,
            theme,
        );
        button(
            frame,
            l.remove_button,
            "Remove",
            model.dashboard.actions.remove_disabled,
            theme,
        );
        button(
            frame,
            l.save_button,
            "Save",
            model.dashboard.actions.save_disabled,
            theme,
        );
        button(
            frame,
            l.cancel_button,
            "Cancel",
            model.dashboard.actions.cancel_disabled,
            theme,
        )
    } else {
        button(
            frame,
            l.edit_button,
            "Edit",
            model.dashboard.actions.edit_disabled,
            theme,
        );
        button(
            frame,
            l.refresh_button,
            if model.refreshing {
                "Refreshing"
            } else {
                "Refresh"
            },
            model.dashboard.actions.refresh_disabled || model.refreshing,
            theme,
        )
    }
    render_overlays(frame, l, model, context)
}
fn button(frame: &mut Frame<'_>, area: Rect, label: &str, disabled: bool, theme: &TundraTheme) {
    let mut b = Button::new(
        format!("system-status.{}", label.to_ascii_lowercase()),
        label,
    );
    b.set_disabled(disabled);
    b.render_borderless_frame(frame, area, theme)
}
fn render_overlays(
    frame: &mut Frame<'_>,
    l: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    context: &RenderContext,
) {
    if let Some(p) = &model.dashboard.picker {
        let w = l.panel.width.min(42);
        let h = l
            .panel
            .height
            .min((p.items.len() as u16).saturating_add(2).max(5));
        let area = Rect::new(
            l.panel.x + (l.panel.width - w) / 2,
            l.panel.y + (l.panel.height - h) / 2,
            w,
            h,
        );
        frame.render_widget(Clear, area);
        let items = p
            .items
            .iter()
            .map(|i| {
                ListItem::new(format!("system-status.add.{}", i.kind.label()), &i.label)
                    .with_description(&i.detail)
                    .disabled(!i.enabled)
            })
            .collect();
        let mut list = List::new("system-status.add-list", items).titled(&p.title);
        list.set_selected(Some(p.selected));
        list.set_focused(true);
        list.render_frame(frame, area, &context.compatibility_theme())
    }
    if let Some(d) = &model.dashboard.dialog {
        let w = l.panel.width.min(48);
        let h = l.panel.height.min(8);
        let area = Rect::new(
            l.panel.x + (l.panel.width - w) / 2,
            l.panel.y + (l.panel.height - h) / 2,
            w,
            h,
        );
        let mut dialog = Dialog::new(
            "system-status.dialog",
            &d.title,
            &d.message,
            vec![
                DialogAction::new(
                    "confirm",
                    if d.confirm_label.is_empty() {
                        "Confirm"
                    } else {
                        &d.confirm_label
                    },
                ),
                DialogAction::new(
                    "cancel",
                    if d.cancel_label.is_empty() {
                        "Cancel"
                    } else {
                        &d.cancel_label
                    },
                ),
            ],
        );
        dialog.open();
        dialog.render_frame(frame, area, &context.compatibility_theme())
    }
}
fn render_detail(
    frame: &mut Frame<'_>,
    l: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    d: SystemStatusDetail,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    frame.render_widget(
        Paragraph::new(format!("{} · Esc Dashboard", d.label())).style(theme.title_style()),
        l.header,
    );
    Surface::new()
        .titled(d.label())
        .bordered(true)
        .render_frame(frame, l.content_panel, context);
    match d {
        SystemStatusDetail::Storage => render_storage(frame, l, model, context),
        SystemStatusDetail::Network => render_network(frame, l, model, context),
        SystemStatusDetail::Diagnostics | SystemStatusDetail::Activity => {
            render_diagnostics_header(frame, l.header, &model.diagnostics, theme);
            if let Some(dl) = &l.diagnostics_content {
                render_diagnostics_content(frame, dl, &model.diagnostics, theme, context)
            }
            render_diagnostics_footer(frame, l.footer, &model.diagnostics, theme, "Esc Dashboard");
            if let (Some(dl), Some(dialog)) = (
                l.diagnostics_repair_dialog.as_ref(),
                model.diagnostics.repair_dialog.as_ref(),
            ) {
                render_diagnostics_repair_dialog(frame, dl, dialog, theme, context)
            }
        }
        _ => {
            if let Some(vm) = model.detail_widget(d) {
                render_formatted_detail(frame, l.canvas, vm, context)
            } else {
                EmptyState::new("No data")
                    .detail("This metric is not available.")
                    .render_frame(frame, l.canvas, context)
            }
        }
    }
    if !matches!(
        d,
        SystemStatusDetail::Diagnostics | SystemStatusDetail::Activity
    ) {
        frame.render_widget(
            Paragraph::new("Esc Dashboard · R Refresh").style(theme.muted_style()),
            l.footer,
        );
        button(frame, l.refresh_button, "Refresh", model.refreshing, theme)
    }
}
fn render_formatted_detail(
    frame: &mut Frame<'_>,
    area: Rect,
    vm: &SystemStatusWidgetViewModel,
    context: &RenderContext,
) {
    let [summary, table] =
        Layout::vertical([Constraint::Length(3.min(area.height)), Constraint::Min(0)]).areas(area);
    frame.render_widget(
        Paragraph::new(
            std::iter::once(Line::raw(vm.primary.as_str()))
                .chain(vm.secondary.iter().map(|s| Line::raw(s.as_str())))
                .collect::<Vec<_>>(),
        ),
        summary,
    );
    if !vm.compact_rows.is_empty() {
        let cols = vm.compact_rows.iter().map(Vec::len).max().unwrap_or(1);
        DataTable::new(
            "system-status.detail",
            (0..cols).map(|i| format!("Field {}", i + 1)),
            vm.compact_rows.clone(),
        )
        .bordered(false)
        .render_frame(frame, table, context)
    }
}
fn render_storage(
    frame: &mut Frame<'_>,
    l: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    context: &RenderContext,
) {
    let SystemStatusContentViewModel::Admin(a) = &model.content else {
        EmptyState::new("Unavailable").render_frame(frame, l.rows_area, context);
        return;
    };
    if state_placeholder(
        frame,
        l.rows_area,
        &a.storage_state,
        a.storage_rows.is_empty(),
        "No storage volumes",
        context,
    ) {
        return;
    }
    let mut t = DataTable::new(
        "system-status.storage",
        [
            "Volume", "Kind", "System", "Access", "Usage", "Used", "Pressure",
        ],
        a.storage_rows.iter().map(|r| {
            vec![
                r.volume.clone(),
                r.kind.clone(),
                r.system_volume.clone(),
                r.access.clone(),
                r.usage.clone(),
                r.used_percentage.clone(),
                r.pressure.clone(),
            ]
        }),
    )
    .bordered(false)
    .with_viewport_start(l.visible_start)
    .with_row_tones(a.storage_rows.iter().map(|r| r.tone).collect());
    t.selected = model.selected_index();
    t.state.focused = true;
    t.render_frame(frame, l.rows_area, context);
    detail_scroll(frame, l, model, context)
}
fn render_network(
    frame: &mut Frame<'_>,
    l: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    context: &RenderContext,
) {
    let SystemStatusContentViewModel::Admin(a) = &model.content else {
        EmptyState::new("Unavailable").render_frame(frame, l.rows_area, context);
        return;
    };
    if state_placeholder(
        frame,
        l.rows_area,
        &a.network_state,
        a.network_rows.is_empty(),
        "No network interfaces",
        context,
    ) {
        return;
    }
    let mut t = DataTable::new(
        "system-status.network",
        ["Name", "Display name", "Kind", "Link", "Addresses"],
        a.network_rows.iter().map(|r| {
            vec![
                r.name.clone(),
                r.display_name.clone(),
                r.kind.clone(),
                r.link_state.clone(),
                r.addresses.clone(),
            ]
        }),
    )
    .bordered(false)
    .with_viewport_start(l.visible_start)
    .with_row_tones(a.network_rows.iter().map(|r| r.tone).collect());
    t.selected = model.selected_index();
    t.state.focused = true;
    t.render_frame(frame, l.rows_area, context);
    detail_scroll(frame, l, model, context)
}
fn state_placeholder(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &SystemStatusSectionState,
    empty: bool,
    title: &str,
    context: &RenderContext,
) -> bool {
    let e = match state {
        SystemStatusSectionState::Loading => Some(EmptyState::new("Loading...")),
        SystemStatusSectionState::Unavailable { message } => {
            Some(EmptyState::new("Unavailable").detail(message))
        }
        SystemStatusSectionState::Stale { message } if empty => {
            Some(EmptyState::new("Stale data").detail(message))
        }
        SystemStatusSectionState::Ready if empty => Some(EmptyState::new(title)),
        _ => None,
    };
    if let Some(e) = e {
        e.render_frame(frame, area, context);
        true
    } else {
        false
    }
}
fn detail_scroll(
    frame: &mut Frame<'_>,
    l: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    context: &RenderContext,
) {
    if model.item_count() > l.visible_capacity {
        let a = Rect::new(
            l.rows_area.right().saturating_sub(1),
            l.rows_area.y,
            1,
            l.rows_area.height,
        );
        Scrollbar::new(model.item_count(), l.visible_capacity, l.visible_start)
            .render_frame(frame, a, context)
    }
}
