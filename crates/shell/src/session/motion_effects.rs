use super::*;
use crate::session::overlays::ShellOverlayCategory;
use ratatui::{
    buffer::{Buffer, Cell, CellDiffOption},
    layout::{Position, Rect},
};
use std::sync::{Arc, Mutex};
use tachyonfx::{Effect, EffectManager, Interpolation, fx};

const PAGE_MS: u32 = ui::MotionTimings::PAGE.as_millis() as u32;
const DIALOG_MS: u32 = ui::MotionTimings::DIALOG.as_millis() as u32;
const POPOVER_MS: u32 = ui::MotionTimings::POPOVER.as_millis() as u32;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum MotionEffectId {
    #[default]
    Page,
    Overlay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OverlayIdentity {
    category: ShellOverlayCategory,
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
struct FrozenUnderlaySnapshot {
    screen: ShellScreen,
    bounds: Rect,
    snapshot: CellSnapshot,
}

#[derive(Debug, Clone)]
struct ActiveVisualOutgoing {
    identity: OverlayIdentity,
    old: CellSnapshot,
    underlay: FrozenUnderlaySnapshot,
    total: Duration,
    remaining: Duration,
    origin: ui::RevealOrigin,
}

#[derive(Debug, Clone)]
struct ExitSnapshotState {
    live_base: Option<Arc<Mutex<Option<BaseFrameSnapshot>>>>,
    old: Vec<(Position, Cell)>,
    underlay: Option<Vec<(Position, Cell)>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeferredClose {
    routed: RoutedEvent,
    overlay: OverlayIdentity,
}

/// Retain the underlying frame so opening and closing never expose blank cells.
#[derive(Debug, Default)]
pub(super) struct ShellMotionEffects {
    manager: EffectManager<MotionEffectId>,
    screen: Option<ShellScreen>,
    overlay: Option<OverlayIdentity>,
    covered_page_overlay: Option<(ShellScreen, OverlayIdentity, ui::RevealOrigin)>,
    overlay_snapshot: Option<CellSnapshot>,
    bounds: Option<Rect>,
    reduced: bool,
    overlay_gate: Duration,
    deferred_close: Option<DeferredClose>,
    exiting: bool,
    completed_exit: Option<OverlayIdentity>,
    base_snapshot: Option<BaseFrameSnapshot>,
    live_base: Arc<Mutex<Option<BaseFrameSnapshot>>>,
    overlay_underlay_snapshot: Option<FrozenUnderlaySnapshot>,
    effects_scheduled_since_process: bool,
    outgoing_block_remaining: Duration,
    active_visual_outgoing: Option<ActiveVisualOutgoing>,
    exit_confirmation: bool,
    page: Option<PageInstanceId>,
    page_gate: Duration,
    page_depth: usize,
    overlay_origin: ui::RevealOrigin,
}

impl ShellMotionEffects {
    pub(super) fn update_layout(
        &mut self,
        state: &ShellSession,
        layout: &ui::ShellFrameLayout,
        theme: ui::ThemeTokens,
        reduced: bool,
    ) {
        self.update_areas(
            state,
            layout.bounds,
            layout.main,
            layout.status_message,
            theme,
            reduced,
        );
    }

    #[cfg(test)]
    fn update(
        &mut self,
        state: &ShellSession,
        full_area: Rect,
        page_area: Rect,
        status_area: Option<Rect>,
        theme: ui::ThemeTokens,
        reduced: bool,
    ) {
        self.update_areas(
            state,
            full_area,
            shell_main_area(page_area),
            status_area,
            theme,
            reduced,
        );
    }

    fn update_areas(
        &mut self,
        state: &ShellSession,
        full_area: Rect,
        page_area: Rect,
        _status_area: Option<Rect>,
        theme: ui::ThemeTokens,
        reduced: bool,
    ) {
        if reduced {
            if !self.reduced || self.manager.is_running() {
                self.clear();
            }
            self.completed_exit = None;
            self.reduced = true;
            self.remember(state, full_area);
            return;
        }

        self.reduced = false;
        if self.bounds.is_some_and(|bounds| bounds != full_area) {
            self.clear();
        }

        let screen = state.content_screen();
        let previous_base = self.base_snapshot.clone();
        let returning = state.screen_stack().len() < self.page_depth;
        let page_changed = self.screen.is_some_and(|previous| previous != screen)
            || self
                .page
                .is_some_and(|page| page != state.content_page_id());
        if page_changed {
            // Outgoing closures carry old page cells. Drop them before scheduling
            // the new page reveal, even when terminal geometry did not change.
            self.clear();
            self.deferred_close = None;
            self.exiting = false;
        }
        let has_overlay = current_overlay(state)
            .is_some_and(|overlay| overlay.kind != ui::MotionOverlayKind::Toast);
        let exit_confirmation = state.active_screen() == ShellScreen::ExitConfirm;
        let exit_confirmation_transition = self.exit_confirmation != exit_confirmation;
        if exit_confirmation && exit_confirmation_transition {
            // A unique-effect cancellation still processes the superseded effect
            // once before removing it. Drop the visual queue outright so the exit
            // dialog cannot inherit one last page frame and flash underneath.
            self.manager = EffectManager::default();
            self.effects_scheduled_since_process = false;
            self.overlay_snapshot = None;
            self.overlay_underlay_snapshot = None;
            self.overlay_gate = Duration::ZERO;
            self.page_gate = Duration::ZERO;
            self.outgoing_block_remaining = Duration::ZERO;
            self.active_visual_outgoing = None;
        }
        if !exit_confirmation && !has_overlay && page_changed {
            let area = page_area;
            if !area.is_empty() {
                let fx = page_effect(area, previous_base, returning);
                self.schedule(MotionEffectId::Page, fx);
                self.page_gate = Duration::from_millis(u64::from(PAGE_MS));
            }
        }

        let overlay = current_overlay(state);
        if self.overlay != overlay {
            if overlay
                .as_ref()
                .is_some_and(|overlay| overlay.kind != ui::MotionOverlayKind::Toast)
                && self
                    .overlay
                    .as_ref()
                    .is_none_or(|overlay| overlay.kind == ui::MotionOverlayKind::Toast)
            {
                // The modal must not inherit a still-running page transition.
                self.manager = EffectManager::default();
                self.page_gate = Duration::ZERO;
                self.effects_scheduled_since_process = false;
            }
            let previous_overlay = self.overlay.clone();
            let covering_page = overlay
                .as_ref()
                .is_some_and(|new| new.category == ShellOverlayCategory::ShellModal)
                && previous_overlay
                    .as_ref()
                    .is_some_and(|old| is_page_overlay(old));
            if covering_page {
                self.covered_page_overlay = previous_overlay
                    .clone()
                    .map(|old| (screen, old, self.overlay_origin));
            }
            let revealing_page = previous_overlay
                .as_ref()
                .is_some_and(|old| old.category == ShellOverlayCategory::ShellModal)
                && self.covered_page_overlay.as_ref().is_some_and(
                    |(covered_screen, covered, _)| {
                        *covered_screen == screen && overlay.as_ref() == Some(covered)
                    },
                );
            let revealed_origin =
                revealing_page.then(|| self.covered_page_overlay.as_ref().unwrap().2);
            if previous_overlay
                .as_ref()
                .is_some_and(|old| old.category == ShellOverlayCategory::ShellModal)
                && overlay
                    .as_ref()
                    .is_none_or(|new| new.category != ShellOverlayCategory::ShellModal)
            {
                self.covered_page_overlay = None;
            }
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
                if let Some(new) = overlay.as_ref().filter(|new| {
                    !revealing_page && !new.immediate && new.kind != ui::MotionOverlayKind::Toast
                }) && let Some(area) = overlay_area(state)
                    && let Some(underlay) =
                        self.freeze_underlay(state.content_screen(), full_area, area)
                {
                    self.overlay_underlay_snapshot = Some(underlay);
                    self.schedule(
                        MotionEffectId::Overlay,
                        self.enter_overlay(state, new.kind, area, theme),
                    );
                    self.overlay_gate = overlay_duration(new.kind);
                }
            } else if critical {
                self.manager.cancel_unique_effect(MotionEffectId::Overlay);
                self.overlay_snapshot = None;
                self.overlay_gate = Duration::ZERO;
                self.exiting = false;
                self.overlay_underlay_snapshot = None;
                self.outgoing_block_remaining = Duration::ZERO;
                self.active_visual_outgoing = None;
            } else if covering_page {
                // The lower page overlay remains visible below the shell modal.
                // It is not an outgoing sibling and must not be animated away.
                self.manager = EffectManager::default();
                self.effects_scheduled_since_process = false;
                self.active_visual_outgoing = None;
                self.outgoing_block_remaining = Duration::ZERO;
                self.overlay_snapshot = None;
                self.overlay_underlay_snapshot = None;
                self.overlay_gate = Duration::ZERO;
                if let (Some(new), Some(area)) = (overlay.as_ref(), overlay_area(state)) {
                    self.schedule(
                        MotionEffectId::Overlay,
                        self.enter_overlay(state, new.kind, area, theme),
                    );
                    self.overlay_gate = overlay_duration(new.kind);
                }
            } else if restoring_deferred {
                self.manager.cancel_unique_effect(MotionEffectId::Overlay);
                self.overlay_gate = Duration::ZERO;
                self.exiting = true;
            } else {
                if let (Some(visual), Some(new)) =
                    (self.active_visual_outgoing.clone(), overlay.as_ref())
                    && visual.remaining > Duration::ZERO
                    && visual.underlay.screen == screen
                    && visual.underlay.bounds == full_area
                    && new.kind != ui::MotionOverlayKind::Toast
                {
                    let replacement = overlay_area(state).and_then(|new_area| {
                        let union = visual.old.area.union(new_area);
                        let base = self.freeze_underlay(screen, full_area, union)?;
                        let new_underlay = self.freeze_underlay(screen, full_area, new_area)?;
                        Some((base, new_underlay, new_area))
                    });
                    if let Some((base, new_underlay, new_area)) = replacement {
                        let consumed = visual.total.saturating_sub(visual.remaining);
                        let linear = consumed.as_secs_f32() / visual.total.as_secs_f32();
                        let start_alpha = 1.0 - (1.0 - linear.clamp(0.0, 1.0)).powi(2);
                        let outgoing = self.outgoing_snapshot_effect_from(
                            visual.old.clone(),
                            Some(base.snapshot),
                            visual.identity.kind,
                            start_alpha,
                            visual.remaining,
                            visual.origin,
                        );
                        self.schedule(
                            MotionEffectId::Overlay,
                            fx::sequence(&[
                                outgoing,
                                self.enter_overlay(state, new.kind, new_area, theme),
                            ]),
                        );
                        self.overlay_underlay_snapshot = Some(new_underlay);
                        self.overlay_gate = visual.remaining + overlay_duration(new.kind);
                        self.outgoing_block_remaining = visual.remaining;
                    } else {
                        self.manager.cancel_unique_effect(MotionEffectId::Overlay);
                        self.active_visual_outgoing = None;
                        self.overlay_snapshot = None;
                        self.overlay_underlay_snapshot = None;
                        self.overlay_gate = Duration::ZERO;
                        self.outgoing_block_remaining = Duration::ZERO;
                    }
                } else {
                    match (&previous_overlay, &overlay) {
                        (Some(old), Some(new))
                            if !revealing_page
                                && old.kind != ui::MotionOverlayKind::Toast
                                && new.kind != ui::MotionOverlayKind::Toast =>
                        {
                            self.overlay_gate = Duration::ZERO;
                            self.outgoing_block_remaining = Duration::ZERO;
                            let old_snapshot = self.overlay_snapshot.take();
                            let new_area = overlay_area(state);
                            let replacement = old_snapshot.and_then(|old_snapshot| {
                                let new_area = new_area?;
                                let union = old_snapshot.area.union(new_area);
                                let base =
                                    self.freeze_underlay(state.content_screen(), full_area, union)?;
                                let old_underlay = self.freeze_underlay(
                                    state.content_screen(),
                                    full_area,
                                    old_snapshot.area,
                                )?;
                                let new_underlay = self.freeze_underlay(
                                    state.content_screen(),
                                    full_area,
                                    new_area,
                                )?;
                                Some((old_snapshot, base, old_underlay, new_underlay, new_area))
                            });
                            if let Some((
                                old_snapshot,
                                base,
                                old_underlay,
                                new_underlay,
                                new_area,
                            )) = replacement
                            {
                                let outgoing = self.outgoing_snapshot_effect(
                                    old_snapshot.clone(),
                                    Some(base.snapshot),
                                    old.kind,
                                );
                                let incoming = self.enter_overlay(state, new.kind, new_area, theme);
                                self.schedule(
                                    MotionEffectId::Overlay,
                                    fx::sequence(&[outgoing, incoming]),
                                );
                                self.overlay_underlay_snapshot = Some(new_underlay);
                                self.overlay_gate =
                                    overlay_duration(old.kind) + overlay_duration(new.kind);
                                self.outgoing_block_remaining = overlay_duration(old.kind);
                                self.active_visual_outgoing = Some(ActiveVisualOutgoing {
                                    identity: old.clone(),
                                    old: old_snapshot,
                                    underlay: old_underlay,
                                    total: overlay_duration(old.kind),
                                    remaining: overlay_duration(old.kind),
                                    origin: self.overlay_origin,
                                });
                            } else {
                                self.manager.cancel_unique_effect(MotionEffectId::Overlay);
                                self.overlay_snapshot = None;
                                self.overlay_underlay_snapshot = None;
                            }
                        }
                        (Some(old), _) if old.kind != ui::MotionOverlayKind::Toast => {
                            self.overlay_gate = Duration::ZERO;
                            let outgoing = self.overlay_snapshot.take().and_then(|snapshot| {
                                let underlay = self.overlay_underlay_snapshot.take()?;
                                (underlay.screen == screen && underlay.bounds == full_area)
                                    .then_some((snapshot, underlay))
                            });
                            if let Some((snapshot, underlay)) = outgoing {
                                self.schedule(
                                    MotionEffectId::Overlay,
                                    self.outgoing_snapshot_effect(
                                        snapshot.clone(),
                                        Some(underlay.snapshot.clone()),
                                        old.kind,
                                    ),
                                );
                                self.overlay_gate = overlay_duration(old.kind);
                                self.outgoing_block_remaining = overlay_duration(old.kind);
                                self.active_visual_outgoing = Some(ActiveVisualOutgoing {
                                    identity: old.clone(),
                                    old: snapshot,
                                    underlay,
                                    total: overlay_duration(old.kind),
                                    remaining: overlay_duration(old.kind),
                                    origin: self.overlay_origin,
                                });
                            } else {
                                self.manager.cancel_unique_effect(MotionEffectId::Overlay);
                                self.active_visual_outgoing = None;
                                self.outgoing_block_remaining = Duration::ZERO;
                            }
                        }
                        (_, Some(new)) if new.kind != ui::MotionOverlayKind::Toast => {
                            if let Some(area) = overlay_area(state) {
                                self.overlay_underlay_snapshot =
                                    self.freeze_underlay(state.content_screen(), full_area, area);
                                self.schedule(
                                    MotionEffectId::Overlay,
                                    self.enter_overlay(state, new.kind, area, theme),
                                );
                                self.overlay_gate = overlay_duration(new.kind);
                            }
                        }
                        _ => {}
                    }
                }
            }
            if let (Some(new), Some(area)) = (overlay.as_ref(), overlay_area(state)) {
                self.overlay_origin =
                    revealed_origin.unwrap_or_else(|| overlay_origin(state, new.kind, area));
            }
        }
        self.remember(state, full_area);
    }

    /// Preserve the shell-modal capture phase through its asynchronous exit.
    pub(super) fn needs_shell_modal_base(&self) -> bool {
        self.overlay
            .as_ref()
            .is_some_and(|overlay| overlay.category == ShellOverlayCategory::ShellModal)
            || self
                .active_visual_outgoing
                .as_ref()
                .is_some_and(|outgoing| {
                    outgoing.identity.category == ShellOverlayCategory::ShellModal
                })
            || self
                .deferred_close
                .as_ref()
                .is_some_and(|close| close.overlay.category == ShellOverlayCategory::ShellModal)
    }

    /// Capture below the animated overlay: page content, or page/chrome for shell modals.
    pub(super) fn capture_base(&mut self, buffer: &Buffer, state: &ShellSession) {
        self.base_snapshot =
            snapshot_normal_cells(buffer, buffer.area).map(|snapshot| BaseFrameSnapshot {
                screen: state.content_screen(),
                bounds: buffer.area,
                cells: snapshot.cells,
            });
        *self.live_base.lock().expect("motion base lock") = self.base_snapshot.clone();
        if let Some(area) = overlay_area(state) {
            self.overlay_underlay_snapshot =
                self.freeze_underlay(state.content_screen(), buffer.area, area);
        }
    }

    /// Capture completed widgets before any effect mutates their cells.
    pub(super) fn capture_overlay(&mut self, buffer: &Buffer, state: &ShellSession) {
        if current_overlay(state)
            .is_some_and(|overlay| overlay.kind != ui::MotionOverlayKind::Toast)
        {
            self.overlay_snapshot =
                overlay_area(state).and_then(|area| snapshot_normal_cells(buffer, area));
        }
    }

    pub(super) fn process(&mut self, delta: Duration, buffer: &mut Buffer, _state: &ShellSession) {
        if self.reduced {
            return;
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
        self.page_gate = self.page_gate.saturating_sub(effective_delta);
        self.outgoing_block_remaining = self
            .outgoing_block_remaining
            .saturating_sub(effective_delta);
        if let Some(outgoing) = self.active_visual_outgoing.as_mut() {
            outgoing.remaining = outgoing.remaining.saturating_sub(effective_delta);
            if outgoing.remaining == Duration::ZERO {
                self.active_visual_outgoing = None;
            }
        }
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
    pub(super) fn blocks_before_route(&self, input: &InputEvent) -> bool {
        if self.reduced
            || matches!(input, InputEvent::Tick | InputEvent::Shutdown)
            || matches!(
                input,
                InputEvent::Resize { .. } | InputEvent::FocusGained | InputEvent::FocusLost
            )
            || self
                .overlay
                .as_ref()
                .is_some_and(|overlay| overlay.immediate)
        {
            return false;
        }
        if self.page_gate > Duration::ZERO && input.is_mouse() {
            return true;
        }
        if self.outgoing_block_remaining > Duration::ZERO
            && (input.is_keyboard() || input.is_mouse())
        {
            return true;
        }
        if self.exiting {
            return true;
        }
        let escape = matches!(input, InputEvent::Key(key) if key.label() == "Esc");
        self.overlay_gate > Duration::ZERO && !escape && (input.is_keyboard() || input.is_mouse())
    }

    pub(super) fn intercept_input(&mut self, routed: &RoutedEvent) -> MotionInputDisposition {
        let input = &routed.input;
        if self.blocks_before_route(input) {
            return MotionInputDisposition::Block;
        }
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
        let escape = matches!(input, InputEvent::Key(key) if key.label() == "Esc");
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
            if Some(underlay.screen) != self.screen || Some(underlay.bounds) != self.bounds {
                self.overlay_underlay_snapshot = None;
                return MotionInputDisposition::Apply;
            }
            self.deferred_close = Some(DeferredClose {
                routed: routed.clone(),
                overlay: overlay.clone(),
            });
            self.overlay_gate = overlay_duration(overlay.kind);
            self.outgoing_block_remaining = overlay_duration(overlay.kind);
            self.exiting = true;
            let exit =
                self.outgoing_snapshot_effect(snapshot, Some(underlay.snapshot), overlay.kind);
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

    fn enter_overlay(
        &self,
        state: &ShellSession,
        kind: ui::MotionOverlayKind,
        area: Rect,
        theme: ui::ThemeTokens,
    ) -> Effect {
        overlay_enter_effect(
            kind,
            area,
            theme,
            overlay_origin(state, kind, area),
            self.live_base.clone(),
        )
    }

    fn outgoing_snapshot_effect(
        &self,
        snapshot: CellSnapshot,
        underlay: Option<CellSnapshot>,
        kind: ui::MotionOverlayKind,
    ) -> Effect {
        self.outgoing_snapshot_effect_from(
            snapshot,
            underlay,
            kind,
            0.0,
            overlay_duration(kind),
            self.overlay_origin,
        )
    }

    fn outgoing_snapshot_effect_from(
        &self,
        snapshot: CellSnapshot,
        underlay: Option<CellSnapshot>,
        kind: ui::MotionOverlayKind,
        start_alpha: f32,
        duration: Duration,
        origin: ui::RevealOrigin,
    ) -> Effect {
        outgoing_snapshot_effect_from(
            snapshot,
            underlay,
            kind,
            start_alpha,
            duration,
            Some(self.live_base.clone()),
            origin,
        )
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
    ) -> Option<FrozenUnderlaySnapshot> {
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
        (!cells.is_empty()).then_some(FrozenUnderlaySnapshot {
            screen,
            bounds,
            snapshot: CellSnapshot { area, cells },
        })
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
        *self.live_base.lock().expect("motion base lock") = None;
        self.overlay_underlay_snapshot = None;
        self.active_visual_outgoing = None;
        self.exit_confirmation = false;
        self.completed_exit = None;
        self.covered_page_overlay = None;
        self.page = None;
        self.page_gate = Duration::ZERO;
    }

    pub(super) fn remember(&mut self, state: &ShellSession, bounds: Rect) {
        self.screen = Some(state.content_screen());
        self.overlay = current_overlay(state);
        self.bounds = Some(bounds);
        self.exit_confirmation = state.active_screen() == ShellScreen::ExitConfirm;
        self.page_depth = state.screen_stack().len();
        self.page = Some(state.content_page_id());
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

fn page_effect(area: Rect, old: Option<BaseFrameSnapshot>, returning: bool) -> Effect {
    fx::effect_fn_buf(
        old,
        (PAGE_MS, Interpolation::QuadOut),
        move |old, context, buffer| {
            let Some(old) = old else {
                return;
            };
            let amount = (context.alpha() * 1_000.0).round() as u16;
            let visible = ui::reveal_rect(
                area,
                if returning { 1_000 - amount } else { amount },
                ui::RevealOrigin::Right,
            );
            let protected = protected_skip_positions(buffer);
            for (position, cell) in &old.cells {
                if area.contains(*position)
                    && (visible.contains(*position) == returning)
                    && !cell_overlaps_protected(*position, cell, &protected)
                {
                    buffer[*position] = cell.clone();
                }
            }
        },
    )
    .with_area(area)
}

fn overlay_origin(
    state: &ShellSession,
    kind: ui::MotionOverlayKind,
    area: Rect,
) -> ui::RevealOrigin {
    if kind != ui::MotionOverlayKind::Popover {
        return ui::RevealOrigin::Center;
    }
    let (x, y) = state
        .interaction
        .pointer_position
        .unwrap_or((area.x, area.y));
    match (x > area.x + area.width / 2, y > area.y + area.height / 2) {
        (false, false) => ui::RevealOrigin::TopLeft,
        (true, false) => ui::RevealOrigin::TopRight,
        (false, true) => ui::RevealOrigin::BottomLeft,
        (true, true) => ui::RevealOrigin::BottomRight,
    }
}

fn overlay_enter_effect(
    kind: ui::MotionOverlayKind,
    area: Rect,
    theme: ui::ThemeTokens,
    origin: ui::RevealOrigin,
    base: Arc<Mutex<Option<BaseFrameSnapshot>>>,
) -> Effect {
    fx::effect_fn_buf(
        base,
        (overlay_duration(kind), Interpolation::QuadOut),
        move |base, context, buffer| {
            let visible = ui::reveal_rect(area, (context.alpha() * 1_000.0).round() as u16, origin);
            let protected = protected_skip_positions(buffer);
            let base = base.lock().expect("motion base lock");
            let cells = base.as_ref().map(|base| {
                base.cells
                    .iter()
                    .map(|(point, cell)| (*point, cell))
                    .collect::<HashMap<_, _>>()
            });
            for position in area.intersection(buffer.area).positions() {
                if !visible.contains(position)
                    && !cell_overlaps_protected(position, &buffer[position], &protected)
                {
                    if let Some(cell) = cells.as_ref().and_then(|cells| cells.get(&position)) {
                        buffer[position] = (*cell).clone();
                    } else {
                        buffer[position]
                            .set_symbol(" ")
                            .set_fg(theme.text)
                            .set_bg(theme.canvas);
                    }
                }
            }
        },
    )
    .with_area(area)
}

#[cfg(test)]
fn outgoing_snapshot_effect(
    snapshot: CellSnapshot,
    underlay: Option<CellSnapshot>,
    kind: ui::MotionOverlayKind,
) -> Effect {
    let origin = if kind == ui::MotionOverlayKind::Popover {
        ui::RevealOrigin::TopLeft
    } else {
        ui::RevealOrigin::Center
    };
    outgoing_snapshot_effect_from(
        snapshot,
        underlay,
        kind,
        0.0,
        overlay_duration(kind),
        None,
        origin,
    )
}

fn outgoing_snapshot_effect_from(
    snapshot: CellSnapshot,
    underlay: Option<CellSnapshot>,
    kind: ui::MotionOverlayKind,
    start_alpha: f32,
    duration: Duration,
    live_base: Option<Arc<Mutex<Option<BaseFrameSnapshot>>>>,
    origin: ui::RevealOrigin,
) -> Effect {
    let panel_area = snapshot.area;
    let duration = u32::try_from(duration.as_millis()).unwrap_or(u32::MAX);
    let area = underlay
        .as_ref()
        .map_or(snapshot.area, |underlay| underlay.area);
    let underlay = underlay.map(|snapshot| snapshot.cells);
    let safe_positions = underlay.as_ref().map(|cells| {
        cells
            .iter()
            .filter(|(_, cell)| cell.diff_option != CellDiffOption::Skip)
            .map(|(position, _)| *position)
            .collect::<HashSet<_>>()
    });
    let old = snapshot
        .cells
        .into_iter()
        .filter(|(position, cell)| {
            cell.diff_option != CellDiffOption::Skip
                && safe_positions
                    .as_ref()
                    .is_none_or(|positions| positions.contains(position))
        })
        .collect();
    let state = ExitSnapshotState {
        old,
        underlay,
        live_base,
    };
    if kind == ui::MotionOverlayKind::Toast {
        return fx::consume_tick();
    }
    fx::effect_fn_buf(
        state,
        (duration, Interpolation::QuadOut),
        move |state, context, buffer| {
            let alpha = start_alpha + (1.0 - start_alpha) * context.alpha();
            let protected = exit_protected_positions(state, buffer);
            restore_underlay(state, buffer, &protected);
            let visible =
                ui::reveal_rect(panel_area, ((1.0 - alpha) * 1_000.0).round() as u16, origin);
            for (position, old) in &state.old {
                if visible.contains(*position)
                    && buffer.area.contains(*position)
                    && !cell_overlaps_protected(*position, old, &protected)
                {
                    buffer[*position] = old.clone();
                }
            }
        },
    )
    .with_area(area)
}

fn cell_overlaps_protected(position: Position, cell: &Cell, protected: &HashSet<Position>) -> bool {
    let width = ratatui::text::Span::raw(cell.symbol()).width().max(1);
    (0..width).any(|offset| {
        protected.contains(&Position::new(
            position.x.saturating_add(offset as u16),
            position.y,
        ))
    })
}

fn protected_skip_positions(buffer: &Buffer) -> HashSet<Position> {
    let mut protected: HashSet<_> = buffer
        .area
        .positions()
        .filter(|position| buffer[*position].diff_option == CellDiffOption::Skip)
        .collect();
    for position in buffer.area.positions() {
        if cell_overlaps_protected(position, &buffer[position], &protected) {
            let width = ratatui::text::Span::raw(buffer[position].symbol())
                .width()
                .max(1);
            for offset in 0..width {
                protected.insert(Position::new(
                    position.x.saturating_add(offset as u16),
                    position.y,
                ));
            }
        }
    }
    protected
}

fn exit_protected_positions(state: &ExitSnapshotState, buffer: &Buffer) -> HashSet<Position> {
    let mut protected = protected_skip_positions(buffer);
    if let Some(base) = state.live_base.as_ref()
        && let Ok(base) = base.lock()
        && let Some(base) = base.as_ref()
    {
        let safe: HashSet<_> = base.cells.iter().map(|(position, _)| *position).collect();
        protected.extend(
            buffer
                .area
                .positions()
                .filter(|position| !safe.contains(position)),
        );
    }
    protected
}

fn restore_underlay(state: &ExitSnapshotState, buffer: &mut Buffer, protected: &HashSet<Position>) {
    let live = state
        .live_base
        .as_ref()
        .and_then(|base| base.lock().ok()?.clone());
    let live_cells = live.as_ref().map(|base| {
        base.cells
            .iter()
            .map(|(p, c)| (*p, c))
            .collect::<std::collections::HashMap<_, _>>()
    });
    if let Some(underlay) = state.underlay.as_ref() {
        for (position, cell) in underlay {
            if buffer.area.contains(*position)
                && !cell_overlaps_protected(*position, cell, protected)
                && cell.diff_option != CellDiffOption::Skip
            {
                if let Some(live) = live_cells.as_ref() {
                    if let Some(current) = live.get(position)
                        && !cell_overlaps_protected(*position, current, protected)
                    {
                        buffer[*position] = (*current).clone();
                    }
                } else {
                    buffer[*position] = cell.clone();
                }
            }
        }
    }
}

fn snapshot_normal_cells(buffer: &Buffer, area: Rect) -> Option<CellSnapshot> {
    if area.is_empty() {
        return None;
    }
    let area = area.intersection(buffer.area);
    let protected = protected_skip_positions(buffer);
    let cells = area
        .positions()
        .filter_map(|position| {
            let cell = &buffer[position];
            (!cell_overlaps_protected(position, cell, &protected)).then(|| (position, cell.clone()))
        })
        .collect();
    Some(CellSnapshot { area, cells })
}

#[cfg(test)]
fn shell_main_area(page_area: Rect) -> Rect {
    match ui::compute_shell_layout(page_area) {
        ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
    }
}

fn is_page_overlay(overlay: &OverlayIdentity) -> bool {
    matches!(
        overlay.category,
        ShellOverlayCategory::PageDialog
            | ShellOverlayCategory::PagePopover
            | ShellOverlayCategory::ContextPopup
    )
}

fn current_overlay(state: &ShellSession) -> Option<OverlayIdentity> {
    state
        .active_overlay_descriptor()
        .filter(|overlay| {
            !(overlay.category == ShellOverlayCategory::ContextPopup
                && overlay.component() == Some(ShellComponent::ContextMenu))
        })
        .map(|overlay| OverlayIdentity {
            category: overlay.category,
            kind: overlay.kind,
            id: overlay.id,
            immediate: overlay.immediate,
        })
}

fn overlay_area(state: &ShellSession) -> Option<Rect> {
    // AA handles its own input rather than registering a page hit region. Use
    // its real dialog bounds so it shares the global dialog enter/exit effects.
    if let Some(model) = state.auto_admin_view() {
        return Some(ui::auto_admin_layout_in(state.auto_admin_available_area(), &model).dialog);
    }
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

#[cfg(test)]
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
#[path = "../../tests/unit/session/motion_effects.rs"]
mod tests;
