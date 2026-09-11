use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph};

use super::layout::{
    NotificationLayout, notification_action_text, notification_layout, wrap_notification_text,
};
use super::model::{
    NotificationLevel, NotificationTone, NotificationViewModel, notification_title,
};
use crate::components::{Button, Scrollbar, Surface};
use crate::{RenderContext, TundraTheme};
pub fn render_notification_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &NotificationViewModel,
    theme: &TundraTheme,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_notification_overlay_context(frame, area, model, &context);
}

pub(crate) fn render_notification_overlay_context(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &NotificationViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
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
    let border = match model.tone {
        NotificationTone::Info => context.theme.border,
        NotificationTone::Success | NotificationTone::Warning => context.theme.accent,
        NotificationTone::Error | NotificationTone::Critical => context.theme.danger,
    };
    let dialog_context = RenderContext {
        theme: crate::ThemeTokens {
            border,
            ..context.theme
        },
        ..*context
    };
    Surface::new()
        .titled(notification_title(model))
        .bordered(true)
        .raised(true)
        .render_frame(frame, layout.dialog, &dialog_context);

    let message_lines = wrap_notification_text(&model.message, layout.message.width)
        .into_iter()
        .skip(layout.scroll_offset)
        .take(usize::from(layout.message.height))
        .map(Line::from)
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(message_lines)
            .style(theme.body_style())
            .alignment(HorizontalAlignment::Center),
        layout.message,
    );

    if let Some(area) = layout.scrollbar {
        Scrollbar::new(
            layout.message_line_count,
            usize::from(layout.message.height),
            layout.scroll_offset,
        )
        .render_frame(frame, area, context);
    }
    if let Some(area) = layout.scroll_hint {
        frame.render_widget(
            Paragraph::new(i18n::tr!("ui-notifications-scroll-help"))
                .alignment(HorizontalAlignment::Center)
                .style(theme.muted_style()),
            area,
        );
    }
    for action_layout in layout.actions {
        let Some(action) = model.actions.get(action_layout.index) else {
            continue;
        };
        let action_text = notification_action_text(action);
        let label = wrap_notification_text(&action_text, action_layout.area.width).join("\n");
        let mut button = Button::new(
            format!("notification.{}.action.{}", model.id, action.id),
            label,
        );
        button.state.selected = action.selected;
        button.render_borderless_frame(frame, action_layout.area, theme);
    }
}

fn render_notification_too_small(frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
    frame.render_widget(Clear, area);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let lines = wrap_notification_text(
        &i18n::tr!("ui-notifications-terminal-is-too-small-to-render-this-notification"),
        area.width,
    )
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
            .alignment(HorizontalAlignment::Center),
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
