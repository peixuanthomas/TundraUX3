use super::*;

const FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);
const STATE_CLOCK_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RedrawIdentity {
    screen: String,
    focus: String,
    overlay: Option<String>,
}

impl RedrawIdentity {
    pub(super) fn from_session(state: &ShellSession) -> Self {
        let overlay = state
            .to_notification_view_model()
            .map(|notification| format!("notification:{}", notification.id))
            .or_else(|| {
                state
                    .to_time_sync_dialog_view_model()
                    .map(|_| "time-sync".to_string())
            })
            .or_else(|| {
                (state.active_screen() == ShellScreen::ExitConfirm)
                    .then(|| "exit-confirm".to_string())
            })
            .or_else(|| state.active_popup().map(|popup| format!("popup:{popup:?}")));
        Self {
            screen: format!("{:?}", state.content_screen()),
            focus: format!("{:?}", state.focused_component()),
            overlay,
        }
    }
}

#[derive(Debug)]
pub(super) struct RedrawScheduler {
    origin: Instant,
    prior: RedrawIdentity,
    current: RedrawIdentity,
    screen_changed_at: Option<Duration>,
    focus_changed_at: Option<Duration>,
    overlay_changed_at: Option<Duration>,
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
            prior: identity.clone(),
            current: identity,
            screen_changed_at: None,
            focus_changed_at: None,
            overlay_changed_at: None,
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

    pub(super) fn observe(&mut self, now: Instant, identity: RedrawIdentity, reduced_motion: bool) {
        let now = self.elapsed(now);
        self.reduced_motion = reduced_motion;
        if self.current != identity {
            if self.current.screen != identity.screen {
                self.prior.screen.clone_from(&self.current.screen);
                self.screen_changed_at = Some(now);
            }
            if self.current.focus != identity.focus {
                self.prior.focus.clone_from(&self.current.focus);
                self.focus_changed_at = Some(now);
            }
            if self.current.overlay != identity.overlay {
                self.prior.overlay.clone_from(&self.current.overlay);
                self.overlay_changed_at = Some(now);
            }
            self.current = identity;
            self.next_motion_frame = (!reduced_motion).then(|| {
                self.last_frame_at
                    .checked_add(FRAME_INTERVAL)
                    .unwrap_or(Duration::MAX)
                    .max(now)
            });
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

    pub(super) fn did_draw(&mut self, now: Instant) {
        let now = self.elapsed(now);
        self.needs_redraw = false;
        self.last_frame_at = now;
        while self.next_state_clock <= now {
            self.next_state_clock = self
                .next_state_clock
                .checked_add(STATE_CLOCK_INTERVAL)
                .unwrap_or(Duration::MAX);
        }
        let frame = ui::MotionFrame {
            now,
            delta: Duration::ZERO,
            reduced_motion: self.reduced_motion,
        };
        let schedules = [
            self.screen_changed_at.map(|changed_at| {
                ui::schedule_motion(
                    ui::MotionIdentity {
                        screen: Some(&self.prior.screen),
                        ..ui::MotionIdentity::default()
                    },
                    ui::MotionIdentity {
                        screen: Some(&self.current.screen),
                        ..ui::MotionIdentity::default()
                    },
                    changed_at,
                    frame,
                )
            }),
            self.focus_changed_at.map(|changed_at| {
                ui::schedule_motion(
                    ui::MotionIdentity {
                        focus: Some(&self.prior.focus),
                        ..ui::MotionIdentity::default()
                    },
                    ui::MotionIdentity {
                        focus: Some(&self.current.focus),
                        ..ui::MotionIdentity::default()
                    },
                    changed_at,
                    frame,
                )
            }),
            self.overlay_changed_at.map(|changed_at| {
                ui::schedule_motion(
                    ui::MotionIdentity {
                        overlay: self.prior.overlay.as_deref(),
                        ..ui::MotionIdentity::default()
                    },
                    ui::MotionIdentity {
                        overlay: self.current.overlay.as_deref(),
                        ..ui::MotionIdentity::default()
                    },
                    changed_at,
                    frame,
                )
            }),
        ];
        let next_redraw_in = schedules
            .into_iter()
            .flatten()
            .filter(|schedule| schedule.active)
            .map(|schedule| schedule.next_redraw_in)
            .min();
        self.next_motion_frame = next_redraw_in.map(|next_redraw_in| {
            now.checked_add(next_redraw_in.min(FRAME_INTERVAL))
                .unwrap_or(Duration::MAX)
        });
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
            overlay: overlay.map(str::to_string),
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
    fn screen_transition_draws_start_mid_and_exact_final_frame() {
        let origin = Instant::now();
        let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
        scheduler.did_draw(origin);
        scheduler.observe(origin, id("settings", "one", None), false);
        assert!(!scheduler.is_due(origin));
        assert!(scheduler.is_due(origin + FRAME_INTERVAL));
        scheduler.did_draw(origin + FRAME_INTERVAL);
        scheduler.did_draw(origin + Duration::from_millis(210));
        assert_eq!(
            scheduler.poll_timeout(origin + Duration::from_millis(210), Duration::MAX),
            Duration::from_millis(10)
        );
        assert!(scheduler.is_due(origin + ui::MotionTimings::PAGE));
        scheduler.did_draw(origin + ui::MotionTimings::PAGE);
        assert!(!scheduler.is_due(origin + ui::MotionTimings::PAGE));
    }

    #[test]
    fn focus_overlay_interruption_and_reversal_restart_timing() {
        let origin = Instant::now();
        let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
        scheduler.did_draw(origin);
        scheduler.observe(origin, id("home", "two", Some("dialog")), false);
        scheduler.did_draw(origin + FRAME_INTERVAL);
        scheduler.observe(
            origin + Duration::from_millis(50),
            id("home", "one", None),
            false,
        );
        scheduler.did_draw(origin + Duration::from_millis(50));
        scheduler.did_draw(origin + Duration::from_millis(200));
        assert_eq!(
            scheduler.poll_timeout(origin + Duration::from_millis(200), Duration::MAX),
            Duration::from_millis(10)
        );
        assert!(scheduler.is_due(origin + Duration::from_millis(210)));
    }

    #[test]
    fn reduced_motion_has_zero_transition_duration() {
        let origin = Instant::now();
        let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), true);
        scheduler.did_draw(origin);
        scheduler.observe(origin, id("settings", "two", Some("dialog")), true);
        assert!(scheduler.is_due(origin));
        scheduler.did_draw(origin);
        assert!(!scheduler.is_due(origin + FRAME_INTERVAL));
        assert!(scheduler.frame(origin).reduced_motion);
    }

    #[test]
    fn event_flood_during_motion_coalesces_to_the_next_frame() {
        let origin = Instant::now();
        let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
        scheduler.did_draw(origin);
        scheduler.observe(origin, id("settings", "one", None), false);

        for millis in [1, 5, 10] {
            let now = origin + Duration::from_millis(millis);
            scheduler.request_redraw();
            assert!(!scheduler.is_due(now));
            assert_eq!(
                scheduler.poll_timeout(now, Duration::MAX),
                FRAME_INTERVAL.saturating_sub(Duration::from_millis(millis))
            );
        }

        assert!(scheduler.is_due(origin + FRAME_INTERVAL));
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
}
