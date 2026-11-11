use super::*;

const FRAME_INTERVAL: Duration = ui::ANIMATION_FRAME_INTERVAL;
const CONTINUOUS_FRAME_INTERVAL: Duration = Duration::from_nanos(33_333_334);
const STATE_CLOCK_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RedrawIdentity {
    language_generation: u64,
    screen: String,
    focus: String,
    keyboard_focus_visible: bool,
    screen_saver_active: bool,
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
            keyboard_focus_visible: state.interaction.keyboard_focus_visible(),
            screen_saver_active: state.screen_saver.active,
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
    next_continuous_frame: Option<Duration>,
    next_state_clock: Duration,
    urgent_redraw: bool,
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
            next_continuous_frame: None,
            next_state_clock: STATE_CLOCK_INTERVAL,
            needs_redraw: true,
            urgent_redraw: true,
            reduced_motion,
        }
    }

    pub(super) fn elapsed(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.origin)
    }

    pub(super) fn request_redraw(&mut self) {
        self.needs_redraw = true;
    }

    pub(super) fn request_input_redraw(&mut self) {
        self.needs_redraw = true;
        self.urgent_redraw = true;
    }

    pub(super) fn request_animation_frame(&mut self, now: Instant) {
        if self.reduced_motion {
            return;
        }
        self.request_animation_frame_in(now, FRAME_INTERVAL);
    }

    pub(super) fn request_animation_frame_in(&mut self, now: Instant, interval: Duration) {
        if self.reduced_motion {
            return;
        }
        let now = self.elapsed(now);
        let deadline = now.saturating_add(interval);
        self.next_motion_frame = Some(
            self.next_motion_frame
                .map_or(deadline, |current| current.min(deadline)),
        );
    }

    pub(super) fn request_continuous_frame(&mut self, now: Instant) {
        self.request_frame_after(now, CONTINUOUS_FRAME_INTERVAL);
    }

    pub(super) fn request_frame_after(&mut self, now: Instant, interval: Duration) {
        let deadline = self.elapsed(now).saturating_add(interval);
        self.next_continuous_frame = Some(
            self.next_continuous_frame
                .map_or(deadline, |current| current.min(deadline)),
        );
    }

    fn next_frame(&self) -> Option<Duration> {
        [self.next_motion_frame, self.next_continuous_frame]
            .into_iter()
            .flatten()
            .min()
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
            self.next_continuous_frame = None;
            self.needs_redraw = true;
            self.urgent_redraw = true;
        } else if reduced_motion {
            self.next_motion_frame = None;
        }
    }

    pub(super) fn is_due(&self, now: Instant) -> bool {
        let now = self.elapsed(now);
        let next_frame = self.next_frame();
        let redraw_request_due =
            self.needs_redraw && next_frame.is_none_or(|deadline| now >= deadline);
        self.urgent_redraw
            || redraw_request_due
            || now >= self.next_state_clock
            || next_frame.is_some_and(|deadline| now >= deadline)
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

    #[cfg(test)]
    pub(super) fn did_draw(&mut self, now: Instant) {
        self.did_draw_finished(now, now);
    }

    pub(super) fn did_draw_finished(&mut self, sampled_at: Instant, completed_at: Instant) {
        let sampled_at = self.elapsed(sampled_at);
        let now = self.elapsed(completed_at);
        self.needs_redraw = false;
        self.urgent_redraw = false;
        self.last_frame_at = sampled_at;
        self.next_motion_frame = None;
        self.next_continuous_frame = None;
        if self.next_state_clock <= now {
            self.next_state_clock = Duration::from_secs(now.as_secs().saturating_add(1));
        }
    }

    pub(super) fn poll_timeout(&self, now: Instant, maximum: Duration) -> Duration {
        let now = self.elapsed(now);
        if self.urgent_redraw || (self.needs_redraw && self.next_frame().is_none()) {
            return Duration::ZERO;
        }
        let deadline = self
            .next_frame()
            .unwrap_or(Duration::MAX)
            .min(self.next_state_clock);
        maximum.min(deadline.saturating_sub(now))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session/redraw/tests.rs"]
mod tests;
