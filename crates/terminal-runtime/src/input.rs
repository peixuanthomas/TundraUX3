//! Encoding keys and paste according to the child terminal mode.
use ratatui::layout::Rect;
use ui::{
    InputPhase, Key as InputKey, KeyEvent as KeyInput, MediaKey, ModifierKey, MouseButton,
    MouseEvent, MouseEventKind, ScrollDirection,
};
pub use vt100::{MouseProtocolEncoding, MouseProtocolMode};
pub const SUPPORTED_KITTY_KEYBOARD_FLAGS: u16 = 1 | 2 | 8;
/// A key-independent input representation.  The controller can use this
/// instead of leaking terminal escape sequences into UI code.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum TerminalInput {
    Bytes(Vec<u8>),
    Text(String),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Delete,
    PageUp,
    PageDown,
    CtrlC,
}

pub fn encode_terminal_input(input: &TerminalInput, application_cursor: bool) -> Vec<u8> {
    match input {
        TerminalInput::Bytes(bytes) => bytes.clone(),
        TerminalInput::Text(text) => text.as_bytes().to_vec(),
        TerminalInput::Enter => b"\r".to_vec(),
        TerminalInput::Backspace => vec![0x7f],
        TerminalInput::Tab => b"\t".to_vec(),
        TerminalInput::Escape => vec![0x1b],
        TerminalInput::CtrlC => vec![0x03],
        TerminalInput::Up => cursor_sequence(b'A', application_cursor),
        TerminalInput::Down => cursor_sequence(b'B', application_cursor),
        TerminalInput::Right => cursor_sequence(b'C', application_cursor),
        TerminalInput::Left => cursor_sequence(b'D', application_cursor),
        TerminalInput::Home => cursor_sequence(b'H', application_cursor),
        TerminalInput::End => cursor_sequence(b'F', application_cursor),
        TerminalInput::Delete => b"\x1b[3~".to_vec(),
        TerminalInput::PageUp => b"\x1b[5~".to_vec(),
        TerminalInput::PageDown => b"\x1b[6~".to_vec(),
    }
}

fn cursor_sequence(final_byte: u8, application_cursor: bool) -> Vec<u8> {
    if application_cursor {
        vec![0x1b, b'O', final_byte]
    } else {
        vec![0x1b, b'[', final_byte]
    }
}

