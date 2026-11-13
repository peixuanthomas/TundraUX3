use crate::{
    CommandLineTerminalSnapshot, RenderContext,
    components::{Button, ButtonEmphasis, DialogLayout},
};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    widgets::Paragraph,
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AutoAdminStopState {
    #[default]
    None,
    Waiting,
    Warning,
    Killing,
}

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
    pub input_editing: crate::components::TextEditState,
    pub input_sensitive: bool,
    pub terminal: Arc<CommandLineTerminalSnapshot>,
    pub stop: AutoAdminStopState,
    pub can_kill: bool,
    pub can_stop: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AutoAdminLayout {
    pub dialog: Rect,
    pub description: Rect,
    pub terminal: Rect,
    pub terminal_scrollbar: Option<crate::CommandLineScrollbarLayout>,
    pub status: Rect,
    pub input: Rect,
    pub buttons: [Rect; 4],
}

/// Receives the full Shell bounds so drawing, hit testing and PTY sizing all
/// reserve the same fixed title/status bars, even immediately after a resize.
pub fn auto_admin_layout(bounds: Rect, model: &AutoAdminViewModel) -> AutoAdminLayout {
    auto_admin_layout_in(
        crate::ShellFrameLayout::new(bounds, None, &RenderContext::default()).main,
        model,
    )
}

/// The compositor reserves the keyboard before laying out the real AA dialog.
pub fn auto_admin_layout_in(main: Rect, model: &AutoAdminViewModel) -> AutoAdminLayout {
    authorization_layout(main, model).0
}

pub fn auto_admin_max_scroll(bounds: Rect, model: &AutoAdminViewModel) -> u16 {
    let layout = auto_admin_layout(bounds, model);
    crate::management_wrapped_lines(&description(model), layout.description.width)
        .len()
        .saturating_sub(usize::from(layout.description.height))
        .min(u16::MAX as usize) as u16
}

