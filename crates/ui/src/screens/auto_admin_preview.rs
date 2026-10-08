//! AA style previews and shared decoration. Production uses Authorization.
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::{AutoAdminLayout, AutoAdminViewModel, RenderContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoAdminPreviewStyle {
    Caution,
    Danger,
    Authorization,
}

pub fn auto_admin_preview_layout(
    bounds: Rect,
    model: &AutoAdminViewModel,
    style: AutoAdminPreviewStyle,
) -> (AutoAdminLayout, Rect) {
    let main = match crate::compute_shell_layout(bounds) {
        crate::ShellLayout::Full { main, .. } | crate::ShellLayout::Compact(main) => main,
    };
    let width = main
        .width
        .saturating_sub(u16::from(main.width >= 50) * 4)
        .min(match style {
            AutoAdminPreviewStyle::Caution => 84,
            AutoAdminPreviewStyle::Danger => 104,
            AutoAdminPreviewStyle::Authorization => 100,
        });
    let height = main.height.min(if model.confirming { 19 } else { 29 });
    let dialog = Rect::new(
        main.x + main.width.saturating_sub(width) / 2,
        main.y
            + if style == AutoAdminPreviewStyle::Danger {
                main.height.saturating_sub(height).min(1)
            } else {
                main.height.saturating_sub(height) / 2
            },
        width,
        height,
    );
    let mut inner = dialog.inner(Margin::new(if width >= 50 { 3 } else { 1 }, 1));
    if style == AutoAdminPreviewStyle::Authorization && width >= 70 {
        inner.x += 18;
        inner.width = inner.width.saturating_sub(18);
    }
    let compact = inner.height < 13;
    let rows = Layout::vertical([
        Constraint::Length(if compact {
            1
        } else if style == AutoAdminPreviewStyle::Danger {
            4
        } else {
            3
        }),
        Constraint::Length(if compact { 1 } else { 3 }),
        Constraint::Min(0),
        Constraint::Length(if compact { 1 } else { 2 }),
        Constraint::Length(if compact { 1 } else { 2 }),
        Constraint::Length(1),
    ])
    .split(inner);
    let count = if model.confirming {
        2
    } else if model.finished {
        1
    } else {
        3
    };
    let buttons = Layout::horizontal(vec![Constraint::Ratio(1, count); count as usize])
        .spacing(u16::from(width >= 50))
        .split(rows[5]);
    (
        AutoAdminLayout {
            dialog,
            description: rows[1],
            terminal: rows[2],
            status: rows[3],
            input: rows[4],
            buttons: std::array::from_fn(|index| buttons.get(index).copied().unwrap_or_default()),
        },
        rows[0],
    )
}

pub fn render_auto_admin_preview(
    frame: &mut Frame<'_>,
    bounds: Rect,
    model: &AutoAdminViewModel,
    style: AutoAdminPreviewStyle,
    context: &RenderContext,
) {
    let (layout, warning) = auto_admin_preview_layout(bounds, model, style);
    render_auto_admin_frame(frame, bounds, &layout, warning, style, None);
    super::auto_admin::render_auto_admin_contents(
        frame,
        &layout,
        model,
        context,
        Some(&i18n::tr!("aa-preview-safe")),
    );
}

pub(super) fn render_auto_admin_frame(
    frame: &mut Frame<'_>,
    bounds: Rect,
    layout: &AutoAdminLayout,
    warning: Rect,
    style: AutoAdminPreviewStyle,
    title_override: Option<&str>,
) {
    let main = match crate::compute_shell_layout(bounds) {
        crate::ShellLayout::Full { main, .. } | crate::ShellLayout::Compact(main) => main,
    };
    render_auto_admin_frame_in_area(frame, main, layout, warning, style, title_override);
}

pub(super) fn render_auto_admin_frame_in_area(
    frame: &mut Frame<'_>,
    main: Rect,
    layout: &AutoAdminLayout,
    warning: Rect,
    style: AutoAdminPreviewStyle,
    title_override: Option<&str>,
) {
    // Keep the page recognizable while making it clearly inactive. Explicit
    // foreground/background values also work when a terminal ignores DIM.
    for y in main.y..main.bottom() {
        for x in main.x..main.right() {
            frame.buffer_mut()[(x, y)].set_style(
                Style::default()
                    .fg(Color::DarkGray)
                    .bg(Color::Black)
                    .remove_modifier(Modifier::BOLD | Modifier::REVERSED)
                    .add_modifier(Modifier::DIM),
            );
        }
    }
    let shadow = Rect::new(
        layout.dialog.x.saturating_add(2),
        layout.dialog.y.saturating_add(1),
        layout.dialog.width,
        layout.dialog.height,
    )
    .intersection(main);
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::Black)),
        shadow,
    );
    let (accent, background, border, title) = match style {
        AutoAdminPreviewStyle::Caution => (
            Color::Yellow,
            Color::Rgb(36, 30, 14),
            BorderType::Double,
            " AA / AutoAdmin / 01 ",
        ),
        AutoAdminPreviewStyle::Danger => (
            Color::LightRed,
            Color::Rgb(39, 18, 23),
            BorderType::Thick,
            " AA / AutoAdmin / 02 ",
        ),
        AutoAdminPreviewStyle::Authorization => (
            Color::LightCyan,
            Color::Rgb(14, 27, 38),
            BorderType::Plain,
            " AA / AutoAdmin / 03 ",
        ),
    };
    frame.render_widget(Clear, layout.dialog);
    frame.render_widget(
        Block::bordered()
            .border_type(border)
            .title(title_override.unwrap_or(title))
            .border_style(Style::default().fg(accent))
            .style(Style::default().fg(Color::White).bg(background)),
        layout.dialog,
    );
    let text = match style {
        AutoAdminPreviewStyle::Caution => format!(
            "! {}\n{}",
            i18n::tr!("aa-preview-warning"),
            i18n::tr!("aa-preview-review")
        ),
        AutoAdminPreviewStyle::Danger => format!(
            "{}\n!! {} !!\n{}",
            "/ ".repeat(usize::from(warning.width / 2)),
            i18n::tr!("aa-preview-warning"),
            i18n::tr!("aa-preview-review")
        ),
        AutoAdminPreviewStyle::Authorization => format!(
            "[AA] {}\n{}",
            i18n::tr!("aa-preview-warning"),
            i18n::tr!("aa-preview-review")
        ),
    };
    let mut banner = Paragraph::new(text)
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(accent).add_modifier(Modifier::BOLD));
    if style == AutoAdminPreviewStyle::Danger {
        banner = banner.style(
            Style::default()
                .fg(Color::White)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        );
    } else if style == AutoAdminPreviewStyle::Authorization {
        banner = banner.block(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(Color::Yellow)),
        );
        if layout.dialog.width >= 70 && !warning.is_empty() {
            let badge = Rect::new(
                layout.dialog.x + 2,
                layout.dialog.y + 1,
                17,
                layout.dialog.height.saturating_sub(2),
            );
            frame.render_widget(Paragraph::new(format!(
                "\n    /\\    /\\\n   /  \\  /  \\\n   |--|  |--|\n   |  |  |  |\n\n   AutoAdmin\n\n   {}",
                i18n::tr!("aa-preview-gate")
            )).wrap(Wrap { trim: false })
                .block(Block::default().borders(Borders::RIGHT).border_style(Style::default().fg(accent)))
                .style(Style::default().fg(accent).add_modifier(Modifier::BOLD)), badge);
        }
    }
    frame.render_widget(banner, warning);
}
