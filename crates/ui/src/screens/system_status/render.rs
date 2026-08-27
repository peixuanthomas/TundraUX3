use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Gauge, Paragraph};

use super::layout::{SystemStatusLayout, system_status_layout, system_status_tab_label};
use super::model::{
    AdminSystemStatusViewModel, SystemStatusContentViewModel, SystemStatusSectionState,
    SystemStatusTab, SystemStatusViewModel, UserSystemStatusViewModel,
};
use crate::components::{
    Button, DataTable, EmptyState, Scrollbar, Surface, TabItem, Tabs, tone_color,
};
use crate::screens::diagnostics::{
    render_diagnostics_content, render_diagnostics_footer, render_diagnostics_header,
    render_diagnostics_repair_dialog,
};
use crate::screens::shell::{fit_cell, render_compact_home, render_status, render_top};
use crate::{RenderContext, ShellChromeViewModel, ShellLayout, TundraTheme, compute_shell_layout};

pub fn render_system_status(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SystemStatusViewModel,
    theme: &TundraTheme,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_system_status_contextual(frame, area, chrome, model, &context);
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
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_main(frame, main, model, theme, context);
            render_status(frame, status, chrome, theme);
        }
    }
}

fn render_main(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &SystemStatusViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
) {
    let layout = system_status_layout(main, model);
    Surface::new()
        .titled("System Status")
        .bordered(true)
        .render_frame(frame, layout.panel, context);
    if model.is_diagnostics() {
        render_diagnostics_header(frame, layout.header, &model.diagnostics, theme);
    } else {
        let heading = if model.refreshing {
            "Refreshing system status..."
        } else {
            "Storage, network, and diagnostics health"
        };
        frame.render_widget(
            Paragraph::new(fit_cell(heading, usize::from(layout.header.width)))
                .style(theme.title_style()),
            layout.header,
        );
    }
    render_tabs(frame, &layout, model, theme);
    let title = if model.is_admin() || model.is_diagnostics() {
        model.tab.label()
    } else {
        "Summary"
    };
    Surface::new()
        .titled(title)
        .bordered(true)
        .render_frame(frame, layout.content_panel, context);
    if let Some(diagnostics) = &layout.diagnostics_content {
        render_diagnostics_content(frame, diagnostics, &model.diagnostics, theme, context);
    } else {
        match &model.content {
            SystemStatusContentViewModel::Admin(admin) => {
                render_admin(frame, &layout, model, admin, context)
            }
            SystemStatusContentViewModel::User(user) => {
                render_user(frame, &layout, user, &model.diagnostics, context)
            }
        }
    }
    let help_width = layout
        .footer
        .width
        .saturating_sub(layout.refresh_button.width);
    let help_area = Rect::new(
        layout.footer.x,
        layout.footer.y,
        help_width,
        layout.footer.height,
    );
    if model.is_diagnostics() {
        render_diagnostics_footer(frame, help_area, &model.diagnostics, theme, "Esc Home");
    } else {
        let feedback = model
            .feedback
            .as_deref()
            .unwrap_or("Tab Switch · R Refresh · Esc Home");
        frame.render_widget(
            Paragraph::new(fit_cell(feedback, usize::from(help_width))).style(theme.muted_style()),
            help_area,
        );
    }
    let diagnostics_busy = model.diagnostics.scanning || model.diagnostics.restart_required;
    let mut refresh = Button::new(
        "system-status.refresh",
        if model.is_diagnostics() && model.diagnostics.scanning {
            "Scanning"
        } else if model.is_diagnostics() {
            "Rescan"
        } else if model.refreshing {
            "Refreshing"
        } else {
            "Refresh"
        },
    );
    refresh.set_disabled(if model.is_diagnostics() {
        diagnostics_busy
    } else {
        model.refreshing
    });
    refresh.render_borderless_frame(frame, layout.refresh_button, theme);

    if let (Some(dialog_layout), Some(dialog)) = (
        layout.diagnostics_repair_dialog.as_ref(),
        model.diagnostics.repair_dialog.as_ref(),
    ) {
        render_diagnostics_repair_dialog(frame, dialog_layout, dialog, theme, context);
    }
}

