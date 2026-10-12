//! Validate JSON, then change whitespace only. Preserve key order, duplicate
//! keys, number spellings and escapes (round-tripping via Value loses these).
use serde::Deserialize;

pub fn format_json(source: &str, newline: &str) -> Result<String, serde_json::Error> {
    let mut parser = serde_json::Deserializer::from_str(source);
    serde::de::IgnoredAny::deserialize(&mut parser)?;
    parser.end()?;
    let mut output = String::new();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        if quoted {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            continue;
        }
        match ch {
            '"' => {
                quoted = true;
                output.push(ch);
            }
            '{' | '[' => {
                output.push(ch);
                while chars.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                    chars.next();
                }
                if chars.peek() == Some(&if ch == '{' { '}' } else { ']' }) {
                    output.push(chars.next().unwrap());
                } else {
                    depth += 1;
                    output.push_str(newline);
                    output.push_str(&"  ".repeat(depth));
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                output.push_str(newline);
                output.push_str(&"  ".repeat(depth));
                output.push(ch);
            }
            ',' => {
                output.push(',');
                output.push_str(newline);
                output.push_str(&"  ".repeat(depth));
            }
            ':' => output.push_str(": "),
            c if c.is_ascii_whitespace() => {}
            c => output.push(c),
        }
    }
    if source.ends_with(['\r', '\n']) {
        output.push_str(newline);
    }
    Ok(output)
}
