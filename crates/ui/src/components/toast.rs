use std::time::Duration;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Widget};

use crate::{
    ColorCapability, MotionDirection, MotionFrame, MotionTransitionKind, RenderContext,
    schedule_motion_range,
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

    pub fn layout(&self, bounds: Rect) -> Rect {
        let width = bounds.width.saturating_sub(6).min(60);
        let height = (crate::management_wrapped_lines(&self.message, width.saturating_sub(6)).len()
            + 2)
        .min(usize::from(bounds.height.saturating_sub(2)).min(6)) as u16;
        Rect::new(
            bounds.right().saturating_sub(width + 2).max(bounds.x),
            bounds.y + u16::from(bounds.height > 1),
            width,
            height,
        )
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        if !self.is_visible(context.motion) || area.is_empty() {
            return;
        }
        crate::automation::control("", "status", &self.message, area, Default::default(), &[]);
        let tokens = context.theme;
        let feedback = match self.tone {
            ToastTone::Info => tokens.accent,
            ToastTone::Success => tokens.success,
            ToastTone::Warning => tokens.warning,
            ToastTone::Danger => tokens.danger,
        };
        let background = if context.capabilities.color == ColorCapability::TrueColor {
            use animate::Interpolate;
            let progress = f32::from(self.visible_progress(context.motion)) / 1_000.0;
            ratatui::style::Color::lerp(&tokens.surface, &tokens.raised, progress)
        } else {
            tokens.raised
        };
        Clear.render(area, buffer);
        buffer.set_style(area, Style::default().fg(tokens.text).bg(background));
        let horizontal = if area.width >= 10 { 3 } else { 0 };
        let vertical = u16::from(area.height > 2);
        let text = Rect::new(
            area.x + horizontal,
            area.y + vertical,
            area.width.saturating_sub(horizontal * 2),
            area.height.saturating_sub(vertical * 2),
        );
        let mut lines = crate::management_wrapped_lines(&self.message, text.width);
        if lines.len() > usize::from(text.height) {
            lines.truncate(usize::from(text.height));
            if let Some(last) = lines.last_mut() {
                *last = super::truncate_to_terminal_width(
                    last,
                    usize::from(text.width.saturating_sub(1)),
                );
                last.push('…');
            }
        }
        Paragraph::new(lines.into_iter().map(Line::from).collect::<Vec<_>>()).render(text, buffer);
        if horizontal > 0 {
            let bounds = buffer.area;
            for x in [area.x, area.right() - 1] {
                for point in Rect::new(x, area.y, 1, area.height)
                    .intersection(bounds)
                    .positions()
                {
                    buffer[point].set_symbol("│").set_fg(feedback);
                }
            }
        }
    }

    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        self.render(area, frame.buffer_mut(), context);
    }
}
