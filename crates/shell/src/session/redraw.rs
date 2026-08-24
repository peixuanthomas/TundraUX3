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
            .to_notification_view_model()
            .map(|notification| RedrawOverlayIdentity {
                kind: ui::MotionOverlayKind::Dialog,
                id: format!("notification:{}", notification.id),
            })
            .or_else(|| {
                state
                    .to_time_sync_dialog_view_model()
                    .map(|_| RedrawOverlayIdentity {
                        kind: ui::MotionOverlayKind::Dialog,
                        id: "time-sync".to_string(),
                    })
            })
            .or_else(|| {
                (state.active_screen() == ShellScreen::ExitConfirm).then(|| RedrawOverlayIdentity {
                    kind: ui::MotionOverlayKind::Dialog,
                    id: "exit-confirm".to_string(),
                })
            })
            .or_else(|| {
                state.active_popup().map(|popup| RedrawOverlayIdentity {
                    kind: ui::MotionOverlayKind::Popover,
                    id: format!("popup:{popup:?}"),
                })
            })
            .or_else(|| {
                state
                    .explorer_overlay_mode
                    .as_ref()
                    .map(|mode| RedrawOverlayIdentity {
                        kind: ui::MotionOverlayKind::Popover,
                        id: format!("explorer:{mode:?}"),
                    })
            })
            .or_else(|| {
                state
                    .settings_state
                    .as_ref()
                    .and_then(|settings| settings.picker.as_ref())
                    .map(|picker| RedrawOverlayIdentity {
                        kind: ui::MotionOverlayKind::Popover,
                        id: format!("settings-picker:{:?}", picker.kind),
                    })
            })
            .or_else(|| {
                let id = if state.setup_custom_color_target.is_some() {
                    Some("setup-custom-color")
                } else if state.clock_create_state.is_some() {
                    Some("clock-create")
                } else if state.editor_settings_dialog.is_some() {
                    Some("editor-settings")
                } else if !state.diagnostics_repair_preview.is_empty() {
                    Some("diagnostics-repair")
                } else if state.settings_state.as_ref().is_some_and(|settings| {
                    settings.color_editor.is_some()
                        || settings.weather_location_editor.is_some()
                        || settings.file_extensions_editor.is_some()
                        || settings.time_sync_server_editor.is_some()
                }) {
                    Some("settings-editor")
                } else {
                    None
                };
                id.map(|id| RedrawOverlayIdentity {
                    kind: ui::MotionOverlayKind::Dialog,
                    id: id.to_string(),
                })
            })
            .or_else(|| {
                let notifications = state.app.notification_center();
                (notifications.alert().is_none())
                    .then(|| notifications.toast())
                    .flatten()
                    .map(|toast| RedrawOverlayIdentity {
                        kind: ui::MotionOverlayKind::Toast,
                        id: format!("{toast}:{:?}", notifications.toast_expires_at()),
                    })
            });
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
    screen_start: u16,
    screen_target: u16,
    focus_changed_at: Option<Duration>,
    focus_start: u16,
    focus_target: u16,
    overlay_changed_at: Option<Duration>,
    overlay_start: u16,
    overlay_target: u16,
    last_frame_at: Duration,
    next_motion_frame: Option<Duration>,
    next_state_clock: Duration,
    needs_redraw: bool,
    reduced_motion: bool,
}

