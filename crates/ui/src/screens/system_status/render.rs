use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};

use super::layout::{SystemStatusLayout, system_status_layout};
use super::model::{
    AdminSystemStatusViewModel, SystemStatusContentViewModel, SystemStatusSectionState,
    SystemStatusTab, SystemStatusViewModel, UserSystemStatusViewModel,
};
use crate::components::{
    Button, DataTable, EmptyState, Scrollbar, Surface, TabItem, Tabs, tone_color,
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
    let heading = if model.refreshing {
        "Refreshing system status..."
    } else {
        "Storage and network health"
    };
    frame.render_widget(
        Paragraph::new(fit_cell(heading, usize::from(layout.header.width)))
            .style(theme.title_style()),
        layout.header,
    );
    if model.is_admin() {
        render_tabs(frame, &layout, model, theme);
    }
    let title = if model.is_admin() {
        model.tab.label()
    } else {
        "Summary"
    };
    Surface::new()
        .titled(title)
        .bordered(true)
        .render_frame(frame, layout.content_panel, context);
    match &model.content {
        SystemStatusContentViewModel::Admin(admin) => {
            render_admin(frame, &layout, model, admin, theme, context)
        }
        SystemStatusContentViewModel::User(user) => render_user(frame, &layout, user, theme),
    }
    let feedback = model
        .feedback
        .as_deref()
        .unwrap_or("D Diagnostics · R Refresh · Esc Home");
    let help_width = layout
        .footer
        .width
        .saturating_sub(layout.diagnostics_button.width)
        .saturating_sub(layout.refresh_button.width);
    frame.render_widget(
        Paragraph::new(fit_cell(feedback, usize::from(help_width))).style(theme.muted_style()),
        Rect::new(
            layout.footer.x,
            layout.footer.y,
            help_width,
            layout.footer.height,
        ),
    );
    Button::new("system-status.diagnostics", "Diagnostics").render_borderless_frame(
        frame,
        layout.diagnostics_button,
        theme,
    );
    let mut refresh = Button::new(
        "system-status.refresh",
        if model.refreshing {
            "Refreshing"
        } else {
            "Refresh"
        },
    );
    refresh.set_disabled(model.refreshing);
    refresh.render_borderless_frame(frame, layout.refresh_button, theme);
}

fn render_tabs(
    frame: &mut Frame<'_>,
    layout: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    theme: &TundraTheme,
) {
    let items = SystemStatusTab::ALL
        .into_iter()
        .map(|tab| {
            TabItem::new(
                format!("system-status.tab.{}", tab.label().to_ascii_lowercase()),
                tab.label(),
            )
        })
        .collect();
    let mut tabs = Tabs::new("system-status.tabs", items);
    tabs.set_selected(
        SystemStatusTab::ALL
            .iter()
            .position(|tab| *tab == model.tab),
    );
    tabs.render_borderless_frame(frame, layout.tabs_area, theme);
}

fn render_admin(
    frame: &mut Frame<'_>,
    layout: &SystemStatusLayout,
    model: &SystemStatusViewModel,
    admin: &AdminSystemStatusViewModel,
    theme: &TundraTheme,
    context: &RenderContext,
) {
    match model.tab {
        SystemStatusTab::Overview => render_lines(
            frame,
            layout.rows_area,
            vec![
                Line::styled(
                    format!("Storage status: {}", admin.overview.storage_status),
                    theme
                        .body_style()
                        .fg(tone_color(admin.overview.storage_tone, theme)),
                ),
                Line::from(format!(
                    "System volume usage: {}",
                    admin.overview.system_volume_usage
                )),
                Line::from(format!(
                    "Active links: {}",
                    admin.overview.active_link_count
                )),
                Line::from(format!("Last refreshed: {}", admin.overview.last_refreshed)),
            ],
            theme,
        ),
        SystemStatusTab::Storage => render_storage(frame, layout, model, admin, context),
        SystemStatusTab::Network => render_network(frame, layout, model, admin, context),
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
    theme: &TundraTheme,
) {
    render_lines(
        frame,
        layout.rows_area,
        vec![
            Line::styled(
                format!("Storage status: {}", user.storage_status),
                theme.body_style().fg(tone_color(user.storage_tone, theme)),
            ),
            Line::from(format!("System volume usage: {}", user.system_volume_usage)),
            Line::styled(
                format!("Network status: {}", user.network_status),
                theme.body_style().fg(tone_color(user.network_tone, theme)),
            ),
            Line::from(format!("Last refreshed: {}", user.last_refreshed)),
        ],
        theme,
    );
}

fn render_lines(frame: &mut Frame<'_>, area: Rect, lines: Vec<Line<'static>>, theme: &TundraTheme) {
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(HorizontalAlignment::Left)
            .style(theme.body_style())
            .wrap(Wrap { trim: true }),
        area,
    );
}
