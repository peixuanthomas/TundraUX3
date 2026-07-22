use crate::{Breadcrumb, RecoveryOutcome, RuntimeSnapshot};
use serde_json::Value;

const MAX_TEXT_BYTES: usize = 4096;
const REDACTED: &str = "[redacted sensitive value]";
const SENSITIVE_TERMS: &[&str] = &[
    "password",
    "passwd",
    "token",
    "secret",
    "clipboard",
    "paste",
    "input",
];

pub(crate) fn text(value: impl AsRef<str>) -> String {
    let value = value.as_ref();
    let lower = value.to_ascii_lowercase();
    if SENSITIVE_TERMS.iter().any(|term| lower.contains(term)) {
        return REDACTED.to_string();
    }
    label(value)
}

fn label(value: &str) -> String {
    let mut output = String::with_capacity(value.len().min(MAX_TEXT_BYTES));
    for character in value.chars() {
        if output.len() >= MAX_TEXT_BYTES {
            break;
        }
        if character == '\n' || character == '\t' || !character.is_control() {
            output.push(character);
        }
    }
    if value.len() > MAX_TEXT_BYTES {
        output.push_str("…[truncated]");
    }
    output
}

pub(crate) fn json(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    let value = if SENSITIVE_TERMS.iter().any(|term| lower.contains(term)) {
                        Value::String(REDACTED.to_string())
                    } else {
                        json(value)
                    };
                    (label(key), value)
                })
                .collect(),
        ),
        Value::Array(array) => Value::Array(array.iter().take(256).map(json).collect()),
        Value::String(value) => Value::String(text(value)),
        other => other.clone(),
    }
}

pub(crate) fn breadcrumb(mut breadcrumb: Breadcrumb) -> Breadcrumb {
    breadcrumb.category = text(breadcrumb.category);
    breadcrumb.message = text(breadcrumb.message);
    breadcrumb
}

pub(crate) fn snapshot(mut snapshot: RuntimeSnapshot) -> RuntimeSnapshot {
    snapshot.screen = snapshot.screen.map(text);
    snapshot.last_command = snapshot.last_command.map(text);
    snapshot.active_operation = snapshot.active_operation.map(text);
    snapshot
}

pub(crate) fn recovery(outcome: RecoveryOutcome) -> RecoveryOutcome {
    match outcome {
        RecoveryOutcome::Pending => RecoveryOutcome::Pending,
        RecoveryOutcome::Recovered(detail) => RecoveryOutcome::Recovered(text(detail)),
        RecoveryOutcome::RecoveredWithWarnings(detail) => {
            RecoveryOutcome::RecoveredWithWarnings(text(detail))
        }
        RecoveryOutcome::ManualActionRequired(detail) => {
            RecoveryOutcome::ManualActionRequired(text(detail))
        }
        RecoveryOutcome::Unrecoverable(detail) => RecoveryOutcome::Unrecoverable(text(detail)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sensitive_text_and_json_fields_are_redacted() {
        assert_eq!(text("password=hunter2"), REDACTED);
        let value = json(&json!({
            "safe": "weather refresh",
            "clipboard_text": "private",
            "nested": { "token": "abc" }
        }));
        assert_eq!(value["safe"], "weather refresh");
        assert_eq!(value["clipboard_text"], REDACTED);
        assert_eq!(value["nested"]["token"], REDACTED);
    }
}
