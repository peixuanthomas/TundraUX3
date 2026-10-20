use ratatui::layout::Rect;
use std::time::Duration;

use crate::{FrostMotion, MotionFrame};

/// Stage durations before applying the user's overall speed percentage.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MotionPhases {
    pub acceleration: Duration,
    pub cruise: Duration,
    pub deceleration: Duration,
}

impl MotionPhases {
    pub fn from_millis(phases: [u16; 3]) -> Self {
        Self {
            acceleration: Duration::from_millis(u64::from(phases[0])),
            cruise: Duration::from_millis(u64::from(phases[1])),
            deceleration: Duration::from_millis(u64::from(phases[2])),
        }
    }

    pub fn for_duration(duration: Duration) -> Self {
        Self {
            acceleration: duration / 4,
            cruise: duration / 2,
            deceleration: duration / 4,
        }
    }

    pub fn duration(self) -> Duration {
        self.acceleration + self.cruise + self.deceleration
    }

    pub fn progress(self, value: u16) -> u16 {
        if value == 0 {
            return 0;
        }
        if value >= 1_000 || self.duration().is_zero() {
            return 1_000;
        }
        let acceleration = self.acceleration.as_secs_f64();
        let cruise = self.cruise.as_secs_f64();
        let deceleration = self.deceleration.as_secs_f64();
        let total = acceleration + cruise + deceleration;
        let elapsed = total * f64::from(value) / 1_000.0;
        let speed = 1.0 / (cruise + (acceleration + deceleration) / 2.0);
        let position = if elapsed < acceleration {
            speed * elapsed * elapsed / (2.0 * acceleration)
        } else if elapsed < acceleration + cruise {
            speed * (acceleration / 2.0 + elapsed - acceleration)
        } else {
            let remaining = total - elapsed;
            1.0 - speed * remaining * remaining / (2.0 * deceleration)
        };
        (position * 1_000.0).round() as u16
    }
}

/// Position in thousandths: accelerate for 25% of the time, cruise for 50%,
/// then decelerate for 25%, with continuous speed and no overshoot.
pub fn ease_in_out_cruise(value: u16) -> u16 {
    MotionPhases::from_millis([250, 500, 250]).progress(value)
}

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
        self.update_with_phases(target, frame, MotionPhases::for_duration(duration))
    }

    pub fn update_with_phases(
        &mut self,
        target: u16,
        frame: MotionFrame,
        phases: MotionPhases,
    ) -> bool {
        let target = target.min(1_000);
        if frame.reduced_motion {
            *self = Self::new(target);
            return false;
        }
        if target != self.target {
            self.start = self.value;
            self.target = target;
            self.motion.begin_with_phases(frame, phases);
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
