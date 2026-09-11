//! Normalize terminal input into the existing shared component event contract.
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ui::{InputEvent, Key};

pub fn translate(event: Event) -> Option<InputEvent> {
    match event {
        // Repeats, releases, paste, and modifier chords cannot authorize operations.
        Event::Key(key)
            if key.kind == KeyEventKind::Press
                && !key.modifiers.intersects(
                    KeyModifiers::CONTROL
                        | KeyModifiers::ALT
                        | KeyModifiers::SUPER
                        | KeyModifiers::HYPER
                        | KeyModifiers::META,
                ) =>
        {
            let code = match key.code {
                KeyCode::Char(' ') => Key::Space,
                KeyCode::Char(c) if !c.is_control() => Key::Char(c),
                KeyCode::Enter => Key::Enter,
                KeyCode::Esc => Key::Escape,
                KeyCode::Backspace => Key::Backspace,
                KeyCode::Delete => Key::Delete,
                KeyCode::Tab => Key::Tab,
                KeyCode::BackTab => Key::BackTab,
                KeyCode::Left => Key::Left,
                KeyCode::Right => Key::Right,
                KeyCode::Up => Key::Up,
                KeyCode::Down => Key::Down,
                KeyCode::Home => Key::Home,
                KeyCode::End => Key::End,
                _ => return None,
            };
            Some(InputEvent::key(code))
        }
        Event::Mouse(mouse) => {
            let position = (mouse.column, mouse.row);
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    Some(InputEvent::mouse_down(ui::MouseButton::Left, position))
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    Some(InputEvent::mouse_up(ui::MouseButton::Left, position))
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    Some(InputEvent::mouse_drag(ui::MouseButton::Left, position))
                }
                MouseEventKind::Moved => Some(InputEvent::mouse_moved(position)),
                MouseEventKind::ScrollUp => {
                    Some(InputEvent::mouse_scroll(ui::ScrollDirection::Up, position))
                }
                MouseEventKind::ScrollDown => Some(InputEvent::mouse_scroll(
                    ui::ScrollDirection::Down,
                    position,
                )),
                _ => None,
            }
        }
        Event::FocusLost => Some(InputEvent::FocusLost),
        Event::FocusGained => Some(InputEvent::FocusGained),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paste_repeat_and_modified_keys_are_never_authorizing_input() {
        assert!(translate(Event::Paste("yes\n".into())).is_none());
        assert_eq!(
            translate(Event::Key(crossterm::event::KeyEvent::new(
                KeyCode::Char(' '),
                KeyModifiers::NONE
            ))),
            Some(InputEvent::key(Key::Space))
        );
        let mut key = crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        key.kind = KeyEventKind::Repeat;
        assert!(translate(Event::Key(key)).is_none());
        key.kind = KeyEventKind::Press;
        key.modifiers = KeyModifiers::CONTROL;
        assert!(translate(Event::Key(key)).is_none());
    }
}