fn authorization_layout(
    main: Rect,
    model: &AutoAdminViewModel,
) -> (AutoAdminLayout, Rect, DialogLayout) {
    let stopping = model.stop == AutoAdminStopState::Warning;
    let width = main
        .width
        .saturating_sub(u16::from(main.width >= 50) * 4)
        .min(100);
    let content_width = DialogLayout::content_width(width);
    let warning_height = u16::from(model.confirming && main.height >= 14);
    let stacked_warning = stopping && content_width < 36;
    let action_height = if stacked_warning {
        2
    } else {
        u16::from(model.confirming || model.finished || model.can_stop || stopping)
    };
    let lines = |text: &str| {
        crate::management_wrapped_lines(text, content_width)
            .len()
            .min(u16::MAX as usize) as u16
    };
    let description_height = lines(&description(model)).min(if stopping {
        14
    } else if model.confirming {
        8
    } else {
        3
    });
    let status_height = if model.confirming || model.finished || stopping {
        lines(&model.status).min(3)
    } else {
        // Keep the PTY size stable as progress messages and input focus change.
        2
    };
    let hint = auto_admin_hint(model);
    let hint_height = if hint.is_empty() {
        0
    } else {
        lines(&hint).min(3)
    };
    let terminal_height = if model.confirming || stopping {
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
    let gap = u16::from(main.height >= 20 && width >= 40);
    let max_height = if main.height > 20 {
        main.height.saturating_sub(main.height / 5)
    } else {
        main.height
    };
    let height = (7
        + warning_height
        + description_height
        + terminal_height
        + status_height
        + hint_height
        + gap * 2
        + action_height)
        .min(max_height)
        .min(36);
    let dialog = Rect::new(
        main.x + main.width.saturating_sub(width) / 2,
        main.y + main.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let chrome = DialogLayout::new(dialog, action_height, None);
    let mut inner = chrome.body;
    let warning_height = warning_height.min(inner.height.saturating_sub(1));
    let warning = Rect::new(inner.x, inner.y, inner.width, warning_height);
    inner.y += warning_height;
    inner.height = inner.height.saturating_sub(warning_height);
    let hint_height = hint_height.min(inner.height.saturating_sub(3 + gap * 2));
    let status_height = status_height.min(inner.height.saturating_sub(hint_height + 1 + gap * 2));
    let body_height = inner
        .height
        .saturating_sub(hint_height + status_height + gap);
    let description_height = description_height.min(body_height);
    let areas = Layout::vertical([
        Constraint::Length(description_height),
        Constraint::Length(gap),
        Constraint::Length(
            body_height
                .saturating_sub(description_height)
                .min(terminal_height),
        ),
        Constraint::Length(status_height),
        Constraint::Length(hint_height),
    ])
    .split(inner);
    let count = if model.confirming || stopping { 2 } else { 1 };
    let button_gap = if inner.width >= 40 { 2 } else { 0 };
    let button_width = inner.width.saturating_sub(button_gap * (count - 1)) / count;
    let button_areas = Layout::horizontal(vec![
        Constraint::Length(button_width.min(34));
        count as usize
    ])
    .flex(Flex::End)
    .spacing(button_gap)
    .split(Rect {
        height: chrome.footer.height.min(1),
        ..chrome.footer
    });
    let mut buttons = std::array::from_fn(|i| button_areas.get(i).copied().unwrap_or_default());
    if !model.confirming && !model.finished && !stopping {
        buttons[1] = if model.can_stop {
            buttons[0]
        } else {
            Rect::default()
        };
        buttons[0] = Rect::default();
    }
    if stacked_warning {
        let action_width = inner.width.min(24);
        let area = Rect::new(
            inner.right().saturating_sub(action_width),
            chrome.footer.y,
            action_width,
            1.min(chrome.footer.height),
        );
        buttons[0] = area;
        buttons[1] = Rect {
            y: area.y + u16::from(chrome.footer.height > 1),
            ..area
        };
    }
    let (terminal, terminal_scrollbar) = auto_admin_terminal_layout(areas[2], model);
    (
        AutoAdminLayout {
            dialog,
            description: areas[0],
            terminal,
            terminal_scrollbar,
            status: areas[3],
            input: areas[4],
            buttons,
        },
        warning,
        chrome,
    )
}

pub(super) fn auto_admin_terminal_layout(
    area: Rect,
    model: &AutoAdminViewModel,
) -> (Rect, Option<crate::CommandLineScrollbarLayout>) {
    // Reserve a column before history arrives so the running PTY keeps its width.
    let terminal = Rect {
        width: area.width.saturating_sub(1),
        ..area
    };
    let scrollbar = if model.confirming || model.stop == AutoAdminStopState::Warning {
        None
    } else {
        crate::command_line_scrollbar_layout(area, &model.terminal)
    };
    (terminal, scrollbar)
}

fn auto_admin_hint(model: &AutoAdminViewModel) -> String {
    if model.stop == AutoAdminStopState::Warning {
        return i18n::tr!("aa-stop-warning-hint");
    }
    model.input.clone().unwrap_or_else(|| {
        if model.finished {
            i18n::tr!("aa-finished-hint")
        } else {
            String::new()
        }
    })
}

fn description(model: &AutoAdminViewModel) -> String {
    if model.stop == AutoAdminStopState::Warning {
        let mut text = i18n::tr!("aa-stop-warning");
        if !model.can_kill {
            text.push_str(&format!("\n{}", i18n::tr!("aa-stop-no-process")));
        }
        text
    } else {
        model.description.clone()
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
    let main = crate::ShellFrameLayout::new(bounds, None, context).main;
    render_auto_admin_in(frame, bounds, main, model, context);
}

pub fn render_auto_admin_in(
    frame: &mut Frame<'_>,
    _bounds: Rect,
    main: Rect,
    model: &AutoAdminViewModel,
    context: &RenderContext,
) {
    let (layout, warning, chrome) = authorization_layout(main, model);
    chrome.render_frame(frame, &i18n::tr!("aa-title"), context);
    if !warning.is_empty() {
        frame.render_widget(
            Paragraph::new(i18n::tr!("aa-preview-warning")).style(
                Style::default()
                    .fg(context.theme.warning)
                    .add_modifier(Modifier::BOLD),
            ),
            warning,
        );
    }
    render_auto_admin_contents(frame, &layout, model, context, None);
}

pub(super) fn render_auto_admin_contents(
    frame: &mut Frame<'_>,
    layout: &AutoAdminLayout,
    model: &AutoAdminViewModel,
    context: &RenderContext,
    hint_override: Option<&str>,
) {
    let _scope = crate::automation::scope("aa", "dialog", "AutoAdmin", layout.dialog);
    frame.render_widget(
        wrapped_paragraph(&description(model), layout.description.width)
            .style(
                Style::default().fg(if model.stop == AutoAdminStopState::Warning {
                    context.theme.danger
                } else {
                    context.theme.text
                }),
            )
            .scroll((
                if model.confirming || model.stop == AutoAdminStopState::Warning {
                    model.scroll
                } else {
                    0
                },
                0,
            )),
        layout.description,
    );
    if !model.confirming && model.stop != AutoAdminStopState::Warning {
        super::command_line::render_terminal_snapshot(
            frame,
            layout.terminal,
            &model.terminal,
            &context.compatibility_theme(),
        );
        if let Some(bar) = layout.terminal_scrollbar {
            let viewport = usize::from(model.terminal.rows.min(layout.terminal.height)).max(1);
            crate::components::Scrollbar::new(
                model.terminal.scrollback_rows.saturating_add(viewport),
                viewport,
                model
                    .terminal
                    .scrollback_rows
                    .saturating_sub(model.terminal.scrollback_offset),
            )
            .render_frame(frame, bar.track, context);
        }
    }
    frame.render_widget(
        wrapped_paragraph(&model.status, layout.status.width)
            .style(Style::default().fg(context.theme.text)),
        layout.status,
    );
    let hint = hint_override
        .map(str::to_owned)
        .unwrap_or_else(|| auto_admin_hint(model));
    if let Some(value) = &model.input
        && model.stop != AutoAdminStopState::Warning
    {
        let mut input = model
            .input_editing
            .input("aa.input", value.strip_prefix("> ").unwrap_or(value))
            .with_sensitive(model.input_sensitive);
        input.set_focused(model.button_focus.is_none());
        input.render_borderless_frame_with_prefix(
            frame,
            layout.input,
            &DialogLayout::body_theme(context),
            "> ",
        );
    } else {
        frame.render_widget(
            wrapped_paragraph(&hint, layout.input.width)
                .style(Style::default().fg(context.theme.muted)),
            layout.input,
        );
    }
    let labels = if model.stop == AutoAdminStopState::Warning {
        vec![
            (
                "aa.wait",
                i18n::tr!("aa-stop-wait"),
                model.button_focus == Some(0),
            ),
            (
                "aa.kill",
                i18n::tr!("aa-stop-kill"),
                model.button_focus == Some(1),
            ),
        ]
    } else if model.confirming {
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
        let mut labels = vec![];
        if model.can_stop {
            labels.push((
                "aa.stop",
                i18n::tr!("aa-stop"),
                model.button_focus == Some(1),
            ));
        }
        labels
    };
    let theme = DialogLayout::footer_theme(context);
    let first = usize::from(
        !model.confirming && !model.finished && model.stop != AutoAdminStopState::Warning,
    );
    for ((id, label, focused), area) in labels
        .into_iter()
        .zip(layout.buttons.into_iter().skip(first))
    {
        let mut button = Button::new(id, label);
        if id == "aa.kill" {
            button = button.with_emphasis(ButtonEmphasis::Danger);
        }
        button.set_disabled(!match id {
            "aa.kill" => model.can_kill,
            "aa.stop" => model.stop == AutoAdminStopState::None,
            _ => true,
        });
        button.set_focused(focused);
        button.render_inline_frame(frame, area, &theme);
    }
}