impl RedrawScheduler {
    pub(super) fn new(origin: Instant, identity: RedrawIdentity, reduced_motion: bool) -> Self {
        let overlay_progress = identity.overlay.as_ref().map_or(0, |_| 1_000);
        Self {
            origin,
            prior: identity.clone(),
            current: identity,
            screen_changed_at: None,
            screen_start: 1_000,
            screen_target: 1_000,
            focus_changed_at: None,
            focus_start: 1_000,
            focus_target: 1_000,
            overlay_changed_at: None,
            overlay_start: overlay_progress,
            overlay_target: overlay_progress,
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

    pub(super) fn observe(&mut self, now: Instant, identity: RedrawIdentity, reduced_motion: bool) {
        let now = self.elapsed(now);
        let frame = ui::MotionFrame {
            now,
            delta: now.saturating_sub(self.last_frame_at),
            reduced_motion: self.reduced_motion,
        };
        let rendered = self.transitions_for_frame(frame);
        self.reduced_motion = reduced_motion;
        if self.current != identity {
            if self.current.screen != identity.screen {
                let active = rendered.screen.filter(|motion| motion.active);
                self.screen_start = active.map_or(0, |motion| motion.progress);
                self.screen_target = if active.is_some() && identity.screen == self.prior.screen {
                    0
                } else {
                    1_000
                };
                self.prior.screen.clone_from(&self.current.screen);
                self.screen_changed_at = Some(now);
            }
            if self.current.focus != identity.focus {
                let active = rendered.focus.filter(|motion| motion.active);
                self.focus_start = active.map_or(0, |motion| motion.progress);
                self.focus_target = if active.is_some() && identity.focus == self.prior.focus {
                    0
                } else {
                    1_000
                };
                self.prior.focus.clone_from(&self.current.focus);
                self.focus_changed_at = Some(now);
            }
            if self.current.overlay != identity.overlay {
                self.overlay_start = rendered.overlay.filter(|motion| motion.active).map_or_else(
                    || self.current.overlay.as_ref().map_or(0, |_| 1_000),
                    |motion| motion.progress,
                );
                self.overlay_target = if identity.overlay.is_some() { 1_000 } else { 0 };
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

    pub(super) fn transitions(&self, now: Instant) -> ui::MotionTransitions {
        self.transitions_for_frame(self.frame(now))
    }

    fn transitions_for_frame(&self, frame: ui::MotionFrame) -> ui::MotionTransitions {
        let screen = self.screen_changed_at.map(|changed_at| {
            ui::schedule_motion_range(
                ui::MotionTransitionKind::Page,
                ui::MotionDirection::Replacing,
                self.screen_start,
                self.screen_target,
                changed_at,
                frame,
            )
        });
        let focus = self.focus_changed_at.map(|changed_at| {
            ui::schedule_motion_range(
                ui::MotionTransitionKind::Focus,
                ui::MotionDirection::Replacing,
                self.focus_start,
                self.focus_target,
                changed_at,
                frame,
            )
        });
        let overlay = self.overlay_changed_at.and_then(|changed_at| {
            let overlay = self
                .current
                .overlay
                .as_ref()
                .or(self.prior.overlay.as_ref())?;
            let direction = if self.overlay_target < self.overlay_start {
                ui::MotionDirection::Exiting
            } else if self.prior.overlay.is_some() && self.current.overlay.is_some() {
                ui::MotionDirection::Replacing
            } else {
                ui::MotionDirection::Entering
            };
            Some(ui::schedule_motion_range(
                match overlay.kind {
                    ui::MotionOverlayKind::Dialog => ui::MotionTransitionKind::Dialog,
                    ui::MotionOverlayKind::Popover => ui::MotionTransitionKind::Popover,
                    ui::MotionOverlayKind::Toast => ui::MotionTransitionKind::Toast,
                },
                direction,
                self.overlay_start,
                self.overlay_target,
                changed_at,
                frame,
            ))
        });
        ui::MotionTransitions {
            screen,
            focus,
            overlay,
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
        let transitions = self.transitions_for_frame(frame);
        let next_redraw_in = [transitions.screen, transitions.focus, transitions.overlay]
            .into_iter()
            .flatten()
            .filter(|transition| transition.active)
            .map(|transition| transition.next_redraw_in)
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
            overlay: overlay.map(|id| RedrawOverlayIdentity {
                kind: if id.contains("popover") {
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
        let before = scheduler.transitions(origin + Duration::from_millis(50));
        scheduler.observe(
            origin + Duration::from_millis(50),
            id("home", "one", None),
            false,
        );
        let after = scheduler.transitions(origin + Duration::from_millis(50));
        assert_eq!(
            after.focus.expect("reversed focus").progress,
            before.focus.expect("entering focus").progress
        );
        assert_eq!(
            after.overlay.expect("reversed dialog").progress,
            before.overlay.expect("entering dialog").progress
        );
        scheduler.did_draw(origin + Duration::from_millis(50));
        scheduler.did_draw(origin + Duration::from_millis(200));
        assert_eq!(
            scheduler.poll_timeout(origin + Duration::from_millis(200), Duration::MAX),
            Duration::from_millis(800)
        );
        assert!(!scheduler.is_due(origin + Duration::from_millis(230)));
    }

    #[test]
    fn page_and_popover_reversals_preserve_the_rendered_progress() {
        let origin = Instant::now();
        let mut page = RedrawScheduler::new(origin, id("home", "one", None), false);
        page.did_draw(origin);
        page.observe(origin, id("settings", "one", None), false);
        let changed = origin + Duration::from_millis(80);
        let before = page
            .transitions(changed)
            .screen
            .expect("entering page")
            .progress;
        page.observe(changed, id("home", "one", None), false);
        assert_eq!(
            page.transitions(changed)
                .screen
                .expect("reversed page")
                .progress,
            before
        );

        let mut popover = RedrawScheduler::new(origin, id("home", "one", None), false);
        popover.did_draw(origin);
        popover.observe(origin, id("home", "one", Some("popover:menu")), false);
        let changed = origin + Duration::from_millis(60);
        let before = popover
            .transitions(changed)
            .overlay
            .expect("entering popover")
            .progress;
        popover.observe(changed, id("home", "one", None), false);
        assert_eq!(
            popover
                .transitions(changed)
                .overlay
                .expect("reversed popover")
                .progress,
            before
        );
    }

    #[test]
    fn dialog_and_popover_keep_their_typed_durations() {
        let origin = Instant::now();
        let mut dialog = RedrawScheduler::new(origin, id("home", "one", None), false);
        dialog.did_draw(origin);
        dialog.observe(origin, id("home", "one", Some("dialog")), false);
        assert_eq!(
            dialog
                .transitions(origin)
                .overlay
                .expect("dialog transition")
                .kind,
            ui::MotionTransitionKind::Dialog
        );
        dialog.did_draw(origin + Duration::from_millis(160));
        assert!(dialog.is_due(origin + ui::MotionTimings::DIALOG));

        let mut popover = RedrawScheduler::new(origin, id("home", "one", None), false);
        popover.did_draw(origin);
        popover.observe(origin, id("home", "one", Some("popover:menu")), false);
        assert_eq!(
            popover
                .transitions(origin)
                .overlay
                .expect("popover transition")
                .kind,
            ui::MotionTransitionKind::Popover
        );
        popover.did_draw(origin + Duration::from_millis(150));
        assert!(popover.is_due(origin + ui::MotionTimings::POPOVER));
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
