use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Borders, Paragraph, Wrap};

use super::{HomeDisplayMode, HomeViewModel};
use crate::TundraTheme;
use crate::screens::shell::{
    ShellChromeViewModel, ShellLayout, compute_shell_layout, render_compact_home, render_status,
    render_top,
};

const HOME_SUMMARY_HEIGHT: u16 = 1;
const HOME_CONTROLS_HEIGHT: u16 = 2;
const HOME_TILE_MAX_HEIGHT: u16 = 8;
const HOME_TILE_MIN_HEIGHT: u16 = 3;
const HOME_TILE_GAP: u16 = 1;

pub fn render_home(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    home: &HomeViewModel,
    theme: &TundraTheme,
) {
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_main(frame, main, home, theme);
            render_status(frame, status, chrome, theme);
        }
    }
}

fn render_main(frame: &mut Frame<'_>, area: Rect, home: &HomeViewModel, theme: &TundraTheme) {
    match home.display_mode() {
        HomeDisplayMode::Debug | HomeDisplayMode::User | HomeDisplayMode::Auth => {
            render_user_main(frame, area, home, theme)
        }
    }
}

fn render_user_main(frame: &mut Frame<'_>, area: Rect, home: &HomeViewModel, theme: &TundraTheme) {
    let outer = theme
        .block()
        .title("Home")
        .borders(Borders::ALL)
        .style(theme.body_style());
    frame.render_widget(outer, area);

    let content = home_content_area(area);
    if content.width == 0 || content.height == 0 {
        return;
    }

    let summary = home_summary_area(area);
    let controls = home_controls_area(area);
    render_home_account_summary(frame, area, summary, home, theme);

    for (index, (entry, tile)) in home
        .entries()
        .iter()
        .zip(home_entry_tile_areas(area, home.entries().len()))
        .enumerate()
    {
        let selected = index == home.selected_entry_index();
        let style = if selected {
            theme.title_style()
        } else {
            theme.body_style()
        };
        let content_width = usize::from(tile.width.saturating_sub(2));
        let mut lines: Vec<Line<'static>> = Vec::new();
        if let Some(icon) = home.home_icon_for_label(&entry.label) {
            lines.extend(
                icon.lines
                    .iter()
                    .map(|line| centered_home_tile_line(line, icon.width(), content_width)),
            );
        }
        lines.push(Line::styled(
            centered_home_tile_text(&entry.label, content_width),
            style,
        ));
        lines.push(Line::from(centered_home_tile_text(
            &entry.description,
            content_width,
        )));

        let tile_widget = Paragraph::new(lines)
            .block(
                theme
                    .block()
                    .borders(Borders::ALL)
                    .style(style)
                    .border_style(theme.selectable_border_style(selected))
                    .title(if selected { "Selected" } else { "" })
                    .title_style(style),
            )
            .style(style);

        frame.render_widget(tile_widget, tile);
    }

    let controls_text = if home.logout_visible() && home.entries().is_empty() {
        "Tab: focus Logout / Clock    L: Logout    Q / Esc: exit"
    } else if home.logout_visible() {
        "Arrows: select    Enter: open    E: explorer    U: users    L: Logout    Q / Esc: exit"
    } else {
        "Arrows: select    Enter: open    E: explorer    U: users    Q / Esc: exit"
    };
    frame.render_widget(
        Paragraph::new(Line::from(controls_text))
            .style(theme.muted_style())
            .wrap(Wrap { trim: true }),
        controls,
    );
}

fn render_home_account_summary(
    frame: &mut Frame<'_>,
    main: Rect,
    summary: Rect,
    home: &HomeViewModel,
    theme: &TundraTheme,
) {
    if summary.width == 0 || summary.height == 0 {
        return;
    }
    let logout = home_logout_area(main, home);
    let user_width = if logout.width > 0 {
        logout.x.saturating_sub(summary.x).saturating_sub(2)
    } else {
        summary.width
    };
    let user = home.current_user.as_deref().unwrap_or("Unknown user");
    frame.render_widget(
        Paragraph::new(Line::from(format!("User: {user}")))
            .style(theme.body_style())
            .wrap(Wrap { trim: true }),
        Rect::new(summary.x, summary.y, user_width, summary.height),
    );
    if logout.width > 0 {
        let style = if home.logout_selected() {
            theme.title_style()
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(Line::styled("[Logout]", style)).style(style),
            logout,
        );
    }
}

fn centered_home_tile_line(
    line: &str,
    measured_width: usize,
    content_width: usize,
) -> Line<'static> {
    Line::from(centered_home_tile_value(
        line,
        measured_width,
        content_width,
    ))
}

fn centered_home_tile_text(text: &str, content_width: usize) -> String {
    centered_home_tile_value(text, text.chars().count(), content_width)
}

fn centered_home_tile_value(text: &str, measured_width: usize, content_width: usize) -> String {
    let padding = " ".repeat(content_width.saturating_sub(measured_width) / 2);
    format!("{padding}{text}")
}

