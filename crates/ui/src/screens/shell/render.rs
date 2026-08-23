use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::centered_rect;
use super::{
    ExitConfirmViewModel, ShellChromeViewModel, ShellLayout, TimeSyncDialogViewModel,
    compute_shell_layout,
};
use crate::components::{Button, Surface, terminal_width, truncate_to_terminal_width};
use crate::screens::notifications::{notification_tone_prefix, notification_tone_style};
use crate::{RenderContext, TundraTheme};

const STATUS_TIME_BUTTON_HORIZONTAL_CHROME: u16 = 4;
const STATUS_TIME_BUTTON_MIN_WIDTH: u16 = 3;
const STATUS_TIME_BUTTON_RESERVED_LEFT_WIDTH: u16 = 12;
const COMPACT_TERMINAL_MESSAGE: &str = "TundraUX 3 needs at least 50x12 terminal cells.";

pub fn render_editor_app(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    editor: &crate::EditorViewModel,
    theme: &TundraTheme,
) -> crate::EditorLayout {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_editor_app_contextual(frame, area, chrome, editor, &context)
}

pub fn render_editor_app_contextual(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    editor: &crate::EditorViewModel,
    context: &RenderContext,
) -> crate::EditorLayout {
    let theme = &context.compatibility_theme();
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => {
            crate::render_editor_contextual(frame, compact, editor, context)
        }
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_status(frame, status, chrome, theme);
            crate::render_editor_contextual(frame, main, editor, context)
        }
    }
}

pub fn render_exit_confirmation(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExitConfirmViewModel,
    theme: &TundraTheme,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_exit_confirmation_contextual(frame, area, model, &context);
}

pub fn render_exit_confirmation_contextual(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExitConfirmViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    let dialog = centered_rect(area, area.width.min(54), area.height.min(8));
    let surface = Surface::new()
        .titled(model.title.clone())
        .bordered(true)
        .raised(true);
    let inner = surface.inner(dialog);

    frame.render_widget(Clear, dialog);
    surface.render_frame(frame, dialog, context);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(model.message.as_str())
            .alignment(HorizontalAlignment::Center)
            .wrap(Wrap { trim: true }),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    if inner.height > 2 {
        let action_row = Rect::new(inner.x, inner.y.saturating_add(2), inner.width, 1);
        let confirm = Button::new("shell.exit.confirm", model.confirm_label.clone());
        let restart = Button::new("shell.exit.restart", model.restart_label.clone());
        let confirm_width = u16::try_from(confirm.rendered_label_width())
            .unwrap_or(u16::MAX)
            .min(action_row.width);
        let restart_width = u16::try_from(restart.rendered_label_width())
            .unwrap_or(u16::MAX)
            .min(action_row.width);
        let actions_width = confirm_width
            .saturating_add(4)
            .saturating_add(restart_width)
            .min(action_row.width);
        let actions_x = action_row
            .x
            .saturating_add(action_row.width.saturating_sub(actions_width) / 2);

        confirm.render_borderless_frame(
            frame,
            Rect::new(actions_x, action_row.y, confirm_width, 1),
            theme,
        );
        restart.render_borderless_frame(
            frame,
            Rect::new(
                actions_x.saturating_add(confirm_width).saturating_add(4),
                action_row.y,
                restart_width.min(
                    action_row
                        .right()
                        .saturating_sub(actions_x.saturating_add(confirm_width).saturating_add(4)),
                ),
                1,
            ),
            theme,
        );
    }

    if inner.height > 3 {
        let cancel = Button::new("shell.exit.cancel", model.cancel_label.clone());
        let cancel_width = u16::try_from(cancel.rendered_label_width())
            .unwrap_or(u16::MAX)
            .min(inner.width);
        cancel.render_borderless_frame(
            frame,
            Rect::new(
                inner
                    .x
                    .saturating_add(inner.width.saturating_sub(cancel_width) / 2),
                inner.y.saturating_add(3),
                cancel_width,
                1,
            ),
            theme,
        );
    }
}

