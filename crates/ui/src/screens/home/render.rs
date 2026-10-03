use crate::components::{terminal_width, truncate_to_terminal_width};
use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};

use super::{HomeDisplayMode, HomeViewModel};
use crate::components::{Button, Scrollbar, Surface};
use crate::{RenderContext, TundraTheme};

const HOME_SUMMARY_HEIGHT: u16 = 1;
const HOME_TILE_MAX_HEIGHT: u16 = 8;
const HOME_TILE_MIN_HEIGHT: u16 = 3;
const HOME_TILE_GAP: u16 = crate::SpringStyle::CARD_GAP;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeItemLayout {
    pub index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeLayout {
    pub items: Vec<HomeItemLayout>,
    pub scrollbar: Option<Rect>,
    pub visible_start: usize,
    pub visible_capacity: usize,
    pub columns: usize,
    /// Empty slots in the final grid row count toward scrollbar movement.
    pub scroll_content_len: usize,
}

impl HomeLayout {
    pub fn entry_at(&self, coordinates: (u16, u16)) -> Option<usize> {
        self.items
            .iter()
            .find_map(|item| rect_contains(item.area, coordinates).then_some(item.index))
    }
}

pub fn home_layout(main: Rect, home: &HomeViewModel) -> HomeLayout {
    home_layout_for_entries(
        main,
        home.entries().len(),
        home.selected_entry_index(),
        home.viewport_offset(),
    )
}

pub trait HomeIconRenderer {
    /// Returns true when a terminal image was rendered for `entry_label`.
    fn render_icon(&self, entry_label: &str, frame: &mut Frame<'_>, area: Rect) -> bool;
}

pub fn render_home_content(
    frame: &mut Frame<'_>,
    area: Rect,
    home: &HomeViewModel,
    context: &RenderContext,
    icons: Option<&dyn HomeIconRenderer>,
) {
    match home.display_mode() {
        HomeDisplayMode::Debug | HomeDisplayMode::User | HomeDisplayMode::Auth => {
            render_user_main(frame, area, home, context, icons)
        }
    }
}

fn render_user_main(
    frame: &mut Frame<'_>,
    area: Rect,
    home: &HomeViewModel,
    context: &RenderContext,
    icons: Option<&dyn HomeIconRenderer>,
) {
    let theme = &context.compatibility_theme();
    Surface::new()
        .titled(i18n::tr!("ui-home-home"))
        .bordered(false)
        .render_frame(frame, area, context);

    let content = home_content_area(area);
    if content.width == 0 || content.height == 0 {
        return;
    }

    let summary = home_summary_area(area);
    let layout = home_layout(area, home);
    render_home_account_summary(frame, area, summary, home, theme);

    for item in &layout.items {
        let index = item.index;
        let entry = &home.entries()[index];
        let tile = item.area;
        let selected = theme.keyboard_focus_visible() && index == home.selected_entry_index();
        let style = if selected {
            theme.title_style()
        } else {
            theme.body_style()
        }
        .bg(context.theme.raised);
        let content_width = usize::from(tile.width.saturating_sub(2));
        let mut surface = Button::new(format!("home.entry.{index}"), "");
        surface.state.selected = selected;
        surface.set_focused(selected);
        if tile.height < HOME_TILE_MIN_HEIGHT {
            let mut compact =
                Button::new(format!("home.entry.{index}"), entry.label_with_shortcut());
            compact.state.selected = selected;
            compact.set_focused(selected);
            compact.render_borderless_frame(frame, tile, theme);
            continue;
        }
        surface.render_surface_frame(frame, tile, theme);
        let icon_area = home_entry_icon_area(tile);
        let rendered_graphic = icon_area.width > 0
            && icon_area.height > 0
            && icons
                .is_some_and(|icons| icons.render_icon(entry.icon_identity(), frame, icon_area));
        if !rendered_graphic && let Some(icon) = home.home_icon_for_label(entry.icon_identity()) {
            for (row, line) in icon
                .lines()
                .iter()
                .take(usize::from(icon_area.height))
                .enumerate()
            {
                frame.render_widget(
                    Paragraph::new(centered_home_tile_line(line, icon.width(), content_width))
                        .alignment(HorizontalAlignment::Left)
                        .style(style),
                    Rect::new(
                        icon_area.x,
                        icon_area
                            .y
                            .saturating_add(u16::try_from(row).unwrap_or(u16::MAX)),
                        icon_area.width,
                        1,
                    ),
                );
            }
        }

        let inner = Rect::new(
            tile.x.saturating_add(1),
            tile.y.saturating_add(1),
            tile.width.saturating_sub(2),
            tile.height.saturating_sub(2),
        );
        let label_y = icon_area.bottom();
        if label_y < inner.bottom() {
            frame.render_widget(
                Paragraph::new(Line::styled(
                    centered_home_tile_text(&entry.label_with_shortcut(), content_width),
                    style,
                ))
                .alignment(HorizontalAlignment::Left)
                .style(style),
                Rect::new(inner.x, label_y, inner.width, 1),
            );
        }
        let description_y = label_y.saturating_add(1);
        if description_y < inner.bottom() {
            frame.render_widget(
                Paragraph::new(centered_home_tile_text(&entry.description, content_width))
                    .alignment(HorizontalAlignment::Left)
                    .style(style),
                Rect::new(inner.x, description_y, inner.width, 1),
            );
        }
    }

    if let Some(area) = layout.scrollbar {
        Scrollbar::new(
            layout.scroll_content_len,
            layout.visible_capacity,
            layout.visible_start,
        )
        .render_frame(frame, area, context);
    }
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
    let fallback = i18n::tr!("ui-home-unknown-user");
    let user = home.current_user.as_deref().unwrap_or(&fallback);
    frame.render_widget(
        Paragraph::new(Line::from(i18n::tr!("ui-home-current-user", user = user)))
            .alignment(HorizontalAlignment::Left)
            .style(theme.body_style())
            .wrap(Wrap { trim: true }),
        Rect::new(summary.x, summary.y, user_width, summary.height),
    );
    if logout.width > 0 {
        let mut button = Button::new("home.logout", i18n::tr!("ui-home-logout-button"));
        button.state.selected = home.logout_selected();
        button.render_borderless_frame(frame, logout, theme);
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
    let text = truncate_to_terminal_width(text, content_width);
    centered_home_tile_value(&text, Line::from(text.as_str()).width(), content_width)
}

fn centered_home_tile_value(text: &str, measured_width: usize, content_width: usize) -> String {
    let padding = " ".repeat(content_width.saturating_sub(measured_width) / 2);
    format!("{padding}{text}")
}

pub fn home_entry_tile_areas(main: Rect, entry_count: usize) -> Vec<Rect> {
    home_layout_for_entries(main, entry_count, 0, 0)
        .items
        .into_iter()
        .map(|item| item.area)
        .collect()
}

fn home_layout_for_entries(
    main: Rect,
    entry_count: usize,
    selected: usize,
    requested: usize,
) -> HomeLayout {
    let mut grid = home_entry_grid_area(main);
    let mut columns = home_entry_column_count(grid.width, entry_count);
    let max_rows = usize::from(
        (grid.height.saturating_add(HOME_TILE_GAP)
            / HOME_TILE_MIN_HEIGHT.saturating_add(HOME_TILE_GAP))
        .max(1),
    );
    let mut rows = entry_count.div_ceil(columns).min(max_rows).max(1);
    let needs_scrollbar =
        entry_count > columns.saturating_mul(rows) && grid.width > 0 && grid.height > 0;
    let scrollbar =
        needs_scrollbar.then(|| Rect::new(grid.right().saturating_sub(1), grid.y, 1, grid.height));
    if needs_scrollbar {
        grid.width = grid.width.saturating_sub(1);
        columns = home_entry_column_count(grid.width, entry_count);
        rows = entry_count.div_ceil(columns).min(max_rows).max(1);
    }
    let capacity = columns.saturating_mul(rows);
    let scroll_content_len = entry_count.div_ceil(columns).saturating_mul(columns);
    let max_start = scroll_content_len.saturating_sub(capacity);
    let mut start = requested.min(max_start) / columns * columns;
    if selected < start {
        start = selected / columns * columns;
    } else if selected < entry_count && selected >= start.saturating_add(capacity) {
        start = (selected / columns).saturating_sub(rows - 1) * columns;
    }
    start = start.min(max_start);
    let horizontal_gap = if columns > 1 { HOME_TILE_GAP } else { 0 };
    let tile_width = grid
        .width
        .saturating_sub(horizontal_gap.saturating_mul((columns - 1) as u16))
        / columns as u16;
    let tile_height = (grid
        .height
        .saturating_sub(HOME_TILE_GAP.saturating_mul((rows - 1) as u16))
        / rows as u16)
        .min(HOME_TILE_MAX_HEIGHT);
    let items = if grid.width == 0 || grid.height == 0 || tile_width == 0 || tile_height == 0 {
        Vec::new()
    } else {
        (start..entry_count)
            .take(capacity)
            .enumerate()
            .map(|(slot, index)| HomeItemLayout {
                index,
                area: Rect::new(
                    grid.x.saturating_add(
                        (slot % columns) as u16 * tile_width.saturating_add(horizontal_gap),
                    ),
                    grid.y.saturating_add(
                        (slot / columns) as u16 * tile_height.saturating_add(HOME_TILE_GAP),
                    ),
                    tile_width,
                    tile_height,
                ),
            })
            .collect()
    };
    HomeLayout {
        items,
        scrollbar,
        visible_start: start,
        visible_capacity: capacity,
        columns,
        scroll_content_len,
    }
}

/// Returns the image allocation shared by Home's ASCII and graphical icons.
///
/// The first four inner rows historically hold the ASCII icon. Terminal
/// images use the same allocation and [`crate::PreparedEditorImage::render_centered`],
/// matching Launcher icon centering without moving Home labels.
pub fn home_entry_icon_area(tile: Rect) -> Rect {
    Rect::new(
        tile.x.saturating_add(1),
        tile.y.saturating_add(1),
        tile.width.saturating_sub(2),
        // Always reserve a row for the application name on short tiles.
        tile.height.saturating_sub(3).min(4),
    )
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

    let logout_label_width =
        u16::try_from(terminal_width(&i18n::tr!("ui-home-logout-button"))).unwrap_or(u16::MAX);
    const ACCOUNT_LOGOUT_GAP: u16 = 2;
    let width = logout_label_width.min(summary.width);
    let fallback = i18n::tr!("ui-home-unknown-user");
    let user = home.current_user.as_deref().unwrap_or(&fallback);
    let user_width = terminal_width(&i18n::tr!("ui-home-current-user", user = user));
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

fn home_entry_grid_area(main: Rect) -> Rect {
    let content = home_content_area(main);
    let y = content
        .y
        .saturating_add(HOME_SUMMARY_HEIGHT.min(content.height));
    Rect::new(
        content.x,
        y,
        content.width,
        content.bottom().saturating_sub(y),
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