pub fn home_entry_tile_areas(main: Rect, entry_count: usize) -> Vec<Rect> {
    if entry_count == 0 {
        return Vec::new();
    }

    let grid = home_entry_grid_area(main);
    if grid.width == 0 || grid.height == 0 {
        return Vec::new();
    }

    let columns = home_entry_column_count(grid.width, entry_count);
    let rows = entry_count.div_ceil(columns);
    let horizontal_gap = if columns > 1 { HOME_TILE_GAP } else { 0 };
    let vertical_gap = if rows > 1 { HOME_TILE_GAP } else { 0 };
    let total_horizontal_gap = horizontal_gap.saturating_mul(columns.saturating_sub(1) as u16);
    let total_vertical_gap = vertical_gap.saturating_mul(rows.saturating_sub(1) as u16);
    let tile_width = grid
        .width
        .saturating_sub(total_horizontal_gap)
        .checked_div(columns as u16)
        .unwrap_or(0);
    let available_height = grid.height.saturating_sub(total_vertical_gap);
    let tile_height = available_height
        .checked_div(rows as u16)
        .unwrap_or(0)
        .min(HOME_TILE_MAX_HEIGHT)
        .max(HOME_TILE_MIN_HEIGHT.min(grid.height));

    let mut areas = Vec::with_capacity(entry_count);
    for index in 0..entry_count {
        let row = index / columns;
        let column = index % columns;
        let x = grid.x.saturating_add(
            (column as u16).saturating_mul(tile_width.saturating_add(horizontal_gap)),
        );
        let y = grid
            .y
            .saturating_add((row as u16).saturating_mul(tile_height.saturating_add(vertical_gap)));
        if x >= grid.x.saturating_add(grid.width) || y >= grid.y.saturating_add(grid.height) {
            break;
        }
        let width = tile_width.min(grid.x.saturating_add(grid.width).saturating_sub(x));
        let height = tile_height.min(grid.y.saturating_add(grid.height).saturating_sub(y));
        if width > 0 && height > 0 {
            areas.push(Rect::new(x, y, width, height));
        }
    }

    areas
}

pub fn home_entry_index_at(
    main: Rect,
    entry_count: usize,
    coordinates: (u16, u16),
) -> Option<usize> {
    home_entry_tile_areas(main, entry_count)
        .into_iter()
        .enumerate()
        .find_map(|(index, area)| rect_contains(area, coordinates).then_some(index))
}

/// Returns the exact Logout control rectangle used by Home rendering.
///
/// Homes without an authenticated account expose a zero-sized area so input
/// routing cannot accidentally make Logout interactive.
pub fn home_logout_area(main: Rect, home: &HomeViewModel) -> Rect {
    let summary = home_summary_area(main);
    if !home.logout_visible() || summary.width == 0 || summary.height == 0 {
        return Rect::new(summary.x.saturating_add(summary.width), summary.y, 0, 0);
    }

    const LOGOUT_LABEL_WIDTH: u16 = 8;
    const ACCOUNT_LOGOUT_GAP: u16 = 2;
    let width = LOGOUT_LABEL_WIDTH.min(summary.width);
    let user_width = home
        .current_user
        .as_deref()
        .unwrap_or("Unknown user")
        .chars()
        .count()
        .saturating_add("User: ".len());
    let desired_offset = u16::try_from(user_width)
        .unwrap_or(u16::MAX)
        .saturating_add(ACCOUNT_LOGOUT_GAP);
    let max_offset = summary.width.saturating_sub(width);
    Rect::new(
        summary.x.saturating_add(desired_offset.min(max_offset)),
        summary.y,
        width,
        1,
    )
}

fn home_content_area(main: Rect) -> Rect {
    Rect::new(
        main.x.saturating_add(1),
        main.y.saturating_add(1),
        main.width.saturating_sub(2),
        main.height.saturating_sub(2),
    )
}

fn home_summary_area(main: Rect) -> Rect {
    let content = home_content_area(main);
    Rect::new(
        content.x,
        content.y,
        content.width,
        HOME_SUMMARY_HEIGHT.min(content.height),
    )
}

fn home_controls_area(main: Rect) -> Rect {
    let content = home_content_area(main);
    let height = HOME_CONTROLS_HEIGHT.min(content.height);
    Rect::new(
        content.x,
        content
            .y
            .saturating_add(content.height.saturating_sub(height)),
        content.width,
        height,
    )
}

fn home_entry_grid_area(main: Rect) -> Rect {
    let content = home_content_area(main);
    let reserved = HOME_SUMMARY_HEIGHT.saturating_add(HOME_CONTROLS_HEIGHT);
    let y = content
        .y
        .saturating_add(HOME_SUMMARY_HEIGHT.min(content.height));
    let bottom = content.y.saturating_add(
        content
            .height
            .saturating_sub(HOME_CONTROLS_HEIGHT.min(content.height)),
    );
    Rect::new(
        content.x,
        y,
        content.width,
        bottom
            .saturating_sub(y)
            .min(content.height.saturating_sub(reserved.min(content.height))),
    )
}

fn home_entry_column_count(width: u16, entry_count: usize) -> usize {
    let max_columns = if width >= 96 {
        4
    } else if width >= 72 {
        3
    } else if width >= 48 {
        2
    } else {
        1
    };

    max_columns.min(entry_count.max(1))
}

fn rect_contains(rect: Rect, coordinates: (u16, u16)) -> bool {
    let right = rect.x.saturating_add(rect.width);
    let bottom = rect.y.saturating_add(rect.height);

    coordinates.0 >= rect.x
        && coordinates.0 < right
        && coordinates.1 >= rect.y
        && coordinates.1 < bottom
}
