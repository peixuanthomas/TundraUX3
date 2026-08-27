use super::*;
use ratatui::{
    buffer::{Buffer, Cell, CellDiffOption},
    layout::{Position, Rect},
    style::Style,
};
use tachyonfx::{
    CellFilter, Effect, EffectManager, Interpolation, Motion, SimpleRng, fx,
    pattern::{DiagonalPattern, InstancedPattern, Pattern, RadialPattern, SweepPattern},
};

const PAGE_MS: u32 = 220;
const DIALOG_MS: u32 = 180;
const POPOVER_MS: u32 = 160;
const FOCUS_MS: u32 = 120;
const TOAST_ENTER_MS: u32 = 200;
const TOAST_EXIT_MS: u32 = 150;
const PREVIEW_MS: u32 = 260;
const EFFECT_SEED: u32 = 0x474c_4143;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum MotionEffectId {
    #[default]
    Page,
    Overlay,
    Focus,
    Toast,
    PreferencePreview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OverlayIdentity {
    kind: ui::MotionOverlayKind,
    id: String,
    immediate: bool,
}

#[derive(Debug, Clone)]
struct CellSnapshot {
    area: Rect,
    cells: Vec<(Position, Cell)>,
}

#[derive(Debug, Clone)]
struct BaseFrameSnapshot {
    screen: ShellScreen,
    bounds: Rect,
    cells: Vec<(Position, Cell)>,
}

#[derive(Debug, Clone)]
struct ExitSnapshotState {
    old: Vec<(Position, Cell)>,
    underlay: Option<Vec<(Position, Cell)>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeferredClose {
    routed: RoutedEvent,
    overlay: OverlayIdentity,
}

/// Post-widget Glacier Flow orchestration. It deliberately owns no layout: every area is
/// supplied by the ordinary shell layout/hit map, keeping visual motion out of routing.
#[derive(Debug, Default)]
pub(super) struct ShellMotionEffects {
    manager: EffectManager<MotionEffectId>,
    screen: Option<ShellScreen>,
    focus: Option<ShellComponent>,
    overlay: Option<OverlayIdentity>,
    overlay_snapshot: Option<CellSnapshot>,
    bounds: Option<Rect>,
    reduced: bool,
    overlay_gate: Duration,
    deferred_close: Option<DeferredClose>,
    exiting: bool,
    completed_exit: Option<OverlayIdentity>,
    base_snapshot: Option<BaseFrameSnapshot>,
    overlay_underlay_snapshot: Option<CellSnapshot>,
    effects_scheduled_since_process: bool,
    outgoing_block_remaining: Duration,
}

impl ShellMotionEffects {
    pub(super) fn update(
        &mut self,
        state: &ShellSession,
        full_area: Rect,
        page_area: Rect,
        status_area: Option<Rect>,
        theme: ui::ThemeTokens,
        reduced: bool,
    ) {
        if reduced {
            if !self.reduced || self.manager.is_running() {
                self.clear();
            }
            self.reduced = true;
            self.remember(state, full_area);
            return;
        }

        let was_reduced = self.reduced;
        self.reduced = false;
        if self.bounds.is_some_and(|bounds| bounds != full_area) {
            self.clear();
        }

        let screen = state.content_screen();
        if self.screen.is_some_and(|old| old != screen) {
            let area = shell_main_area(page_area);
            if !area.is_empty() {
                let fx = page_effect(screen, area, theme);
                self.schedule(MotionEffectId::Page, fx);
            }
        }

        let focus = state.focused_component();
        if self.focus.is_some_and(|old| old != focus)
            && let Some(area) = focused_area(state, focus)
        {
            self.schedule(
                MotionEffectId::Focus,
                fx::fade_from_fg(theme.accent_soft, (FOCUS_MS, Interpolation::QuadOut))
                    .with_area(area)
                    .with_filter(CellFilter::Text),
            );
        }

        let overlay = current_overlay(state);
        if self.overlay != overlay {
            let previous_overlay = self.overlay.clone();
            let completed_old = self.completed_exit.is_some()
                && self.completed_exit.as_ref() == self.overlay.as_ref();
            let critical = overlay.as_ref().is_some_and(|overlay| overlay.immediate);
            let restoring_deferred = self
                .deferred_close
                .as_ref()
                .is_some_and(|deferred| Some(&deferred.overlay) == overlay.as_ref());
            if completed_old {
                self.manager.cancel_unique_effect(MotionEffectId::Overlay);
                self.overlay_snapshot = None;
                self.overlay_gate = Duration::ZERO;
                self.completed_exit = None;
                self.overlay_underlay_snapshot = None;
                self.outgoing_block_remaining = Duration::ZERO;
            } else if critical {
                self.manager.cancel_unique_effect(MotionEffectId::Overlay);
                self.overlay_snapshot = None;
                self.overlay_gate = Duration::ZERO;
                self.exiting = false;
                self.overlay_underlay_snapshot = None;
                self.outgoing_block_remaining = Duration::ZERO;
            } else if restoring_deferred {
                self.manager.cancel_unique_effect(MotionEffectId::Overlay);
                self.overlay_gate = Duration::ZERO;
                self.exiting = true;
            } else {
                match (&previous_overlay, &overlay) {
                    (Some(old), Some(new))
                        if old.kind != ui::MotionOverlayKind::Toast
                            && new.kind != ui::MotionOverlayKind::Toast =>
                    {
                        let outgoing = self
                            .overlay_snapshot
                            .take()
                            .map(|snapshot| outgoing_snapshot_effect(snapshot, None, old.kind));
                        let incoming = overlay_area(state)
                            .map(|area| overlay_enter_effect(new.kind, area, theme));
                        self.overlay_gate = Duration::ZERO;
                        match (outgoing, incoming) {
                            (Some(outgoing), Some(incoming)) => {
                                self.schedule(
                                    MotionEffectId::Overlay,
                                    fx::sequence(&[outgoing, incoming]),
                                );
                                self.overlay_gate =
                                    overlay_duration(old.kind) + overlay_duration(new.kind) / 2;
                                self.outgoing_block_remaining = overlay_duration(old.kind);
                            }
                            (Some(effect), None) => {
                                self.schedule(MotionEffectId::Overlay, effect);
                                self.overlay_gate = overlay_duration(old.kind);
                                self.outgoing_block_remaining = overlay_duration(old.kind);
                            }
                            (None, Some(effect)) => {
                                self.schedule(MotionEffectId::Overlay, effect);
                                self.overlay_gate = overlay_duration(new.kind) / 2;
                            }
                            (None, None) => {}
                        }
                    }
                    (Some(old), _) if old.kind != ui::MotionOverlayKind::Toast => {
                        self.overlay_gate = Duration::ZERO;
                        if let Some(snapshot) = self.overlay_snapshot.take() {
                            self.schedule(
                                MotionEffectId::Overlay,
                                outgoing_snapshot_effect(snapshot, None, old.kind),
                            );
                            self.overlay_gate = overlay_duration(old.kind);
                            self.outgoing_block_remaining = overlay_duration(old.kind);
                        }
                    }
                    (_, Some(new)) if new.kind != ui::MotionOverlayKind::Toast => {
                        if let Some(area) = overlay_area(state) {
                            self.overlay_underlay_snapshot =
                                self.freeze_underlay(state.content_screen(), full_area, area);
                            self.schedule(
                                MotionEffectId::Overlay,
                                overlay_enter_effect(new.kind, area, theme),
                            );
                            self.overlay_gate = overlay_duration(new.kind) / 2;
                        }
                    }
                    _ => {}
                }
            }
            if let Some(area) = status_area.filter(|area| !area.is_empty()) {
                match (&previous_overlay, &overlay) {
                    (None, Some(new)) if new.kind == ui::MotionOverlayKind::Toast => self.schedule(
                        MotionEffectId::Toast,
                        fx::sweep_in(
                            Motion::LeftToRight,
                            4,
                            0,
                            theme.accent_soft,
                            (TOAST_ENTER_MS, Interpolation::QuadOut),
                        )
                        .with_area(area)
                        .with_filter(CellFilter::Text),
                    ),
                    (Some(old), None) if old.kind == ui::MotionOverlayKind::Toast => self.schedule(
                        MotionEffectId::Toast,
                        fx::fade_to_fg(theme.muted, (TOAST_EXIT_MS, Interpolation::QuadOut))
                            .with_area(area)
                            .with_filter(CellFilter::Text),
                    ),
                    _ => {}
                }
            }
        }

        if was_reduced && screen == ShellScreen::Settings {
            let area = shell_main_area(page_area);
            if !area.is_empty() {
                self.schedule(
                    MotionEffectId::PreferencePreview,
                    preference_preview_effect(area, theme),
                );
            }
        }
        self.remember(state, full_area);
    }

    pub(super) fn process(&mut self, delta: Duration, buffer: &mut Buffer, state: &ShellSession) {
        if self.reduced {
            return;
        }
        let overlay = current_overlay(state);
        if overlay
            .as_ref()
            .is_none_or(|overlay| overlay.kind == ui::MotionOverlayKind::Toast)
        {
            let snapshot = snapshot_normal_cells(buffer, buffer.area);
            self.base_snapshot = snapshot.map(|snapshot| BaseFrameSnapshot {
                screen: state.content_screen(),
                bounds: buffer.area,
                cells: snapshot.cells,
            });
        } else if let Some(area) = overlay_area(state) {
            self.overlay_snapshot = snapshot_normal_cells(buffer, area);
        }
        let effective_delta = if self.effects_scheduled_since_process {
            self.effects_scheduled_since_process = false;
            Duration::ZERO
        } else {
            delta
        };
        self.manager
            .process_effects(effective_delta, buffer, buffer.area);
        self.overlay_gate = self.overlay_gate.saturating_sub(effective_delta);
        self.outgoing_block_remaining = self
            .outgoing_block_remaining
            .saturating_sub(effective_delta);
    }

    pub(super) fn is_running(&self) -> bool {
        !self.reduced && self.manager.is_running()
    }

    pub(super) fn cancel_for_bounds_change(&mut self) {
        self.clear();
    }

    pub(super) fn cancel_for_suspend(&mut self, state: &ShellSession) -> Option<RoutedEvent> {
        self.clear();
        self.take_deferred_close(state)
    }

    // Escape is the one cancellation path identifiable before controller mutation at
    // this boundary. Other close buttons use the localized post-action snapshot path.
    pub(super) fn intercept_input(&mut self, routed: &RoutedEvent) -> MotionInputDisposition {
        let input = &routed.input;
        if self.reduced || matches!(input, InputEvent::Tick | InputEvent::Shutdown) {
            return MotionInputDisposition::Apply;
        }
        if matches!(
            input,
            InputEvent::Resize { .. } | InputEvent::FocusGained | InputEvent::FocusLost
        ) {
            return MotionInputDisposition::Apply;
        }
        if self
            .overlay
            .as_ref()
            .is_some_and(|overlay| overlay.immediate)
        {
            return MotionInputDisposition::Apply;
        }
        if self.outgoing_block_remaining > Duration::ZERO
            && (input.is_keyboard() || input.is_mouse())
        {
            return MotionInputDisposition::Block;
        }
        if self.exiting {
            return MotionInputDisposition::Block;
        }
        let escape = matches!(input, InputEvent::Key(key) if key.label() == "Esc");
        if self.overlay_gate > Duration::ZERO
            && !escape
            && (input.is_keyboard() || input.is_mouse())
        {
            return MotionInputDisposition::Block;
        }
        if (escape || routed.command.is_overlay_cancel_or_close())
            && let Some(overlay) = self.overlay.as_ref()
            && overlay.kind != ui::MotionOverlayKind::Toast
        {
            let Some(snapshot) = self.overlay_snapshot.clone() else {
                return MotionInputDisposition::Apply;
            };
            let Some(underlay) = self.overlay_underlay_snapshot.clone() else {
                return MotionInputDisposition::Apply;
            };
            self.deferred_close = Some(DeferredClose {
                routed: routed.clone(),
                overlay: overlay.clone(),
            });
            self.overlay_gate = overlay_duration(overlay.kind);
            self.outgoing_block_remaining = overlay_duration(overlay.kind);
            self.exiting = true;
            let exit = outgoing_snapshot_effect(snapshot, Some(underlay), overlay.kind);
            self.schedule(MotionEffectId::Overlay, exit);
            return MotionInputDisposition::Defer;
        }
        MotionInputDisposition::Apply
    }

    pub(super) fn take_deferred_close(&mut self, state: &ShellSession) -> Option<RoutedEvent> {
        if self.overlay_gate > Duration::ZERO {
            return None;
        }
        let deferred = self.deferred_close.as_ref()?;
        let current = current_overlay(state);
        if current.as_ref() == Some(&deferred.overlay) {
            let deferred = self.deferred_close.take().expect("deferred close exists");
            self.exiting = false;
            self.overlay_snapshot = None;
            self.completed_exit = Some(deferred.overlay);
            return Some(deferred.routed);
        }
        if current.as_ref().is_some_and(|overlay| overlay.immediate) {
            self.exiting = false;
            return None;
        }
        self.exiting = false;
        self.deferred_close = None;
        None
    }

    fn schedule(&mut self, id: MotionEffectId, effect: Effect) {
        self.manager.add_unique_effect(id, effect);
        self.effects_scheduled_since_process = true;
    }

    fn freeze_underlay(
        &self,
        screen: ShellScreen,
        bounds: Rect,
        area: Rect,
    ) -> Option<CellSnapshot> {
        let base = self.base_snapshot.as_ref()?;
        if base.screen != screen || base.bounds != bounds || area.is_empty() {
            return None;
        }
        let cells = base
            .cells
            .iter()
            .filter(|(position, _)| area.contains(*position))
            .cloned()
            .collect::<Vec<_>>();
        (!cells.is_empty()).then_some(CellSnapshot { area, cells })
    }

    fn clear(&mut self) {
        self.manager = EffectManager::default();
        self.overlay_snapshot = None;
        self.overlay_gate = Duration::ZERO;
        // Bounds/reduced-motion cancellation must flush an already deferred close on
        // the next runtime boundary, exactly once.
        self.exiting = self.deferred_close.is_some();
        self.effects_scheduled_since_process = false;
        self.outgoing_block_remaining = Duration::ZERO;
        self.base_snapshot = None;
        self.overlay_underlay_snapshot = None;
    }

    fn remember(&mut self, state: &ShellSession, bounds: Rect) {
        self.screen = Some(state.content_screen());
        self.focus = Some(state.focused_component());
        self.overlay = current_overlay(state);
        self.bounds = Some(bounds);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MotionInputDisposition {
    Apply,
    Defer,
    Block,
}

fn overlay_duration(kind: ui::MotionOverlayKind) -> Duration {
    Duration::from_millis(match kind {
        ui::MotionOverlayKind::Dialog => u64::from(DIALOG_MS),
        ui::MotionOverlayKind::Popover => u64::from(POPOVER_MS),
        ui::MotionOverlayKind::Toast => 0,
    })
}

fn page_effect(screen: ShellScreen, area: Rect, theme: ui::ThemeTokens) -> Effect {
    if matches!(screen, ShellScreen::Editor | ShellScreen::CommandLine) {
        return fx::sweep_in(
            Motion::LeftToRight,
            6,
            0,
            theme.canvas,
            (PAGE_MS, Interpolation::QuadOut),
        )
        .with_area(area)
        .with_filter(CellFilter::Text);
    }
    fx::parallel(&[
        fx::coalesce_from(
            Style::default().fg(theme.accent_soft),
            (PAGE_MS, Interpolation::QuadOut),
        )
        .with_area(area)
        .with_filter(CellFilter::Text)
        .with_pattern(DiagonalPattern::top_left_to_bottom_right().with_transition_width(6.0))
        .with_rng(SimpleRng::new(EFFECT_SEED)),
        fx::fade_from_fg(theme.accent_soft, (PAGE_MS, Interpolation::QuadOut))
            .with_area(area)
            .with_filter(CellFilter::Text)
            .with_pattern(DiagonalPattern::top_left_to_bottom_right().with_transition_width(6.0)),
    ])
}

fn overlay_enter_effect(kind: ui::MotionOverlayKind, area: Rect, theme: ui::ThemeTokens) -> Effect {
    match kind {
        ui::MotionOverlayKind::Dialog => fx::parallel(&[
            fx::coalesce_from(
                Style::default().fg(theme.accent_soft),
                (DIALOG_MS, Interpolation::QuadOut),
            )
            .with_filter(CellFilter::Text)
            .with_pattern(RadialPattern::center().with_transition_width(4.0))
            .with_rng(SimpleRng::new(EFFECT_SEED)),
            fx::fade_from_fg(theme.accent_soft, (DIALOG_MS, Interpolation::QuadOut))
                .with_filter(CellFilter::Text),
        ])
        .with_area(area),
        ui::MotionOverlayKind::Popover => fx::sweep_in(
            Motion::UpToDown,
            4,
            0,
            theme.accent_soft,
            (POPOVER_MS, Interpolation::QuadOut),
        )
        .with_area(area)
        .with_filter(CellFilter::Text),
        ui::MotionOverlayKind::Toast => fx::consume_tick(),
    }
}

fn preference_preview_effect(area: Rect, theme: ui::ThemeTokens) -> Effect {
    fx::parallel(&[
        fx::coalesce_from(
            Style::default().fg(theme.raised),
            (PREVIEW_MS, Interpolation::QuadOut),
        )
        .with_pattern(RadialPattern::center().with_transition_width(5.0))
        .with_rng(SimpleRng::new(EFFECT_SEED)),
        fx::fade_from_fg(theme.accent_soft, (PREVIEW_MS, Interpolation::QuadOut))
            .with_pattern(RadialPattern::center().with_transition_width(5.0)),
    ])
    .with_area(area)
    .with_filter(CellFilter::Text)
}

fn outgoing_snapshot_effect(
    snapshot: CellSnapshot,
    underlay: Option<CellSnapshot>,
    kind: ui::MotionOverlayKind,
) -> Effect {
    let duration = match kind {
        ui::MotionOverlayKind::Dialog => DIALOG_MS,
        _ => POPOVER_MS,
    };
    let area = snapshot.area;
    let state = ExitSnapshotState {
        old: snapshot.cells,
        underlay: underlay.map(|snapshot| snapshot.cells),
    };
    match kind {
        ui::MotionOverlayKind::Dialog => fx::effect_fn_buf(
            state.clone(),
            (duration, Interpolation::QuadOut),
            |state, context, buffer| {
                restore_underlay(state, buffer);
                if context.alpha() <= 0.0 {
                    for (position, old) in safe_old_cells(state) {
                        if buffer.area.contains(*position) {
                            buffer[*position] = old.clone();
                        }
                    }
                    return;
                }
                if context.alpha() >= 1.0 {
                    return;
                }
                let mut pattern = RadialPattern::center()
                    .with_transition_width(4.0)
                    .for_frame(context.alpha(), context.area);
                for (position, old) in safe_old_cells(state) {
                    if pattern.map_alpha(*position) < 0.5 && buffer.area.contains(*position) {
                        buffer[*position] = old.clone();
                    }
                }
            },
        )
        .with_area(area),
        ui::MotionOverlayKind::Popover => fx::effect_fn_buf(
            state,
            (duration, Interpolation::QuadOut),
            |state, context, buffer| {
                restore_underlay(state, buffer);
                if context.alpha() <= 0.0 {
                    for (position, old) in safe_old_cells(state) {
                        if buffer.area.contains(*position) {
                            buffer[*position] = old.clone();
                        }
                    }
                    return;
                }
                if context.alpha() >= 1.0 {
                    return;
                }
                let mut pattern =
                    SweepPattern::down_to_up(4).for_frame(context.alpha(), context.area);
                for (position, old) in safe_old_cells(state) {
                    if pattern.map_alpha(*position) < 0.5 && buffer.area.contains(*position) {
                        buffer[*position] = old.clone();
                    }
                }
            },
        )
        .with_area(area),
        ui::MotionOverlayKind::Toast => fx::consume_tick(),
    }
}

fn restore_underlay(state: &ExitSnapshotState, buffer: &mut Buffer) {
    if let Some(underlay) = state.underlay.as_ref() {
        for (position, cell) in underlay {
            if buffer.area.contains(*position) && cell.diff_option != CellDiffOption::Skip {
                buffer[*position] = cell.clone();
            }
        }
    }
}

fn safe_old_cells(state: &ExitSnapshotState) -> impl Iterator<Item = &(Position, Cell)> {
    state.old.iter().filter(|(position, cell)| {
        cell.diff_option != CellDiffOption::Skip
            && state.underlay.as_ref().is_none_or(|underlay| {
                underlay.iter().any(|(underlay_position, underlay_cell)| {
                    underlay_position == position
                        && underlay_cell.diff_option != CellDiffOption::Skip
                })
            })
    })
}

fn snapshot_normal_cells(buffer: &Buffer, area: Rect) -> Option<CellSnapshot> {
    if area.is_empty() {
        return None;
    }
    let area = area.intersection(buffer.area);
    let cells = area
        .positions()
        .filter_map(|position| {
            let cell = &buffer[position];
            (cell.diff_option != CellDiffOption::Skip).then(|| (position, cell.clone()))
        })
        .collect();
    Some(CellSnapshot { area, cells })
}

fn shell_main_area(page_area: Rect) -> Rect {
    match ui::compute_shell_layout(page_area) {
        ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
    }
}

fn current_overlay(state: &ShellSession) -> Option<OverlayIdentity> {
    state
        .active_overlay_descriptor()
        .map(|overlay| OverlayIdentity {
            kind: overlay.kind,
            id: overlay.id,
            immediate: overlay.immediate,
        })
}

fn overlay_area(state: &ShellSession) -> Option<Rect> {
    bounds_for_regions(
        state
            .hit_map()
            .regions()
            .iter()
            .filter(|region| {
                matches!(
                    region.layer,
                    ShellHitLayer::ShellModal | ShellHitLayer::AppOverlay
                )
            })
            .map(|region| region.area),
    )
}

fn focused_area(state: &ShellSession, focused: ShellComponent) -> Option<Rect> {
    bounds_for_regions(
        state
            .hit_map()
            .regions()
            .iter()
            .filter(|region| region.component == focused)
            .map(|region| region.area),
    )
}

fn bounds_for_regions(regions: impl Iterator<Item = Rect>) -> Option<Rect> {
    regions
        .filter(|area| !area.is_empty())
        .reduce(|a, b| a.union(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshots_exclude_image_protocol_skip_cells() {
        let area = Rect::new(0, 0, 2, 1);
        let mut buffer = Buffer::empty(area);
        buffer[(0, 0)].set_symbol("A");
        buffer[(1, 0)].diff_option = CellDiffOption::Skip;
        let snapshot = snapshot_normal_cells(&buffer, area).unwrap();
        assert_eq!(snapshot.cells.len(), 1);
        assert_eq!(snapshot.cells[0].0, Position::new(0, 0));
    }

    #[test]
    fn areas_are_bounded_unions_and_empty_regions_are_ignored() {
        assert_eq!(
            bounds_for_regions(
                [Rect::new(3, 4, 2, 2), Rect::new(1, 5, 3, 1), Rect::ZERO].into_iter()
            ),
            Some(Rect::new(1, 4, 4, 2))
        );
    }

    #[test]
    fn editor_page_recipe_never_changes_symbols() {
        let area = Rect::new(0, 0, 4, 1);
        let mut buffer = Buffer::with_lines(["rust"]);
        let before: Vec<_> = area
            .positions()
            .map(|p| buffer[p].symbol().to_owned())
            .collect();
        let theme = ui::ThemeTokens::glacier_night();
        let mut fx = page_effect(ShellScreen::Editor, area, theme);
        fx.process(Duration::from_millis(100), &mut buffer, area);
        let after: Vec<_> = area
            .positions()
            .map(|p| buffer[p].symbol().to_owned())
            .collect();
        assert_eq!(before, after);
    }

    #[test]
    fn semantic_cancel_is_deferred_and_replayed_once() {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        while state.notification_dismiss_active_modal_without_response() {}
        state.active_popup = Some(ShellPopup {
            owner: Some(ShellComponent::Home),
            anchor: (20, 10),
        });
        state.refresh_hit_map();
        let overlay = current_overlay(&state).unwrap();
        let mut motion = ShellMotionEffects {
            overlay: Some(overlay),
            overlay_snapshot: Some(CellSnapshot {
                area: Rect::new(1, 1, 2, 1),
                cells: Vec::new(),
            }),
            overlay_underlay_snapshot: Some(CellSnapshot {
                area: Rect::new(1, 1, 2, 1),
                cells: Vec::new(),
            }),
            ..ShellMotionEffects::default()
        };
        let routed = state
            .clone()
            .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
        assert_eq!(
            motion.intercept_input(&routed),
            MotionInputDisposition::Defer
        );
        assert!(motion.take_deferred_close(&state).is_none());
        motion.overlay_gate = Duration::ZERO;
        assert_eq!(motion.take_deferred_close(&state), Some(routed));
        assert!(motion.take_deferred_close(&state).is_none());
    }

    #[test]
    fn reduced_motion_cancels_effects_and_flushes_pending_close() {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
        state.apply_input(InputEvent::from_key_label("q"));
        let overlay = current_overlay(&state).unwrap();
        let routed = state
            .clone()
            .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
        let mut motion = ShellMotionEffects {
            overlay: Some(overlay.clone()),
            deferred_close: Some(DeferredClose { routed, overlay }),
            exiting: true,
            overlay_gate: Duration::from_millis(80),
            ..ShellMotionEffects::default()
        };
        motion.clear();
        assert_eq!(motion.overlay_gate, Duration::ZERO);
        assert!(motion.take_deferred_close(&state).is_some());
        assert!(motion.take_deferred_close(&state).is_none());
    }

    #[test]
    fn missing_overlay_snapshot_closes_immediately_without_invisible_delay() {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
        state.apply_input(InputEvent::from_key_label("q"));
        let routed = state
            .clone()
            .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
        let mut motion = ShellMotionEffects {
            overlay: current_overlay(&state),
            ..ShellMotionEffects::default()
        };
        assert_eq!(
            motion.intercept_input(&routed),
            MotionInputDisposition::Apply
        );
        assert_eq!(motion.overlay_gate, Duration::ZERO);
        assert!(!motion.manager.is_running());
    }

    #[test]
    fn resize_bypasses_exit_gate_and_flushes_original_route_once() {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
        state.apply_input(InputEvent::from_key_label("q"));
        let overlay = current_overlay(&state).unwrap();
        let close = state
            .clone()
            .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
        let resize = state.clone().route_input_at(
            InputEvent::Resize {
                width: 100,
                height: 30,
            },
            Instant::now(),
        );
        let mut motion = ShellMotionEffects {
            overlay: Some(overlay.clone()),
            deferred_close: Some(DeferredClose {
                routed: close.clone(),
                overlay,
            }),
            exiting: true,
            overlay_gate: Duration::from_millis(90),
            ..ShellMotionEffects::default()
        };
        assert_eq!(
            motion.intercept_input(&resize),
            MotionInputDisposition::Apply
        );
        let generation = state.hit_map_generation();
        state.apply_input(resize.input.clone());
        assert_eq!(state.terminal_size(), (100, 30));
        assert!(state.hit_map_generation() > generation);
        assert_eq!(state.hit_map().terminal_size(), (100, 30));
        motion.cancel_for_bounds_change();
        assert_eq!(motion.take_deferred_close(&state), Some(close));
        assert!(motion.take_deferred_close(&state).is_none());
        assert!(!motion.manager.is_running());
        assert!(motion.overlay_snapshot.is_none());
    }

    #[test]
    fn suspend_flushes_close_before_resume_resize_rebuilds_geometry() {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        state.apply_input(InputEvent::from_key_label("q"));
        let original = current_overlay(&state).unwrap();
        let routed = state
            .clone()
            .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
        let mut motion = ShellMotionEffects {
            overlay: Some(original),
            overlay_snapshot: Some(CellSnapshot {
                area: Rect::new(20, 10, 40, 10),
                cells: Vec::new(),
            }),
            overlay_underlay_snapshot: Some(CellSnapshot {
                area: Rect::new(20, 10, 40, 10),
                cells: Vec::new(),
            }),
            ..ShellMotionEffects::default()
        };
        assert_eq!(
            motion.intercept_input(&routed),
            MotionInputDisposition::Defer
        );
        let routed = motion.cancel_for_suspend(&state).expect("close flush");
        let platform = platform::native_platform();
        state.apply_routed_event(routed, platform.as_ref(), Instant::now());
        assert!(current_overlay(&state).is_none());
        assert!(motion.cancel_for_suspend(&state).is_none());

        let generation = state.hit_map_generation();
        state.apply_input(InputEvent::Resize {
            width: 100,
            height: 30,
        });
        assert_eq!(state.terminal_size(), (100, 30));
        assert!(state.hit_map_generation() > generation);
        assert_eq!(state.hit_map().terminal_size(), (100, 30));
        assert!(current_overlay(&state).is_none());
    }

    #[test]
    fn preempted_overlay_never_retargets_deferred_cancel() {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
        state.apply_input(InputEvent::from_key_label("q"));
        let overlay = OverlayIdentity {
            kind: ui::MotionOverlayKind::Dialog,
            id: "preempted:A".into(),
            immediate: false,
        };
        let routed = state
            .clone()
            .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
        let mut motion = ShellMotionEffects {
            deferred_close: Some(DeferredClose {
                routed,
                overlay: overlay.clone(),
            }),
            exiting: true,
            ..ShellMotionEffects::default()
        };
        assert_eq!(
            current_overlay(&state).unwrap().kind,
            ui::MotionOverlayKind::Dialog
        );
        assert_ne!(current_overlay(&state), Some(overlay));
        assert!(motion.take_deferred_close(&state).is_none());
        assert!(motion.deferred_close.is_none());
    }

    #[test]
    fn critical_modal_preempts_then_restores_original_deferred_route() {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
        state.apply_input(InputEvent::from_key_label("q"));
        let original = current_overlay(&state).unwrap();
        assert!(!original.immediate);
        let routed = state
            .clone()
            .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
        let mut motion = ShellMotionEffects {
            overlay: Some(original.clone()),
            overlay_snapshot: Some(CellSnapshot {
                area: Rect::new(20, 10, 40, 10),
                cells: Vec::new(),
            }),
            overlay_underlay_snapshot: Some(CellSnapshot {
                area: Rect::new(20, 10, 40, 10),
                cells: Vec::new(),
            }),
            ..ShellMotionEffects::default()
        };
        assert_eq!(
            motion.intercept_input(&routed),
            MotionInputDisposition::Defer
        );

        state.notify_critical_modal("Critical", "Interrupt", Vec::new());
        motion.update(
            &state,
            Rect::new(0, 0, 120, 40),
            Rect::new(0, 0, 120, 40),
            Some(Rect::new(0, 37, 120, 3)),
            ui::ThemeTokens::glacier_night(),
            false,
        );
        assert!(current_overlay(&state).unwrap().immediate);
        assert_eq!(motion.overlay_gate, Duration::ZERO);
        assert!(!motion.exiting);
        assert!(motion.overlay_snapshot.is_none());
        assert!(motion.take_deferred_close(&state).is_none());
        let critical_input = state
            .clone()
            .route_input_at(InputEvent::from_key_label("Tab"), Instant::now());
        assert_eq!(
            motion.intercept_input(&critical_input),
            MotionInputDisposition::Apply
        );

        assert!(state.notification_dismiss_active_modal_without_response());
        assert_eq!(current_overlay(&state), Some(original.clone()));
        motion.update(
            &state,
            Rect::new(0, 0, 120, 40),
            Rect::new(0, 0, 120, 40),
            Some(Rect::new(0, 37, 120, 3)),
            ui::ThemeTokens::glacier_night(),
            false,
        );
        assert_eq!(motion.take_deferred_close(&state), Some(routed));
        assert!(motion.take_deferred_close(&state).is_none());
    }

    #[test]
    fn critical_modal_without_prior_overlay_is_immediate_and_ungated() {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
        let mut motion = ShellMotionEffects::default();
        motion.update(
            &state,
            Rect::new(0, 0, 120, 40),
            Rect::new(0, 0, 120, 40),
            None,
            ui::ThemeTokens::glacier_night(),
            false,
        );
        state.notify_critical_modal("Critical", "Natural", Vec::new());
        motion.update(
            &state,
            Rect::new(0, 0, 120, 40),
            Rect::new(0, 0, 120, 40),
            None,
            ui::ThemeTokens::glacier_night(),
            false,
        );
        assert!(motion.overlay.as_ref().unwrap().immediate);
        assert_eq!(motion.overlay_gate, Duration::ZERO);
        assert!(motion.overlay_snapshot.is_none());
        let pointer = state.clone().route_input_at(
            InputEvent::mouse_down(PointerButton::Left, (1, 1)),
            Instant::now(),
        );
        assert_eq!(
            motion.intercept_input(&pointer),
            MotionInputDisposition::Apply
        );
    }

    #[test]
    fn identity_sync_gates_confirm_before_first_render() {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        while state.notification_dismiss_active_modal_without_response() {}
        assert!(current_overlay(&state).is_none());
        let mut motion = ShellMotionEffects::default();
        motion.update(
            &state,
            Rect::new(0, 0, 120, 40),
            Rect::new(0, 0, 120, 40),
            Some(Rect::new(0, 37, 120, 3)),
            ui::ThemeTokens::glacier_night(),
            false,
        );
        state.apply_input(InputEvent::from_key_label("q"));
        assert!(!current_overlay(&state).unwrap().immediate);
        assert!(overlay_area(&state).is_some());
        motion.update(
            &state,
            Rect::new(0, 0, 120, 40),
            Rect::new(0, 0, 120, 40),
            Some(Rect::new(0, 37, 120, 3)),
            ui::ThemeTokens::glacier_night(),
            false,
        );
        assert!(motion.overlay_gate > Duration::ZERO);
        let confirm = state
            .clone()
            .route_input_at(InputEvent::from_key_label("Enter"), Instant::now());
        assert_eq!(
            motion.intercept_input(&confirm),
            MotionInputDisposition::Block
        );
        let outside = state.clone().route_input_at(
            InputEvent::mouse_down(PointerButton::Left, (0, 0)),
            Instant::now(),
        );
        assert_eq!(
            motion.intercept_input(&outside),
            MotionInputDisposition::Block
        );
    }

    #[test]
    fn replacement_with_only_incoming_geometry_gates_from_mutation() {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
        let mut motion = ShellMotionEffects {
            overlay: Some(OverlayIdentity {
                kind: ui::MotionOverlayKind::Popover,
                id: "preempted".into(),
                immediate: false,
            }),
            ..ShellMotionEffects::default()
        };
        state.apply_input(InputEvent::from_key_label("q"));
        motion.update(
            &state,
            Rect::new(0, 0, 120, 40),
            Rect::new(0, 0, 120, 40),
            Some(Rect::new(0, 37, 120, 3)),
            ui::ThemeTokens::glacier_night(),
            false,
        );
        assert_eq!(
            motion.overlay_gate,
            overlay_duration(ui::MotionOverlayKind::Dialog) / 2
        );
        let confirm = state
            .clone()
            .route_input_at(InputEvent::from_key_label("Enter"), Instant::now());
        assert_eq!(
            motion.intercept_input(&confirm),
            MotionInputDisposition::Block
        );
    }

    fn exit_buffer(kind: ui::MotionOverlayKind, elapsed: Duration) -> Buffer {
        let full = Rect::new(0, 0, 11, 7);
        let area = Rect::new(2, 1, 7, 5);
        let mut old = Buffer::filled(full, Cell::new("O"));
        for position in full
            .positions()
            .filter(|position| !area.contains(*position))
        {
            old[position].set_symbol("Z");
        }
        let snapshot = snapshot_normal_cells(&old, area).unwrap();
        let mut underlay_buffer = Buffer::filled(full, Cell::new("N"));
        for position in full
            .positions()
            .filter(|position| !area.contains(*position))
        {
            underlay_buffer[position].set_symbol("Z");
        }
        let underlay = snapshot_normal_cells(&underlay_buffer, area).unwrap();
        let mut current = old;
        let mut effect = outgoing_snapshot_effect(snapshot, Some(underlay), kind);
        effect.process(elapsed, &mut current, full);
        current
    }

    #[test]
    fn dialog_and_popover_snapshot_exits_are_spatial_single_stage_and_end_natural() {
        let full = Rect::new(0, 0, 11, 7);
        let area = Rect::new(2, 1, 7, 5);
        for kind in [
            ui::MotionOverlayKind::Dialog,
            ui::MotionOverlayKind::Popover,
        ] {
            let start = exit_buffer(kind, Duration::ZERO);
            assert!(
                area.positions()
                    .all(|position| start[position].symbol() == "O")
            );
            assert!(
                full.positions()
                    .filter(|position| !area.contains(*position))
                    .all(|position| start[position].symbol() == "Z")
            );
            let duration = overlay_duration(kind);
            let final_frame = exit_buffer(kind, duration);
            assert!(
                area.positions()
                    .all(|position| final_frame[position].symbol() == "N")
            );

            let mut old = Buffer::filled(area, Cell::new("O"));
            let snapshot = snapshot_normal_cells(&old, area).unwrap();
            let mut effect = outgoing_snapshot_effect(snapshot, None, kind);
            effect.process(
                duration.saturating_sub(Duration::from_millis(1)),
                &mut old,
                area,
            );
            assert!(effect.running());
            effect.process(Duration::from_millis(1), &mut old, area);
            assert!(!effect.running());
        }
        let dialog_mid = exit_buffer(
            ui::MotionOverlayKind::Dialog,
            overlay_duration(ui::MotionOverlayKind::Dialog) / 2,
        );
        let popover_mid = exit_buffer(
            ui::MotionOverlayKind::Popover,
            overlay_duration(ui::MotionOverlayKind::Popover) / 2,
        );
        assert_ne!(dialog_mid, popover_mid);
    }

    #[test]
    fn radial_preference_preview_has_center_weight_and_finishes_natural() {
        let area = Rect::new(0, 0, 15, 9);
        let theme = ui::ThemeTokens::glacier_night();
        let mut natural = Buffer::filled(area, Cell::new("T"));
        natural.set_style(area, Style::default().fg(ratatui::style::Color::White));

        let mut start = natural.clone();
        preference_preview_effect(area, theme).process(Duration::ZERO, &mut start, area);
        let mut mid = natural.clone();
        preference_preview_effect(area, theme).process(
            Duration::from_millis(u64::from(PREVIEW_MS / 2)),
            &mut mid,
            area,
        );
        let mut end = natural.clone();
        preference_preview_effect(area, theme).process(
            Duration::from_millis(u64::from(PREVIEW_MS)),
            &mut end,
            area,
        );
        assert_ne!(start, natural);
        assert_ne!(mid[(7, 4)], mid[(7, 0)]);
        assert_eq!(end, natural);
    }

    #[test]
    fn newly_scheduled_effects_ignore_idle_delta_then_advance_normally() {
        let state = ShellSession::new(ShellLaunchConfig::default(), (40, 12));
        let area = Rect::new(0, 0, 40, 12);
        let mut buffer = Buffer::filled(area, Cell::new("T"));
        let mut motion = ShellMotionEffects::default();
        motion.overlay_gate = Duration::from_millis(DIALOG_MS.into());
        motion.schedule(
            MotionEffectId::Overlay,
            fx::fade_from_fg(
                ui::ThemeTokens::glacier_night().accent_soft,
                (DIALOG_MS, Interpolation::Linear),
            )
            .with_area(area),
        );
        motion.schedule(
            MotionEffectId::Focus,
            fx::fade_from_fg(
                ui::ThemeTokens::glacier_night().accent_soft,
                (FOCUS_MS, Interpolation::Linear),
            )
            .with_area(area),
        );
        motion.process(Duration::from_secs(1), &mut buffer, &state);
        assert!(motion.is_running());
        assert_eq!(motion.overlay_gate, Duration::from_millis(DIALOG_MS.into()));
        motion.process(Duration::from_millis(17), &mut buffer, &state);
        assert_eq!(
            motion.overlay_gate,
            Duration::from_millis(DIALOG_MS.into()) - Duration::from_millis(17)
        );

        motion.overlay_gate = Duration::from_millis(POPOVER_MS.into());
        motion.schedule(
            MotionEffectId::Overlay,
            fx::fade_from_fg(
                ui::ThemeTokens::glacier_night().accent_soft,
                (POPOVER_MS, Interpolation::Linear),
            )
            .with_area(area),
        );
        motion.process(Duration::from_secs(1), &mut buffer, &state);
        assert_eq!(
            motion.overlay_gate,
            Duration::from_millis(POPOVER_MS.into())
        );
    }

    #[test]
    fn post_mutation_outgoing_blocks_second_escape_only_for_old_phase() {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        while state.notification_dismiss_active_modal_without_response() {}
        state.ui.screen_stack = vec![ShellScreen::Settings];
        state.settings_state = Some(SettingsState {
            category: ui::SettingsCategory::Appearance,
            selected_field: ui::SettingsField::Theme,
            status: String::new(),
            scroll_offset: 0,
            picker: Some(SettingsPickerState {
                kind: ui::SettingsPickerKind::Theme,
                query: String::new(),
                selected_index: 0,
                window_start: 0,
                image_icons_supported: false,
            }),
            color_editor: None,
            weather_location_editor: None,
            file_extensions_editor: None,
            time_sync_server_editor: None,
            time_sync_validation_request_id: None,
        });
        state.refresh_hit_map();
        let old = current_overlay(&state).expect("settings picker");
        let area = Rect::new(20, 8, 30, 10);
        let mut old_buffer = Buffer::filled(area, Cell::new("P"));
        old_buffer[(20, 8)].set_symbol("P");
        let mut motion = ShellMotionEffects {
            overlay: Some(old),
            overlay_snapshot: snapshot_normal_cells(&old_buffer, area),
            bounds: Some(Rect::new(0, 0, 120, 40)),
            screen: Some(state.content_screen()),
            ..ShellMotionEffects::default()
        };
        state.settings_state.as_mut().unwrap().picker = None;
        state.refresh_hit_map();
        motion.update(
            &state,
            Rect::new(0, 0, 120, 40),
            Rect::new(0, 0, 120, 40),
            None,
            ui::ThemeTokens::glacier_night(),
            false,
        );
        assert_eq!(
            motion.outgoing_block_remaining,
            Duration::from_millis(POPOVER_MS.into())
        );
        let second_escape = state
            .clone()
            .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
        assert_eq!(
            motion.intercept_input(&second_escape),
            MotionInputDisposition::Block
        );
        let screen = state.content_screen();
        let mut natural = Buffer::filled(Rect::new(0, 0, 120, 40), Cell::new("N"));
        motion.process(Duration::from_secs(1), &mut natural, &state);
        assert_eq!(state.content_screen(), screen);
        assert_eq!(
            motion.outgoing_block_remaining,
            Duration::from_millis(POPOVER_MS.into())
        );
        motion.process(
            Duration::from_millis(POPOVER_MS.into()),
            &mut natural,
            &state,
        );
        assert_eq!(motion.outgoing_block_remaining, Duration::ZERO);
        assert_eq!(
            motion.intercept_input(&second_escape),
            MotionInputDisposition::Apply
        );
    }

    #[test]
    fn stale_base_screen_or_bounds_falls_back_to_immediate_close() {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        while state.notification_dismiss_active_modal_without_response() {}
        let mut motion = ShellMotionEffects {
            base_snapshot: Some(BaseFrameSnapshot {
                screen: ShellScreen::Settings,
                bounds: Rect::new(0, 0, 80, 24),
                cells: vec![(Position::new(1, 1), Cell::new("N"))],
            }),
            bounds: Some(Rect::new(0, 0, 120, 40)),
            screen: Some(state.content_screen()),
            ..ShellMotionEffects::default()
        };
        state.apply_input(InputEvent::from_key_label("q"));
        motion.update(
            &state,
            Rect::new(0, 0, 120, 40),
            Rect::new(0, 0, 120, 40),
            Some(Rect::new(0, 37, 120, 3)),
            ui::ThemeTokens::glacier_night(),
            false,
        );
        assert!(motion.overlay_underlay_snapshot.is_none());
        motion.overlay_snapshot = Some(CellSnapshot {
            area: Rect::new(20, 10, 40, 10),
            cells: vec![(Position::new(20, 10), Cell::new("A"))],
        });
        let close = state
            .clone()
            .route_input_at(InputEvent::from_key_label("Esc"), Instant::now());
        assert_eq!(
            motion.intercept_input(&close),
            MotionInputDisposition::Apply
        );
        assert!(motion.deferred_close.is_none());
    }
}
