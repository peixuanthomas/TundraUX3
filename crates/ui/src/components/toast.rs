use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::{MotionFrame, MotionTimings, RenderContext, ease_in_cubic, ease_out_cubic};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToastTone {
    #[default]
    Info,
    Success,
    Warning,
    Danger,
}

/// A transient notification with deterministic Frost Motion timing. The host
/// checks `requests_redraw` and does not need an idle animation timer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub message: String,
    pub tone: ToastTone,
    pub shown_at: Duration,
    pub dismiss_at: Option<Duration>,
}

impl Toast {
    pub fn new(message: impl Into<String>, tone: ToastTone, frame: MotionFrame) -> Self {
        Self {
            message: message.into(),
            tone,
            shown_at: frame.now,
            dismiss_at: None,
        }
    }

    pub fn dismiss(&mut self, frame: MotionFrame) {
        self.dismiss_at = Some(frame.now);
    }

    pub fn is_visible(&self, frame: MotionFrame) -> bool {
        self.dismiss_at.is_none_or(|dismissed| {
            !frame.reduced_motion && frame.now.saturating_sub(dismissed) < MotionTimings::TOAST_EXIT
        })
    }

    pub fn requests_redraw(&self, frame: MotionFrame) -> bool {
        if frame.reduced_motion {
            return false;
        }
        match self.dismiss_at {
            Some(dismissed) => frame.now.saturating_sub(dismissed) < MotionTimings::TOAST_EXIT,
            None => frame.now.saturating_sub(self.shown_at) < MotionTimings::TOAST_ENTER,
        }
    }

    pub fn visible_progress(&self, frame: MotionFrame) -> u16 {
        if frame.reduced_motion {
            return if self.dismiss_at.is_some() { 0 } else { 1_000 };
        }
        let (started_at, duration, entering) = match self.dismiss_at {
            Some(dismissed) => (dismissed, MotionTimings::TOAST_EXIT, false),
            None => (self.shown_at, MotionTimings::TOAST_ENTER, true),
        };
        let normalized = (frame
            .now
            .saturating_sub(started_at)
            .as_millis()
            .saturating_mul(1_000)
            / duration.as_millis().max(1))
        .min(1_000) as u16;
        if entering {
            ease_out_cubic(normalized)
        } else {
            1_000_u16.saturating_sub(ease_in_cubic(normalized))
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        if !self.is_visible(context.motion) || area.width == 0 || area.height == 0 {
            return;
        }
        let area = if self.visible_progress(context.motion) < 500 {
            Rect::new(
                area.x.saturating_add(1),
                area.y,
                area.width.saturating_sub(1),
                area.height,
            )
        } else {
            area
        };
        let tokens = context.theme;
        let tone = match self.tone {
            ToastTone::Info => tokens.accent_strong,
            ToastTone::Success => tokens.success,
            ToastTone::Warning => tokens.warning,
            ToastTone::Danger => tokens.danger,
        };
        let style = Style::default().fg(tokens.text).bg(tokens.raised);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(tone).bg(tokens.raised))
            .style(style);
        let inner = block.inner(area);
        block.render(area, buffer);
        Paragraph::new(self.message.as_str())
            .alignment(HorizontalAlignment::Left)
            .style(style.add_modifier(Modifier::BOLD))
            .render(inner, buffer);
    }

    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        self.render(area, frame.buffer_mut(), context);
    }
}