pub fn render_time_sync_failure_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &TimeSyncDialogViewModel,
    theme: &TundraTheme,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_time_sync_failure_dialog_contextual(frame, area, model, &context);
}

pub fn render_time_sync_failure_dialog_contextual(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &TimeSyncDialogViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    let dialog = centered_rect(area, area.width.min(34), area.height.min(5));
    let surface = Surface::new()
        .titled("Time Sync")
        .bordered(true)
        .raised(true);
    let inner = surface.inner(dialog);
    let dialog_widget = Paragraph::new(Line::from(model.message()))
        .style(theme.error_style())
        .alignment(HorizontalAlignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, dialog);
    let mut danger_context = *context;
    danger_context.theme.border = context.theme.danger;
    surface.render_frame(frame, dialog, &danger_context);
    frame.render_widget(dialog_widget, inner);
}

pub(crate) fn render_compact_home(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let (notification, style) = status_presentation(&chrome.status, theme);
    if area.width <= 2 || area.height <= 2 {
        let notification = truncate_status_text(&notification, area.width);
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(Line::styled(notification, style)).alignment(HorizontalAlignment::Left),
            area,
        );
        return;
    }

    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    let surface = Surface::new().titled("TundraUX 3").bordered(true);
    let inner = surface.inner(area);
    surface.render_frame(frame, area, &context);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let notification = truncate_status_text(&notification, inner.width);
    frame.render_widget(
        Paragraph::new(Line::styled(notification, style)).alignment(HorizontalAlignment::Center),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    if inner.height > 1 {
        let size_message = truncate_status_text(COMPACT_TERMINAL_MESSAGE, inner.width);
        frame.render_widget(
            Paragraph::new(size_message)
                .style(theme.muted_style())
                .alignment(HorizontalAlignment::Center),
            Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1),
        );
    }
}

pub(crate) fn render_top(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    theme: &TundraTheme,
) {
    let stack = if chrome.screen_stack.is_empty() {
        "Home".to_string()
    } else {
        chrome.screen_stack.join(" > ")
    };
    let lines = vec![
        Line::styled(chrome.app_name.clone(), theme.title_style()),
        Line::styled(
            format!(
                "{} | {:?} | {}x{} | {}",
                chrome.build_mode,
                chrome.display_mode,
                chrome.terminal_size.0,
                chrome.terminal_size.1,
                stack
            ),
            theme.muted_style(),
        ),
    ];
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    let surface = Surface::new().bordered(true);
    let inner = surface.inner(area);
    surface.render_frame(frame, area, &context);
    frame.render_widget(
        Paragraph::new(lines).alignment(HorizontalAlignment::Left),
        inner,
    );
}

pub(crate) fn render_status(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let time_button = chrome
        .status
        .time_button_label
        .as_ref()
        .map(|label| status_time_button_area(area, label))
        .filter(|area| area.width > 0 && area.height > 0);

    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    let surface = Surface::new().titled("Status").bordered(true);
    let inner = surface.inner(area);
    surface.render_frame(frame, area, &context);
    let left_width = match time_button {
        Some(button) if button.x > inner.x => button.x.saturating_sub(inner.x).saturating_sub(1),
        Some(_) => 0,
        None => inner.width,
    };
    let left_area = Rect::new(inner.x, inner.y, left_width.min(inner.width), inner.height);
    if left_area.width > 0 && left_area.height > 0 {
        let (notification, style) = status_presentation(&chrome.status, theme);
        let notification = truncate_status_text(&notification, left_area.width);
        frame.render_widget(
            Paragraph::new(Line::styled(notification, style))
                .alignment(HorizontalAlignment::Left)
                .style(theme.body_style()),
            left_area,
        );
    }

    if let (Some(label), Some(button_area)) = (&chrome.status.time_button_label, time_button) {
        render_status_time_button(
            frame,
            button_area,
            label,
            chrome.status.time_button_selected,
            theme,
        );
    }
}

