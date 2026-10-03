//! Metadata-only diagnostics for discarded terminal input (TundraUX3 patch).

use parking_lot::Mutex;

/// Why an input report was discarded before it could become an operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputErrorKind {
    /// An unsupported or malformed complete report.
    Malformed,
    /// A report interrupted by another report or left incomplete past its deadline.
    Incomplete,
    /// A report exceeded the bounded input buffer.
    TooLong,
}

/// Describes a discarded report without exposing typed or pasted content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputError {
    pub kind: InputErrorKind,
    pub protocol: &'static str,
    pub buffered_bytes: usize,
}

static HANDLER: Mutex<Option<fn(InputError)>> = Mutex::new(None);

/// Install a process-wide, nonblocking diagnostic sink. The callback must not
/// read terminal events or write to the terminal; it runs inside the reader.
pub fn set_input_error_handler(handler: fn(InputError)) {
    *HANDLER.lock() = Some(handler);
}

pub(crate) fn report(error: InputError) {
    let handler = *HANDLER.lock();
    if let Some(handler) = handler {
        handler(error);
    }
}
