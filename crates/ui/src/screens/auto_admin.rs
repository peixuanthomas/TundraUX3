use crate::{CommandLineTerminalSnapshot, RenderContext, components::Button};
use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Margin, Rect},
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
    pub status: Rect,
    pub input: Rect,
    pub buttons: [Rect; 4],
}

/// Receives the full Shell bounds so drawing, hit testing and PTY sizing all
/// reserve the same fixed title/status bars, even immediately after a resize.
pub fn auto_admin_layout(bounds: Rect, model: &AutoAdminViewModel) -> AutoAdminLayout {
    authorization_layout(bounds, model).0
}

pub fn auto_admin_max_scroll(bounds: Rect, model: &AutoAdminViewModel) -> u16 {
    let layout = auto_admin_layout(bounds, model);
    crate::management_wrapped_lines(&description(model), layout.description.width)
        .len()
        .saturating_sub(usize::from(layout.description.height))
        .min(u16::MAX as usize) as u16
}

fn authorization_layout(bounds: Rect, model: &AutoAdminViewModel) -> (AutoAdminLayout, Rect) {
    let stopping = model.stop == AutoAdminStopState::Warning;
    let main = match crate::compute_shell_layout(bounds) {
        crate::ShellLayout::Full { main, .. } | crate::ShellLayout::Compact(main) => main,
    };
    let width = main
        .width
        .saturating_sub(u16::from(main.width >= 50) * 4)
        .min(100);
    let roomy = main.height >= 16 && width >= 40;
    let padding = Margin::new(if width >= 50 { 3 } else { 1 }, 1);
    let badge_width = if width >= 70 && !stopping { 18 } else { 0 };
    let content_width = width.saturating_sub(padding.horizontal * 2 + badge_width);
    let warning_height = if stopping {
        0
    } else if roomy {
        3
    } else {
        1
    };
    let stop_row = model.can_stop && !model.confirming && !model.finished && !stopping;
    let stacked_warning = stopping && content_width < 36;
    let action_height = if stop_row || stacked_warning { 2 } else { 1 };
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
    let hint_height = if model.confirming || model.finished || stopping {
        lines(&auto_admin_hint(model, content_width)).min(3)
    } else {
        lines(&i18n::tr!("aa-terminal-hint"))
            .max(lines(&i18n::tr!("aa-buttons-hint")))
            .min(3)
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
    let gap = u16::from(roomy);
    let max_height = if main.height > 20 {
        main.height.saturating_sub(main.height / 5)
    } else {
        main.height
    };
    let height = (padding.vertical * 2
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
    let mut inner = dialog.inner(padding);
    inner.x += badge_width;
    inner.width = inner.width.saturating_sub(badge_width);
    // Reserve actions and at least one description row before the banner on
    // very short terminals. All consumers use this same content rectangle.
    let warning_height = warning_height.min(inner.height.saturating_sub(action_height + 1));
    let warning = Rect::new(inner.x, inner.y, inner.width, warning_height);
    inner.y += warning_height;
    inner.height = inner.height.saturating_sub(warning_height);
    let button_height = inner.height.min(action_height);
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
    let count = if model.confirming || stopping {
        2
    } else if model.finished {
        1
    } else if model.can_stop {
        4
    } else {
        3
    };
    let button_gap = if inner.width >= 40 { 2 } else { 0 };
    let row_count = if stop_row { 3 } else { count };
    let button_width = inner.width.saturating_sub(button_gap * (row_count - 1)) / row_count;
    let button_areas = Layout::horizontal(vec![
        Constraint::Length(button_width.min(18));
        row_count as usize
    ])
    .flex(Flex::Center)
    .spacing(button_gap)
    .split(Rect {
        height: areas[6].height.min(1),
        ..areas[6]
    });
    let mut buttons = std::array::from_fn(|i| button_areas.get(i).copied().unwrap_or_default());
    if stop_row || stacked_warning {
        let action_width = inner.width.min(if stopping { 24 } else { 18 });
        let area = Rect::new(
            inner.x + inner.width.saturating_sub(action_width) / 2,
            areas[6].y,
            action_width,
            1.min(areas[6].height),
        );
        if stop_row {
            buttons[3] = Rect {
                y: area.y + u16::from(areas[6].height > 1),
                ..area
            };
        } else {
            buttons[0] = area;
            buttons[1] = Rect {
                y: area.y + u16::from(areas[6].height > 1),
                ..area
            };
        }
    }
    (
        AutoAdminLayout {
            dialog,
            description: areas[0],
            terminal: areas[2],
            status: areas[3],
            input: areas[4],
            buttons,
        },
        warning,
    )
}

fn auto_admin_hint(model: &AutoAdminViewModel, width: u16) -> String {
    if model.stop == AutoAdminStopState::Warning {
        return i18n::tr!("aa-stop-warning-hint");
    }
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
    let (layout, warning) = authorization_layout(bounds, model);
    super::auto_admin_preview::render_auto_admin_frame(
        frame,
        bounds,
        &layout,
        warning,
        if model.stop == AutoAdminStopState::Warning {
            crate::AutoAdminPreviewStyle::Danger
        } else {
            crate::AutoAdminPreviewStyle::Authorization
        },
        Some(" AutoAdmin (AA) "),
    );
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
        wrapped_paragraph(&description(model), layout.description.width).scroll((
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
    }
    frame.render_widget(
        wrapped_paragraph(&model.status, layout.status.width),
        layout.status,
    );
    let hint = hint_override
        .map(str::to_owned)
        .unwrap_or_else(|| auto_admin_hint(model, layout.input.width));
    frame.render_widget(wrapped_paragraph(&hint, layout.input.width), layout.input);
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
        let mut labels = vec![
            ("aa.y", "y".into(), model.button_focus == Some(0)),
            ("aa.n", "n".into(), model.button_focus == Some(1)),
            ("aa.enter", "Enter".into(), model.button_focus == Some(2)),
        ];
        if model.can_stop {
            labels.push((
                "aa.stop",
                i18n::tr!("aa-stop"),
                model.button_focus == Some(3),
            ));
        }
        labels
    };
    let theme = context.compatibility_theme();
    for ((id, label, focused), area) in labels.into_iter().zip(layout.buttons) {
        let mut button = Button::new(id, label);
        button.set_disabled(!match id {
            "aa.kill" => model.can_kill,
            "aa.stop" => model.stop == AutoAdminStopState::None,
            "aa.y" | "aa.n" | "aa.enter" => model.stop == AutoAdminStopState::None,
            _ => true,
        });
        button.set_focused(focused);
        button.render_borderless_frame(frame, area, &theme);
    }
}
