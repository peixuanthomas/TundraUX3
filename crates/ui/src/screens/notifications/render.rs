use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Borders, Clear, Paragraph};

use super::layout::{
    NOTIFICATION_TOO_SMALL_MESSAGE, NotificationLayout, notification_action_text,
    notification_layout, wrap_notification_text,
};
use super::model::{NotificationLevel, NotificationTone, NotificationViewModel};
use crate::TundraTheme;
use crate::theme::solid_border_style;
pub fn render_notification_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &NotificationViewModel,
    theme: &TundraTheme,
) {
    if model.level != NotificationLevel::Modal {
        return;
    }

    let layout = match notification_layout(area, model) {
        NotificationLayout::Dialog(layout) => layout,
        NotificationLayout::TooSmall { .. } => {
            render_notification_too_small(frame, area, theme);
            return;
        }
    };

    frame.render_widget(Clear, layout.dialog);
    let tone_style = notification_tone_style(model.tone, theme);
    frame.render_widget(
        theme
            .block()
            .title(format!(
                "{} {}",
                notification_tone_prefix(model.tone),
                model.title
            ))
            .title_style(tone_style)
            .borders(Borders::ALL)
            .border_style(solid_border_style(tone_style))
            .style(tone_style),
        layout.dialog,
    );

    let message_lines = wrap_notification_text(&model.message, layout.message.width)
        .into_iter()
        .map(Line::from)
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(message_lines)
            .style(theme.body_style())
            .alignment(Alignment::Center),
        layout.message,
    );

    for action_layout in layout.actions {
        let Some(action) = model.actions.get(action_layout.index) else {
            continue;
        };
        let action_text = notification_action_text(action);
        let action_lines = wrap_notification_text(&action_text, action_layout.area.width)
            .into_iter()
            .map(Line::from)
            .collect::<Vec<_>>();
        let style = if action.selected {
            theme.title_style()
        } else {
            theme.body_style()
        };
        frame.render_widget(
            Paragraph::new(action_lines)
                .style(style)
                .alignment(Alignment::Center),
            action_layout.area,
        );
    }
}

fn render_notification_too_small(frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
    frame.render_widget(Clear, area);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let lines = wrap_notification_text(NOTIFICATION_TOO_SMALL_MESSAGE, area.width)
        .into_iter()
        .map(Line::from)
        .collect::<Vec<_>>();
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .min(area.height);
    let prompt = centered_rect(area, area.width, height);
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme.error_style())
            .alignment(Alignment::Center),
        prompt,
    );
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}
pub(crate) fn notification_tone_prefix(tone: NotificationTone) -> &'static str {
    match tone {
        NotificationTone::Info => "[INFO]",
        NotificationTone::Success => "[SUCCESS]",
        NotificationTone::Warning => "[WARN]",
        NotificationTone::Error => "[ERROR]",
        NotificationTone::Critical => "[CRITICAL]",
    }
}

pub(crate) fn notification_tone_style(
    tone: NotificationTone,
    theme: &TundraTheme,
) -> ratatui::style::Style {
    match tone {
        NotificationTone::Info => theme.body_style(),
        NotificationTone::Success => theme.title_style(),
        NotificationTone::Warning => theme.title_style(),
        NotificationTone::Error | NotificationTone::Critical => theme.error_style(),
    }
}
