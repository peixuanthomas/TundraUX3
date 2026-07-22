use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Borders, Clear, Paragraph, Wrap};

use super::centered_rect;
use super::{
    ExitConfirmViewModel, ShellChromeViewModel, ShellLayout, TimeSyncDialogViewModel,
    compute_shell_layout,
};
use crate::TundraTheme;
use crate::screens::notifications::{notification_tone_prefix, notification_tone_style};
use crate::theme::solid_border_style;

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
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => crate::render_editor(frame, compact, editor, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_status(frame, status, chrome, theme);
            crate::render_editor(frame, main, editor, theme)
        }
    }
}

pub fn render_exit_confirmation(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExitConfirmViewModel,
    theme: &TundraTheme,
) {
    let dialog = centered_rect(area, area.width.min(46), area.height.min(7));
    let lines = vec![
        Line::from(model.message.clone()),
        Line::from(""),
        Line::from(format!("{}    {}", model.confirm_label, model.cancel_label)),
    ];
    let dialog_widget = Paragraph::new(lines)
        .block(
            theme
                .block()
                .title(model.title.as_str())
                .borders(Borders::ALL)
                .style(theme.body_style()),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, dialog);
    frame.render_widget(dialog_widget, dialog);
}

pub fn render_time_sync_failure_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &TimeSyncDialogViewModel,
    theme: &TundraTheme,
) {
    let dialog = centered_rect(area, area.width.min(34), area.height.min(5));
    let dialog_widget = Paragraph::new(Line::from(model.message()))
        .block(
            theme
                .block()
                .title("Time Sync")
                .borders(Borders::ALL)
                .border_style(solid_border_style(theme.error_style()))
                .style(theme.error_style()),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, dialog);
    frame.render_widget(dialog_widget, dialog);
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
        frame.render_widget(Paragraph::new(Line::styled(notification, style)), area);
        return;
    }

    let block = theme
        .block()
        .title("TundraUX 3")
        .borders(Borders::ALL)
        .style(theme.body_style());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let notification = truncate_status_text(&notification, inner.width);
    frame.render_widget(
        Paragraph::new(Line::styled(notification, style)).alignment(Alignment::Center),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    if inner.height > 1 {
        let size_message = truncate_status_text(COMPACT_TERMINAL_MESSAGE, inner.width);
        frame.render_widget(
            Paragraph::new(size_message)
                .style(theme.muted_style())
                .alignment(Alignment::Center),
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
    let top = Paragraph::new(lines).block(
        theme
            .block()
            .borders(Borders::ALL)
            .style(theme.body_style()),
    );

    frame.render_widget(top, area);
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

    frame.render_widget(
        theme
            .block()
            .title("Status")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        area,
    );

    let inner = theme.block().borders(Borders::ALL).inner(area);
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
            Paragraph::new(Line::styled(notification, style)).style(theme.body_style()),
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

    let label_width = u16::try_from(label.chars().count()).unwrap_or(u16::MAX);
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
    let length = text.chars().count();
    if length <= width {
        return text;
    }
    if width <= 3 {
        return text.chars().take(width).collect();
    }

    let visible = text.chars().take(width - 3).collect::<String>();
    format!("{visible}...")
}

fn render_status_time_button(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &str,
    selected: bool,
    theme: &TundraTheme,
) {
    let style = if selected {
        theme.title_style()
    } else {
        theme.body_style()
    };
    let button = Paragraph::new(label.to_string())
        .style(style)
        .block(
            theme
                .block()
                .borders(Borders::ALL)
                .style(style)
                .border_style(theme.selectable_border_style(selected)),
        )
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });

    frame.render_widget(Clear, area);
    frame.render_widget(button, area);
}

pub(crate) fn fit_cell(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut characters = text.chars();
    let mut fitted = characters.by_ref().take(width).collect::<String>();
    if characters.next().is_some() && width > 1 {
        fitted.pop();
        fitted.push('…');
    }
    let used = fitted.chars().count();
    fitted.extend(std::iter::repeat_n(' ', width.saturating_sub(used)));
    fitted
}
