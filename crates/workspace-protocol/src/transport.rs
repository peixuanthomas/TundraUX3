//! Bounded framing over an already authenticated, nonblocking stream.

use serde::{Serialize, de::DeserializeOwned};
use std::collections::VecDeque;
use std::io::{self, Read, Write};

const LENGTH_BYTES: usize = size_of::<u32>();
const IO_CHUNK_BYTES: usize = 64 * 1024;
const IO_ROUNDS: usize = 16;
const MAX_MESSAGES: usize = 32;

/// Each message consists of a big-endian u32 length followed by one JSON value.
pub struct Framed<S> {
    stream: S,
    limit: usize,
    incoming: Vec<u8>,
    outgoing: VecDeque<Vec<u8>>,
    outgoing_bytes: usize,
    sent: usize,
    disconnected: Option<io::Error>,
}

impl<S: Read + Write> Framed<S> {
    pub fn new(stream: S, max_frame_bytes: usize) -> Self {
        Self {
            stream,
            limit: max_frame_bytes.min(u32::MAX as usize - LENGTH_BYTES),
            incoming: Vec::new(),
            outgoing: VecDeque::new(),
            outgoing_bytes: 0,
            sent: 0,
            disconnected: None,
        }
    }

    /// Queues one message. A full queue returns `WouldBlock` without queuing it.
    pub fn queue(&mut self, message: &impl Serialize) -> io::Result<()> {
        if self.disconnected.is_some() {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let room = self.limit.saturating_sub(self.outgoing_bytes);
        if room == 0 || self.outgoing.len() >= MAX_MESSAGES {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        let mut bytes = FrameWriter {
            bytes: vec![0; LENGTH_BYTES],
            room,
            limit_error: if self.outgoing.is_empty() {
                io::ErrorKind::InvalidInput
            } else {
                io::ErrorKind::WouldBlock
            },
        };
        serde_json::to_writer(&mut bytes, message)?;
        let mut bytes = bytes.bytes;
        let size = (bytes.len() - LENGTH_BYTES) as u32;
        bytes[..LENGTH_BYTES].copy_from_slice(&size.to_be_bytes());
        self.outgoing_bytes += bytes.len();
        self.outgoing.push_back(bytes);
        Ok(())
    }

    pub fn pending_write(&self) -> bool {
        !self.outgoing.is_empty()
    }

    /// Reads and writes at most 1 MiB each, with at most 32 decoded messages per call.
    pub fn poll<T: DeserializeOwned>(&mut self) -> io::Result<Vec<T>> {
        if let Some(error) = &self.disconnected {
            return Err(io::Error::new(error.kind(), error.to_string()));
        }
        let mut messages = Vec::new();
        let mut consumed = 0;
        if let Err(error) = self
            .receive(&mut consumed, &mut messages)
            .and_then(|()| self.send())
        {
            self.disconnected = Some(error);
        }
        if consumed != 0 {
            self.incoming.drain(..consumed);
            if self.incoming.is_empty() && self.incoming.capacity() > 1024 * 1024 {
                self.incoming = Vec::new();
            }
        }
        if messages.is_empty()
            && let Some(error) = &self.disconnected
        {
            return Err(io::Error::new(error.kind(), error.to_string()));
        }
        Ok(messages)
    }

    fn receive<T: DeserializeOwned>(
        &mut self,
        consumed: &mut usize,
        messages: &mut Vec<T>,
    ) -> io::Result<()> {
        let mut chunk = [0; IO_CHUNK_BYTES];
        for round in 0..=IO_ROUNDS {
            while messages.len() < MAX_MESSAGES {
                let input = &self.incoming[*consumed..];
                let Some(header) = input.first_chunk::<LENGTH_BYTES>() else {
                    break;
                };
                let size = u32::from_be_bytes(*header) as usize;
                if size == 0 || size > self.limit {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid frame length",
                    ));
                }
                let end = LENGTH_BYTES + size;
                let Some(payload) = input.get(LENGTH_BYTES..end) else {
                    break;
                };
                let message = serde_json::from_slice(payload)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                messages.push(message);
                *consumed += end;
            }
            if messages.len() == MAX_MESSAGES || round == IO_ROUNDS {
                break;
            }
            match self.stream.read(&mut chunk) {
                Ok(0) if self.incoming.len() == *consumed => {
                    return Err(io::ErrorKind::BrokenPipe.into());
                }
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(count) => self.incoming.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn send(&mut self) -> io::Result<()> {
        for _ in 0..IO_ROUNDS {
            let Some(frame) = self.outgoing.front() else {
                break;
            };
            let end = (self.sent + IO_CHUNK_BYTES).min(frame.len());
            match self.stream.write(&frame[self.sent..end]) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(count) => self.sent += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
            if self.sent == frame.len() {
                self.outgoing_bytes -= frame.len();
                self.outgoing.pop_front();
                self.sent = 0;
            }
        }
        Ok(())
    }
}

struct FrameWriter {
    bytes: Vec<u8>,
    room: usize,
    limit_error: io::ErrorKind,
}

impl Write for FrameWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.room {
            return Err(io::Error::new(
                self.limit_error,
                "workspace frame exceeds available space",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        self.room -= bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
