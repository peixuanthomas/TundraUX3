use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::{
    MotionDirection, MotionFrame, MotionTransitionKind, RenderContext, schedule_motion_range,
};

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
    enter_start_progress: u16,
    dismiss_start_progress: u16,
}

impl Toast {
    pub fn new(message: impl Into<String>, tone: ToastTone, frame: MotionFrame) -> Self {
        Self {
            message: message.into(),
            tone,
            shown_at: frame.now,
            dismiss_at: None,
            enter_start_progress: 0,
            dismiss_start_progress: 1_000,
        }
    }

    pub fn dismiss(&mut self, frame: MotionFrame) {
        self.dismiss_start_progress = self.visible_progress(frame);
        self.dismiss_at = Some(frame.now);
    }

    pub fn resume(&mut self, frame: MotionFrame) {
        let progress = self.visible_progress(frame);
        self.enter_start_progress = progress;
        self.shown_at = frame.now;
        self.dismiss_at = None;
    }

    pub fn is_visible(&self, frame: MotionFrame) -> bool {
        self.dismiss_at.is_none() || (!frame.reduced_motion && self.visible_progress(frame) > 0)
    }

    pub fn requests_redraw(&self, frame: MotionFrame) -> bool {
        if frame.reduced_motion {
            return false;
        }
        let transition = match self.dismiss_at {
            Some(dismissed) => schedule_motion_range(
                MotionTransitionKind::Toast,
                MotionDirection::Exiting,
                self.dismiss_start_progress,
                0,
                dismissed,
                frame,
            ),
            None => schedule_motion_range(
                MotionTransitionKind::Toast,
                MotionDirection::Entering,
                self.enter_start_progress,
                1_000,
                self.shown_at,
                frame,
            ),
        };
        transition.active
    }

    pub fn visible_progress(&self, frame: MotionFrame) -> u16 {
        if frame.reduced_motion {
            return if self.dismiss_at.is_some() { 0 } else { 1_000 };
        }
        match self.dismiss_at {
            Some(dismissed) => {
                schedule_motion_range(
                    MotionTransitionKind::Toast,
                    MotionDirection::Exiting,
                    self.dismiss_start_progress,
                    0,
                    dismissed,
                    frame,
                )
                .progress
            }
            None => {
                schedule_motion_range(
                    MotionTransitionKind::Toast,
                    MotionDirection::Entering,
                    self.enter_start_progress,
                    1_000,
                    self.shown_at,
                    frame,
                )
                .progress
            }
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