pub fn key_event_bytes(key: &KeyInput, application_cursor: bool) -> Option<Vec<u8>> {
    if !key.phase.is_press_like() {
        return None;
    }
    let control = key.modifiers.is_control();
    let modifier =
        1 + u8::from(key.modifiers.shift) + 2 * u8::from(key.modifiers.alt) + 4 * u8::from(control);
    if modifier > 1 {
        let sequence = match key.key {
            InputKey::Left => Some(format!("\x1b[1;{modifier}D")),
            InputKey::Right => Some(format!("\x1b[1;{modifier}C")),
            InputKey::Up => Some(format!("\x1b[1;{modifier}A")),
            InputKey::Down => Some(format!("\x1b[1;{modifier}B")),
            InputKey::Home => Some(format!("\x1b[1;{modifier}H")),
            InputKey::End => Some(format!("\x1b[1;{modifier}F")),
            InputKey::Insert => Some(format!("\x1b[2;{modifier}~")),
            InputKey::Delete => Some(format!("\x1b[3;{modifier}~")),
            InputKey::PageUp => Some(format!("\x1b[5;{modifier}~")),
            InputKey::PageDown => Some(format!("\x1b[6;{modifier}~")),
            InputKey::F(number @ 1..=4) => Some(format!(
                "\x1b[1;{modifier}{}",
                char::from(b'P' + number - 1)
            )),
            InputKey::F(number @ 5..=12) => {
                let parameter = [15, 17, 18, 19, 20, 21, 23, 24][usize::from(number - 5)];
                Some(format!("\x1b[{parameter};{modifier}~"))
            }
            InputKey::Tab if modifier == 2 => Some("\x1b[Z".to_owned()),
            _ => None,
        };
        if let Some(sequence) = sequence {
            return Some(sequence.into_bytes());
        }
    }
    let mut bytes = match &key.key {
        InputKey::Char(character) if control => {
            control_character(*character).map(|byte| vec![byte])?
        }
        InputKey::Char(character) => character.to_string().into_bytes(),
        InputKey::Space if control => vec![0],
        InputKey::Space => vec![b' '],
        InputKey::Enter => encode_terminal_input(&TerminalInput::Enter, application_cursor),
        InputKey::Escape => encode_terminal_input(&TerminalInput::Escape, application_cursor),
        InputKey::Backspace => encode_terminal_input(&TerminalInput::Backspace, application_cursor),
        InputKey::Tab => encode_terminal_input(&TerminalInput::Tab, application_cursor),
        InputKey::BackTab => b"\x1b[Z".to_vec(),
        InputKey::Delete => encode_terminal_input(&TerminalInput::Delete, application_cursor),
        InputKey::Insert => b"\x1b[2~".to_vec(),
        InputKey::Left => encode_terminal_input(&TerminalInput::Left, application_cursor),
        InputKey::Right => encode_terminal_input(&TerminalInput::Right, application_cursor),
        InputKey::Up => encode_terminal_input(&TerminalInput::Up, application_cursor),
        InputKey::Down => encode_terminal_input(&TerminalInput::Down, application_cursor),
        InputKey::Home => encode_terminal_input(&TerminalInput::Home, application_cursor),
        InputKey::End => encode_terminal_input(&TerminalInput::End, application_cursor),
        InputKey::PageUp => encode_terminal_input(&TerminalInput::PageUp, application_cursor),
        InputKey::PageDown => encode_terminal_input(&TerminalInput::PageDown, application_cursor),
        InputKey::F(number) => function_key_bytes(*number)?,
        InputKey::Null => vec![0],
        InputKey::KeypadBegin => b"\x1b[E".to_vec(),
        InputKey::CapsLock
        | InputKey::ScrollLock
        | InputKey::NumLock
        | InputKey::PrintScreen
        | InputKey::Pause
        | InputKey::Menu
        | InputKey::Media(_)
        | InputKey::Modifier(_)
        | InputKey::Other(_) => return None,
    };
    if key.modifiers.alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

/// Encodes the supported Kitty enhancements: disambiguation, event types and all keys.
pub fn kitty_key_event_bytes(key: &KeyInput, flags: u16) -> Option<Vec<u8>> {
    if flags & SUPPORTED_KITTY_KEYBOARD_FLAGS == 0 {
        return key_event_bytes(key, false);
    }
    let all_keys = flags & 8 != 0;
    let event_types = flags & 2 != 0;
    let disambiguate = flags & 1 != 0 || all_keys;
    if key.phase == InputPhase::Release && !event_types {
        return None;
    }
    if matches!(key.key, InputKey::Modifier(_)) && !all_keys {
        return None;
    }
    let keypad = (key.state.keypad && disambiguate)
        .then(|| kitty_keypad_code(&key.key))
        .flatten();
    // Text and these three controls retain their recovery-friendly legacy form.
    if !all_keys
        && keypad.is_none()
        && (matches!(
            key.key,
            InputKey::Enter | InputKey::Tab | InputKey::BackTab | InputKey::Backspace
        ) || matches!(key.key, InputKey::Char(_) | InputKey::Space)
            && (!key.has_non_shift_modifier() || !disambiguate))
    {
        return key
            .phase
            .is_press_like()
            .then(|| key_event_bytes(key, false))
            .flatten();
    }
    let (mut code, suffix) = keypad.or_else(|| kitty_key_code(&key.key))?;
    // Win32 and legacy input already apply ASCII letter case to the character.
    if let InputKey::Char(character) = key.key
        && character.is_ascii_uppercase()
        && (key.modifiers.shift || key.state.caps_lock)
    {
        code = u32::from(character.to_ascii_lowercase());
    }
    let modifiers = 1
        + u16::from(key.modifiers.shift || matches!(key.key, InputKey::BackTab))
        + 2 * u16::from(key.modifiers.alt)
        + 4 * u16::from(key.modifiers.is_control())
        + 8 * u16::from(key.modifiers.super_key)
        + 16 * u16::from(key.modifiers.hyper)
        + 32 * u16::from(key.modifiers.meta)
        + 64 * u16::from(key.state.caps_lock)
        + 128 * u16::from(key.state.num_lock);
    let phase = match (event_types, key.phase) {
        (true, InputPhase::Repeat) => ":2",
        (true, InputPhase::Release) => ":3",
        _ => "",
    };
    Some(format!("\x1b[{code};{modifiers}{phase}{}", char::from(suffix)).into_bytes())
}

fn kitty_key_code(key: &InputKey) -> Option<(u32, u8)> {
    let code = match key {
        InputKey::Char(character) => *character as u32,
        InputKey::Space => 32,
        InputKey::Null => 0,
        InputKey::Escape => 27,
        InputKey::Enter => 13,
        InputKey::Tab | InputKey::BackTab => 9,
        InputKey::Backspace => 127,
        InputKey::Insert => return Some((2, b'~')),
        InputKey::Delete => return Some((3, b'~')),
        InputKey::PageUp => return Some((5, b'~')),
        InputKey::PageDown => return Some((6, b'~')),
        InputKey::Left => return Some((1, b'D')),
        InputKey::Right => return Some((1, b'C')),
        InputKey::Up => return Some((1, b'A')),
        InputKey::Down => return Some((1, b'B')),
        InputKey::Home => return Some((1, b'H')),
        InputKey::End => return Some((1, b'F')),
        InputKey::F(1) => return Some((1, b'P')),
        InputKey::F(2) => return Some((1, b'Q')),
        InputKey::F(3) => return Some((13, b'~')),
        InputKey::F(4) => return Some((1, b'S')),
        InputKey::F(number @ 5..=12) => {
            return Some((
                [15, 17, 18, 19, 20, 21, 23, 24][usize::from(number - 5)],
                b'~',
            ));
        }
        InputKey::F(number @ 13..=35) => 57376 + u32::from(number - 13),
        InputKey::CapsLock => 57358,
        InputKey::ScrollLock => 57359,
        InputKey::NumLock => 57360,
        InputKey::PrintScreen => 57361,
        InputKey::Pause => 57362,
        InputKey::Menu => 57363,
        InputKey::KeypadBegin => return Some((1, b'E')),
        InputKey::Media(key) => match key {
            MediaKey::Play => 57428,
            MediaKey::Pause => 57429,
            MediaKey::PlayPause => 57430,
            MediaKey::Reverse => 57431,
            MediaKey::Stop => 57432,
            MediaKey::FastForward => 57433,
            MediaKey::Rewind => 57434,
            MediaKey::TrackNext => 57435,
            MediaKey::TrackPrevious => 57436,
            MediaKey::Record => 57437,
            MediaKey::LowerVolume => 57438,
            MediaKey::RaiseVolume => 57439,
            MediaKey::MuteVolume => 57440,
        },
        InputKey::Modifier(key) => match key {
            ModifierKey::LeftShift => 57441,
            ModifierKey::LeftControl => 57442,
            ModifierKey::LeftAlt => 57443,
            ModifierKey::LeftSuper => 57444,
            ModifierKey::LeftHyper => 57445,
            ModifierKey::LeftMeta => 57446,
            ModifierKey::RightShift => 57447,
            ModifierKey::RightControl => 57448,
            ModifierKey::RightAlt => 57449,
            ModifierKey::RightSuper => 57450,
            ModifierKey::RightHyper => 57451,
            ModifierKey::RightMeta => 57452,
            ModifierKey::IsoLevel3Shift => 57453,
            ModifierKey::IsoLevel5Shift => 57454,
        },
        InputKey::F(_) | InputKey::Other(_) => return None,
    };
    Some((code, b'u'))
}

fn kitty_keypad_code(key: &InputKey) -> Option<(u32, u8)> {
    let code = match key {
        InputKey::Char(character @ '0'..='9') => 57399 + u32::from(*character) - u32::from('0'),
        InputKey::Char('.') => 57409,
        InputKey::Char('/') => 57410,
        InputKey::Char('*') => 57411,
        InputKey::Char('-') => 57412,
        InputKey::Char('+') => 57413,
        InputKey::Enter => 57414,
        InputKey::Char('=') => 57415,
        InputKey::Char(',') => 57416,
        InputKey::Left => 57417,
        InputKey::Right => 57418,
        InputKey::Up => 57419,
        InputKey::Down => 57420,
        InputKey::PageUp => 57421,
        InputKey::PageDown => 57422,
        InputKey::Home => 57423,
        InputKey::End => 57424,
        InputKey::Insert => 57425,
        InputKey::Delete => 57426,
        InputKey::KeypadBegin => return Some((1, b'E')),
        _ => return None,
    };
    Some((code, b'u'))
}

fn control_character(character: char) -> Option<u8> {
    let value = u8::try_from(character).ok()?;
    match value {
        b' ' => Some(0),
        b'@'..=b'_' => Some(value & 0x1f),
        b'a'..=b'z' => Some(value.to_ascii_uppercase() & 0x1f),
        b'?' => Some(0x7f),
        _ => None,
    }
}

fn function_key_bytes(number: u8) -> Option<Vec<u8>> {
    let sequence: &[u8] = match number {
        1 => b"\x1bOP",
        2 => b"\x1bOQ",
        3 => b"\x1bOR",
        4 => b"\x1bOS",
        5 => b"\x1b[15~",
        6 => b"\x1b[17~",
        7 => b"\x1b[18~",
        8 => b"\x1b[19~",
        9 => b"\x1b[20~",
        10 => b"\x1b[21~",
        11 => b"\x1b[23~",
        12 => b"\x1b[24~",
        _ => return None,
    };
    Some(sequence.to_vec())
}

pub fn paste_bytes(text: &str, bracketed_paste: bool) -> Vec<u8> {
    if !bracketed_paste {
        return text.as_bytes().to_vec();
    }
    let mut bytes = Vec::with_capacity(text.len().saturating_add(12));
    bytes.extend_from_slice(b"\x1b[200~");
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(b"\x1b[201~");
    bytes
}

/// Converts host coordinates to zero-based child coordinates, excluding chrome.
pub fn mouse_to_terminal(mouse: &MouseEvent, content_area: Rect) -> Option<MouseEvent> {
    content_area
        .contains((mouse.column(), mouse.row()).into())
        .then(|| {
            MouseEvent::new(
                mouse.column() - content_area.x,
                mouse.row() - content_area.y,
                mouse.kind,
            )
            .with_modifiers(mouse.modifiers)
        })
}

/// Encodes a mouse event within the supplied terminal viewport.
pub fn mouse_event_bytes(
    mouse: &MouseEvent,
    content_area: Rect,
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    terminal_mouse_bytes(&mouse_to_terminal(mouse, content_area)?, mode, encoding)
}

pub(crate) fn terminal_mouse_bytes(
    mouse: &MouseEvent,
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    if mode == MouseProtocolMode::None {
        return None;
    }
    let button_code = |button| match button {
        MouseButton::Left => 0_u8,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let (mut code, released) = match mouse.kind {
        MouseEventKind::Down(button) => (button_code(button), false),
        MouseEventKind::Up(button) if mode != MouseProtocolMode::Press => {
            (button_code(button), true)
        }
        MouseEventKind::Drag(button)
            if matches!(
                mode,
                MouseProtocolMode::ButtonMotion | MouseProtocolMode::AnyMotion
            ) =>
        {
            (button_code(button) | 32, false)
        }
        MouseEventKind::Moved if mode == MouseProtocolMode::AnyMotion => (35, false),
        MouseEventKind::Scroll(direction) => (
            match direction {
                ScrollDirection::Up => 64,
                ScrollDirection::Down => 65,
                ScrollDirection::Left => 66,
                ScrollDirection::Right => 67,
            },
            false,
        ),
        _ => return None,
    };
    if mouse.modifiers.shift {
        code |= 4;
    }
    if mouse.modifiers.alt {
        code |= 8;
    }
    if mouse.modifiers.is_control() {
        code |= 16;
    }
    let column = u32::from(mouse.column()) + 1;
    let row = u32::from(mouse.row()) + 1;
    if encoding == MouseProtocolEncoding::Sgr {
        return Some(
            format!(
                "\x1b[<{code};{column};{row}{}",
                if released { 'm' } else { 'M' }
            )
            .into_bytes(),
        );
    }
    if released {
        code = (code & !3) | 3;
    }
    let mut bytes = b"\x1b[M".to_vec();
    bytes.push(code + 32);
    let limit = if encoding == MouseProtocolEncoding::Utf8 {
        2015
    } else {
        223
    };
    if column > limit || row > limit {
        return None;
    }
    for coordinate in [column, row] {
        if encoding == MouseProtocolEncoding::Utf8 {
            let mut buffer = [0; 4];
            bytes.extend_from_slice(
                char::from_u32(coordinate + 32)?
                    .encode_utf8(&mut buffer)
                    .as_bytes(),
            );
        } else {
            bytes.push(u8::try_from(coordinate + 32).ok()?);
        }
    }
    Some(bytes)
}

#[cfg(test)]
#[path = "../tests/unit/input.rs"]
mod tests;
