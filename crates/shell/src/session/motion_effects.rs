use super::*;
use ratatui::{
    buffer::{Buffer, Cell, CellDiffOption},
    layout::{Position, Rect},
    style::Style,
};
use tachyonfx::{
    CellFilter, Effect, EffectManager, Interpolation, Motion, SimpleRng, fx,
    pattern::{DiagonalPattern, RadialPattern},
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
}

#[derive(Debug, Clone)]
struct CellSnapshot {
    area: Rect,
    cells: Vec<(Position, Cell)>,
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
    theme: Option<ui::ThemeTokens>,
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
                self.manager.add_unique_effect(MotionEffectId::Page, fx);
            }
        }

        let focus = state.focused_component();
        if self.focus.is_some_and(|old| old != focus)
            && let Some(area) = focused_area(state, focus)
        {
            self.manager.add_unique_effect(
                MotionEffectId::Focus,
                fx::fade_from_fg(theme.accent_soft, (FOCUS_MS, Interpolation::QuadOut))
                    .with_area(area)
                    .with_filter(CellFilter::Text),
            );
        }

        let overlay = current_overlay(state);
        if self.overlay != overlay {
            match (&self.overlay, &overlay) {
                (Some(old), Some(new))
                    if old.kind != ui::MotionOverlayKind::Toast
                        && new.kind != ui::MotionOverlayKind::Toast =>
                {
                    let outgoing = self
                        .overlay_snapshot
                        .take()
                        .map(|snapshot| outgoing_snapshot_effect(snapshot, old.kind));
                    let incoming =
                        overlay_area(state).map(|area| overlay_enter_effect(new.kind, area, theme));
                    self.overlay_gate = Duration::ZERO;
                    match (outgoing, incoming) {
                        (Some(outgoing), Some(incoming)) => {
                            self.manager.add_unique_effect(
                                MotionEffectId::Overlay,
                                fx::sequence(&[outgoing, incoming]),
                            );
                            self.overlay_gate =
                                overlay_duration(old.kind) + overlay_duration(new.kind) / 2;
                        }
                        (Some(effect), None) => {
                            self.manager
                                .add_unique_effect(MotionEffectId::Overlay, effect);
                            self.overlay_gate = overlay_duration(old.kind);
                        }
                        (None, Some(effect)) => {
                            self.manager
                                .add_unique_effect(MotionEffectId::Overlay, effect);
                            self.overlay_gate = overlay_duration(new.kind) / 2;
                        }
                        (None, None) => {}
                    }
                }
                (Some(old), _) if old.kind != ui::MotionOverlayKind::Toast => {
                    self.overlay_gate = Duration::ZERO;
                    if let Some(snapshot) = self.overlay_snapshot.take() {
                        self.manager.add_unique_effect(
                            MotionEffectId::Overlay,
                            outgoing_snapshot_effect(snapshot, old.kind),
                        );
                        self.overlay_gate = overlay_duration(old.kind);
                    }
                }
                (_, Some(new)) if new.kind != ui::MotionOverlayKind::Toast => {
                    if let Some(area) = overlay_area(state) {
                        self.manager.add_unique_effect(
                            MotionEffectId::Overlay,
                            overlay_enter_effect(new.kind, area, theme),
                        );
                        self.overlay_gate = overlay_duration(new.kind) / 2;
                    }
                }
                _ => {}
            }
            if let Some(area) = status_area.filter(|area| !area.is_empty()) {
                match (&self.overlay, &overlay) {
                    (None, Some(new)) if new.kind == ui::MotionOverlayKind::Toast => {
                        self.manager.add_unique_effect(
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
                        )
                    }
                    (Some(old), None) if old.kind == ui::MotionOverlayKind::Toast => {
                        self.manager.add_unique_effect(
                            MotionEffectId::Toast,
                            fx::fade_to_fg(theme.muted, (TOAST_EXIT_MS, Interpolation::QuadOut))
                                .with_area(area)
                                .with_filter(CellFilter::Text),
                        )
                    }
                    _ => {}
                }
            }
        }

        if was_reduced && screen == ShellScreen::Settings {
            let area = shell_main_area(page_area);
            if !area.is_empty() {
                self.manager.add_unique_effect(
                    MotionEffectId::PreferencePreview,
                    fx::sweep_in(
                        Motion::LeftToRight,
                        6,
                        0,
                        theme.accent_soft,
                        (PREVIEW_MS, Interpolation::QuadOut),
                    )
                    .with_area(area)
                    .with_filter(CellFilter::Text),
                );
            }
        }
        self.remember(state, full_area);
        self.theme = Some(theme);
    }

    pub(super) fn process(&mut self, delta: Duration, buffer: &mut Buffer, state: &ShellSession) {
        if self.reduced {
            return;
        }
        if let Some(area) = overlay_area(state) {
            self.overlay_snapshot = snapshot_normal_cells(buffer, area);
        }
        self.manager.process_effects(delta, buffer, buffer.area);
        self.overlay_gate = self.overlay_gate.saturating_sub(delta);
    }

    pub(super) fn is_running(&self) -> bool {
        !self.reduced && self.manager.is_running()
    }

    pub(super) fn cancel_for_bounds_change(&mut self) {
        self.clear();
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
            let Some(snapshot) = self.overlay_snapshot.as_ref() else {
                return MotionInputDisposition::Apply;
            };
            let Some(theme) = self.theme else {
                return MotionInputDisposition::Apply;
            };
            self.deferred_close = Some(DeferredClose {
                routed: routed.clone(),
                overlay: overlay.clone(),
            });
            self.overlay_gate = overlay_duration(overlay.kind);
            self.exiting = true;
            let exit = fx::fade_to_fg(theme.muted, (self.overlay_gate, Interpolation::QuadOut))
                .with_area(snapshot.area)
                .with_filter(CellFilter::Text);
            self.manager
                .add_unique_effect(MotionEffectId::Overlay, exit);
            return MotionInputDisposition::Defer;
        }
        MotionInputDisposition::Apply
    }

    pub(super) fn take_deferred_close(&mut self, state: &ShellSession) -> Option<RoutedEvent> {
        if self.exiting && self.overlay_gate == Duration::ZERO {
            self.exiting = false;
            let deferred = self.deferred_close.take()?;
            return (current_overlay(state) == Some(deferred.overlay)).then_some(deferred.routed);
        }
        None
    }

    fn clear(&mut self) {
        self.manager = EffectManager::default();
        self.overlay_snapshot = None;
        self.overlay_gate = Duration::ZERO;
        // Bounds/reduced-motion cancellation must flush an already deferred close on
        // the next runtime boundary, exactly once.
        self.exiting = self.deferred_close.is_some();
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

fn outgoing_snapshot_effect(snapshot: CellSnapshot, kind: ui::MotionOverlayKind) -> Effect {
    let duration = match kind {
        ui::MotionOverlayKind::Dialog => DIALOG_MS,
        _ => POPOVER_MS,
    };
    let area = snapshot.area;
    fx::effect_fn_buf(
        snapshot.cells,
        (duration, Interpolation::QuadOut),
        |cells, context, buffer| {
            let alpha = context.alpha();
            for (position, old) in cells.iter() {
                let local_y = position.y.saturating_sub(context.area.y) as f32;
                let height = context.area.height.max(1) as f32;
                if alpha < 1.0 - local_y / height && buffer.area.contains(*position) {
                    buffer[*position] = old.clone();
                }
            }
        },
    )
    .with_area(area)
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
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
        state.apply_input(InputEvent::from_key_label("q"));
        let overlay = current_overlay(&state).unwrap();
        let mut motion = ShellMotionEffects {
            overlay: Some(overlay),
            overlay_snapshot: Some(CellSnapshot {
                area: Rect::new(1, 1, 2, 1),
                cells: Vec::new(),
            }),
            theme: Some(ui::ThemeTokens::glacier_night()),
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
            theme: Some(ui::ThemeTokens::glacier_night()),
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
    fn preempted_overlay_never_retargets_deferred_cancel() {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
        state.apply_input(InputEvent::from_key_label("q"));
        let overlay = OverlayIdentity {
            kind: ui::MotionOverlayKind::Dialog,
            id: "preempted:A".into(),
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
    fn identity_sync_gates_confirm_before_first_render() {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
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
}
