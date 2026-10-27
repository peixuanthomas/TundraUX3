//! Live keyboard input stays attached to one job, page or management form.
use super::*;
use crate::previews::screen_keyboard::physical::PhysicalKeyboard;
use ui::{ScreenKeyboardAction as Action, ScreenKeyboardLayout, ScreenKeyboardModifiers};
mod page;

#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyboardOwner {
    AutoAdmin(AutoAdminJob),
    CommandLine(PageInstanceId),
    Script(PageInstanceId),
    Management(PageInstanceId, String),
    Page(PageInstanceId, String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct InputKeyboard {
    owner: Option<KeyboardOwner>,
    target: String,
    pub open: bool,
    pub visibility: u16,
    motion: ui::PanelMotion,
    pub modifiers: ScreenKeyboardModifiers,
    pub section: ui::ScreenKeyboardSection,
    pub physical: PhysicalKeyboard,
    pub pressed: Option<(ui::components::ButtonRegion, Instant)>,
}

impl ShellSession {
    /// Advance once per rendered frame. Input uses the last painted geometry.
    pub(super) fn advance_input_keyboard(&mut self, frame: ui::MotionFrame) -> bool {
        let target = if self.input_keyboard_open() { 1_000 } else { 0 };
        let frame = self.interaction_motion_frame(frame);
        let phases = ui::MotionPhases::from_millis(self.animation_timings().page);
        let keyboard = &mut self.screen_keyboard;
        let animating = keyboard.motion.update_with_phases(target, frame, phases);
        let visibility = keyboard.motion.value();
        let moved = keyboard.visibility != visibility;
        if moved {
            // A moving key must not finish a press at its former position.
            keyboard.pressed = None;
        }
        keyboard.visibility = visibility;
        if moved {
            self.cancel_auto_admin_pointer();
            self.cancel_management_pointer_gesture();
        }
        self.resize_auto_admin();
        animating
    }

    pub(super) fn poll_input_keyboard(&mut self, now: Instant) -> bool {
        let mut changed = self.sync_input_keyboard();
        changed |= self.screen_keyboard.physical.expire(now);
        if self
            .screen_keyboard
            .pressed
            .as_ref()
            .is_some_and(|(_, at)| now.saturating_duration_since(*at) > Duration::from_millis(500))
        {
            self.screen_keyboard.pressed = None;
            changed = true;
        }
        changed
    }
    fn input_keyboard_owner(&self) -> Option<KeyboardOwner> {
        let owner = if self.auto_admin_visible() {
            self.auto_admin_keyboard_job().map(KeyboardOwner::AutoAdmin)
        } else if self.active_screen() == ShellScreen::Editor
            && self.pages.editor.script.enabled
            && self.interactive_overlays().is_empty()
        {
            Some(KeyboardOwner::Script(self.active_page_id()))
        } else if self.active_screen() == ShellScreen::CommandLine
            && self.interactive_overlays().is_empty()
        {
            Some(KeyboardOwner::CommandLine(self.active_page_id()))
        } else if self.interactive_overlays().iter().all(|overlay| {
            overlay.component()
                == Some(if self.config_editor_form_visible() {
                    ShellComponent::Editor
                } else {
                    ShellComponent::Management
                })
        }) {
            self.management_keyboard_identity()
                .map(|identity| KeyboardOwner::Management(self.active_page_id(), identity))
        } else {
            None
        };
        owner.or_else(|| {
            self.page_keyboard_identity()
                .map(|id| KeyboardOwner::Page(self.active_page_id(), id))
        })
    }

    pub(super) fn sync_input_keyboard(&mut self) -> bool {
        let owner = self.input_keyboard_owner();
        let target = self.page_keyboard_target();
        if self.screen_keyboard.owner == owner {
            if self.screen_keyboard.target == target {
                return false;
            }
            self.screen_keyboard.target = target;
            self.screen_keyboard.pressed = None;
            self.screen_keyboard.physical.clear();
            return true;
        }
        let was_open = self.screen_keyboard.visibility > 0;
        self.screen_keyboard = InputKeyboard {
            owner,
            target,
            ..Default::default()
        };
        self.screen_keyboard.physical.reports_release = cfg!(windows);
        self.cancel_auto_admin_pointer();
        self.cancel_management_pointer_gesture();
        if was_open {
            self.resize_auto_admin();
        }
        true
    }

    pub(super) fn input_keyboard_open(&self) -> bool {
        self.screen_keyboard.open
            && self.screen_keyboard.owner == self.input_keyboard_owner()
            && self.screen_keyboard.owner.is_some()
    }

    pub(super) fn input_keyboard_layout(&self) -> ScreenKeyboardLayout {
        let mut bounds = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        if let ui::ShellLayout::Full { status, .. } = ui::compute_shell_layout(bounds) {
            let slide = (3 * u32::from(self.input_keyboard_visibility())).div_ceil(1_000) as u16;
            bounds.height = status.y + slide;
        }
        ui::screen_keyboard_input_layout_with_visibility(
            bounds,
            self.input_keyboard_open(),
            self.input_keyboard_visibility(),
            self.auto_admin_visible(),
            self.screen_keyboard.section,
        )
    }

    fn input_keyboard_visibility(&self) -> u16 {
        if self.screen_keyboard.owner == self.input_keyboard_owner() {
            self.screen_keyboard.visibility
        } else {
            0
        }
    }

    pub(super) fn auto_admin_available_area(&self) -> Rect {
        let bounds = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let mut main = ui::ShellFrameLayout::new(bounds, None, &ui::RenderContext::default())
            .with_help_button(self.help_button_available())
            .main;
        if self.auto_admin_visible() {
            let bottom = self.input_keyboard_layout().keyboard.y.min(
                self.keyboard_launcher_area()
                    .map_or(bounds.bottom(), |area| area.y),
            );
            main.height = main.height.min(bottom.saturating_sub(main.y));
        }
        main
    }

    pub(super) fn command_line_input_area(&self) -> Option<Rect> {
        let bounds = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let layout = self.input_keyboard_content_layout(
            ui::ShellFrameLayout::new(bounds, None, &ui::RenderContext::default())
                .with_help_button(self.help_button_available()),
        );
        ui::command_line_terminal_area_in(&layout)
    }

    pub(super) fn input_keyboard_content_layout(
        &self,
        layout: ui::ShellFrameLayout,
    ) -> ui::ShellFrameLayout {
        if matches!(self.input_keyboard_owner(), Some(KeyboardOwner::Page(..)))
            && self.input_keyboard_visibility() == 0
        {
            return layout;
        }
        if matches!(
            self.input_keyboard_owner(),
            Some(
                KeyboardOwner::CommandLine(_)
                    | KeyboardOwner::Script(_)
                    | KeyboardOwner::Management(..)
                    | KeyboardOwner::Page(..)
            )
        ) {
            layout.with_input_keyboard(
                self.input_keyboard_layout().keyboard,
                self.input_keyboard_visibility(),
            )
        } else {
            layout
        }
    }

    pub(super) fn keyboard_launcher_area(&self) -> Option<Rect> {
        if self.input_keyboard_owner().is_none() {
            return None;
        }
        if let Some(button) = self
            .input_keyboard_layout()
            .buttons
            .iter()
            .find(|button| button.action == ui::ScreenKeyboardAction::ToggleKeyboard)
        {
            return Some(button.area);
        }
        let bounds = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let mut footer = bounds;
        if let ui::ShellLayout::Full { status, .. } = ui::compute_shell_layout(bounds) {
            let slide = (3 * u32::from(self.input_keyboard_visibility())).div_ceil(1_000) as u16;
            footer.height = status.y + slide;
        }
        Some(ui::screen_keyboard_launcher_area(footer))
    }

    pub(super) fn toggle_input_keyboard(&mut self) {
        self.sync_input_keyboard();
        if self.screen_keyboard.owner.is_none() {
            return;
        }
        let opening = !self.screen_keyboard.open;
        if opening
            && matches!(
                self.screen_keyboard.owner,
                Some(KeyboardOwner::Management(..))
            )
        {
            self.focus_management_keyboard_input();
            self.sync_input_keyboard();
        }
        if opening && matches!(self.screen_keyboard.owner, Some(KeyboardOwner::Page(..))) {
            self.focus_page_keyboard_input();
            self.sync_input_keyboard();
        }
        self.screen_keyboard.open = opening;
        self.screen_keyboard.modifiers = Default::default();
        self.screen_keyboard.physical.clear();
        self.screen_keyboard.pressed = None;
        self.interaction.button_capture = None;
        self.cancel_auto_admin_pointer();
        self.cancel_management_pointer_gesture();
        self.resize_auto_admin();
    }

    /// Returns a real key/paste event, or consumes a keyboard panel click.
    pub(super) fn route_screen_keyboard_input(
        &mut self,
        input: InputEvent,
        platform: &dyn Platform,
        at: Instant,
    ) -> Option<InputEvent> {
        self.sync_input_keyboard();
        if matches!(input, InputEvent::Key(_) | InputEvent::Paste(_)) {
            self.screen_keyboard.pressed = None;
        }
        if matches!(input, InputEvent::Resize { .. } | InputEvent::FocusLost) {
            self.screen_keyboard.pressed = None;
            self.screen_keyboard.physical.clear();
        }
        let InputEvent::Mouse(mouse) = &input else {
            return Some(input);
        };
        let layout = self.input_keyboard_layout();
        let point = ratatui::layout::Position::from(mouse.coordinates());
        let key = layout
            .buttons
            .iter()
            .find(|button| button.area.contains(point));
        let launcher = self
            .keyboard_launcher_area()
            .filter(|area| area.contains(point));
        let region = key.map(|button| button.region()).or_else(|| {
            launcher.map(|area| ui::components::ButtonRegion {
                id: "input.keyboard".into(),
                area,
                disabled: false,
            })
        });
        if region.is_none()
            && !layout.keyboard.contains(point)
            && (layout.usable || !layout.hint.contains(point))
            && self.screen_keyboard.pressed.is_none()
        {
            return Some(input);
        }
        self.update_button_input_mode(&input);
        // Crossing into the panel cannot finish a press started in the AA/page.
        self.cancel_auto_admin_pointer();
        self.cancel_management_pointer_gesture();
        self.interaction.button_capture = None;
        let activated = match mouse.kind {
            ui::MouseEventKind::Down(ui::MouseButton::Left) => {
                self.screen_keyboard.pressed = region.clone().map(|region| (region, at));
                false
            }
            ui::MouseEventKind::Up(ui::MouseButton::Left) => self
                .screen_keyboard
                .pressed
                .take()
                .is_some_and(|(pressed, start)| {
                    Some(&pressed) == region.as_ref()
                        && at.saturating_duration_since(start) <= Duration::from_millis(500)
                }),
            ui::MouseEventKind::Click(ui::MouseButton::Left) => true,
            // Terminals report even a small held movement as Drag. Keep the
            // press while it stays on this key; leaving it cancels permanently.
            ui::MouseEventKind::Drag(ui::MouseButton::Left) | ui::MouseEventKind::Move => {
                if self
                    .screen_keyboard
                    .pressed
                    .as_ref()
                    .is_some_and(|(pressed, _)| Some(pressed) != region.as_ref())
                {
                    self.screen_keyboard.pressed = None;
                }
                false
            }
            ui::MouseEventKind::Scroll(_)
            | ui::MouseEventKind::Down(_)
            | ui::MouseEventKind::Drag(_) => {
                self.screen_keyboard.pressed = None;
                false
            }
            _ => false,
        };
        if !activated {
            return None;
        }
        self.screen_keyboard.pressed = None;
        if launcher.is_some() {
            self.toggle_input_keyboard();
            return None;
        }
        let action = key?.action;
        let modifiers = &mut self.screen_keyboard.modifiers;
        match action {
            Action::ToggleKeyboard => {
                self.toggle_input_keyboard();
                return None;
            }
            Action::Letters | Action::Functions => {
                self.screen_keyboard.section = if action == Action::Letters {
                    ui::ScreenKeyboardSection::Letters
                } else {
                    ui::ScreenKeyboardSection::Functions
                };
                return None;
            }
            Action::Shift | Action::RightShift => modifiers.shift = !modifiers.shift,
            Action::CapsLock => modifiers.caps_lock = !modifiers.caps_lock,
            Action::LeftCtrl => modifiers.left_ctrl = !modifiers.left_ctrl,
            Action::RightCtrl => modifiers.right_ctrl = !modifiers.right_ctrl,
            Action::Alt | Action::RightAlt => modifiers.alt = !modifiers.alt,
            _ => {
                let event = if action == Action::Paste {
                    match platform.read_clipboard_text() {
                        Ok(text) if text.is_empty() => {
                            self.notify_status(i18n::msg!("screen-keyboard-clipboard-empty"));
                            return None;
                        }
                        Ok(text) => InputEvent::Paste(text),
                        Err(error) => {
                            self.notify_status(i18n::msg!(
                                "screen-keyboard-clipboard-error",
                                error = error.to_string()
                            ));
                            return None;
                        }
                    }
                } else {
                    InputEvent::Key(keyboard_key(action, *modifiers)?)
                };
                if self.auto_admin_visible() {
                    self.send_auto_admin_keyboard_input(&event);
                    return None;
                }
                return Some(event);
            }
        }
        None
    }
}

fn keyboard_key(action: Action, modifiers: ScreenKeyboardModifiers) -> Option<KeyInput> {
    let key = match action {
        Action::Escape => InputKey::Escape,
        Action::Tab if modifiers.shift => InputKey::BackTab,
        Action::Tab => InputKey::Tab,
        Action::Backspace => InputKey::Backspace,
        Action::Enter => InputKey::Enter,
        Action::Space => InputKey::Char(' '),
        Action::Function(number) => InputKey::F(number),
        Action::Insert => InputKey::Insert,
        Action::Delete => InputKey::Delete,
        Action::Home => InputKey::Home,
        Action::End => InputKey::End,
        Action::PageUp => InputKey::PageUp,
        Action::PageDown => InputKey::PageDown,
        Action::Up => InputKey::Up,
        Action::Down => InputKey::Down,
        Action::Left => InputKey::Left,
        Action::Right => InputKey::Right,
        Action::Letter(_) | Action::Character(_) => InputKey::Char(action.character(modifiers)?),
        _ => return None,
    };
    Some(KeyInput::with_modifiers(
        key,
        ui::KeyModifiers::new(
            modifiers.left_ctrl || modifiers.right_ctrl,
            modifiers.alt,
            modifiers.shift,
        ),
    ))
}

#[cfg(test)]
#[path = "../../tests/unit/session/screen_keyboard.rs"]
mod tests;
