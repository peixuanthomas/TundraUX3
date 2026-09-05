use super::*;
use crate::session::queries::ShellOverlayCategory;
use ratatui::{
    buffer::{Buffer, Cell, CellDiffOption},
    layout::{Margin, Position, Rect},
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
    overlay_underlay_snapshot: Option<FrozenUnderlaySnapshot>,
    effects_scheduled_since_process: bool,
    outgoing_block_remaining: Duration,
    active_visual_outgoing: Option<ActiveVisualOutgoing>,
    suppress_focus_after_generic_popup: bool,
    exit_confirmation: bool,
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
            self.completed_exit = None;
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
            self.outgoing_block_remaining = Duration::ZERO;
            self.active_visual_outgoing = None;
        }
        if !exit_confirmation && self.screen.is_some_and(|old| old != screen) {
            let area = shell_main_area(page_area);
            if !area.is_empty() {
                let fx = page_effect(screen, area, theme);
                self.schedule(MotionEffectId::Page, fx);
            }
        }

        let generic_popup = is_unrendered_generic_popup(state);
        let focus = state.focused_component();
        let suppress_focus = if generic_popup {
            self.suppress_focus_after_generic_popup = true;
            true
        } else if self.suppress_focus_after_generic_popup {
            self.suppress_focus_after_generic_popup = false;
            true
        } else {
            false
        };
        if !suppress_focus
            && !exit_confirmation
            && !self.exit_confirmation
            && self.focus.is_some_and(|old| old != focus)
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
                if let Some(new) = overlay
                    .as_ref()
                    .filter(|new| !new.immediate && new.kind != ui::MotionOverlayKind::Toast)
                    && let Some(area) = overlay_area(state)
                    && let Some(underlay) =
                        self.freeze_underlay(state.content_screen(), full_area, area)
                {
                    self.overlay_underlay_snapshot = Some(underlay);
                    self.schedule(
                        MotionEffectId::Overlay,
                        overlay_enter_effect(new.kind, area, theme),
                    );
                    self.overlay_gate = overlay_duration(new.kind) / 2;
                }
            } else if critical {
                self.manager.cancel_unique_effect(MotionEffectId::Overlay);
                self.overlay_snapshot = None;
                self.overlay_gate = Duration::ZERO;
                self.exiting = false;
                self.overlay_underlay_snapshot = None;
                self.outgoing_block_remaining = Duration::ZERO;
                self.active_visual_outgoing = None;
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
                        let outgoing = outgoing_snapshot_effect_from(
                            visual.old.clone(),
                            Some(base.snapshot),
                            visual.identity.kind,
                            start_alpha,
                            visual.remaining,
                        );
                        self.schedule(
                            MotionEffectId::Overlay,
                            fx::sequence(&[
                                outgoing,
                                overlay_enter_effect(new.kind, new_area, theme),
                            ]),
                        );
                        self.overlay_underlay_snapshot = Some(new_underlay);
                        self.overlay_gate = visual.remaining + overlay_duration(new.kind) / 2;
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
                            if old.kind != ui::MotionOverlayKind::Toast
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
                                let outgoing = outgoing_snapshot_effect(
                                    old_snapshot.clone(),
                                    Some(base.snapshot),
                                    old.kind,
                                );
                                let incoming = overlay_enter_effect(new.kind, new_area, theme);
                                self.schedule(
                                    MotionEffectId::Overlay,
                                    fx::sequence(&[outgoing, incoming]),
                                );
                                self.overlay_underlay_snapshot = Some(new_underlay);
                                self.overlay_gate =
                                    overlay_duration(old.kind) + overlay_duration(new.kind) / 2;
                                self.outgoing_block_remaining = overlay_duration(old.kind);
                                self.active_visual_outgoing = Some(ActiveVisualOutgoing {
                                    identity: old.clone(),
                                    old: old_snapshot,
                                    underlay: old_underlay,
                                    total: overlay_duration(old.kind),
                                    remaining: overlay_duration(old.kind),
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
                                    outgoing_snapshot_effect(
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
                                    overlay_enter_effect(new.kind, area, theme),
                                );
                                self.overlay_gate = overlay_duration(new.kind) / 2;
                            }
                        }
                        _ => {}
                    }
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

        if was_reduced && screen == ShellScreen::Settings && !exit_confirmation {
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
            let exit = outgoing_snapshot_effect(snapshot, Some(underlay.snapshot), overlay.kind);
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
        self.overlay_underlay_snapshot = None;
        self.active_visual_outgoing = None;
        self.suppress_focus_after_generic_popup = false;
        self.exit_confirmation = false;
    }

    fn remember(&mut self, state: &ShellSession, bounds: Rect) {
        self.screen = Some(state.content_screen());
        self.focus = Some(state.focused_component());
        self.overlay = current_overlay(state);
        self.bounds = Some(bounds);
        self.exit_confirmation = state.active_screen() == ShellScreen::ExitConfirm;
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
        return fx::parallel(&[
            fx::sweep_in(
                Motion::LeftToRight,
                6,
                0,
                theme.canvas,
                (PAGE_MS, Interpolation::QuadOut),
            )
            .with_area(area)
            .with_filter(CellFilter::Text),
            fx::fade_from_fg(theme.accent_soft, (PAGE_MS, Interpolation::QuadOut))
                .with_area(area)
                .with_filter(surface_border_filter())
                .with_pattern(
                    DiagonalPattern::top_left_to_bottom_right().with_transition_width(6.0),
                ),
        ]);
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
            .with_filter(surface_animation_filter())
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
            .with_area(area)
            .with_filter(CellFilter::Text)
            .with_pattern(RadialPattern::center().with_transition_width(4.0))
            .with_rng(SimpleRng::new(EFFECT_SEED)),
            fx::fade_from_fg(theme.accent_soft, (DIALOG_MS, Interpolation::QuadOut))
                .with_area(area)
                .with_filter(surface_animation_filter()),
        ])
        .with_area(area),
        ui::MotionOverlayKind::Popover => fx::parallel(&[
            fx::sweep_in(
                Motion::UpToDown,
                4,
                0,
                theme.accent_soft,
                (POPOVER_MS, Interpolation::QuadOut),
            )
            .with_area(area)
            .with_filter(CellFilter::Text),
            fx::fade_from_fg(theme.accent_soft, (POPOVER_MS, Interpolation::QuadOut))
                .with_area(area)
                .with_filter(surface_border_filter())
                .with_pattern(SweepPattern::up_to_down(4)),
        ]),
        ui::MotionOverlayKind::Toast => fx::consume_tick(),
    }
}

