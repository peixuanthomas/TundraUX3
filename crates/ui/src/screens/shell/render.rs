use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::centered_rect;
use super::{
    ExitConfirmViewModel, ShellChromeViewModel, ShellFrameLayout, ShellLayout,
    TimeSyncDialogViewModel,
};
use crate::components::{Button, Surface, terminal_width, truncate_to_terminal_width};
use crate::screens::notifications::{notification_tone_prefix, notification_tone_style};
use crate::{RenderContext, TundraTheme};

const STATUS_TIME_BUTTON_HORIZONTAL_CHROME: u16 = 4;
const STATUS_TIME_BUTTON_MIN_WIDTH: u16 = 3;
const STATUS_TIME_BUTTON_RESERVED_LEFT_WIDTH: u16 = 12;
fn compact_terminal_message() -> String {
    i18n::tr!("ui-shell-tundraux-3-needs-at-least-50x12-terminal-cells")
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
    let mut notification = crate::NotificationViewModel::new(
        "shell.exit",
        crate::NotificationLevel::Modal,
        crate::NotificationTone::Warning,
        &model.title,
        &model.message,
        vec![
            crate::NotificationActionViewModel::new("exit", &model.confirm_label),
            crate::NotificationActionViewModel::new("restart", &model.restart_label),
            crate::NotificationActionViewModel::new("cancel", &model.cancel_label),
        ],
    );
    notification.stacked_actions = true;
    crate::screens::notifications::render_notification_overlay_context(
        frame,
        area,
        &notification,
        context,
    );
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
        .titled(i18n::tr!("ui-shell-time-sync"))
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

pub fn render_compact_home(
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
        let size_message = truncate_status_text(&compact_terminal_message(), inner.width);
        frame.render_widget(
            Paragraph::new(size_message)
                .style(theme.muted_style())
                .alignment(HorizontalAlignment::Center),
            Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1),
        );
    }
}

/// Draw the shell-owned surfaces once, after application overlays and before shell modals.
pub fn render_shell_chrome(
    frame: &mut Frame<'_>,
    layout: &ShellFrameLayout,
    chrome: &ShellChromeViewModel,
    context: &RenderContext,
) {
    let ShellLayout::Full { top, status, .. } = layout.shell else {
        return;
    };
    let title_area = Rect::new(
        top.x,
        top.y,
        layout
            .back_button
            // Share the separator with the button instead of drawing two borders.
            .map_or(top.width, |button| {
                button.x.saturating_sub(top.x).saturating_add(1)
            }),
        top.height,
    );
    render_top(frame, title_area, chrome, context);
    if let Some(area) = layout.back_button {
        let mut button = Button::new("shell.back", crate::assets::BACK_ICON.trim());
        button.state.hovered = chrome.back_button_hovered;
        button.render_frame(frame, area, &context.compatibility_theme());
    }
    render_status(frame, status, layout, chrome, context);
}

fn render_top(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    context: &RenderContext,
) {
    let theme = context.compatibility_theme();
    let surface = Surface::new().bordered(true);
    let inner = surface.inner(area);
    surface.render_frame(frame, area, context);
    if inner.is_empty() {
        return;
    }
    let title = truncate_status_text(&chrome.app_name, inner.width);
    let title_width = u16::try_from(terminal_width(&title))
        .unwrap_or(inner.width)
        .min(inner.width);
    frame.render_widget(
        Paragraph::new(Line::styled(title, theme.title_style())),
        Rect::new(inner.x, inner.y, title_width, 1),
    );
    let info_width = inner.width.saturating_sub(title_width.saturating_add(2));
    if info_width == 0 {
        return;
    }
    let stack = if chrome.screen_stack.is_empty() {
        i18n::tr!("ui-shell-home")
    } else {
        chrome.screen_stack.join(" > ")
    };
    let info = format!(
        "{} | {:?} | {}x{} | {}",
        chrome.build_mode,
        chrome.display_mode,
        chrome.terminal_size.0,
        chrome.terminal_size.1,
        stack
    );
    frame.render_widget(
        Paragraph::new(Line::styled(
            truncate_status_text(&info, info_width),
            theme.muted_style(),
        ))
        .alignment(HorizontalAlignment::Right),
        Rect::new(
            inner.right().saturating_sub(info_width),
            inner.y,
            info_width,
            1,
        ),
    );
}

fn render_status(
    frame: &mut Frame<'_>,
    area: Rect,
    layout: &ShellFrameLayout,
    chrome: &ShellChromeViewModel,
    context: &RenderContext,
) {
    if area.is_empty() {
        return;
    }
    let theme = context.compatibility_theme();
    let surface = Surface::new()
        .titled(i18n::tr!("ui-shell-status"))
        .bordered(true);
    surface.render_frame(frame, area, context);
    if let Some(message) = layout.status_message {
        let inner = surface.inner(message);
        if !inner.is_empty() {
            let (text, style) = status_presentation(&chrome.status, &theme);
            frame.render_widget(
                Paragraph::new(Line::styled(
                    truncate_status_text(&text, inner.width),
                    style,
                )),
                inner,
            );
        }
    }
    if let (Some(label), Some(area)) = (&chrome.status.time_button_label, layout.time_button) {
        render_status_time_button(
            frame,
            area,
            label,
            chrome.status.time_button_selected,
            &theme,
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
#[path = "../../../tests/unit/screens/shell/render/tests.rs"]
mod tests;
