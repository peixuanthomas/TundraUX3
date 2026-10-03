//! Bounded framing around the upstream event decoder (TundraUX3 patch).
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crate::event::{
    input_error, sys::unix::parse::parse_event, Event, InputError, InputErrorKind, InternalEvent,
    KeyCode,
};

const REPORT_TIMEOUT: Duration = Duration::from_millis(250);
const ESCAPE_TIMEOUT: Duration = Duration::from_millis(50);
const PASTE_TIMEOUT: Duration = Duration::from_secs(5);
const REPORT_LIMIT: usize = 4096;
const PASTE_LIMIT: usize = 1024 * 1024;
const PASTE_END: &[u8] = b"\x1b[201~";

#[derive(Debug, Clone, Copy)]
enum Frame {
    Key,
    Csi,
    Sgr,
    X10,
    Ss3,
    Paste,
    String { osc: bool },
}

impl Frame {
    fn of(buffer: &[u8]) -> Self {
        if buffer.starts_with(b"\x1b[200~") {
            Self::Paste
        } else if buffer.starts_with(b"\x1b[<") {
            Self::Sgr
        } else if buffer.starts_with(b"\x1b[M") {
            Self::X10
        } else if buffer.starts_with(b"\x1b[") {
            Self::Csi
        } else if buffer.starts_with(b"\x1bO") {
            Self::Ss3
        } else if buffer.first() == Some(&0x1b)
            && matches!(buffer.get(1), Some(b']' | b'P' | b'_' | b'^' | b'X'))
        {
            Self::String {
                osc: buffer[1] == b']',
            }
        } else {
            Self::Key
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Key => "key/utf8",
            Self::Csi => "csi",
            Self::Sgr => "sgr-mouse",
            Self::X10 => "x10-mouse",
            Self::Ss3 => "ss3",
            Self::Paste => "bracketed-paste",
            Self::String { .. } => "control-string",
        }
    }

    fn opaque(self) -> bool {
        matches!(self, Self::Paste | Self::String { .. })
    }

    fn complete(self, buffer: &[u8]) -> bool {
        match self {
            Self::Key => true, // The upstream UTF-8 decoder determines completeness.
            Self::Csi => {
                buffer.len() > 2 && buffer != b"\x1b[[" && final_byte(*buffer.last().unwrap())
            }
            Self::Sgr => matches!(buffer.last(), Some(b'M' | b'm')),
            Self::X10 => buffer.len() == 6,
            Self::Ss3 => buffer.len() >= 3,
            Self::Paste => buffer.ends_with(PASTE_END),
            Self::String { osc } => {
                buffer.ends_with(b"\x1b\\") || (osc && buffer.ends_with(b"\x07"))
            }
        }
    }
}

fn final_byte(byte: u8) -> bool {
    (0x40..=0x7e).contains(&byte)
}

#[derive(Debug)]
struct Discard {
    frame: Frame,
    remaining: usize,
    terminator_prefix: usize,
}

impl Discard {
    fn new(frame: Frame, buffer: &[u8]) -> Self {
        let terminator = if matches!(frame, Frame::Paste) {
            PASTE_END
        } else {
            b"\x1b\\"
        };
        let terminator_prefix = (1..terminator.len())
            .rev()
            .find(|&n| buffer.ends_with(&terminator[..n]))
            .unwrap_or(0);
        Self {
            frame,
            remaining: 6usize.saturating_sub(buffer.len()),
            terminator_prefix,
        }
    }

    // Return (consume this byte, finished discarding). A fresh Escape starts a
    // new report except inside opaque string/paste content.
    fn advance(&mut self, byte: u8) -> (bool, bool) {
        if byte == 0x1b && !self.frame.opaque() {
            return (false, true);
        }
        let done = match self.frame {
            Frame::Key => {
                return if byte & 0xc0 == 0x80 {
                    (true, false)
                } else {
                    (false, true)
                }
            }
            Frame::Csi | Frame::Ss3 => final_byte(byte),
            Frame::Sgr => matches!(byte, b'M' | b'm'),
            Frame::X10 => {
                self.remaining = self.remaining.saturating_sub(1);
                self.remaining == 0
            }
            Frame::Paste | Frame::String { .. } => {
                let end = if matches!(self.frame, Frame::Paste) {
                    PASTE_END
                } else {
                    b"\x1b\\"
                };
                self.terminator_prefix = if byte == end[self.terminator_prefix] {
                    self.terminator_prefix + 1
                } else {
                    usize::from(byte == end[0])
                };
                self.terminator_prefix == end.len()
                    || (matches!(self.frame, Frame::String { osc: true }) && byte == 7)
            }
        };
        (true, done)
    }
}

