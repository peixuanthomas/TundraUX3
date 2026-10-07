//! Standalone debug keyboard. Text stays in this loop unless explicitly copied
//! to the system clipboard; keys are never injected into another application.
use std::{
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

use crossterm::event;
use ratatui::layout::Rect;
use ui::{
    InputEvent, Key, MouseButton, MouseEventKind, RenderContext, ScreenKeyboardAction,
    ScreenKeyboardLayout, ScreenKeyboardModifiers, ScreenKeyboardViewModel, TundraTheme,
    components::{ButtonFrame, ButtonRegion},
};

use crate::{ShellAppConfig, TerminalGuard, crossterm_event_to_input};

const BUTTON_MAX_PRESS: Duration = Duration::from_millis(500);

pub fn run_screen_keyboard(
    output: &mut impl Write,
    appearance: &storage::AppearanceConfig,
    platform: &dyn platform::Platform,
) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other(
            "screen-keyboard requires an interactive terminal (stdin and stdout)",
        ));
    }
    let config = ShellAppConfig::from_appearance(appearance);
    let theme = TundraTheme::default_dark()
        .with_border_shape(config.border_shape)
        .with_border_color(config.border_color)
        .with_accent_color(config.accent_color);
    let mut terminal = TerminalGuard::enter(output)?;
    let mut model = ScreenKeyboardState::default();
    let mut bounds = Rect::default();
    let mut layout = ui::screen_keyboard_layout(bounds, model.collapsed);
    let mut dirty = true;
    let origin = Instant::now();
    loop {
        let now = Instant::now();
        dirty |= model.expire_press(now);
        if dirty {
            terminal.terminal_mut().draw(|frame| {
                if bounds != frame.area() || layout.collapsed != model.collapsed {
                    bounds = frame.area();
                    layout = ui::screen_keyboard_layout(bounds, model.collapsed);
                    model.cancel_pointer();
                    model.ensure_visible_focus(&layout);
                }
                let mut context = RenderContext::from_theme_with_motion_preference(
                    &theme,
                    origin.elapsed(),
                    matches!(
                        appearance.motion_preference,
                        storage::MotionPreference::Reduced
                    ),
                    crate::terminal_session::text_render_capabilities(),
                );
                context.buttons = Some(model.button_frame(&context));
                ui::render_screen_keyboard(
                    frame,
                    bounds,
                    &layout,
                    &ScreenKeyboardViewModel {
                        text: &model.text,
                        focus: model.focus,
                        modifiers: model.modifiers,
                        last_key: &model.last_key,
                        message: &model.message,
                    },
                    &context,
                );
            })?;
            dirty = false;
        }
        if event::poll(Duration::from_millis(100))? {
            let input = crossterm_event_to_input(event::read()?);
            if model.handle_input(input, &layout, Instant::now()) {
                break;
            }
            model.apply_clipboard(platform);
            dirty = true;
        }
    }
    terminal.restore()
}

struct PointerPress {
    action: ScreenKeyboardAction,
    region: ButtonRegion,
    at: Instant,
}

struct ScreenKeyboardState {
    text: String,
    last_key: String,
    message: String,
    modifiers: ScreenKeyboardModifiers,
    pending_clipboard: Option<ScreenKeyboardAction>,
    collapsed: bool,
    navigating_buttons: bool,
    focus: ScreenKeyboardAction,
    keyboard_focus_visible: bool,
    mouse_coordinates: Option<(u16, u16)>,
    hovered: Option<ButtonRegion>,
    pressed: Option<PointerPress>,
}

impl Default for ScreenKeyboardState {
    fn default() -> Self {
        Self {
            text: String::new(),
            last_key: String::new(),
            message: String::new(),
            modifiers: ScreenKeyboardModifiers::default(),
            pending_clipboard: None,
            collapsed: false,
            navigating_buttons: true,
            focus: ScreenKeyboardAction::Letter('q'),
            keyboard_focus_visible: true,
            mouse_coordinates: None,
            hovered: None,
            pressed: None,
        }
    }
}

impl ScreenKeyboardState {
    fn button_frame(&self, context: &RenderContext) -> ButtonFrame {
        let mut frame = ButtonFrame::new(
            self.hovered.clone(),
            self.pressed.as_ref().map(|pressed| pressed.region.clone()),
            &context.compatibility_theme(),
        );
        frame.keyboard_focus_visible = self.keyboard_focus_visible;
        frame
    }

