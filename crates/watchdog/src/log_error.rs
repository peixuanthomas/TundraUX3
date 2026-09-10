use runtime_log::RuntimeLogEvent;
use std::error::Error;

/// Capture a bounded cause chain before an error crosses a string-only boundary.
/// Callers must pass metadata errors, never payloads or file contents.
pub fn capture_error(event: &mut RuntimeLogEvent, error: &(dyn Error + 'static)) {
    let mut current = Some(error);
    for _ in 0..16 {
        let Some(error) = current else {
            break;
        };
        if let Some(io) = error.downcast_ref::<std::io::Error>() {
            event.os_error_code = event.os_error_code.or(io.raw_os_error().map(i64::from));
        }
        event
            .error_chain
            .push(runtime_log::sanitize_text(&error.to_string()));
        current = error.source();
    }
}
