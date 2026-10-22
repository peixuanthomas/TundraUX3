//! Encoding keys and paste according to the child terminal mode.
use ratatui::layout::Rect;
use ui::{
    Key as InputKey, KeyEvent as KeyInput, MouseButton, MouseEvent, MouseEventKind, ScrollDirection,
};
pub use vt100::{MouseProtocolEncoding, MouseProtocolMode};
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
        InputKey::Other(_) => return None,
    };
    if key.modifiers.alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

fn control_character(character: char) -> Option<u8> {
    let value = u8::try_from(character).ok()?;
    match value {
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

/// Encodes physical terminal mouse events in the mode selected by the child.
/// Synthetic Click/DoubleClick events must not duplicate their Down/Up reports.
pub fn mouse_event_bytes(
    mouse: &MouseEvent,
    content_area: Rect,
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    if mode == MouseProtocolMode::None
        || mouse.column() < content_area.x
        || mouse.column() >= content_area.right()
        || mouse.row() < content_area.y
        || mouse.row() >= content_area.bottom()
    {
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
    let column = u32::from(mouse.column() - content_area.x) + 1;
    let row = u32::from(mouse.row() - content_area.y) + 1;
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