fn render_tabs(
    frame: &mut Frame<'_>,
    layout: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    theme: &TundraTheme,
) {
    let items = model
        .tabs()
        .iter()
        .copied()
        .map(|tab| {
            TabItem::new(
                format!("system-status.tab.{}", tab.label().to_ascii_lowercase()),
                system_status_tab_label(tab, layout.tabs_area.width, model.is_admin()),
            )
        })
        .collect();
    let mut tabs = Tabs::new("system-status.tabs", items);
    tabs.set_selected(model.tabs().iter().position(|tab| *tab == model.tab));
    tabs.render_borderless_frame(frame, layout.tabs_area, theme);
}

fn render_admin(
    frame: &mut Frame<'_>,
    layout: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    admin: &AdminSystemStatusViewModel,
    context: &RenderContext,
) {
    match model.tab {
        SystemStatusTab::Overview => render_overview(
            frame,
            layout.rows_area,
            OverviewData {
                storage_status: &admin.overview.storage_status,
                storage_tone: admin.overview.storage_tone,
                system_volume_usage: &admin.overview.system_volume_usage,
                system_volume_used_percentage: admin.overview.system_volume_used_percentage,
                network_status: &admin.overview.network_status,
                network_tone: admin.overview.network_tone,
                active_link_count: Some(&admin.overview.active_link_count),
                last_refreshed: &admin.overview.last_refreshed,
            },
            &model.diagnostics,
            context,
        ),
        SystemStatusTab::Storage => render_storage(frame, layout, model, admin, context),
        SystemStatusTab::Network => render_network(frame, layout, model, admin, context),
        SystemStatusTab::Health | SystemStatusTab::Logs | SystemStatusTab::Incidents => {}
    }
}

fn render_storage(
    frame: &mut Frame<'_>,
    layout: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    admin: &AdminSystemStatusViewModel,
    context: &RenderContext,
) {
    if render_state(
        frame,
        layout.rows_area,
        &admin.storage_state,
        admin.storage_rows.is_empty(),
        "No storage volumes",
        context,
    ) {
        return;
    }
    let rows = admin.storage_rows.iter().map(|r| {
        vec![
            r.volume.clone(),
            r.kind.clone(),
            r.system_volume.clone(),
            r.access.clone(),
            r.usage.clone(),
            r.used_percentage.clone(),
            r.pressure.clone(),
        ]
    });
    let mut table = DataTable::new(
        "system-status.storage",
        [
            "Volume", "Kind", "System", "Access", "Usage", "Used", "Pressure",
        ],
        rows,
    )
    .bordered(false)
    .with_viewport_start(layout.visible_start)
    .with_row_tones(admin.storage_rows.iter().map(|r| r.tone).collect());
    table.selected = model.selected_index();
    table.state.focused = true;
    table.render_frame(frame, layout.rows_area, context);
    render_stale_notice(frame, layout, &admin.storage_state, context);
    render_scrollbar(frame, layout, model, context);
}

fn render_network(
    frame: &mut Frame<'_>,
    layout: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    admin: &AdminSystemStatusViewModel,
    context: &RenderContext,
) {
    if render_state(
        frame,
        layout.rows_area,
        &admin.network_state,
        admin.network_rows.is_empty(),
        "No network interfaces",
        context,
    ) {
        return;
    }
    let rows = admin.network_rows.iter().map(|r| {
        vec![
            r.name.clone(),
            r.display_name.clone(),
            r.kind.clone(),
            r.link_state.clone(),
            r.addresses.clone(),
        ]
    });
    let mut table = DataTable::new(
        "system-status.network",
        ["Name", "Display name", "Kind", "Link", "Addresses"],
        rows,
    )
    .bordered(false)
    .with_viewport_start(layout.visible_start)
    .with_row_tones(admin.network_rows.iter().map(|r| r.tone).collect());
    table.selected = model.selected_index();
    table.state.focused = true;
    table.render_frame(frame, layout.rows_area, context);
    render_stale_notice(frame, layout, &admin.network_state, context);
    render_scrollbar(frame, layout, model, context);
}

