//! One entry point for physical input in both the runtime and headless sessions.
use crate::session::*;

impl ShellSession {
    /// Consume panel/modal input, adapt shared button gestures, then normalize
    /// navigation. The caller routes the result to the child terminal or Shell.
    /// Returning `None` means consumed; it must never fall through to a page.
    pub(in crate::session) fn prepare_input(
        &mut self,
        input: InputEvent,
        platform: &dyn Platform,
        received_at: Instant,
    ) -> Option<InputEvent> {
        self.synchronize_overlay_focus();
        let input = self.route_screen_keyboard_input(input, platform, received_at)?;
        if self.handle_auto_admin_input_at(&input, received_at) {
            return None;
        }
        let input = self.prepare_button_input(input, received_at)?;
        Some(self.normalize_shell_navigation_input(input))
    }
}

impl ShellSession {
    pub(in crate::session) fn update_button_input_mode(&mut self, input: &InputEvent) {
        let keyboard_action = match input {
            InputEvent::Key(key) if key.phase.is_press_like() => {
                if self.auto_admin_visible() {
                    true
                } else {
                    let (_, command) = self.route_key_input(key);
                    if key.phase == InputPhase::Press
                        && command == ShellCommand::BeginExplorerSearch
                    {
                        self.interaction.guard_opening_key(key.key.clone());
                    }
                    !matches!(
                        command,
                        ShellCommand::Noop
                            | ShellCommand::RecordInput
                            | ShellCommand::CaptureOverlayInput
                    )
                }
            }
            _ => false,
        };
        self.interaction.observe(input, keyboard_action);
    }
}

impl ShellSession {
    pub(in crate::session) fn normalize_shell_navigation_input(
        &self,
        input: InputEvent,
    ) -> InputEvent {
        if let InputEvent::Mouse(mouse) = &input
            && mouse.kind == ui::MouseEventKind::Down(PointerButton::Left)
            && !self.auto_admin_visible()
            && !self.notification_has_active_modal()
            && !self.time_sync_dialog_visible
            && self.pages.common.active_popup.is_none()
            && self.pages.diagnostics.diagnostics_repair_preview.is_empty()
            && matches!(
                self.active_screen(),
                ShellScreen::Diagnostics | ShellScreen::SystemStatus
            )
            && let Some(button) = self.button_at(mouse.coordinates())
            && let Some(code) = button
                .id
                .as_str()
                .strip_prefix("diagnostics.toolbar.")
                .and_then(|value| value.parse::<u32>().ok())
                .and_then(char::from_u32)
        {
            return InputEvent::Key(KeyInput::new(InputKey::Char(code)));
        }
        if let InputEvent::Mouse(mouse) = &input
            && mouse.kind == ui::MouseEventKind::Down(PointerButton::Left)
            && self.hit_map.target_at(mouse.coordinates()) == Some(ShellComponent::BackButton)
        {
            if self.active_screen() == ShellScreen::CommandLine
                && !self.notification_has_active_modal()
                && !self.auto_admin_visible()
            {
                return input;
            }
            if self.active_screen() == ShellScreen::Editor
                && self.app.storage_config().editor.vim_mode_enabled
                && self.pages.editor.editor_focus == ui::EditorFocus::Canvas
                && self.pages.editor.editor_load_state.is_none()
                && self.pages.editor.editor_save_state.is_none()
                && self
                    .active_overlay_descriptor()
                    .is_none_or(|overlay| overlay.kind == ui::MotionOverlayKind::Toast)
                && self.pages.editor.editor_search_bar.is_none()
            {
                // Vim reserves physical Esc; the Back button still closes the document.
                return InputEvent::Key(KeyInput::with_modifiers(
                    InputKey::Char('w'),
                    InputModifiers::CTRL,
                ));
            }
            return InputEvent::Key(KeyInput::new(InputKey::Escape));
        }
        input
    }
}