    fn cancel_pointer(&mut self) {
        self.pressed = None;
        self.hovered = None;
    }

    fn expire_press(&mut self, now: Instant) -> bool {
        if self
            .pressed
            .as_ref()
            .is_some_and(|press| now.saturating_duration_since(press.at) > BUTTON_MAX_PRESS)
        {
            self.cancel_pointer();
            return true;
        }
        false
    }

    fn ensure_visible_focus(&mut self, layout: &ScreenKeyboardLayout) {
        if !layout
            .buttons
            .iter()
            .any(|button| button.action == self.focus)
            && let Some(button) = layout.buttons.first()
        {
            self.focus = button.action;
        }
    }

    fn use_keyboard(&mut self) {
        self.keyboard_focus_visible = true;
        self.cancel_pointer();
    }

    fn activate(&mut self, action: ScreenKeyboardAction) -> bool {
        self.message.clear();
        match action {
            ScreenKeyboardAction::Exit | ScreenKeyboardAction::Escape => return true,
            ScreenKeyboardAction::ToggleKeyboard => {
                self.collapsed = !self.collapsed;
                self.cancel_pointer();
                self.clear_one_shot_modifiers();
                self.focus = ScreenKeyboardAction::ToggleKeyboard;
            }
            ScreenKeyboardAction::Clear => {
                self.text.clear();
                self.clear_one_shot_modifiers();
            }
            ScreenKeyboardAction::Copy | ScreenKeyboardAction::Paste => {
                self.pending_clipboard = Some(action);
                self.clear_one_shot_modifiers();
            }
            ScreenKeyboardAction::Shift => self.modifiers.shift = !self.modifiers.shift,
            ScreenKeyboardAction::CapsLock => {
                self.modifiers.caps_lock = !self.modifiers.caps_lock;
            }
            ScreenKeyboardAction::LeftCtrl => self.modifiers.left_ctrl = !self.modifiers.left_ctrl,
            ScreenKeyboardAction::RightCtrl => {
                self.modifiers.right_ctrl = !self.modifiers.right_ctrl
            }
            ScreenKeyboardAction::Alt => self.modifiers.alt = !self.modifiers.alt,
            _ => {
                self.type_key(action, action.character(self.modifiers), self.modifiers);
                return false;
            }
        }
        self.last_key = key_name(action, None);
        false
    }

    fn clear_one_shot_modifiers(&mut self) {
        self.modifiers = ScreenKeyboardModifiers {
            caps_lock: self.modifiers.caps_lock,
            ..Default::default()
        };
    }

    fn type_key(
        &mut self,
        action: ScreenKeyboardAction,
        character: Option<char>,
        modifiers: ScreenKeyboardModifiers,
    ) {
        self.message.clear();
        let mut parts = Vec::new();
        if modifiers.left_ctrl {
            parts.push("Ctrl".to_owned());
        }
        if modifiers.right_ctrl {
            parts.push("RCtrl".to_owned());
        }
        if modifiers.alt {
            parts.push("Alt".to_owned());
        }
        if modifiers.shift {
            parts.push("Shift".to_owned());
        }
        parts.push(key_name(action, character));
        self.last_key = parts.join("+");
        let control = modifiers.left_ctrl || modifiers.right_ctrl;
        if control && !modifiers.alt {
            match character.map(|c| c.to_ascii_lowercase()) {
                Some('c') => self.pending_clipboard = Some(ScreenKeyboardAction::Copy),
                Some('v') => self.pending_clipboard = Some(ScreenKeyboardAction::Paste),
                _ => {}
            }
        } else if !control && !modifiers.alt {
            if action == ScreenKeyboardAction::Backspace {
                self.text.pop();
            } else if let Some(character) = character {
                self.text.push(character);
            }
        }
        self.clear_one_shot_modifiers();
    }

    fn append_paste(&mut self, text: &str) {
        // Keep pasted text as text, including Unicode/newlines, while excluding
        // terminal controls and normalizing clipboard line endings.
        self.text.extend(
            text.replace("\r\n", "\n")
                .replace('\r', "\n")
                .chars()
                .filter(|c| !c.is_control() || matches!(c, '\n' | '\t')),
        );
        self.clear_one_shot_modifiers();
        self.navigating_buttons = false;
    }

