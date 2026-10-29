use super::interaction::ButtonPointerCapture;
use crate::session::overlays::ResolvedExplorerOverlay;
use crate::session::*;

const BUTTON_MAX_PRESS: Duration = Duration::from_millis(500);

impl ShellSession {
    pub(in crate::session) fn button_at(
        &self,
        point: CellPosition,
    ) -> Option<ui::components::ButtonRegion> {
        if self.screen_keyboard.visibility > 0 {
            if let Some(button) = self
                .input_keyboard_layout()
                .buttons
                .iter()
                .find(|button| rect_contains(button.area, point))
            {
                return Some(button.region());
            }
        }
        if let Some(area) = self
            .keyboard_launcher_area()
            .filter(|area| rect_contains(*area, point))
        {
            return Some(ui::components::ButtonRegion {
                id: "input.keyboard".into(),
                area,
                disabled: false,
            });
        }
        if self.hit_map.target_at(point) == Some(ShellComponent::BackButton)
            && let Some(area) = self.frame_layout.and_then(|layout| layout.back_button)
        {
            return Some(ui::components::ButtonRegion {
                id: "shell.back".into(),
                area,
                disabled: false,
            });
        }
        if self.auto_admin_visible() {
            return self
                .button_regions
                .iter()
                .rev()
                .find(|button| {
                    button.id.as_str().starts_with("aa.") && rect_contains(button.area, point)
                })
                .cloned();
        }
        if let Some(id) = self.notification_active_modal_id() {
            let prefix = format!("notification.{id}.");
            return self
                .button_regions
                .iter()
                .rev()
                .find(|button| {
                    let notification_action = button.id.as_str().starts_with(&prefix);
                    let modal_back = button.id.as_str() == "shell.back"
                        && self.hit_map.target_at(point) == Some(ShellComponent::BackButton);
                    (notification_action || modal_back) && rect_contains(button.area, point)
                })
                .cloned();
        }
        if self.help_open {
            let ui::NotificationLayout::Dialog(layout) = self.page_help_layout() else {
                return None;
            };
            return layout
                .actions
                .iter()
                .find(|action| rect_contains(action.area, point))
                .map(|action| ui::components::ButtonRegion {
                    id: "notification.page-help.action.close".into(),
                    area: action.area,
                    disabled: false,
                });
        }
        if self.content_screen() == ShellScreen::Settings
            && self
                .pages
                .settings
                .settings_state
                .as_ref()
                .is_some_and(|state| state.number_editor.is_some())
            && let Some(layout) = self.current_settings_layout()
            && rect_contains(layout.main, point)
        {
            let number = layout.number_editor.as_ref().unwrap();
            let (id, area) = match ui::settings_hit_test(&layout, point) {
                Some(ui::SettingsHitTarget::AdjustField(_, -1)) => {
                    ("settings.number.decrease", number.decrease)
                }
                Some(ui::SettingsHitTarget::AdjustField(_, 1)) => {
                    ("settings.number.increase", number.increase)
                }
                Some(ui::SettingsHitTarget::OverlayApply) => {
                    ("settings.overlay.apply", layout.overlay_apply.unwrap())
                }
                Some(ui::SettingsHitTarget::OverlayCancel) => {
                    ("settings.overlay.cancel", layout.overlay_cancel.unwrap())
                }
                _ => return None,
            };
            return Some(ui::components::ButtonRegion {
                id: id.into(),
                area,
                disabled: false,
            });
        }
        if self.resolved_overlay_owner() == Some(ShellComponent::Explorer)
            && matches!(
                self.resolved_explorer_overlay(),
                Some(ResolvedExplorerOverlay::Semantic(
                    ExplorerOverlayMode::ContextMenu { .. }
                ))
            )
            && self.explorer_hit_target_at(point).is_none()
        {
            // Covered toolbar buttons must not consume the outside click.
            return None;
        }
        if self.active_screen() == ShellScreen::Logs
            && self.logs_has_active_overlay()
            && self
                .logs_main_area()
                .is_some_and(|main| rect_contains(main, point))
        {
            // A newly opened modal can still have the previous page's button
            // registry until the next frame. Never capture a covered button.
            return self.logs_button_at(point);
        }
        if self.management_overlay_contains(point) {
            return self.management_button_at(point);
        }
        if self.active_screen() == ShellScreen::Launcher
            && self.pages.launcher.launcher_run_settings.is_some()
            && let ui::ShellLayout::Full { main, .. } =
                self.shell_layout_for(Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1))
            && rect_contains(main, point)
        {
            return self.launcher_run_settings_button_at(point);
        }
        if self.pages.user_management.user_management_more_open
            && self.active_screen() == ShellScreen::UserManagement
        {
            return self.user_management_button_at(point);
        }
        self.user_management_button_at(point)
            .or_else(|| self.management_button_at(point))
            .or_else(|| self.logs_button_at(point))
            .or_else(|| self.table_sort_button_at(point))
            .or_else(|| self.explorer_selection_button_at(point))
            .or_else(|| {
                self.button_regions
                    .iter()
                    .rev()
                    .find(|button| rect_contains(button.area, point))
                    .cloned()
            })
            .or_else(|| {
                if self.hit_map.target_at(point) == Some(ShellComponent::StatusBar) {
                    return self.frame_layout.and_then(|layout| {
                        layout
                            .status_message
                            .filter(|area| rect_contains(*area, point))
                            .map(|area| ui::components::ButtonRegion {
                                id: "shell.status".into(),
                                area,
                                disabled: false,
                            })
                    });
                }
                // Details rows are drawn as a table, but launch on release just
                // like icon buttons. Use the same layout as their hit testing.
                if self.active_screen() != ShellScreen::Launcher
                    || self.hit_map.target_at(point) != Some(ShellComponent::Launcher)
                {
                    return None;
                }
                let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
                let ui::ShellLayout::Full { main, .. } = self.shell_layout_for(area) else {
                    return None;
                };
                let model = self.to_launcher_view_model();
                let layout = ui::launcher_layout(main, &model);
                let Some(ui::LauncherHitTarget::Item(index)) = layout.hit_test(point.0, point.1)
                else {
                    return None;
                };
                let item = model.items.get(index)?;
                Some(ui::components::ButtonRegion {
                    id: format!("launcher.item.{}", item.id).into(),
                    area: layout.items.iter().find(|item| item.index == index)?.area,
                    disabled: !model.organizing && item.status != ui::LauncherItemStatus::Ready,
                })
            })
    }

    /// Capture shared buttons before page routing. Existing page actions receive
    /// their press only after a matching release, while text selection and
    /// scrollbars retain their original down/drag/up stream.
    pub(in crate::session) fn prepare_button_input(
        &mut self,
        input: InputEvent,
        received_at: Instant,
    ) -> Option<InputEvent> {
        self.synchronize_overlay_focus();
        self.interaction.completed_button_click_at = None;
        if self.interaction.consumes_opening_repeat(&input) {
            return None;
        }
        self.update_button_input_mode(&input);
        let InputEvent::Mouse(mouse) = input else {
            let cancel_launcher_drag = self.active_screen() == ShellScreen::Launcher
                && self.pages.launcher.launcher_drag.is_some()
                && matches!(&input, InputEvent::Key(key) if key.key == InputKey::Escape && key.phase.is_press_like());
            if matches!(
                input,
                InputEvent::FocusLost
                    | InputEvent::Resize { .. }
                    | InputEvent::Key(_)
                    | InputEvent::Paste(_)
            ) {
                self.cancel_pointer_gestures_for_modal();
                self.last_click = None;
            }
            if cancel_launcher_drag {
                return None;
            }
            if matches!(input, InputEvent::FocusLost) {
                self.interaction.pointer_position = None;
                self.pages.launcher.launcher_drag = None;
                self.pages.editor.editor_cursor_acceleration = None;
            }
            if matches!(input, InputEvent::Resize { .. }) {
                self.button_regions.clear();
            }
            return Some(input);
        };
        self.interaction.pointer_position = Some(mouse.coordinates());
        // Wheel reports are not button releases. A wheel or a move outside
        // the captured button cancels its pending action permanently.
        if matches!(mouse.kind, ui::MouseEventKind::Scroll(_))
            || (matches!(mouse.kind, ui::MouseEventKind::Moved)
                && self
                    .interaction
                    .button_capture
                    .as_ref()
                    .is_some_and(|capture| {
                        !rect_contains(capture.region.area, mouse.coordinates())
                    }))
        {
            self.interaction.button_capture = None;
            self.notification_pointer_capture = None;
            self.last_click = None;
        }
        if self.notification_has_active_modal() && self.handle_notification_scrollbar(mouse) {
            self.interaction.button_capture = None;
            return None;
        }
        if self.help_open
            && !self.notification_has_active_modal()
            && self.handle_help_scrollbar(mouse)
        {
            self.interaction.button_capture = None;
            return None;
        }
        if self.interactive_overlays().is_empty()
            && (self.handle_home_pointer_scrollbar(&mouse)
                || self.handle_launcher_pointer_scrollbar(&mouse)
                || self.handle_diagnostics_detail_pointer(mouse)
                || self.handle_explorer_locations_pointer(mouse))
        {
            self.interaction.button_capture = None;
            return None;
        }
        match mouse.kind {
            ui::MouseEventKind::Down(PointerButton::Left) => {
                self.interaction.button_capture = None;
                if let Some(region) = self.button_at(mouse.coordinates()) {
                    if region.disabled {
                        return None;
                    }
                    // Notifications already validate releases; launcher cards
                    // need the original stream to support drag and drop.
                    let native_release = region.id.as_str().starts_with("notification.")
                        && region.id.as_str() != "notification.page-help.action.close"
                        || region.id.as_str().starts_with("launcher.item.")
                        || region.id.as_str().starts_with("script.");
                    self.interaction.button_capture = Some(ButtonPointerCapture {
                        region,
                        page: self.active_page_id(),
                        overlay: self
                            .active_overlay_descriptor()
                            .filter(|overlay| overlay.kind != ui::MotionOverlayKind::Toast)
                            .map(|overlay| overlay.id),
                        input: mouse,
                        pressed_at: received_at,
                        native_release,
                        activate_on_release: false,
                    });
                    if !native_release {
                        return None;
                    }
                }
            }
            ui::MouseEventKind::Up(PointerButton::Left) => {
                if let Some(capture) = self.interaction.button_capture.take() {
                    let held = received_at.saturating_duration_since(capture.pressed_at);
                    let matches_button = held <= BUTTON_MAX_PRESS
                        && capture.page == self.active_page_id()
                        && capture.overlay
                            == self
                                .active_overlay_descriptor()
                                .filter(|overlay| overlay.kind != ui::MotionOverlayKind::Toast)
                                .map(|overlay| overlay.id)
                        && self.button_at(mouse.coordinates()).as_ref() == Some(&capture.region);
                    if capture.native_release {
                        if capture.activate_on_release && matches_button {
                            return Some(InputEvent::Mouse(MouseInput {
                                kind: ui::MouseEventKind::Click(PointerButton::Left),
                                ..mouse
                            }));
                        }
                        if !matches_button {
                            self.notification_pointer_capture = None;
                        }
                        return Some(input);
                    }
                    if matches_button {
                        self.interaction.completed_button_click_at = Some(received_at);
                        return Some(InputEvent::Mouse(MouseInput {
                            position: mouse.position,
                            ..capture.input
                        }));
                    }
                    return None;
                }
            }
            ui::MouseEventKind::Down(_) => {
                self.interaction.button_capture = None;
                self.notification_pointer_capture = None;
            }
            ui::MouseEventKind::Drag(PointerButton::Left) => {
                self.last_click = None;
                if self
                    .interaction
                    .button_capture
                    .as_ref()
                    .is_some_and(|capture| capture.region.id.as_str().starts_with("notification."))
                {
                    self.interaction.button_capture = None;
                }
                if let Some(capture) = &mut self.interaction.button_capture {
                    capture.activate_on_release = false;
                }
                if let Some(capture) = &self.interaction.button_capture
                    && !capture.native_release
                {
                    self.interaction.button_capture = None;
                    return None;
                }
            }
            _ => {}
        }
        Some(input)
    }
}
