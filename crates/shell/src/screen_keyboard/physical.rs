//! Terminal key feedback only; it never types text or toggles virtual modifiers.
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, ModifierKeyCode};
use ui::ScreenKeyboardAction;

use super::character_action;

const FLASH: Duration = Duration::from_millis(200);
const MINIMUM_FLASH: Duration = Duration::from_millis(100);

struct Press {
    code: KeyCode,
    action: ScreenKeyboardAction,
    at: Instant,
    expires: Option<Instant>,
    inferred: bool,
}

#[derive(Default)]
pub(super) struct PhysicalKeyboard {
    pub reports_release: bool,
    presses: Vec<Press>,
}

impl PhysicalKeyboard {
    pub fn clear(&mut self) {
        self.presses.clear();
    }

    pub fn expire(&mut self, now: Instant) -> bool {
        let count = self.presses.len();
        self.presses
            .retain(|press| press.expires.is_none_or(|end| now < end));
        count != self.presses.len()
    }

    pub fn pressed_actions(&self) -> Vec<ScreenKeyboardAction> {
        self.presses.iter().map(|press| press.action).collect()
    }

    pub fn observe(&mut self, key: KeyEvent, now: Instant) {
        self.expire(now);
        let code = normalize(key.code);
        let action = action_for(code);
        if key.kind != KeyEventKind::Release {
            self.presses.retain(|press| !press.inferred);
        }
        if let Some(action) = action {
            if key.kind == KeyEventKind::Release {
                if let Some(press) = self.presses.iter_mut().find(|press| press.code == code) {
                    // Very fast taps (including ConPTY's synthesized key-up) must
                    // survive long enough to be visible in a rendered frame.
                    press.expires = Some(now.max(press.at + MINIMUM_FLASH));
                }
            } else if let Some(press) = self.presses.iter_mut().find(|press| press.code == code) {
                press.expires = (!self.reports_release).then_some(now + FLASH);
                press.at = now;
            } else {
                self.presses.push(Press {
                    code,
                    action,
                    at: now,
                    expires: (!self.reports_release).then_some(now + FLASH),
                    inferred: false,
                });
            }
        }

        // Legacy terminals report modifiers only alongside another key, without
        // a left/right identity or an independent release. Show a short pulse.
        for (flag, modifier, actions) in [
            (
                KeyModifiers::SHIFT,
                ModifierKeyCode::LeftShift,
                &[ScreenKeyboardAction::Shift][..],
            ),
            (
                KeyModifiers::CONTROL,
                ModifierKeyCode::LeftControl,
                &[
                    ScreenKeyboardAction::LeftCtrl,
                    ScreenKeyboardAction::RightCtrl,
                ][..],
            ),
            (
                KeyModifiers::ALT,
                ModifierKeyCode::LeftAlt,
                &[ScreenKeyboardAction::Alt][..],
            ),
        ] {
            let active = key.modifiers.contains(flag)
                || (flag == KeyModifiers::SHIFT && key.code == KeyCode::BackTab);
            if key.kind != KeyEventKind::Release
                && active
                && !action.is_some_and(|action| actions.contains(&action))
                && !self
                    .presses
                    .iter()
                    .any(|press| actions.contains(&press.action))
            {
                self.presses.push(Press {
                    code: KeyCode::Modifier(modifier),
                    action: actions[0],
                    at: now,
                    expires: Some(now + FLASH),
                    inferred: true,
                });
            }
        }
    }
}

fn normalize(code: KeyCode) -> KeyCode {
    match code {
        KeyCode::Char(' ') => code,
        KeyCode::Char(character) => match character_action(character) {
            ScreenKeyboardAction::Letter(base) | ScreenKeyboardAction::Character(base) => {
                KeyCode::Char(base)
            }
            _ => unreachable!(),
        },
        KeyCode::BackTab => KeyCode::Tab,
        _ => code,
    }
}

fn action_for(code: KeyCode) -> Option<ScreenKeyboardAction> {
    use ScreenKeyboardAction as Action;
    Some(match code {
        KeyCode::Char(' ') => Action::Space,
        KeyCode::Char(c) if c.is_ascii_alphabetic() => Action::Letter(c),
        KeyCode::Char(c) if "`1234567890-=[]\\;',./".contains(c) => Action::Character(c),
        KeyCode::F(n @ 1..=12) => Action::Function(n),
        KeyCode::Esc => Action::Escape,
        KeyCode::Tab => Action::Tab,
        KeyCode::Enter => Action::Enter,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::CapsLock => Action::CapsLock,
        KeyCode::Modifier(ModifierKeyCode::LeftShift | ModifierKeyCode::RightShift) => {
            Action::Shift
        }
        KeyCode::Modifier(ModifierKeyCode::LeftControl) => Action::LeftCtrl,
        KeyCode::Modifier(ModifierKeyCode::RightControl) => Action::RightCtrl,
        KeyCode::Modifier(ModifierKeyCode::LeftAlt | ModifierKeyCode::RightAlt) => Action::Alt,
        _ => return None,
    })
}