    fn apply_clipboard(&mut self, platform: &dyn platform::Platform) {
        let result = match self.pending_clipboard.take() {
            Some(ScreenKeyboardAction::Copy) => {
                platform.write_clipboard_text(&self.text).map(|()| {
                    self.message = i18n::tr!("screen-keyboard-copied");
                })
            }
            Some(ScreenKeyboardAction::Paste) => platform.read_clipboard_text().map(|text| {
                self.append_paste(&text);
                self.message = i18n::tr!("screen-keyboard-pasted");
            }),
            _ => return,
        };
        if let Err(error) = result {
            self.message = i18n::tr!("screen-keyboard-clipboard-error", error = error.to_string());
        }
    }

    fn step_focus(&mut self, layout: &ScreenKeyboardLayout, backwards: bool) {
        let count = layout.buttons.len();
        if count == 0 {
            return;
        }
        let current = layout
            .buttons
            .iter()
            .position(|button| button.action == self.focus)
            .unwrap_or(0);
        let index = if backwards {
            (current + count - 1) % count
        } else {
            (current + 1) % count
        };
        self.focus = layout.buttons[index].action;
    }

    fn vertical_focus(&mut self, layout: &ScreenKeyboardLayout, up: bool) {
        let Some(current) = layout
            .buttons
            .iter()
            .find(|button| button.action == self.focus)
        else {
            return;
        };
        let center = current.area.x + current.area.width / 2;
        let candidates = layout.buttons.iter().filter(|button| {
            if up {
                button.area.y < current.area.y
            } else {
                button.area.y > current.area.y
            }
        });
        if let Some(next) = candidates.min_by_key(|button| {
            (
                button.area.y.abs_diff(current.area.y),
                (button.area.x + button.area.width / 2).abs_diff(center),
            )
        }) {
            self.focus = next.action;
        }
    }