fn preference_preview_effect(area: Rect, theme: ui::ThemeTokens) -> Effect {
    fx::parallel(&[
        fx::coalesce_from(
            Style::default().fg(theme.raised),
            (PREVIEW_MS, Interpolation::QuadOut),
        )
        .with_area(area)
        .with_filter(CellFilter::Text)
        .with_pattern(RadialPattern::center().with_transition_width(5.0))
        .with_rng(SimpleRng::new(EFFECT_SEED)),
        fx::fade_from_fg(theme.accent_soft, (PREVIEW_MS, Interpolation::QuadOut))
            .with_area(area)
            .with_filter(surface_animation_filter())
            .with_pattern(RadialPattern::center().with_transition_width(5.0)),
    ])
    .with_area(area)
}

fn surface_border_filter() -> CellFilter {
    CellFilter::Outer(Margin::new(1, 1))
}

fn surface_animation_filter() -> CellFilter {
    CellFilter::AnyOf(vec![CellFilter::Text, surface_border_filter()])
}

fn outgoing_snapshot_effect(
    snapshot: CellSnapshot,
    underlay: Option<CellSnapshot>,
    kind: ui::MotionOverlayKind,
) -> Effect {
    outgoing_snapshot_effect_from(snapshot, underlay, kind, 0.0, overlay_duration(kind))
}

fn outgoing_snapshot_effect_from(
    snapshot: CellSnapshot,
    underlay: Option<CellSnapshot>,
    kind: ui::MotionOverlayKind,
    start_alpha: f32,
    duration: Duration,
) -> Effect {
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
    let state = ExitSnapshotState { old, underlay };
    match kind {
        ui::MotionOverlayKind::Dialog => fx::effect_fn_buf(
            state.clone(),
            (duration, Interpolation::QuadOut),
            move |state, context, buffer| {
                let alpha = start_alpha + (1.0 - start_alpha) * context.alpha();
                let protected = protected_skip_positions(buffer);
                restore_underlay(state, buffer, &protected);
                if alpha <= 0.0 {
                    for (position, old) in &state.old {
                        if protected.contains(position) {
                            continue;
                        }
                        if buffer.area.contains(*position) {
                            buffer[*position] = old.clone();
                        }
                    }
                    return;
                }
                if alpha >= 1.0 {
                    return;
                }
                let mut pattern = RadialPattern::center()
                    .with_transition_width(4.0)
                    .for_frame(alpha, context.area);
                for (position, old) in &state.old {
                    if protected.contains(position) {
                        continue;
                    }
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
            move |state, context, buffer| {
                let alpha = start_alpha + (1.0 - start_alpha) * context.alpha();
                let protected = protected_skip_positions(buffer);
                restore_underlay(state, buffer, &protected);
                if alpha <= 0.0 {
                    for (position, old) in &state.old {
                        if protected.contains(position) {
                            continue;
                        }
                        if buffer.area.contains(*position) {
                            buffer[*position] = old.clone();
                        }
                    }
                    return;
                }
                if alpha >= 1.0 {
                    return;
                }
                let mut pattern = SweepPattern::down_to_up(4).for_frame(alpha, context.area);
                for (position, old) in &state.old {
                    if protected.contains(position) {
                        continue;
                    }
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

fn protected_skip_positions(buffer: &Buffer) -> HashSet<Position> {
    buffer
        .area
        .positions()
        .filter(|position| buffer[*position].diff_option == CellDiffOption::Skip)
        .collect()
}

fn restore_underlay(state: &ExitSnapshotState, buffer: &mut Buffer, protected: &HashSet<Position>) {
    if let Some(underlay) = state.underlay.as_ref() {
        for (position, cell) in underlay {
            if buffer.area.contains(*position)
                && !protected.contains(position)
                && cell.diff_option != CellDiffOption::Skip
            {
                buffer[*position] = cell.clone();
            }
        }
    }
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
        .filter(|overlay| {
            !(overlay.category == ShellOverlayCategory::ContextPopup
                && overlay.component() == Some(ShellComponent::ContextMenu))
        })
        .map(|overlay| OverlayIdentity {
            kind: overlay.kind,
            id: overlay.id,
            immediate: overlay.immediate,
        })
}

fn is_unrendered_generic_popup(state: &ShellSession) -> bool {
    state.active_overlay_descriptor().is_some_and(|overlay| {
        overlay.category == ShellOverlayCategory::ContextPopup
            && overlay.component() == Some(ShellComponent::ContextMenu)
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
#[path = "tests/motion_effects.rs"]
mod tests;
