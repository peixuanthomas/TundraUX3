use crate::{
    InputEvent, InputKey, InputModifiers, InputPhase, KeyInput, MouseInput, PointerButton,
    ScrollDirection,
};
use crossterm::event::{Event, KeyCode, KeyEvent, MouseEvent, MouseEventKind};

pub(crate) fn resets_login_idle_timeout(input: &InputEvent) -> bool {
    matches!(
        input,
        InputEvent::Key(_)
            | InputEvent::Mouse(_)
            | InputEvent::Resize { .. }
            | InputEvent::FocusGained
            | InputEvent::Paste(_)
    )
}

pub fn crossterm_event_to_input(event: Event) -> InputEvent {
    match event {
        Event::Key(key_event) => InputEvent::Key(key_event_to_input(key_event)),
        Event::Mouse(mouse_event) => mouse_event_to_input(mouse_event),
        Event::Resize(width, height) => InputEvent::Resize { width, height },
        Event::FocusGained => InputEvent::FocusGained,
        Event::FocusLost => InputEvent::FocusLost,
        Event::Paste(value) => InputEvent::Paste(value),
    }
}

fn key_event_to_input(key_event: KeyEvent) -> KeyInput {
    let key = match key_event.code {
        KeyCode::Char(character) => InputKey::Character(character),
        KeyCode::Enter => InputKey::Enter,
        KeyCode::Esc => InputKey::Escape,
        KeyCode::Backspace => InputKey::Backspace,
        KeyCode::Tab => InputKey::Tab,
        KeyCode::BackTab => InputKey::BackTab,
        KeyCode::Left => InputKey::Left,
        KeyCode::Right => InputKey::Right,
        KeyCode::Up => InputKey::Up,
        KeyCode::Down => InputKey::Down,
        KeyCode::Delete => InputKey::Delete,
        KeyCode::Insert => InputKey::Insert,
        KeyCode::Home => InputKey::Home,
        KeyCode::End => InputKey::End,
        KeyCode::PageUp => InputKey::PageUp,
        KeyCode::PageDown => InputKey::PageDown,
        KeyCode::F(number) => InputKey::Function(number),
        other => InputKey::Other(format!("{other:?}")),
    };

    KeyInput::new(
        key,
        InputModifiers::from(key_event.modifiers),
        InputPhase::from(key_event.kind),
    )
}

#[cfg(test)]
pub(crate) fn key_event_to_label(key_event: KeyEvent) -> String {
    key_event_to_input(key_event).label()
}

pub(crate) fn mouse_event_to_input(mouse_event: MouseEvent) -> InputEvent {
    let coordinates = (mouse_event.column, mouse_event.row);
    let modifiers = InputModifiers::from(mouse_event.modifiers);
    let mouse = match mouse_event.kind {
        MouseEventKind::Down(button) => MouseInput::Down {
            button: PointerButton::from(button),
            coordinates,
            modifiers,
        },
        MouseEventKind::Up(button) => MouseInput::Up {
            button: PointerButton::from(button),
            coordinates,
            modifiers,
        },
        MouseEventKind::Drag(button) => MouseInput::Drag {
            button: PointerButton::from(button),
            coordinates,
            modifiers,
        },
        MouseEventKind::Moved => MouseInput::Moved {
            coordinates,
            modifiers,
        },
        MouseEventKind::ScrollDown => MouseInput::Scroll {
            direction: ScrollDirection::Down,
            coordinates,
            modifiers,
        },
        MouseEventKind::ScrollUp => MouseInput::Scroll {
            direction: ScrollDirection::Up,
            coordinates,
            modifiers,
        },
        MouseEventKind::ScrollLeft => MouseInput::Scroll {
            direction: ScrollDirection::Left,
            coordinates,
            modifiers,
        },
        MouseEventKind::ScrollRight => MouseInput::Scroll {
            direction: ScrollDirection::Right,
            coordinates,
            modifiers,
        },
    };

    InputEvent::Mouse(mouse)
}