fn render_stale_notice(
    frame: &mut Frame<'_>,
    layout: &SystemStatusLayout,
    state: &SystemStatusSectionState,
    context: &RenderContext,
) {
    if let (Some(area), SystemStatusSectionState::Stale { message }) = (layout.notice_area, state) {
        EmptyState::new("Stale data")
            .detail(message)
            .render_frame(frame, area, context);
    }
}

fn render_state(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &SystemStatusSectionState,
    empty: bool,
    empty_title: &str,
    context: &RenderContext,
) -> bool {
    let placeholder = match state {
        SystemStatusSectionState::Loading => Some(EmptyState::new("Loading...")),
        SystemStatusSectionState::Unavailable { message } => {
            Some(EmptyState::new("Unavailable").detail(message))
        }
        SystemStatusSectionState::Stale { message } if empty => {
            Some(EmptyState::new("Stale data").detail(message))
        }
        SystemStatusSectionState::Ready if empty => Some(EmptyState::new(empty_title)),
        _ => None,
    };
    if let Some(state) = placeholder {
        state.render_frame(frame, area, context);
        true
    } else {
        false
    }
}

fn render_scrollbar(
    frame: &mut Frame<'_>,
    layout: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    context: &RenderContext,
) {
    if let Some(area) = layout.scrollbar {
        Scrollbar::new(
            model.item_count(),
            layout.visible_capacity,
            layout.visible_start,
        )
        .render_frame(frame, area, context);
    }
}

fn render_user(
    frame: &mut Frame<'_>,
    layout: &SystemStatusLayout,
    user: &UserSystemStatusViewModel,
    diagnostics: &crate::DiagnosticsViewModel,
    context: &RenderContext,
) {
    render_overview(
        frame,
        layout.rows_area,
        OverviewData {
            storage_status: &user.storage_status,
            storage_tone: user.storage_tone,
            system_volume_usage: &user.system_volume_usage,
            system_volume_used_percentage: user.system_volume_used_percentage,
            network_status: &user.network_status,
            network_tone: user.network_tone,
            active_link_count: None,
            last_refreshed: &user.last_refreshed,
        },
        diagnostics,
        context,
    );
}

#[derive(Debug, Clone, Copy)]
struct OverviewData<'a> {
    storage_status: &'a str,
    storage_tone: crate::components::ComponentTone,
    system_volume_usage: &'a str,
    system_volume_used_percentage: Option<u8>,
    network_status: &'a str,
    network_tone: crate::components::ComponentTone,
    active_link_count: Option<&'a str>,
    last_refreshed: &'a str,
}

