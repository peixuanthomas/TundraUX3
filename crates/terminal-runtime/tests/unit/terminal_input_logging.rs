use super::*;
use crossterm::event::{InputError, InputErrorKind};

#[test]
fn discarded_reports_produce_metadata_only_warning_logs() {
    for (kind, code) in [
        (InputErrorKind::Malformed, "UX_TERMINAL_INPUT_MALFORMED"),
        (InputErrorKind::Incomplete, "UX_TERMINAL_INPUT_INCOMPLETE"),
        (InputErrorKind::TooLong, "UX_TERMINAL_INPUT_TOO_LONG"),
    ] {
        let event = discarded_terminal_input_log(InputError {
            kind,
            protocol: "sgr-mouse",
            buffered_bytes: 12,
        });
        assert_eq!(event.level, runtime_log::LogLevel::Warning);
        assert_eq!(event.context.module, "ux.terminal.input");
        assert_eq!(event.error_code.as_deref(), Some(code));
        assert_eq!(event.alert_key.as_deref(), Some(code));
        assert!(event.message.contains("sgr-mouse"));
        assert!(event.message.contains("buffered_bytes=12"));
    }
}
