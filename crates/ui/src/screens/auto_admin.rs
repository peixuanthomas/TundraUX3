use crate::{
    CommandLineTerminalSnapshot, RenderContext,
    components::{Button, Surface},
};
use ratatui::{
    Frame,
    layout::{Margin, Rect},
    widgets::{Clear, Paragraph, Wrap},
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
pub fn auto_admin_layout(bounds: Rect, confirming: bool) -> AutoAdminLayout {
    let main = match crate::compute_shell_layout(bounds) {
        crate::ShellLayout::Full { main, .. } | crate::ShellLayout::Compact(main) => main,
    };
    let width = if main.width > 80 {
        main.width.saturating_sub(main.width / 10)
    } else {
        main.width
    }
    .min(140);
    let height = if main.height > 20 {
        main.height.saturating_sub(main.height / 5)
    } else {
        main.height
    }
    .min(36);
    let dialog = Rect::new(
        main.x + main.width.saturating_sub(width) / 2,
        main.y + main.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let inner = dialog.inner(Margin::new(1, 1));
    let footer_height = inner.height.min(3);
    let body_height = inner.height.saturating_sub(footer_height);
    let description_height = if confirming {
        body_height
    } else {
        body_height.min(3)
    };
    let description = Rect::new(inner.x, inner.y, inner.width, description_height);
    let terminal = Rect::new(
        inner.x,
        inner.y + description_height,
        inner.width,
        body_height.saturating_sub(description_height),
    );
    let footer = inner.y + body_height;
    let width = inner.width / 4;
    AutoAdminLayout {
        dialog,
        description,
        terminal,
        status: Rect::new(inner.x, footer, inner.width, u16::from(footer_height > 0)),
        input: Rect::new(
            inner.x,
            footer + u16::from(footer_height > 0),
            inner.width,
            u16::from(footer_height > 1),
        ),
        buttons: std::array::from_fn(|i| {
            Rect::new(
                inner.x + width * i as u16,
                footer + footer_height.saturating_sub(1),
                if i == 3 {
                    inner.width - width * 3
                } else {
                    width
                },
                u16::from(footer_height > 2),
            )
        }),
    }
}

pub fn render_auto_admin(
    frame: &mut Frame<'_>,
    bounds: Rect,
    model: &AutoAdminViewModel,
    context: &RenderContext,
) {
    let layout = auto_admin_layout(bounds, model.confirming);
    frame.render_widget(Clear, layout.dialog);
    Surface::new()
        .titled("AutoAdmin (AA)")
        .bordered(true)
        .raised(true)
        .render_frame(frame, layout.dialog, context);
    frame.render_widget(
        Paragraph::new(model.description.as_str())
            .wrap(Wrap { trim: false })
            .scroll((if model.confirming { model.scroll } else { 0 }, 0)),
        layout.description,
    );
    if !model.confirming {
        super::command_line::render_terminal_snapshot(frame, layout.terminal, &model.terminal);
    }
    frame.render_widget(Paragraph::new(model.status.as_str()), layout.status);
    let hint = if !model.confirming && !model.finished && model.button_focus.is_some() {
        i18n::tr!(if layout.input.width < 80 {
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
    };
    frame.render_widget(Paragraph::new(hint), layout.input);
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