pub fn status_time_button_area(status: Rect, label: &str) -> Rect {
    if status.width == 0 || status.height == 0 || label.is_empty() {
        return Rect::new(
            status.x.saturating_add(status.width),
            status.y,
            0,
            status.height,
        );
    }

    let label_width = u16::try_from(terminal_width(label)).unwrap_or(u16::MAX);
    let desired_width = label_width.saturating_add(STATUS_TIME_BUTTON_HORIZONTAL_CHROME);
    let max_width = if status.width
        > STATUS_TIME_BUTTON_RESERVED_LEFT_WIDTH.saturating_add(STATUS_TIME_BUTTON_MIN_WIDTH)
    {
        status
            .width
            .saturating_sub(STATUS_TIME_BUTTON_RESERVED_LEFT_WIDTH)
    } else {
        status.width
    };
    let min_width = STATUS_TIME_BUTTON_MIN_WIDTH.min(max_width);
    let width = desired_width
        .min(max_width)
        .max(min_width)
        .min(status.width);

    Rect::new(
        status.x.saturating_add(status.width.saturating_sub(width)),
        status.y,
        width,
        status.height,
    )
}

fn status_presentation(status: &crate::StatusViewModel, theme: &TundraTheme) -> (String, Style) {
    if let Some(alert) = &status.error {
        return (
            format!("{} {alert}", notification_tone_prefix(status.alert_tone)),
            notification_tone_style(status.alert_tone, theme),
        );
    }
    if let Some(toast) = &status.toast {
        return (toast.clone(), theme.muted_style());
    }
    (status.status.clone(), theme.body_style())
}

fn truncate_status_text(text: &str, width: u16) -> String {
    let text = text
        .chars()
        .map(|character| match character {
            '\r' | '\n' => ' ',
            character => character,
        })
        .collect::<String>();
    let width = usize::from(width);
    if terminal_width(&text) <= width {
        return text;
    }
    if width <= 3 {
        return truncate_to_terminal_width(&text, width);
    }

    let content_width = width.saturating_sub(3);
    let visible = truncate_to_terminal_width(&text, content_width);
    format!("{visible}...")
}

fn render_status_time_button(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    selected: bool,
    theme: &TundraTheme,
) {
    let mut button = Button::new("shell.status.time", label.to_string());
    button.state.selected = selected;

    frame.render_widget(Clear, area);
    button.render_frame(frame, area, theme);
}

#[cfg(test)]
fn text_width(text: &str) -> u16 {
    u16::try_from(terminal_width(text)).unwrap_or(u16::MAX)
}

pub(crate) fn fit_cell(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let text_width = terminal_width(text);
    if text_width <= width {
        let mut fitted = text.to_string();
        fitted.extend(std::iter::repeat_n(' ', width.saturating_sub(text_width)));
        return fitted;
    }

    let content_width = width.saturating_sub(1);
    let mut fitted = truncate_to_terminal_width(text, content_width);
    fitted.push('…');
    let used = terminal_width(&fitted);
    fitted.extend(std::iter::repeat_n(' ', width.saturating_sub(used)));
    fitted
}

#[cfg(test)]
mod tests {
    use super::{fit_cell, text_width, truncate_status_text};
    use ratatui::text::Line;

    #[test]
    fn cell_fitting_and_status_truncation_use_terminal_display_width() {
        assert_eq!(text_width("界面"), 4);
        assert_eq!(fit_cell("界面", 5), "界面 ");
        assert_eq!(fit_cell("界面", 3), "界…");
        assert_eq!(Line::from(fit_cell("界面", 3)).width(), 3);
        assert_eq!(truncate_status_text("界面状态", 5), "界...");
        assert_eq!(Line::from(truncate_status_text("界面状态", 5)).width(), 5);
    }
}
