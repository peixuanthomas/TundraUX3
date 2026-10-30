use super::*;
use crate::components::{Button, ButtonEmphasis, Scrollbar, Surface};
use crate::{RenderContext, render_editor_overlay};
use ratatui::{
    Frame,
    style::{Modifier, Style},
    text::Line,
    widgets::{Clear, Paragraph},
};

pub fn render_script_contextual(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ScriptViewModel,
    context: &RenderContext,
) -> ScriptLayout {
    render_script_contextual_with_terminal_images(
        frame,
        area,
        model,
        context,
        &mut crate::TerminalImageCache::default(),
        &mut crate::TerminalImageCache::default(),
        None,
    )
}

pub fn render_script_contextual_with_terminal_images(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ScriptViewModel,
    context: &RenderContext,
    editor_images: &mut crate::TerminalImageCache,
    output_images: &mut crate::TerminalImageCache,
    picker: Option<&crate::EditorImagePicker>,
) -> ScriptLayout {
    let layout = script_layout(area, model);
    let theme = context.compatibility_theme();
    frame.render_widget(Clear, area);
    if model
        .detail_expansion
        .is_some_and(|amount| amount > 0 && amount < 1_000)
    {
        editor_images.clear();
        output_images.clear();
        Surface::new()
            .bordered(true)
            .render_frame(frame, layout.editor_area, context);
        for (_, rect) in &layout.panes {
            Surface::new()
                .bordered(true)
                .render_frame(frame, *rect, context);
        }
        return layout;
    }
    crate::render_editor_contextual_with_terminal_images(
        frame,
        layout.editor_area,
        &model.decorated_editor(),
        context,
        editor_images,
        picker,
    );
    for (pane, rect) in &layout.panes {
        Surface::new()
            .bordered(true)
            .titled(pane.title())
            .focused(model.focus == *pane)
            .render_frame(frame, *rect, context);
    }
    if !layout.output_area.is_empty() {
        if let Some(output) = &model.output {
            crate::screens::command_line::render_native_terminal_snapshot_with_images(
                frame,
                layout.output_area,
                output,
                &theme,
                model.focus == ScriptPane::Output,
                output_images,
                picker,
            );
        } else {
            output_images.clear();
            frame.render_widget(
                Paragraph::new(i18n::tr!("script-no-output")).style(theme.muted_style()),
                layout.output_area,
            );
        }
    } else {
        output_images.clear();
    }
    let variables = model.variable_rows();
    for (pane, index, rect) in &layout.rows {
        let selected = match pane {
            ScriptPane::Variables => model.selected_variable == *index,
            ScriptPane::Stack => model.selected_stack == *index,
            ScriptPane::Breakpoints => model.selected_breakpoint == *index,
            _ => false,
        };
        let text = match pane {
            ScriptPane::Variables => variables.get(*index).map(|row| {
                let symbol = if row.variable.children.is_empty() {
                    " "
                } else if model.expanded_variables.contains(&row.key) {
                    "v"
                } else {
                    ">"
                };
                format!(
                    "{}{} {} = {}",
                    "  ".repeat(row.depth),
                    symbol,
                    row.variable.name,
                    row.variable.value
                )
            }),
            ScriptPane::Stack => model.state.stack.get(*index).map(|row| {
                format!(
                    "{} {}:{} {}",
                    row.index,
                    row.name,
                    row.line,
                    row.path
                        .file_name()
                        .unwrap_or(row.path.as_os_str())
                        .to_string_lossy()
                )
            }),
            ScriptPane::Breakpoints => model.state.breakpoints.get(*index).map(|row| {
                format!(
                    "{} {} {} {}",
                    if row.enabled { "[x]" } else { "[ ]" },
                    row.line,
                    if !row.enabled {
                        String::new()
                    } else if row.rejected {
                        i18n::tr!("script-breakpoint-rejected")
                    } else if row.verified {
                        i18n::tr!("script-breakpoint-verified")
                    } else {
                        i18n::tr!("script-breakpoint-unverified")
                    },
                    row.path
                        .file_name()
                        .unwrap_or(row.path.as_os_str())
                        .to_string_lossy()
                )
            }),
            ScriptPane::Expression => layout.expression_lines.get(*index).cloned(),
            _ => None,
        };
        if let Some(text) = text {
            let text = crate::terminal_safe_text(&text).into_owned();
            if *pane == ScriptPane::Expression {
                frame.render_widget(Paragraph::new(text).style(theme.body_style()), *rect);
            } else {
                let mut button = Button::new(format!("script.row.{pane:?}.{index}"), text)
                    .with_bracketed_label(false)
                    .with_centered_label(false)
                    .with_current(selected);
                button.state.focused = selected && model.focus == *pane;
                button.render_borderless_frame(frame, *rect, &theme);
            }
        }
    }
    for (pane, rect) in &layout.tabs {
        Button::new(format!("script.tab.{pane:?}"), pane.title())
            .with_current(model.detail == *pane)
            .render_borderless_frame(frame, *rect, &theme);
    }
    for (pane, scrollbar) in &layout.scrollbars {
        let (offset, count, capacity) = match pane {
            ScriptPane::Variables => (
                model.scrolls[pane.index()],
                variables.len(),
                scrollbar.track.height,
            ),
            ScriptPane::Stack => (
                model.scrolls[pane.index()],
                model.state.stack.len(),
                scrollbar.track.height,
            ),
            ScriptPane::Expression => (
                model.scrolls[pane.index()],
                layout.expression_lines.len(),
                scrollbar.track.height,
            ),
            ScriptPane::Breakpoints => (
                model.scrolls[pane.index()],
                model.state.breakpoints.len(),
                scrollbar.track.height,
            ),
            ScriptPane::Output => model.output.as_ref().map_or((0, 0, 0), |output| {
                (
                    output
                        .scrollback_rows
                        .saturating_sub(output.scrollback_offset),
                    output.scrollback_rows + usize::from(output.rows),
                    scrollbar.track.height,
                )
            }),
            ScriptPane::Source => (0, 0, 0),
        };
        Scrollbar::new(count, usize::from(capacity), offset).render_frame(
            frame,
            scrollbar.track,
            context,
        );
    }
    primary_action_bar(model.state.status, model.editor.dirty)
        .with_selected(
            model
                .selected_action
                .map(|action| format!("action.{action:?}").into()),
        )
        .render(frame, area, context);
    for (action, rect) in &layout.actions {
        if rect.is_empty() || primary_actions(model.state.status).contains(action) {
            continue;
        }
        let mut button = Button::new(
            format!("script.action.{action:?}"),
            action.label(model.editor.dirty),
        );
        button.state.disabled = !model.action_enabled(*action);
        button.state.focused = model.selected_action == Some(*action);
        button.emphasis = match action {
            ScriptAction::Stop => ButtonEmphasis::Danger,
            ScriptAction::Run | ScriptAction::Debug | ScriptAction::Continue => {
                ButtonEmphasis::Primary
            }
            _ => ButtonEmphasis::Normal,
        };
        button.render_borderless_frame(frame, *rect, &theme);
    }
    if !layout.expression_input.is_empty() {
        let mut input = model
            .expression
            .clone()
            .with_capture_name(i18n::tr!("script-expression"))
            .with_placeholder(i18n::tr!("script-expression-placeholder"));
        input.set_focused(model.focus == ScriptPane::Expression && model.selected_action.is_none());
        input.state.disabled = model.state.status != ScriptStatus::Paused;
        input.render_borderless_frame_with_prefix(frame, layout.expression_input, &theme, "> ");
    }
    let mut status = i18n::tr!(match model.state.status {
        ScriptStatus::Idle => "script-status-idle",
        ScriptStatus::Saving => "script-status-saving",
        ScriptStatus::Starting => "script-status-starting",
        ScriptStatus::Running => "script-status-running",
        ScriptStatus::Paused => "script-status-paused",
        ScriptStatus::Stopping => "script-status-stopping",
        ScriptStatus::Finished => "script-status-finished",
        ScriptStatus::Failed => "script-status-failed",
    });
    if let Some(location) = &model.state.location {
        status.push_str(&format!("  {}:{}", location.path.display(), location.line));
    }
    if let Some(code) = model.state.exit_code {
        status.push_str(&format!("  {}", i18n::tr!("script-exit-code", code = code)));
    }
    if let Some(error) = &model.state.error {
        status.push_str(&format!("  {}", error.render_current()));
    }
    status = crate::terminal_safe_text(&status).into_owned();
    frame.render_widget(
        Paragraph::new(Line::from(status)).style(
            Style::default()
                .fg(context.theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        layout.status,
    );
    layout
}

pub fn render_script_overlay(
    frame: &mut Frame<'_>,
    layout: &ScriptLayout,
    model: &ScriptViewModel,
    context: &RenderContext,
) {
    let editor_model = model.decorated_editor();
    let mut editor_layout = layout.editor.clone();
    editor_layout.area = layout.area;
    editor_layout.settings = crate::editor_layout(layout.area, &editor_model).settings;
    render_editor_overlay(frame, &editor_layout, &editor_model, context);
}
