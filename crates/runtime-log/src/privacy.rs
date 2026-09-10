use crate::RuntimeLogEvent;
use std::path::PathBuf;

pub(crate) const MAX_TEXT_BYTES: usize = 2048;
pub(crate) const MAX_RECORD_BYTES: usize = 64 * 1024;

fn bounded(text: &str, limit: usize) -> &str {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Defense in depth for metadata. Arbitrary opaque secrets cannot be identified
/// reliably: producers must obey the metadata-only construction contract.
pub fn sanitize_text(text: &str) -> String {
    let text = bounded(text, MAX_TEXT_BYTES);
    let clean: String = text
        .chars()
        .filter(|c| !c.is_control() && *c != '\u{7f}')
        .collect();
    let lower = clean.to_ascii_lowercase();
    let markers = [
        "authorization",
        "proxy-authorization",
        "password",
        "passwd",
        "secret",
        "access_token",
        "refresh_token",
        "id_token",
        "api_key",
        "api-key",
        "apikey",
        "token",
        "cookie",
        "clipboard",
        "file_body",
        "file body",
        "file_content",
        "file content",
        "file contents",
        "command_input",
        "command input",
        "credential",
        "api key",
        "private_key",
        "private-key",
        "private key",
        "bearer ",
    ];
    // Redact the remainder of a secret-bearing assignment/header. Keeping the
    // prefix preserves the failure category without retaining ambiguous values.
    let mut cutoff = clean.len();
    for marker in markers {
        for (index, _) in lower.match_indices(marker) {
            let before = lower[..index].chars().next_back();
            if before.is_some_and(|c| c.is_ascii_alphanumeric()) {
                continue;
            }
            let end = index + marker.len();
            let after = lower[end..].chars().next();
            if marker == "bearer " || after.is_none_or(|c| !c.is_ascii_alphanumeric()) {
                cutoff = cutoff.min(index);
                break;
            }
        }
    }
    let mut result = if cutoff < clean.len() {
        format!("{}[REDACTED]", &clean[..cutoff])
    } else {
        clean
    };
    // Remove URL userinfo even when no recognizable secret key was supplied.
    let mut search = 0;
    while let Some(offset) = result[search..].find("://") {
        let start = search + offset + 3;
        let end = result[start..]
            .find(['/', ' ', '?', '#'])
            .map_or(result.len(), |p| start + p);
        if let Some(at) = result[start..end].rfind('@') {
            let finish = start + at + 1;
            result.replace_range(start..finish, "[REDACTED]@");
            search = start + "[REDACTED]@".len();
        } else {
            search = end;
        }
        if search >= result.len() {
            break;
        }
    }
    result
}
fn opt(value: &mut Option<String>) {
    if let Some(text) = value {
        *text = sanitize_text(text);
    }
}
fn path(value: &mut Option<PathBuf>) {
    if let Some(p) = value {
        *p = PathBuf::from(sanitize_text(&p.to_string_lossy()));
    }
}
/// Bound and sanitize all externally supplied string fields before persistence.
pub fn sanitize_event(event: &mut RuntimeLogEvent) {
    event.event_id = sanitize_text(&event.event_id);
    event.message = sanitize_text(&event.message);
    opt(&mut event.context.run_id);
    event.context.app = sanitize_text(&event.context.app);
    event.context.module = sanitize_text(&event.context.module);
    event.context.operation = sanitize_text(&event.context.operation);
    opt(&mut event.context.operation_id);
    opt(&mut event.context.task_id);
    opt(&mut event.context.owner_id);
    opt(&mut event.error_code);
    opt(&mut event.incident_id);
    opt(&mut event.alert_key);
    opt(&mut event.native_source);
    opt(&mut event.timestamp_note);
    if event.error_chain.len() > 8 {
        let root = event.error_chain.pop().unwrap_or_default();
        let omitted = event.error_chain.len() - 6;
        event.error_chain.truncate(6);
        event
            .error_chain
            .push(format!("[{omitted} intermediate causes omitted]"));
        event.error_chain.push(root);
    }
    for cause in &mut event.error_chain {
        *cause = sanitize_text(cause);
    }
    path(&mut event.source_path);
    path(&mut event.target_path);
}
