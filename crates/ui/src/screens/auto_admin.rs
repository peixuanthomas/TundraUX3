use crate::{
    CommandLineTerminalSnapshot, RenderContext,
    components::{Button, Surface},
};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Margin, Rect},
    widgets::{Clear, Paragraph},
};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AutoAdminViewModel {
    pub description: String,
    pub status: String,
    pub confirming: bool,
    pub finished: bool,
    pub approve_selected: bool,
    pub button_focus: Option<usize>,
    pub scroll: u16,
    pub input: Option<String>,
    pub terminal: Arc<CommandLineTerminalSnapshot>,
}

#[derive(Debug, Clone, Copy)]
pub struct AutoAdminLayout {
    pub dialog: Rect,
    pub description: Rect,
    pub terminal: Rect,
    pub status: Rect,
    pub input: Rect,
    pub buttons: [Rect; 4],
}

/// Receives the full Shell bounds so drawing, hit testing and PTY sizing all
/// reserve the same fixed title/status bars, even immediately after a resize.
pub fn auto_admin_layout(bounds: Rect, model: &AutoAdminViewModel) -> AutoAdminLayout {
    let main = match crate::compute_shell_layout(bounds) {
        crate::ShellLayout::Full { main, .. } | crate::ShellLayout::Compact(main) => main,
    };
    let width = if main.width > 80 {
        main.width.saturating_sub(main.width / 10)
    } else {
        main.width
    }
    .min(if model.confirming { 84 } else { 110 });
    let roomy = main.height >= 16 && width >= 40;
    let padding = Margin::new(if roomy { 3 } else { 1 }, if roomy { 2 } else { 1 });
    let content_width = width.saturating_sub(padding.horizontal * 2);
    let lines = |text: &str| {
        crate::management_wrapped_lines(text, content_width)
            .len()
            .min(u16::MAX as usize) as u16
    };
    let description_height = lines(&model.description).min(if model.confirming { 8 } else { 3 });
    let status_height = if model.confirming || model.finished {
        lines(&model.status).min(3)
    } else {
        // Keep the PTY size stable as progress messages and input focus change.
        2
    };
    let hint_height = if model.confirming || model.finished {
        lines(&auto_admin_hint(model, content_width)).min(3)
    } else {
        lines(&i18n::tr!("aa-terminal-hint"))
            .max(lines(&i18n::tr!("aa-buttons-hint")))
            .min(3)
    };
    let terminal_height = if model.confirming {
        0
    } else if model.finished {
        // Completed/denied requests need only the rows that contain output.
        // The backing PTY retains its full size and scrollback for review.
        model
            .terminal
            .cells
            .chunks(usize::from(model.terminal.columns.max(1)))
            .rposition(|row| row.iter().any(|cell| !cell.symbol.trim().is_empty()))
            .map_or(0, |row| (row + 1).min(20) as u16)
    } else {
        20
    };
    let gap = u16::from(roomy);
    let max_height = if main.height > 20 {
        main.height.saturating_sub(main.height / 5)
    } else {
        main.height
    };
    let height = (padding.vertical * 2
        + description_height
        + terminal_height
        + status_height
        + hint_height
        + gap * 2
        + 1)
    .min(max_height)
    .min(36);
    let dialog = Rect::new(
        main.x + main.width.saturating_sub(width) / 2,
        main.y + main.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let inner = dialog.inner(padding);
    let button_height = inner.height.min(1);
    let hint_height = hint_height.min(inner.height.saturating_sub(3 + gap * 2));
    let status_height = status_height.min(
        inner
            .height
            .saturating_sub(button_height + hint_height + 1 + gap * 2),
    );
    let body_height = inner
        .height
        .saturating_sub(button_height + hint_height + status_height + gap * 2);
    let description_height = description_height.min(body_height);
    let areas = Layout::vertical([
        Constraint::Length(description_height),
        Constraint::Length(gap),
        Constraint::Length(body_height.saturating_sub(description_height)),
        Constraint::Length(status_height),
        Constraint::Length(hint_height),
        Constraint::Length(gap),
        Constraint::Length(button_height),
    ])
    .split(inner);
    let count = if model.confirming {
        2
    } else if model.finished {
        1
    } else {
        4
    };
    let button_gap = if inner.width >= 40 { 2 } else { 0 };
    let button_width = inner.width.saturating_sub(button_gap * (count - 1)) / count;
    let button_areas = Layout::horizontal(vec![
        Constraint::Length(button_width.min(18));
        count as usize
    ])
    .flex(Flex::Center)
    .spacing(button_gap)
    .split(areas[6]);
    AutoAdminLayout {
        dialog,
        description: areas[0],
        terminal: areas[2],
        status: areas[3],
        input: areas[4],
        buttons: std::array::from_fn(|i| button_areas.get(i).copied().unwrap_or_default()),
    }
}

fn auto_admin_hint(model: &AutoAdminViewModel, width: u16) -> String {
    if !model.confirming && !model.finished && model.button_focus.is_some() {
        i18n::tr!(if width < 80 {
            "aa-buttons-hint-compact"
        } else {
            "aa-buttons-hint"
        })
    } else {
        model.input.clone().unwrap_or_else(|| {
            i18n::tr!(if model.confirming {
                "aa-confirm-hint"
            } else if model.finished {
                "aa-finished-hint"
            } else {
                "aa-terminal-hint"
            })
        })
    }
}

fn wrapped_paragraph(text: &str, width: u16) -> Paragraph<'static> {
    Paragraph::new(crate::management_wrapped_lines(text, width).join("\n"))
}