fn render_overview(
    frame: &mut Frame<'_>,
    area: Rect,
    data: OverviewData<'_>,
    diagnostics: &crate::DiagnosticsViewModel,
    context: &RenderContext,
) {
    if area.is_empty() {
        return;
    }
    let theme = &context.compatibility_theme();
    let storage_height = if area.height >= 8 {
        4
    } else {
        area.height.min(3)
    };
    let vertical_gap = u16::from(area.height.saturating_sub(storage_height) >= 4);
    let [storage_card, _, lower_cards] = Layout::vertical([
        Constraint::Length(storage_height),
        Constraint::Length(vertical_gap),
        Constraint::Min(0),
    ])
    .areas(area);
    let horizontal_gap = u16::from(lower_cards.width >= 43);
    let [diagnostics_card, _, network_card] = Layout::horizontal([
        Constraint::Percentage(50),
        Constraint::Length(horizontal_gap),
        Constraint::Min(0),
    ])
    .areas(lower_cards);

    let storage_surface = Surface::new()
        .titled(format!(" System disk · {} ", data.storage_status))
        .bordered(true)
        .raised(true);
    let storage_inner = storage_surface.inner(storage_card);
    storage_surface.render_frame(frame, storage_card, context);
    if storage_inner.height > 0 {
        let show_details = storage_inner.height > 1;
        let [details_area, gauge_area] = Layout::vertical([
            Constraint::Length(u16::from(show_details)),
            Constraint::Min(0),
        ])
        .areas(storage_inner);
        if show_details {
            frame.render_widget(
                Paragraph::new(data.system_volume_usage).style(theme.muted_style()),
                details_area,
            );
        }
        if let Some(percentage) = data.system_volume_used_percentage {
            let label = if show_details {
                format!("{percentage}% used")
            } else {
                format!("{percentage}% used · {}", data.system_volume_usage)
            };
            frame.render_widget(
                Gauge::default()
                    .style(theme.surface_style())
                    .gauge_style(theme.body_style().fg(tone_color(data.storage_tone, theme)))
                    .percent(u16::from(percentage))
                    .label(label)
                    .use_unicode(true),
                gauge_area,
            );
        } else {
            frame.render_widget(
                Paragraph::new("Usage unavailable").style(theme.muted_style()),
                gauge_area,
            );
        }
    }

    let (diagnostics_status, diagnostics_tone) = diagnostics_overview(diagnostics);
    render_overview_card(
        frame,
        diagnostics_card,
        "Diagnostics",
        diagnostics_status,
        diagnostics_tone,
        None,
        context,
    );

    let network_status = match data.active_link_count {
        Some("1") => format!("{} · 1 active link", data.network_status),
        Some(count) => format!("{} · {count} active links", data.network_status),
        None => data.network_status.to_string(),
    };
    render_overview_card(
        frame,
        network_card,
        "Network",
        network_status,
        data.network_tone,
        Some(format!("Updated {}", data.last_refreshed)),
        context,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_overview_card(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    value: String,
    tone: crate::components::ComponentTone,
    detail: Option<String>,
    context: &RenderContext,
) {
    if area.is_empty() {
        return;
    }
    let theme = &context.compatibility_theme();
    let surface = Surface::new()
        .titled(format!(" {title} "))
        .bordered(true)
        .raised(true);
    let inner = surface.inner(area);
    surface.render_frame(frame, area, context);
    let mut lines = vec![Line::styled(
        value,
        theme.body_style().fg(tone_color(tone, theme)),
    )];
    if let Some(detail) = detail {
        lines.push(Line::styled(detail, theme.muted_style()));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn diagnostics_overview(
    diagnostics: &crate::DiagnosticsViewModel,
) -> (String, crate::components::ComponentTone) {
    use crate::DiagnosticsStatus;
    use crate::components::ComponentTone;

    if diagnostics.scanned_at.is_none() {
        return if diagnostics.scanning {
            ("Scanning...".to_string(), ComponentTone::Accent)
        } else {
            ("Not scanned".to_string(), ComponentTone::Muted)
        };
    }

    let warnings = diagnostics
        .checks
        .iter()
        .filter(|check| check.status == DiagnosticsStatus::Warning)
        .count();
    let failures = diagnostics
        .checks
        .iter()
        .filter(|check| check.status == DiagnosticsStatus::Fail)
        .count();
    if failures > 0 {
        let failure_label = if failures == 1 {
            "1 failure".to_string()
        } else {
            format!("{failures} failures")
        };
        let label = if warnings == 0 {
            failure_label
        } else {
            format!("{failure_label} · {warnings} warnings")
        };
        (label, ComponentTone::Danger)
    } else if warnings > 0 {
        let label = if warnings == 1 {
            "1 warning".to_string()
        } else {
            format!("{warnings} warnings")
        };
        (label, ComponentTone::Warning)
    } else {
        ("No issues".to_string(), ComponentTone::Success)
    }
}
