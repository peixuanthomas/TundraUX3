use super::{layout::*, model::*};
use crate::components::{
    Button, ComponentState, DataTable, Dialog, DialogAction, EmptyState, List, ListItem,
    MetricCard, Scrollbar, Surface, tone_color,
};
use crate::screens::diagnostics::{
    render_diagnostics_content, render_diagnostics_footer, render_diagnostics_repair_dialog,
};
use crate::screens::shell::{fit_cell, render_compact_home, render_status, render_top};
use crate::{RenderContext, ShellChromeViewModel, ShellLayout, TundraTheme, compute_shell_layout};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Sparkline};

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
        .titled(i18n::tr!("ui-system-status-system-status"))
        .bordered(false)
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
        i18n::tr!(
            "ui-system-status-updated",
            time = model.dashboard.updated.clone()
        )
    };
    let width = usize::from(l.header.width);
    let left = if model.dashboard.editing {
        i18n::tr!("ui-system-status-edit-mode-save-to-keep-changes-esc-cancel")
    } else {
        i18n::tr!("ui-system-status-dashboard")
    };
    let gap = width.saturating_sub(left.len() + updated.len());
    frame.render_widget(
        Paragraph::new(format!("{left}{}{updated}", " ".repeat(gap))).style(theme.title_style()),
        l.header,
    );
    Surface::new().render_frame(frame, l.content_panel, context);
    if l.empty_canvas {
        EmptyState::new(i18n::tr!("ui-system-status-dashboard-needs-more-room"))
            .detail(i18n::tr!(
                "ui-system-status-increase-the-terminal-height-to-show-metric-cards"
            ))
            .render_frame(frame, l.canvas, context)
    } else {
        let widgets = model.dashboard.widgets(l.profile);
        for g in &l.widgets {
            if let Some(vm) = widgets.iter().find(|w| w.kind == g.kind) {
                let mut card = MetricCard::new(vm);
                card.editing = model.dashboard.editing;
                card.state = ComponentState::default()
                    .selected(model.dashboard.selected == Some(g.kind))
                    .focused(model.dashboard.focus == SystemStatusDashboardFocus::Widget(g.kind));
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
    let hint = model.dashboard.feedback.clone().unwrap_or_else(|| {
        if model.dashboard.editing {
            i18n::tr!("ui-system-status-arrows-move-enter-select-esc-cancel")
        } else {
            i18n::tr!("ui-system-status-h-diagnostics-e-edit-esc-home")
        }
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
        Paragraph::new(fit_cell(&hint, usize::from(help.width))).style(theme.muted_style()),
        help,
    );
    if model.dashboard.editing {
        button(
            frame,
            l.add_button,
            "system-status.add",
            &i18n::tr!("ui-system-status-add"),
            model.dashboard.actions.add_disabled,
            model.dashboard.focus == SystemStatusDashboardFocus::Add,
            theme,
        );
        button(
            frame,
            l.size_button,
            "system-status.size",
            &i18n::tr!("ui-system-status-size"),
            model.dashboard.actions.size_disabled,
            model.dashboard.focus == SystemStatusDashboardFocus::Size,
            theme,
        );
        button(
            frame,
            l.remove_button,
            "system-status.remove",
            &i18n::tr!("ui-system-status-remove"),
            model.dashboard.actions.remove_disabled,
            model.dashboard.focus == SystemStatusDashboardFocus::Remove,
            theme,
        );
        button(
            frame,
            l.save_button,
            "system-status.save",
            &i18n::tr!("ui-system-status-save"),
            model.dashboard.actions.save_disabled,
            model.dashboard.focus == SystemStatusDashboardFocus::Save,
            theme,
        );
        button(
            frame,
            l.cancel_button,
            "system-status.cancel",
            &i18n::tr!("ui-system-status-cancel"),
            model.dashboard.actions.cancel_disabled,
            model.dashboard.focus == SystemStatusDashboardFocus::Cancel,
            theme,
        )
    } else {
        button(
            frame,
            l.edit_button,
            "system-status.edit",
            &i18n::tr!("ui-system-status-edit"),
            model.dashboard.actions.edit_disabled,
            model.dashboard.focus == SystemStatusDashboardFocus::Edit,
            theme,
        );
        button(
            frame,
            l.refresh_button,
            "system-status.refresh",
            if model.refreshing {
                i18n::tr!("ui-system-status-refreshing")
            } else {
                i18n::tr!("ui-system-status-refresh")
            },
            model.dashboard.actions.refresh_disabled || model.refreshing,
            model.dashboard.focus == SystemStatusDashboardFocus::Refresh,
            theme,
        )
    }
    render_overlays(frame, l, model, context)
}
fn button(
    frame: &mut Frame<'_>,
    area: Rect,
    id: &str,
    label: impl Into<String>,
    disabled: bool,
    focused: bool,
    theme: &TundraTheme,
) {
    let label = label.into();
    let mut b = Button::new(id, label);
    b.set_disabled(disabled);
    b.set_focused(focused);
    b.render_borderless_frame(frame, area, theme)
}
fn render_overlays(
    frame: &mut Frame<'_>,
    l: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    context: &RenderContext,
) {
    if let Some(p) = model.dashboard.size_picker {
        let w = l.panel.width.min(42);
        let h = l.panel.height.min(5);
        let area = system_status_picker_area(l.panel, w, h, model.dashboard.picker_anchor);
        frame.render_widget(Clear, area);
        let items = ["2x2", "2x4", "4x4"]
            .into_iter()
            .enumerate()
            .map(|(index, label)| ListItem::new(format!("system-status.size.{index}"), label))
            .collect();
        let mut list = List::new("system-status.size-list", items)
            .titled(i18n::tr!("ui-system-status-widget-size"));
        list.set_selected(Some(p.selected));
        list.set_focused(true);
        list.render_frame(frame, area, &context.compatibility_theme());
        return;
    }
    if let Some(p) = &model.dashboard.picker {
        let w = l.panel.width.min(42);
        let h = l
            .panel
            .height
            .min((p.items.len() as u16).saturating_add(2).max(5));
        let area = system_status_picker_area(l.panel, w, h, model.dashboard.picker_anchor);
        frame.render_widget(Clear, area);
        let items = p
            .items
            .iter()
            .map(|i| {
                ListItem::new(format!("system-status.add.{:?}", i.kind), &i.label)
                    .with_description(&i.detail)
                    .disabled(!i.enabled)
            })
            .collect();
        let mut list = List::new("system-status.add-list", items)
            .with_viewport_start(l.picker_viewport_start)
            .titled(&p.title);
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
                        i18n::tr!("ui-system-status-confirm")
                    } else {
                        d.confirm_label.clone()
                    },
                ),
                DialogAction::new(
                    "cancel",
                    if d.cancel_label.is_empty() {
                        i18n::tr!("ui-system-status-cancel")
                    } else {
                        d.cancel_label.clone()
                    },
                ),
            ],
        );
        dialog.open();
        dialog.set_selected_action(Some(d.selected_action));
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
        Paragraph::new(i18n::tr!(
            "ui-system-status-detail-title",
            title = d.label()
        ))
        .style(theme.title_style()),
        l.header,
    );
    Surface::new()
        .titled(d.label())
        .bordered(true)
        .render_frame(frame, l.content_panel, context);
    match d {
        SystemStatusDetail::Overview => super::overview::render_overview(frame, l, model, context),
        SystemStatusDetail::Storage => render_storage(frame, l, model, context),
        SystemStatusDetail::Network => render_network(frame, l, model, context),
        SystemStatusDetail::Diagnostics
        | SystemStatusDetail::Logs
        | SystemStatusDetail::Incidents => {
            let mut diagnostics = model.diagnostics.clone();
            diagnostics.tab = d.diagnostics_tab().expect("diagnostics module route");
            if let Some(dl) = &l.diagnostics_content {
                render_diagnostics_content(frame, dl, &diagnostics, theme, context)
            }
            render_diagnostics_footer(
                frame,
                l.footer,
                &diagnostics,
                theme,
                &i18n::tr!("ui-system-status-esc-dashboard"),
            );
            if let (Some(dl), Some(dialog)) = (
                l.diagnostics_repair_dialog.as_ref(),
                model.diagnostics.repair_dialog.as_ref(),
            ) {
                render_diagnostics_repair_dialog(frame, dl, dialog, theme, context)
            }
        }
        _ => {
            if let Some(vm) = model.detail_widget(d) {
                render_formatted_detail(frame, l, model, vm, context)
            } else {
                EmptyState::new(i18n::tr!("ui-system-status-no-data"))
                    .detail(i18n::tr!("ui-system-status-this-metric-is-not-available"))
                    .render_frame(frame, l.canvas, context)
            }
        }
    }
    if !matches!(
        d,
        SystemStatusDetail::Diagnostics | SystemStatusDetail::Logs | SystemStatusDetail::Incidents
    ) {
        frame.render_widget(
            Paragraph::new(i18n::tr!("ui-system-status-esc-dashboard-r-refresh"))
                .style(theme.muted_style()),
            l.footer,
        );
        button(
            frame,
            l.refresh_button,
            "system-status.refresh",
            i18n::tr!("ui-system-status-refresh"),
            model.refreshing,
            false,
            theme,
        )
    }
}
fn render_formatted_detail(
    frame: &mut Frame<'_>,
    layout: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    vm: &SystemStatusWidgetViewModel,
    context: &RenderContext,
) {
    match &vm.state {
        SystemStatusWidgetState::Loading => {
            EmptyState::new(i18n::tr!("ui-system-status-loading")).render_frame(
                frame,
                layout.canvas,
                context,
            );
            return;
        }
        SystemStatusWidgetState::Unavailable { message } => {
            EmptyState::new(i18n::tr!("ui-system-status-unavailable"))
                .detail(message)
                .render_frame(frame, layout.canvas, context);
            return;
        }
        SystemStatusWidgetState::Stale { message } if vm.primary.is_empty() => {
            EmptyState::new(i18n::tr!("ui-system-status-stale-data"))
                .detail(message)
                .render_frame(frame, layout.canvas, context);
            return;
        }
        _ => {}
    }
    let mut summary_lines = std::iter::once(Line::raw(vm.primary.as_str()))
        .chain(vm.secondary.iter().map(|line| Line::raw(line.as_str())))
        .collect::<Vec<_>>();
    if let SystemStatusWidgetState::Stale { message } = &vm.state {
        summary_lines.push(Line::styled(
            i18n::tr!("ui-system-status-stale-message", message = message),
            Style::default().fg(context.theme.warning),
        ));
    }
    frame.render_widget(Paragraph::new(summary_lines), layout.detail_summary_area);
    if let Some(trend) = vm.trend.as_ref().filter(|trend| !trend.is_empty()) {
        frame.render_widget(
            Sparkline::default()
                .data(trend)
                .style(Style::default().fg(tone_color(vm.tone, &context.compatibility_theme()))),
            layout.detail_trend_area,
        );
    }
    if !vm.compact_rows.is_empty() {
        let mut table = DataTable::new(
            "system-status.detail",
            detail_headers(vm.kind),
            vm.compact_rows.clone(),
        )
        .bordered(false)
        .with_viewport_start(layout.visible_start);
        table.selected = model.selected_index();
        table.state.focused = true;
        table.render_frame(frame, layout.rows_area, context);
        detail_scroll(frame, layout, model, context)
    }
}
fn detail_headers(kind: SystemStatusWidgetKind) -> Vec<String> {
    match kind {
        SystemStatusWidgetKind::SystemOverview => vec![
            i18n::tr!("ui-system-status-subsystem"),
            i18n::tr!("ui-system-status-status"),
        ],
        SystemStatusWidgetKind::Cpu => vec![
            i18n::tr!("ui-system-status-core"),
            i18n::tr!("ui-system-status-usage"),
        ],
        SystemStatusWidgetKind::Memory => vec![
            i18n::tr!("ui-system-status-metric"),
            i18n::tr!("ui-system-status-value"),
        ],
        SystemStatusWidgetKind::Storage => vec![
            i18n::tr!("ui-system-status-volume"),
            i18n::tr!("ui-system-status-usage"),
        ],
        SystemStatusWidgetKind::Network => vec![
            i18n::tr!("ui-system-status-interface"),
            i18n::tr!("ui-system-status-down"),
            i18n::tr!("ui-system-status-up"),
        ],
        SystemStatusWidgetKind::Temperature => vec![
            i18n::tr!("ui-system-status-sensor"),
            i18n::tr!("ui-system-status-current"),
            i18n::tr!("ui-system-status-critical"),
        ],
        SystemStatusWidgetKind::Battery => vec![
            i18n::tr!("ui-system-status-battery"),
            i18n::tr!("ui-system-status-charge"),
            i18n::tr!("ui-system-status-state"),
        ],
        SystemStatusWidgetKind::UptimeLoad => vec![
            i18n::tr!("ui-system-status-window"),
            i18n::tr!("ui-system-status-load"),
        ],
        SystemStatusWidgetKind::TopProcesses => {
            vec![
                i18n::tr!("ui-system-status-sort"),
                i18n::tr!("ui-system-status-pid"),
                i18n::tr!("ui-system-status-process"),
                i18n::tr!("ui-system-status-cpu"),
                i18n::tr!("ui-system-status-memory"),
            ]
        }
        SystemStatusWidgetKind::Diagnostics => vec![
            i18n::tr!("ui-system-status-check"),
            i18n::tr!("ui-system-status-status"),
        ],
        SystemStatusWidgetKind::Logs => vec![
            i18n::tr!("ui-system-status-log"),
            i18n::tr!("ui-system-status-size"),
            i18n::tr!("ui-system-status-modified"),
        ],
        SystemStatusWidgetKind::Incidents => vec![
            i18n::tr!("ui-system-status-when"),
            i18n::tr!("ui-system-status-app"),
            i18n::tr!("ui-system-status-summary"),
        ],
    }
}
fn render_storage(
    frame: &mut Frame<'_>,
    l: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    context: &RenderContext,
) {
    let SystemStatusContentViewModel::Admin(a) = &model.content else {
        EmptyState::new(i18n::tr!("ui-system-status-unavailable")).render_frame(
            frame,
            l.rows_area,
            context,
        );
        return;
    };
    if state_placeholder(
        frame,
        l.rows_area,
        &a.storage_state,
        a.storage_rows.is_empty(),
        &i18n::tr!("ui-system-status-no-storage-volumes"),
        context,
    ) {
        return;
    }
    let mut t = DataTable::new(
        "system-status.storage",
        [
            i18n::tr!("ui-system-status-volume"),
            i18n::tr!("ui-system-status-kind"),
            i18n::tr!("ui-system-status-system"),
            i18n::tr!("ui-system-status-access"),
            i18n::tr!("ui-system-status-usage"),
            i18n::tr!("ui-system-status-used"),
            i18n::tr!("ui-system-status-pressure"),
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
        EmptyState::new(i18n::tr!("ui-system-status-unavailable")).render_frame(
            frame,
            l.rows_area,
            context,
        );
        return;
    };
    if state_placeholder(
        frame,
        l.rows_area,
        &a.network_state,
        a.network_rows.is_empty(),
        &i18n::tr!("ui-system-status-no-network-interfaces"),
        context,
    ) {
        return;
    }
    let mut t = DataTable::new(
        "system-status.network",
        [
            i18n::tr!("ui-system-status-name"),
            i18n::tr!("ui-system-status-display-name"),
            i18n::tr!("ui-system-status-kind"),
            i18n::tr!("ui-system-status-link"),
            i18n::tr!("ui-system-status-down"),
            i18n::tr!("ui-system-status-up"),
            i18n::tr!("ui-system-status-addresses"),
        ],
        a.network_rows.iter().map(|r| {
            vec![
                r.name.clone(),
                r.display_name.clone(),
                r.kind.clone(),
                r.link_state.clone(),
                r.received_rate.clone(),
                r.transmitted_rate.clone(),
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
        SystemStatusSectionState::Loading => {
            Some(EmptyState::new(i18n::tr!("ui-system-status-loading")))
        }
        SystemStatusSectionState::Unavailable { message } => {
            Some(EmptyState::new(i18n::tr!("ui-system-status-unavailable")).detail(message))
        }
        SystemStatusSectionState::Stale { message } if empty => {
            Some(EmptyState::new(i18n::tr!("ui-system-status-stale-data")).detail(message))
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
    if let Some(a) = l.scrollbar {
        Scrollbar::new(model.item_count(), l.visible_capacity, l.visible_start)
            .render_frame(frame, a, context)
    }
}
