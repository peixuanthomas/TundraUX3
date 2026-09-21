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