pub fn render_auto_admin(
    frame: &mut Frame<'_>,
    bounds: Rect,
    model: &AutoAdminViewModel,
    context: &RenderContext,
) {
    let layout = auto_admin_layout(bounds, model);
    frame.render_widget(Clear, layout.dialog);
    Surface::new()
        .titled("AutoAdmin (AA)")
        .bordered(true)
        .raised(true)
        .render_frame(frame, layout.dialog, context);
    render_auto_admin_contents(frame, &layout, model, context, None);
}

pub(super) fn render_auto_admin_contents(
    frame: &mut Frame<'_>,
    layout: &AutoAdminLayout,
    model: &AutoAdminViewModel,
    context: &RenderContext,
    hint_override: Option<&str>,
) {
    frame.render_widget(
        wrapped_paragraph(&model.description, layout.description.width)
            .scroll((if model.confirming { model.scroll } else { 0 }, 0)),
        layout.description,
    );
    if !model.confirming {
        super::command_line::render_terminal_snapshot(
            frame,
            layout.terminal,
            &model.terminal,
            &context.compatibility_theme(),
        );
    }
    frame.render_widget(
        wrapped_paragraph(&model.status, layout.status.width),
        layout.status,
    );
    let hint = hint_override
        .map(str::to_owned)
        .unwrap_or_else(|| auto_admin_hint(model, layout.input.width));
    frame.render_widget(wrapped_paragraph(&hint, layout.input.width), layout.input);
    let labels = if model.confirming {
        vec![
            (
                "aa.approve",
                i18n::tr!("aa-approve"),
                model.approve_selected,
            ),
            ("aa.deny", i18n::tr!("aa-deny"), !model.approve_selected),
        ]
    } else if model.finished {
        vec![("aa.close", i18n::tr!("aa-close"), true)]
    } else {
        vec![
            ("aa.y", "y".into(), model.button_focus == Some(0)),
            ("aa.n", "n".into(), model.button_focus == Some(1)),
            ("aa.enter", "Enter".into(), model.button_focus == Some(2)),
            (
                "aa.close",
                i18n::tr!("aa-background"),
                model.button_focus == Some(3),
            ),
        ]
    };
    let theme = context.compatibility_theme();
    for ((id, label, focused), area) in labels.into_iter().zip(layout.buttons) {
        let mut button = Button::new(id, label);
        button.set_focused(focused);
        button.render_borderless_frame(frame, area, &theme);
    }
}
