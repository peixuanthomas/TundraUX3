use super::*;

const FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);
const STATE_CLOCK_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RedrawIdentity {
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

    pub(super) fn frame(&self, now: Instant) -> ui::MotionFrame {
        let now = self.elapsed(now);
        ui::MotionFrame {
            now,
            delta: now.saturating_sub(self.last_frame_at),
            reduced_motion: self.reduced_motion,
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
mod tests {
    use super::*;

    fn id(screen: &str, focus: &str, overlay: Option<&str>) -> RedrawIdentity {
        RedrawIdentity {
            screen: screen.into(),
            focus: focus.into(),
            overlay: overlay.map(|id| RedrawOverlayIdentity {
                kind: if id.contains("toast") {
                    ui::MotionOverlayKind::Toast
                } else if id.contains("popover") {
                    ui::MotionOverlayKind::Popover
                } else {
                    ui::MotionOverlayKind::Dialog
                },
                id: id.to_string(),
            }),
        }
    }

    #[test]
    fn initial_idle_and_state_clock_are_event_driven() {
        let origin = Instant::now();
        let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
        assert!(scheduler.is_due(origin));
        scheduler.did_draw(origin);
        assert!(!scheduler.is_due(origin + Duration::from_millis(999)));
        assert_eq!(
            scheduler.poll_timeout(origin, Duration::MAX),
            Duration::from_secs(1)
        );
        assert!(scheduler.is_due(origin + Duration::from_secs(1)));
        scheduler.did_draw(origin + Duration::from_secs(1));
        assert!(!scheduler.is_due(origin + Duration::from_secs(1)));
    }

    #[test]
    fn all_shell_identity_changes_redraw_immediately_without_transitions() {
        let origin = Instant::now();
        for changed in [
            id("settings", "one", None),
            id("home", "two", None),
            id("home", "one", Some("dialog")),
            id("home", "one", Some("popover:menu")),
            id("home", "one", Some("toast:notice")),
        ] {
            let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
            scheduler.did_draw(origin);
            scheduler.request_animation_frame(origin);

            scheduler.observe(origin, changed, false);

            assert!(scheduler.is_due(origin));
            assert_eq!(
                scheduler.transitions(origin),
                ui::MotionTransitions::default()
            );
            scheduler.did_draw(origin);
            assert!(!scheduler.is_due(origin + FRAME_INTERVAL));
        }
    }

    #[test]
    fn settings_picker_opens_without_a_transition() {
        let origin = Instant::now();
        let mut scheduler = RedrawScheduler::new(origin, id("settings", "settings", None), false);
        scheduler.did_draw(origin);

        scheduler.observe(
            origin,
            id(
                "settings",
                "settings",
                Some("popover:settings-picker:Theme"),
            ),
            false,
        );

        assert!(scheduler.is_due(origin));
        assert_eq!(
            scheduler.transitions(origin),
            ui::MotionTransitions::default()
        );
    }

    #[test]
    fn exit_confirmation_is_an_instant_screen_change() {
        let origin = Instant::now();
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
        let mut scheduler =
            RedrawScheduler::new(origin, RedrawIdentity::from_session(&state), false);
        scheduler.did_draw(origin);

        state.apply_input(InputEvent::from_key_label("q"));
        scheduler.observe(origin, RedrawIdentity::from_session(&state), false);

        assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
        assert_eq!(
            scheduler.transitions(origin),
            ui::MotionTransitions::default()
        );
        assert!(scheduler.is_due(origin));
    }

    #[test]
    fn reduced_motion_is_forwarded_to_independent_widgets() {
        let origin = Instant::now();
        let scheduler = RedrawScheduler::new(origin, id("home", "one", None), true);
        assert!(scheduler.frame(origin).reduced_motion);
    }

    #[test]
    fn idle_event_redraw_is_immediate() {
        let origin = Instant::now();
        let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
        scheduler.did_draw(origin);
        scheduler.request_redraw();
        assert!(scheduler.is_due(origin + Duration::from_millis(1)));
        assert_eq!(
            scheduler.poll_timeout(origin + Duration::from_millis(1), Duration::MAX),
            Duration::ZERO
        );
    }

    #[test]
    fn independent_widget_animation_can_request_the_next_frame() {
        let origin = Instant::now();
        let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
        scheduler.did_draw(origin);
        scheduler.request_animation_frame(origin);
        assert_eq!(
            scheduler.poll_timeout(origin, Duration::MAX),
            FRAME_INTERVAL
        );
        assert!(!scheduler.is_due(origin + Duration::from_millis(10)));
        assert!(scheduler.is_due(origin + FRAME_INTERVAL));
    }
}
