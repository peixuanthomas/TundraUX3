use ratatui::layout::Rect;
use std::time::Duration;

use crate::{FrostMotion, MotionFrame};

/// A reversible panel opening, in thousandths, using the shared motion clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PanelMotion {
    value: u16,
    start: u16,
    target: u16,
    motion: FrostMotion,
}

impl PanelMotion {
    pub fn new(value: u16) -> Self {
        let value = value.min(1_000);
        Self {
            value,
            start: value,
            target: value,
            motion: FrostMotion::default(),
        }
    }

    pub fn value(self) -> u16 {
        self.value
    }

    pub fn update(&mut self, target: u16, frame: MotionFrame, duration: Duration) -> bool {
        let target = target.min(1_000);
        if frame.reduced_motion {
            *self = Self::new(target);
            return false;
        }
        if target != self.target {
            self.start = self.value;
            self.target = target;
            self.motion.begin(frame, duration);
        }
        let phase = i32::from(self.motion.progress(frame, true));
        self.value = (i32::from(self.start)
            + (i32::from(self.target) - i32::from(self.start)) * phase / 1_000)
            as u16;
        self.motion.requests_redraw(frame)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevealOrigin {
    Right,
    Center,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Default for RevealOrigin {
    fn default() -> Self {
        Self::Center
    }
}

/// Visible rectangle of a panel; closing uses the same origin in reverse.
pub fn reveal_rect(area: Rect, amount: u16, origin: RevealOrigin) -> Rect {
    let amount = u32::from(amount.min(1_000));
    let width = (u32::from(area.width) * amount / 1_000) as u16;
    let height = if origin == RevealOrigin::Right {
        area.height
    } else {
        (u32::from(area.height) * amount / 1_000) as u16
    };
    let x = match origin {
        RevealOrigin::Right | RevealOrigin::TopRight | RevealOrigin::BottomRight => {
            area.right() - width
        }
        RevealOrigin::Center => area.x + (area.width - width) / 2,
        _ => area.x,
    };
    let y = match origin {
        RevealOrigin::BottomLeft | RevealOrigin::BottomRight => area.bottom() - height,
        RevealOrigin::Center => area.y + (area.height - height) / 2,
        _ => area.y,
    };
    Rect::new(x, y, width, height)
}

/// Grow the trailing pane while folding the adjacent pane along their shared edge.
pub fn expand_trailing_pane(list: Rect, details: Rect, amount: u16) -> (Rect, Rect) {
    let amount = u32::from(amount.min(1_000));
    let mut list = list;
    let mut details = details;
    if list.x < details.x {
        let shift = (u32::from(list.width) * amount / 1_000) as u16;
        list.width -= shift;
        details.x -= shift;
        details.width += shift;
    } else {
        let shift = (u32::from(list.height) * amount / 1_000) as u16;
        list.height -= shift;
        details.y -= shift;
        details.height += shift;
    }
    (
        if list.is_empty() {
            Rect::default()
        } else {
            list
        },
        details,
    )
}
