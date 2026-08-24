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
    prior: RedrawIdentity,
    current: RedrawIdentity,
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
            let screen_changed = self.current.screen != identity.screen;
            if screen_changed {
                self.focus_changed_at = None;
                self.focus_start = 1_000;
                self.focus_target = 1_000;
                self.overlay_changed_at = None;
                let overlay_progress = identity.overlay.as_ref().map_or(0, |_| 1_000);
                self.overlay_start = overlay_progress;
                self.overlay_target = overlay_progress;
                self.prior = identity.clone();
            } else if self.current.focus != identity.focus {
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
            if !screen_changed && self.current.overlay != identity.overlay {
                let active = rendered.overlay.filter(|motion| motion.active);
                self.overlay_start = active.map_or_else(
                    || match (&self.current.overlay, &identity.overlay) {
                        (Some(_), Some(_)) => 0,
                        (Some(_), None) => 1_000,
                        (None, Some(_)) | (None, None) => 0,
                    },
                    |motion| motion.progress,
                );
                self.overlay_target = if identity.overlay.is_some() { 1_000 } else { 0 };
                self.prior.overlay.clone_from(&self.current.overlay);
                self.overlay_changed_at = Some(now);
            }
            self.current = identity;
            let frame = ui::MotionFrame {
                now,
                delta: now.saturating_sub(self.last_frame_at),
                reduced_motion,
            };
            let transitions = self.transitions_for_frame(frame);
            let has_active_motion = [transitions.focus, transitions.overlay]
                .into_iter()
                .flatten()
                .any(|transition| transition.active);
            self.next_motion_frame = (!reduced_motion && has_active_motion).then(|| {
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
            screen: None,
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
    fn screen_changes_redraw_immediately_without_a_transition() {
        let origin = Instant::now();
        let mut scheduler = RedrawScheduler::new(origin, id("home", "one", Some("dialog")), false);
        scheduler.did_draw(origin);
        scheduler.observe(origin, id("settings", "two", None), false);
        assert!(scheduler.is_due(origin));
        assert_eq!(
            scheduler.transitions(origin),
            ui::MotionTransitions::default()
        );
        scheduler.did_draw(origin);
        assert!(!scheduler.is_due(origin + FRAME_INTERVAL));
    }

    #[test]
    fn screen_changes_cancel_an_in_flight_focus_transition() {
        let origin = Instant::now();
        let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
        scheduler.did_draw(origin);
        scheduler.observe(origin, id("home", "two", None), false);
        let changed = origin + Duration::from_millis(50);
        assert!(scheduler.transitions(changed).focus.is_some());

        scheduler.observe(changed, id("settings", "settings-list", None), false);

        assert_eq!(
            scheduler.transitions(changed),
            ui::MotionTransitions::default()
        );
        assert!(scheduler.is_due(changed));
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
    fn popover_reversals_preserve_the_rendered_progress() {
        let origin = Instant::now();
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
    fn settled_overlay_replacement_reveals_from_zero_before_becoming_ready() {
        let origin = Instant::now();
        for overlay in ["dialog-b", "popover:b"] {
            let mut scheduler =
                RedrawScheduler::new(origin, id("home", "one", Some("dialog-a")), false);
            scheduler.did_draw(origin);
            scheduler.observe(origin, id("home", "one", Some(overlay)), false);
            let start = scheduler
                .transitions(origin)
                .overlay
                .expect("replacement start");
            assert_eq!(start.direction, ui::MotionDirection::Replacing);
            assert_eq!(start.progress, 0);
            assert_eq!(start.phase_progress, 0);
            assert!(!start.interaction_ready());

            let duration = if overlay.contains("popover") {
                ui::MotionTimings::POPOVER
            } else {
                ui::MotionTimings::DIALOG
            };
            let before_end = scheduler
                .transitions(origin + duration.saturating_sub(Duration::from_millis(1)))
                .overlay
                .expect("replacement before end");
            assert!(before_end.active);
            assert!(before_end.phase_progress < 1_000);
            let end = scheduler
                .transitions(origin + duration)
                .overlay
                .expect("replacement end");
            assert!(!end.active);
            assert_eq!((end.progress, end.phase_progress), (1_000, 1_000));
        }
    }

    #[test]
    fn in_flight_overlay_replacement_resets_readiness_phase_without_visual_jump() {
        let origin = Instant::now();
        let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
        scheduler.did_draw(origin);
        scheduler.observe(origin, id("home", "one", Some("dialog-a")), false);
        let replaced_at = origin + Duration::from_millis(80);
        let carried = scheduler
            .transitions(replaced_at)
            .overlay
            .expect("entering dialog")
            .progress;
        assert!(carried > 500);

        scheduler.observe(replaced_at, id("home", "one", Some("dialog-b")), false);
        let replacement = scheduler
            .transitions(replaced_at)
            .overlay
            .expect("replacement start");
        assert_eq!(replacement.progress, carried);
        assert_eq!(replacement.phase_progress, 0);
        assert!(!replacement.interaction_ready());

        let ready = scheduler
            .transitions(replaced_at + Duration::from_millis(20))
            .overlay
            .expect("replacement threshold");
        assert!(ready.phase_progress >= 500);
        assert!(ready.interaction_ready());
    }

    #[test]
    fn newly_tracked_overlay_groups_cover_enter_exit_and_replacement_frames() {
        let origin = Instant::now();
        for (overlay, duration) in [
            ("launcher-confirm:launch", ui::MotionTimings::DIALOG),
            ("popover:editor-file", ui::MotionTimings::POPOVER),
            ("user-management:create", ui::MotionTimings::DIALOG),
        ] {
            let mut scheduler = RedrawScheduler::new(origin, id("screen", "owner", None), false);
            scheduler.did_draw(origin);
            scheduler.observe(origin, id("screen", "owner", Some(overlay)), false);
            let start = scheduler.transitions(origin).overlay.expect("enter start");
            assert_eq!((start.progress, start.phase_progress), (0, 0));
            assert!(!start.interaction_ready());
            let mid = scheduler
                .transitions(origin + duration / 2)
                .overlay
                .expect("enter mid");
            assert!(mid.active && mid.phase_progress > 0 && mid.phase_progress < 1_000);
            let final_frame = scheduler
                .transitions(origin + duration)
                .overlay
                .expect("enter final");
            assert_eq!(
                (final_frame.progress, final_frame.phase_progress),
                (1_000, 1_000)
            );
            assert!(final_frame.interaction_ready());

            scheduler.observe(origin + duration, id("screen", "owner", None), false);
            let exit = scheduler
                .transitions(origin + duration)
                .overlay
                .expect("exit start");
            assert_eq!(exit.direction, ui::MotionDirection::Exiting);
            assert_eq!((exit.progress, exit.phase_progress), (1_000, 0));
            assert!(exit.interaction_ready());
            let exited = scheduler
                .transitions(origin + duration * 2)
                .overlay
                .expect("exit final");
            assert!(!exited.active);
            assert_eq!((exited.progress, exited.phase_progress), (0, 1_000));
        }

        for (prior, replacement) in [
            ("launcher-confirm:launch", "launcher-confirm:remove"),
            ("popover:editor-file", "popover:editor-edit"),
            ("user-management:create", "user-management:password"),
        ] {
            let mut scheduler =
                RedrawScheduler::new(origin, id("screen", "owner", Some(prior)), false);
            scheduler.did_draw(origin);
            scheduler.observe(origin, id("screen", "owner", Some(replacement)), false);
            let motion = scheduler.transitions(origin).overlay.expect("replacement");
            assert_eq!(motion.direction, ui::MotionDirection::Replacing);
            assert_eq!((motion.progress, motion.phase_progress), (0, 0));
            assert!(!motion.interaction_ready());
        }
    }

    #[test]
    fn explorer_dialog_lifecycles_cover_all_semantic_variants() {
        let origin = Instant::now();
        let variants = [
            "explorer-restore-conflict",
            "explorer-operation-conflict",
            "explorer-input:new-folder",
            "explorer-input:new-text-file",
            "explorer-input:rename",
            "explorer-input:restore-destination",
        ];
        for overlay in variants {
            let mut scheduler = RedrawScheduler::new(origin, id("explorer", "owner", None), false);
            scheduler.did_draw(origin);
            scheduler.observe(origin, id("explorer", "owner", Some(overlay)), false);
            let start = scheduler.transitions(origin).overlay.expect("dialog start");
            assert_eq!((start.progress, start.phase_progress), (0, 0));
            assert!(!start.interaction_ready());
            let mid = scheduler
                .transitions(origin + ui::MotionTimings::DIALOG / 2)
                .overlay
                .expect("dialog mid");
            assert!(mid.active && mid.phase_progress >= 500);
            let final_frame = scheduler
                .transitions(origin + ui::MotionTimings::DIALOG)
                .overlay
                .expect("dialog final");
            assert!(!final_frame.active);
            assert_eq!(
                (final_frame.progress, final_frame.phase_progress),
                (1_000, 1_000)
            );

            scheduler.observe(
                origin + ui::MotionTimings::DIALOG,
                id("explorer", "owner", None),
                false,
            );
            let exit = scheduler
                .transitions(origin + ui::MotionTimings::DIALOG)
                .overlay
                .expect("dialog exit");
            assert_eq!(exit.direction, ui::MotionDirection::Exiting);
            assert!(exit.interaction_ready());
        }

        let mut replacement =
            RedrawScheduler::new(origin, id("explorer", "owner", Some(variants[0])), false);
        replacement.did_draw(origin);
        replacement.observe(origin, id("explorer", "owner", Some(variants[1])), false);
        let settled = replacement
            .transitions(origin)
            .overlay
            .expect("replacement");
        assert_eq!(settled.direction, ui::MotionDirection::Replacing);
        assert_eq!((settled.progress, settled.phase_progress), (0, 0));
        assert!(!settled.interaction_ready());

        let reduced = RedrawScheduler::new(origin, id("explorer", "owner", None), true);
        assert!(reduced.frame(origin).reduced_motion);
    }

    #[test]
    fn explorer_semantic_popover_and_dialog_replacements_restart_gated_phase() {
        let origin = Instant::now();
        for replacement in [
            "popover:explorer-sort",
            "popover:explorer-options",
            "popover:explorer-properties",
            "explorer-dialog:delete-to-trash",
            "explorer-dialog:dump-trash",
        ] {
            let mut scheduler = RedrawScheduler::new(
                origin,
                id("explorer", "owner", Some("popover:explorer-context")),
                false,
            );
            scheduler.did_draw(origin);
            scheduler.observe(origin, id("explorer", "owner", Some(replacement)), false);
            let start = scheduler
                .transitions(origin)
                .overlay
                .expect("replacement start");
            assert_eq!(start.direction, ui::MotionDirection::Replacing);
            assert_eq!((start.progress, start.phase_progress), (0, 0));
            assert!(!start.interaction_ready());
            let duration = if replacement.contains("popover") {
                ui::MotionTimings::POPOVER
            } else {
                ui::MotionTimings::DIALOG
            };
            let final_frame = scheduler
                .transitions(origin + duration)
                .overlay
                .expect("replacement final");
            assert_eq!(
                (final_frame.progress, final_frame.phase_progress),
                (1_000, 1_000)
            );
            assert!(final_frame.interaction_ready());
        }

        let reduced = RedrawScheduler::new(
            origin,
            id("explorer", "owner", Some("popover:explorer-context")),
            true,
        );
        assert!(reduced.frame(origin).reduced_motion);
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
        scheduler.observe(origin, id("home", "two", None), false);

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
