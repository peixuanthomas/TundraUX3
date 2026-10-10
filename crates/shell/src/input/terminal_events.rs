use crate::{
    InputEvent, InputKey, InputModifiers, InputPhase, KeyInput, MouseInput, PointerButton,
    ScrollDirection,
};
use crossterm::event::{
    Event, KeyCode as CrosstermKeyCode, KeyEvent as CrosstermKeyEvent, KeyEventKind,
    KeyModifiers as CrosstermKeyModifiers, MouseButton as CrosstermMouseButton,
    MouseEvent as CrosstermMouseEvent, MouseEventKind as CrosstermMouseEventKind,
};
use ui::{MouseEventKind, Point};

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

fn key_event_to_input(key_event: CrosstermKeyEvent) -> KeyInput {
    let key = match key_event.code {
        CrosstermKeyCode::Char(character) => InputKey::Char(character),
        CrosstermKeyCode::Enter => InputKey::Enter,
        CrosstermKeyCode::Esc => InputKey::Escape,
        CrosstermKeyCode::Backspace => InputKey::Backspace,
        CrosstermKeyCode::Tab => InputKey::Tab,
        CrosstermKeyCode::BackTab => InputKey::BackTab,
        CrosstermKeyCode::Left => InputKey::Left,
        CrosstermKeyCode::Right => InputKey::Right,
        CrosstermKeyCode::Up => InputKey::Up,
        CrosstermKeyCode::Down => InputKey::Down,
        CrosstermKeyCode::Delete => InputKey::Delete,
        CrosstermKeyCode::Insert => InputKey::Insert,
        CrosstermKeyCode::Home => InputKey::Home,
        CrosstermKeyCode::End => InputKey::End,
        CrosstermKeyCode::PageUp => InputKey::PageUp,
        CrosstermKeyCode::PageDown => InputKey::PageDown,
        CrosstermKeyCode::F(number) => InputKey::F(number),
        other => InputKey::Other(format!("{other:?}")),
    };
    KeyInput::with_phase(
        key,
        modifiers_from_crossterm(key_event.modifiers),
        phase_from_crossterm(key_event.kind),
    )
}

fn modifiers_from_crossterm(modifiers: CrosstermKeyModifiers) -> InputModifiers {
    let control = modifiers.contains(CrosstermKeyModifiers::CONTROL);
    InputModifiers {
        shift: modifiers.contains(CrosstermKeyModifiers::SHIFT),
        control,
        ctrl: control,
        alt: modifiers.contains(CrosstermKeyModifiers::ALT),
        super_key: modifiers.contains(CrosstermKeyModifiers::SUPER),
        hyper: modifiers.contains(CrosstermKeyModifiers::HYPER),
        meta: modifiers.contains(CrosstermKeyModifiers::META),
    }
}

const fn phase_from_crossterm(kind: KeyEventKind) -> InputPhase {
    match kind {
        KeyEventKind::Press => InputPhase::Press,
        KeyEventKind::Repeat => InputPhase::Repeat,
        KeyEventKind::Release => InputPhase::Release,
    }
}

#[cfg(test)]
pub(crate) fn key_event_to_label(key_event: CrosstermKeyEvent) -> String {
    key_event_to_input(key_event).label()
}

pub(crate) fn mouse_event_to_input(mouse_event: CrosstermMouseEvent) -> InputEvent {
    let kind = match mouse_event.kind {
        CrosstermMouseEventKind::Down(button) => {
            MouseEventKind::Down(button_from_crossterm(button))
        }
        CrosstermMouseEventKind::Up(button) => MouseEventKind::Up(button_from_crossterm(button)),
        CrosstermMouseEventKind::Drag(button) => {
            MouseEventKind::Drag(button_from_crossterm(button))
        }
        CrosstermMouseEventKind::Moved => MouseEventKind::Moved,
        CrosstermMouseEventKind::ScrollDown => MouseEventKind::Scroll(ScrollDirection::Down),
        CrosstermMouseEventKind::ScrollUp => MouseEventKind::Scroll(ScrollDirection::Up),
        CrosstermMouseEventKind::ScrollLeft => MouseEventKind::Scroll(ScrollDirection::Left),
        CrosstermMouseEventKind::ScrollRight => MouseEventKind::Scroll(ScrollDirection::Right),
    };
    InputEvent::Mouse(MouseInput {
        position: Point::new(mouse_event.column, mouse_event.row),
        kind,
        modifiers: modifiers_from_crossterm(mouse_event.modifiers),
    })
}

const fn button_from_crossterm(button: CrosstermMouseButton) -> PointerButton {
    match button {
        CrosstermMouseButton::Left => PointerButton::Left,
        CrosstermMouseButton::Right => PointerButton::Right,
        CrosstermMouseButton::Middle => PointerButton::Middle,
    }
}
