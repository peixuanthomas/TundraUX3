use super::*;

const FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);
const STATE_CLOCK_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RedrawIdentity {
    language_generation: u64,
    screen: String,
    focus: String,
    overlay: Option<RedrawOverlayIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RedrawOverlayIdentity {
    kind: ui::MotionOverlayKind,
    id: String,
}

impl RedrawIdentity {
    pub(super) fn from_session(state: &ShellSession) -> Self {
        let overlay = state
            .active_overlay_descriptor()
            .map(|overlay| RedrawOverlayIdentity {
                kind: overlay.kind,
                id: overlay.id,
            });
        Self {
            language_generation: state.language.generation(),
            screen: format!("{:?}", state.active_screen()),
            focus: format!("{:?}", state.focused_component()),
            overlay,
        }
    }
}

#[derive(Debug)]
pub(super) struct RedrawScheduler {
    origin: Instant,
    current: RedrawIdentity,
    last_frame_at: Duration,
    next_motion_frame: Option<Duration>,
    next_state_clock: Duration,
    needs_redraw: bool,
    reduced_motion: bool,
}

impl RedrawScheduler {
    pub(super) fn new(origin: Instant, identity: RedrawIdentity, reduced_motion: bool) -> Self {
        Self {
            origin,
            current: identity,
            last_frame_at: Duration::ZERO,
            next_motion_frame: None,
            next_state_clock: STATE_CLOCK_INTERVAL,
            needs_redraw: true,
            reduced_motion,
        }
    }

    pub(super) fn elapsed(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.origin)
    }

    pub(super) fn request_redraw(&mut self) {
        self.needs_redraw = true;
    }

    pub(super) fn request_animation_frame(&mut self, now: Instant) {
        if self.reduced_motion {
            return;
        }
        let now = self.elapsed(now);
        let deadline = now.checked_add(FRAME_INTERVAL).unwrap_or(Duration::MAX);
        self.next_motion_frame = Some(
            self.next_motion_frame
                .map_or(deadline, |current| current.min(deadline)),
        );
    }

    pub(super) fn observe(
        &mut self,
        _now: Instant,
        identity: RedrawIdentity,
        reduced_motion: bool,
    ) {
        self.reduced_motion = reduced_motion;
        if self.current != identity {
            self.current = identity;
            self.next_motion_frame = None;
            self.needs_redraw = true;
        } else if reduced_motion {
            self.next_motion_frame = None;
        }
    }

    pub(super) fn is_due(&self, now: Instant) -> bool {
        let now = self.elapsed(now);
        let redraw_request_due = self.needs_redraw
            && self
                .next_motion_frame
                .is_none_or(|deadline| now >= deadline);
        redraw_request_due
            || now >= self.next_state_clock
            || self
                .next_motion_frame
                .is_some_and(|deadline| now >= deadline)
    }

    pub(super) fn frame(&self, now: Instant, animation_speed_percent: u16) -> ui::MotionFrame {
        let now = self.elapsed(now);
        ui::MotionFrame {
            now,
            delta: now.saturating_sub(self.last_frame_at),
            reduced_motion: self.reduced_motion,
            animation_speed_percent,
        }
    }

    pub(super) fn transitions(&self, _now: Instant) -> ui::MotionTransitions {
        // Shell screens, focus changes, menus, and overlays are intentionally
        // instantaneous. Widget-local animations request redraws separately.
        ui::MotionTransitions::default()
    }

    pub(super) fn did_draw(&mut self, now: Instant) {
        let now = self.elapsed(now);
        self.needs_redraw = false;
        self.last_frame_at = now;
        self.next_motion_frame = None;
        while self.next_state_clock <= now {
            self.next_state_clock = self
                .next_state_clock
                .checked_add(STATE_CLOCK_INTERVAL)
                .unwrap_or(Duration::MAX);
        }
    }

    pub(super) fn poll_timeout(&self, now: Instant, maximum: Duration) -> Duration {
        let now = self.elapsed(now);
        if self.needs_redraw && self.next_motion_frame.is_none() {
            return Duration::ZERO;
        }
        let deadline = self
            .next_motion_frame
            .unwrap_or(Duration::MAX)
            .min(self.next_state_clock);
        maximum.min(deadline.saturating_sub(now))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session/redraw/tests.rs"]
mod tests;
