//! One ordered, bounded input queue shared by keys and terminal replies.
use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Condvar, Mutex};

const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
struct State {
    pending: VecDeque<Vec<u8>>,
    bytes: usize,
    closed: bool,
    error: Option<(io::ErrorKind, String)>,
}

#[derive(Default)]
pub(crate) struct InputQueue {
    state: Mutex<State>,
    ready: Condvar,
}

impl InputQueue {
    pub(crate) fn writer(self: &Arc<Self>) -> Box<dyn Write + Send> {
        Box::new(QueuedWriter(self.clone()))
    }

    pub(crate) fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        state.pending.clear();
        state.bytes = 0;
        self.ready.notify_all();
    }

    pub(crate) fn check_error(&self) -> io::Result<()> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match &state.error {
            Some((kind, message)) => Err(io::Error::new(*kind, message.clone())),
            None => Ok(()),
        }
    }

    pub(crate) fn run(&self, mut writer: Box<dyn Write + Send>) {
        loop {
            let bytes = {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                while state.pending.is_empty() && !state.closed {
                    state = self.ready.wait(state).unwrap_or_else(|e| e.into_inner());
                }
                if state.closed {
                    return;
                }
                let bytes = state.pending.pop_front().unwrap();
                state.bytes -= bytes.len();
                bytes
            };
            if let Err(error) = writer.write_all(&bytes).and_then(|_| writer.flush()) {
                let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
                if !state.closed {
                    state.error = Some((error.kind(), error.to_string()));
                }
                state.closed = true;
                state.pending.clear();
                state.bytes = 0;
                return;
            }
        }
    }
}

struct QueuedWriter(Arc<InputQueue>);

impl Write for QueuedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((kind, message)) = &state.error {
            return Err(io::Error::new(*kind, message.clone()));
        }
        if state.closed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "terminal input is closed",
            ));
        }
        if bytes.len() > MAX_INPUT_BYTES.saturating_sub(state.bytes) {
            state.error = Some((
                io::ErrorKind::WouldBlock,
                "terminal input queue is full".into(),
            ));
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "terminal input queue is full",
            ));
        }
        if bytes.is_empty() {
            return Ok(0);
        }
        state.bytes += bytes.len();
        state.pending.push_back(bytes.to_vec());
        self.0.ready.notify_one();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.check_error()
    }
}

#[cfg(test)]
#[path = "../tests/unit/input_queue.rs"]
mod tests;
