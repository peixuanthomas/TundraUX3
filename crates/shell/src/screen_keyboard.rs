//! Standalone debug keyboard. Text lives only in this loop and is never injected
//! into another application or saved to configuration.
use std::{
    io::{self, IsTerminal, Write},
    time::{Duration, Instant},
};

use crossterm::event;
use ratatui::layout::Rect;
use ui::{
    InputEvent, Key, MouseButton, MouseEventKind, RenderContext, ScreenKeyboardAction,
    ScreenKeyboardLayout, ScreenKeyboardViewModel, TundraTheme,
    components::{ButtonFrame, ButtonRegion},
};

use crate::{ShellAppConfig, TerminalGuard, crossterm_event_to_input};

const BUTTON_MAX_PRESS: Duration = Duration::from_millis(500);

pub fn run_screen_keyboard(
    output: &mut impl Write,
    appearance: &storage::AppearanceConfig,
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
    let mut layout = ui::screen_keyboard_layout(bounds);
    let mut dirty = true;
    let origin = Instant::now();
    loop {
        let now = Instant::now();
        dirty |= model.expire_press(now);
        if dirty {
            terminal.terminal_mut().draw(|frame| {
                if bounds != frame.area() {
                    bounds = frame.area();
                    layout = ui::screen_keyboard_layout(bounds);
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
        match action {
            ScreenKeyboardAction::Letter(letter) => self.text.push(letter),
            ScreenKeyboardAction::Backspace => {
                self.text.pop();
            }
            ScreenKeyboardAction::Clear => self.text.clear(),
            ScreenKeyboardAction::Exit => return true,
        }
        false
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
                if key.key == Key::Escape || key.is_ctrl_c() {
                    return true;
                }
                if key.modifiers.has_non_shift_modifier() {
                    return false;
                }
                match key.key {
                    Key::Char(letter) if letter.is_ascii_alphabetic() => {
                        self.use_keyboard();
                        self.text.push(letter);
                        let action = ScreenKeyboardAction::Letter(letter.to_ascii_lowercase());
                        if layout.buttons.iter().any(|button| button.action == action) {
                            self.focus = action;
                        }
                    }
                    Key::Backspace => {
                        self.use_keyboard();
                        self.activate(ScreenKeyboardAction::Backspace);
                    }
                    Key::Tab | Key::BackTab | Key::Left | Key::Right => {
                        self.use_keyboard();
                        let backwards = matches!(key.key, Key::BackTab | Key::Left)
                            || (key.key == Key::Tab && key.modifiers.shift);
                        self.step_focus(layout, backwards);
                    }
                    Key::Up | Key::Down => {
                        self.use_keyboard();
                        self.vertical_focus(layout, key.key == Key::Up);
                    }
                    Key::Enter | Key::Space | Key::Char(' ') => {
                        self.use_keyboard();
                        if layout
                            .buttons
                            .iter()
                            .any(|button| button.action == self.focus)
                        {
                            return self.activate(self.focus);
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
                self.mouse_coordinates = None;
                self.keyboard_focus_visible = false;
            }
            InputEvent::Resize { .. } => self.cancel_pointer(),
            _ => {}
        }
        false
    }
}

#[cfg(test)]
#[path = "../tests/unit/screen_keyboard.rs"]
mod tests;