    fn handle_input(
        &mut self,
        input: InputEvent,
        layout: &ScreenKeyboardLayout,
        now: Instant,
    ) -> bool {
        match input {
            InputEvent::Key(key) if key.is_press_like() => {
                self.pressed = None;
                if key.key == Key::Escape {
                    return true;
                }
                if key.modifiers.super_key || key.modifiers.hyper || key.modifiers.meta {
                    return false;
                }
                let modifiers = ScreenKeyboardModifiers {
                    shift: self.modifiers.shift || key.modifiers.shift || key.key == Key::BackTab,
                    left_ctrl: self.modifiers.left_ctrl
                        || key.modifiers.control
                        || key.modifiers.ctrl,
                    right_ctrl: self.modifiers.right_ctrl,
                    alt: self.modifiers.alt || key.modifiers.alt,
                    caps_lock: self.modifiers.caps_lock,
                };
                match key.key {
                    Key::Char(character) if !character.is_control() && character != ' ' => {
                        self.use_keyboard();
                        self.navigating_buttons = false;
                        let action = character_action(character);
                        // The terminal already applies physical Shift/CapsLock.
                        // Only modifiers latched on screen change that character.
                        let typed = if character.is_ascii_alphabetic() {
                            if self.modifiers.shift ^ self.modifiers.caps_lock {
                                if character.is_ascii_uppercase() {
                                    character.to_ascii_lowercase()
                                } else {
                                    character.to_ascii_uppercase()
                                }
                            } else {
                                character
                            }
                        } else if self.modifiers.shift {
                            action.character(self.modifiers).unwrap_or(character)
                        } else {
                            character
                        };
                        self.type_key(action, Some(typed), modifiers);
                        if layout.buttons.iter().any(|button| button.action == action) {
                            self.focus = action;
                        }
                    }
                    Key::Backspace => {
                        self.use_keyboard();
                        self.navigating_buttons = false;
                        self.type_key(ScreenKeyboardAction::Backspace, None, modifiers);
                    }
                    Key::Tab | Key::BackTab if key.modifiers.is_control() || key.modifiers.alt => {
                        self.use_keyboard();
                        self.navigating_buttons = false;
                        self.type_key(ScreenKeyboardAction::Tab, Some('\t'), modifiers);
                    }
                    Key::Tab | Key::BackTab | Key::Left | Key::Right => {
                        self.use_keyboard();
                        self.navigating_buttons = true;
                        let backwards = matches!(key.key, Key::BackTab | Key::Left)
                            || (key.key == Key::Tab && key.modifiers.shift);
                        self.step_focus(layout, backwards);
                    }
                    Key::Up | Key::Down => {
                        self.use_keyboard();
                        self.navigating_buttons = true;
                        self.vertical_focus(layout, key.key == Key::Up);
                    }
                    Key::Enter | Key::Space | Key::Char(' ') => {
                        self.use_keyboard();
                        if self.navigating_buttons
                            && !key.modifiers.is_control()
                            && !key.modifiers.alt
                            && layout
                                .buttons
                                .iter()
                                .any(|button| button.action == self.focus)
                        {
                            return self.activate(self.focus);
                        } else {
                            self.navigating_buttons = false;
                            let action = if key.key == Key::Enter {
                                ScreenKeyboardAction::Enter
                            } else {
                                ScreenKeyboardAction::Space
                            };
                            self.type_key(action, action.character(modifiers), modifiers);
                        }
                    }
                    Key::F(number @ 1..=12) => {
                        self.use_keyboard();
                        self.navigating_buttons = false;
                        let action = ScreenKeyboardAction::Function(number);
                        self.type_key(action, None, modifiers);
                        if layout.buttons.iter().any(|button| button.action == action) {
                            self.focus = action;
                        }
                    }
                    _ => {}
                }
            }
            InputEvent::Mouse(mouse) => {
                let coordinates = mouse.coordinates();
                if mouse.kind == MouseEventKind::Moved
                    && self.mouse_coordinates == Some(coordinates)
                {
                    return false;
                }
                self.mouse_coordinates = Some(coordinates);
                self.keyboard_focus_visible = false;
                self.navigating_buttons = false;
                let hit = layout
                    .buttons
                    .iter()
                    .find(|button| button.area.contains(coordinates.into()));
                match mouse.kind {
                    MouseEventKind::Moved => {
                        self.hovered = hit.map(|button| button.region());
                        if self
                            .pressed
                            .as_ref()
                            .is_some_and(|press| self.hovered.as_ref() != Some(&press.region))
                        {
                            self.pressed = None;
                        }
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        self.cancel_pointer();
                        if let Some(button) = hit {
                            let region = button.region();
                            self.focus = button.action;
                            self.hovered = Some(region.clone());
                            self.pressed = Some(PointerPress {
                                action: button.action,
                                region,
                                at: now,
                            });
                        }
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        self.hovered = None;
                        if let Some(press) = self.pressed.take()
                            && now.saturating_duration_since(press.at) <= BUTTON_MAX_PRESS
                            && hit.is_some_and(|button| button.region() == press.region)
                        {
                            return self.activate(press.action);
                        }
                    }
                    // Dragging cancels the press even if the pointer returns to
                    // the original key before release. Clicks are never trusted
                    // without the original down/up pair.
                    _ => self.cancel_pointer(),
                }
            }
            InputEvent::FocusLost => {
                self.cancel_pointer();
                self.clear_one_shot_modifiers();
                self.mouse_coordinates = None;
                self.keyboard_focus_visible = false;
            }
            InputEvent::Resize { .. } => self.cancel_pointer(),
            InputEvent::Paste(text) => {
                self.use_keyboard();
                self.append_paste(&text);
                self.last_key = "Paste".to_owned();
                self.message = i18n::tr!("screen-keyboard-pasted");
            }
            _ => {}
        }
        false
    }
}

fn character_action(character: char) -> ScreenKeyboardAction {
    if character.is_ascii_alphabetic() {
        ScreenKeyboardAction::Letter(character.to_ascii_lowercase())
    } else {
        let base = "~!@#$%^&*()_+{}|:\"<>?"
            .chars()
            .zip("`1234567890-=[]\\;',./".chars())
            .find_map(|(shifted, base)| (shifted == character).then_some(base))
            .unwrap_or(character);
        ScreenKeyboardAction::Character(base)
    }
}

fn key_name(action: ScreenKeyboardAction, character: Option<char>) -> String {
    use ScreenKeyboardAction::*;
    match action {
        Letter(c) | Character(c) => return character.unwrap_or(c).to_ascii_uppercase().to_string(),
        Function(number) => return format!("F{number}"),
        Backspace => "Backspace",
        Clear => "Clear",
        Exit => "Exit",
        Escape => "Esc",
        Tab => "Tab",
        CapsLock => "CapsLock",
        Enter => "Enter",
        Shift => "Shift",
        Space => "Space",
        LeftCtrl => "Ctrl",
        RightCtrl => "RCtrl",
        Alt => "Alt",
        ToggleKeyboard => "Keyboard",
        Copy => "Copy",
        Paste => "Paste",
    }
    .to_owned()
}

#[cfg(test)]
#[path = "../tests/unit/screen_keyboard.rs"]
mod tests;
