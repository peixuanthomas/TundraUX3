//! Decode ConPTY's CSI Vk;Sc;Uc;Kd;Cs;Rc_ reports for Linux/WSL.
//! https://github.com/microsoft/terminal/blob/main/doc/specs/%234999%20-%20Improved%20keyboard%20handling%20in%20Conpty.md
use crate::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, ModifierKeyCode};
use std::{collections::HashSet, io};

#[derive(Debug, Default)]
pub(super) struct Decoder {
    surrogate: Option<(u16, u16, bool, u32)>,
    pressed: HashSet<u16>,
}

pub(super) enum Decoded {
    Keys(Vec<KeyEvent>),
    Bytes(Vec<u8>),
}

impl Decoder {
    pub(super) fn decode(&mut self, report: &[u8]) -> io::Result<Decoded> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid win32 key report");
        let text = std::str::from_utf8(&report[2..report.len() - 1]).map_err(|_| invalid())?;
        let mut fields = [0u32, 0, 0, 0, 0, 1];
        for (index, value) in text.split(';').enumerate() {
            if index >= fields.len() {
                return Err(invalid());
            }
            if !value.is_empty() {
                fields[index] = value.parse().map_err(|_| invalid())?;
            }
        }
        let [vk, scan, unicode, down, control, repeat] = fields;
        if vk > u16::MAX as u32
            || scan > u16::MAX as u32
            || unicode > u16::MAX as u32
            || down > 1
            || repeat > 1024
        {
            return Err(invalid());
        }
        let vk = vk as u16;
        let down = down == 1;
        let mut modifiers = KeyModifiers::empty();
        modifiers.set(KeyModifiers::SHIFT, control & 0x10 != 0);
        modifiers.set(KeyModifiers::CONTROL, control & 0x0c != 0);
        modifiers.set(KeyModifiers::ALT, control & 0x03 != 0);
        let character = match unicode as u16 {
            high @ 0xd800..=0xdbff => {
                self.surrogate = Some((high, vk, down, control));
                return Ok(Decoded::Keys(Vec::new()));
            }
            low @ 0xdc00..=0xdfff => {
                let Some((high, old_vk, old_down, old_control)) = self.surrogate.take() else {
                    return Err(invalid());
                };
                if (old_vk, old_down, old_control) != (vk, down, control) {
                    return Err(invalid());
                }
                char::decode_utf16([high, low])
                    .next()
                    .unwrap()
                    .map_err(|_| invalid())?
            }
            value => {
                self.surrogate = None;
                char::from_u32(u32::from(value)).ok_or_else(invalid)?
            }
        };
        // ConPTY wraps pasted text and VT reports in VK=0 records. Feed those
        // bytes through the normal parser, including bracketed paste/mouse CSI.
        if vk == 0 {
            return Ok(Decoded::Bytes(if down {
                character
                    .to_string()
                    .repeat(repeat.max(1) as usize)
                    .into_bytes()
            } else {
                Vec::new()
            }));
        }
        let enhanced = control & 0x100 != 0;
        let code = match vk {
            0x08 => KeyCode::Backspace,
            0x09 if modifiers.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
            0x09 => KeyCode::Tab,
            0x0d => KeyCode::Enter,
            0x1b => KeyCode::Esc,
            0x21 => KeyCode::PageUp,
            0x22 => KeyCode::PageDown,
            0x23 => KeyCode::End,
            0x24 => KeyCode::Home,
            0x25 => KeyCode::Left,
            0x26 => KeyCode::Up,
            0x27 => KeyCode::Right,
            0x28 => KeyCode::Down,
            0x2d => KeyCode::Insert,
            0x2e => KeyCode::Delete,
            0x70..=0x87 => KeyCode::F((vk - 0x70 + 1) as u8),
            0x10 => KeyCode::Modifier(if scan == 0x36 {
                ModifierKeyCode::RightShift
            } else {
                ModifierKeyCode::LeftShift
            }),
            0x11 => KeyCode::Modifier(if enhanced {
                ModifierKeyCode::RightControl
            } else {
                ModifierKeyCode::LeftControl
            }),
            0x12 => KeyCode::Modifier(if enhanced {
                ModifierKeyCode::RightAlt
            } else {
                ModifierKeyCode::LeftAlt
            }),
            0x14 => KeyCode::CapsLock,
            0x90 => KeyCode::NumLock,
            0x91 => KeyCode::ScrollLock,
            0x20 if character == '\0' => KeyCode::Char(' '),
            0x41..=0x5a if character.is_control() => {
                KeyCode::Char(if modifiers.contains(KeyModifiers::SHIFT) {
                    char::from_u32(u32::from(vk)).unwrap()
                } else {
                    char::from_u32(u32::from(vk) + 32).unwrap()
                })
            }
            0xdb if character.is_control() && modifiers.contains(KeyModifiers::CONTROL) => {
                KeyCode::Char('[')
            }
            0xdc if character.is_control() && modifiers.contains(KeyModifiers::CONTROL) => {
                KeyCode::Char('\\')
            }
            0xdd if character.is_control() && modifiers.contains(KeyModifiers::CONTROL) => {
                KeyCode::Char(']')
            }
            0xbd if character.is_control() && modifiers.contains(KeyModifiers::CONTROL) => {
                KeyCode::Char('_')
            }
            0x36 if character.is_control() && modifiers.contains(KeyModifiers::CONTROL) => {
                KeyCode::Char('^')
            }
            0x32 if character.is_control() && modifiers.contains(KeyModifiers::CONTROL) => {
                KeyCode::Char('@')
            }
            _ if character != '\0' => {
                // AltGr produces printable text, not Ctrl+Alt commands.
                if control & 0x09 == 0x09 && !character.is_control() {
                    modifiers.remove(KeyModifiers::CONTROL | KeyModifiers::ALT);
                }
                KeyCode::Char(character)
            }
            _ => return Ok(Decoded::Keys(Vec::new())),
        };
        let kind = if down {
            if self.pressed.insert(vk) {
                KeyEventKind::Press
            } else {
                KeyEventKind::Repeat
            }
        } else {
            self.pressed.remove(&vk);
            KeyEventKind::Release
        };
        let mut state = KeyEventState::empty();
        state.set(KeyEventState::CAPS_LOCK, control & 0x80 != 0);
        state.set(KeyEventState::NUM_LOCK, control & 0x20 != 0);
        state.set(
            KeyEventState::KEYPAD,
            (0x60..=0x6f).contains(&vk) || (vk == 0x0d && enhanced),
        );
        let mut keys = vec![KeyEvent::new_with_kind_and_state(
            code, modifiers, kind, state,
        )];
        if down {
            keys.extend((1..repeat.max(1)).map(|_| {
                KeyEvent::new_with_kind_and_state(code, modifiers, KeyEventKind::Repeat, state)
            }));
        }
        Ok(Decoded::Keys(keys))
    }
}