#[derive(Debug, Default)]
pub(super) struct Parser {
    buffer: Vec<u8>,
    internal_events: VecDeque<InternalEvent>,
    last_byte_at: Option<Instant>,
    discard: Option<Discard>,
}

impl Parser {
    pub(super) fn advance(&mut self, buffer: &[u8], more: bool) {
        self.advance_at(buffer, more, Instant::now());
    }

    fn advance_at(&mut self, buffer: &[u8], more: bool, now: Instant) {
        self.expire_at(now);
        for (idx, &byte) in buffer.iter().enumerate() {
            let more = idx + 1 < buffer.len() || more;
            if let Some(discard) = self.discard.as_mut() {
                let (consumed, done) = discard.advance(byte);
                if done {
                    self.discard = None;
                }
                if consumed {
                    continue;
                }
            }
            if byte == 0x1b && !self.buffer.is_empty() && !Frame::of(&self.buffer).opaque() {
                if self.buffer == b"\x1b" {
                    self.internal_events
                        .push_back(InternalEvent::Event(Event::Key(KeyCode::Esc.into())));
                } else {
                    self.report(InputErrorKind::Incomplete);
                }
                self.clear();
            }
            self.buffer.push(byte);
            self.last_byte_at = Some(now);
            let frame = Frame::of(&self.buffer);
            let limit = if matches!(frame, Frame::Paste) {
                PASTE_LIMIT
            } else {
                REPORT_LIMIT
            };
            if self.buffer.len() > limit {
                self.reject(InputErrorKind::TooLong, !frame.complete(&self.buffer));
                continue;
            }
            if !frame.complete(&self.buffer) {
                continue;
            }
            // Control strings are framed and dropped whole; interpreting their
            // payload as Alt+']' followed by ordinary keys would trigger actions.
            let parsed = if matches!(frame, Frame::String { .. }) {
                Err(std::io::Error::other("unsupported control string"))
            } else {
                // A read boundary is not a report boundary. Briefly retain a
                // lone Escape so a split CSI prefix is not emitted as a key.
                parse_event(&self.buffer, more || self.buffer == b"\x1b")
            };
            match parsed {
                Ok(Some(event)) => {
                    self.internal_events.push_back(event);
                    self.clear();
                }
                Ok(None) if matches!(frame, Frame::Key) => {}
                Ok(None) => self.reject(InputErrorKind::Malformed, false),
                Err(_) => self.reject(InputErrorKind::Malformed, false),
            }
        }
    }

    fn clear(&mut self) {
        self.buffer.clear();
        self.last_byte_at = None;
    }

    fn report(&self, kind: InputErrorKind) {
        input_error::report(InputError {
            kind,
            protocol: Frame::of(&self.buffer).name(),
            buffered_bytes: self.buffer.len(),
        });
    }

    fn reject(&mut self, kind: InputErrorKind, discard_tail: bool) {
        self.report(kind);
        if discard_tail {
            self.discard = Some(Discard::new(Frame::of(&self.buffer), &self.buffer));
        }
        self.clear();
    }

    fn deadline(&self) -> Option<Instant> {
        self.last_byte_at.map(|last| {
            last + if self.buffer == b"\x1b" {
                ESCAPE_TIMEOUT
            } else if matches!(Frame::of(&self.buffer), Frame::Paste) {
                PASTE_TIMEOUT
            } else {
                REPORT_TIMEOUT
            }
        })
    }

    fn expire_at(&mut self, now: Instant) {
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            if self.buffer == b"\x1b" {
                self.internal_events
                    .push_back(InternalEvent::Event(Event::Key(KeyCode::Esc.into())));
                self.clear();
            } else {
                self.reject(InputErrorKind::Incomplete, true);
            }
        }
    }

    pub(super) fn poll_timeout(&self, requested: Option<Duration>) -> Option<Duration> {
        match (requested, self.deadline()) {
            (Some(timeout), Some(deadline)) => {
                Some(timeout.min(deadline.saturating_duration_since(Instant::now())))
            }
            (None, Some(deadline)) => Some(deadline.saturating_duration_since(Instant::now())),
            _ => requested,
        }
    }
}

impl Iterator for Parser {
    type Item = InternalEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.expire_at(Instant::now());
        self.internal_events.pop_front()
    }
}

#[cfg(test)]
#[path = "../../../../tests/escape_input.rs"]
mod escape_input_tests;
