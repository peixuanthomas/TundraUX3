use std::fs;
use std::path::{Path, PathBuf};

struct ForbiddenApi {
    description: &'static str,
    needles: &'static [&'static str],
}

const FORBIDDEN_APIS: &[ForbiddenApi] = &[
    ForbiddenApi {
        description: "installing a global panic hook",
        needles: &["panic::set_hook(", "usestd::panic::set_hook"],
    },
    ForbiddenApi {
        description: "taking ownership of the global panic hook",
        needles: &["panic::take_hook(", "usestd::panic::take_hook"],
    },
    ForbiddenApi {
        description: "spawning an unmanaged standard thread",
        needles: &[
            "thread::spawn(",
            "usestd::thread::spawn",
            "thread::Builder::new(",
            "usestd::thread::Builder",
        ],
    },
    ForbiddenApi {
        description: "spawning an unmanaged Tokio task",
        needles: &[
            "tokio::spawn(",
            "tokio::task::spawn(",
            "tokio::task::spawn_blocking(",
            "tokio::task::spawn_local(",
            "usetokio::spawn",
            "usetokio::{spawn",
            "usetokio::task::spawn",
            "usetokio::task::{spawn",
        ],
    },
];

#[test]
fn production_code_uses_the_workspace_watchdog_for_hooks_and_tasks() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root should resolve");
    let crates = workspace.join("crates");
    let mut rust_sources = Vec::new();

    for entry in fs::read_dir(&crates).expect("workspace crates directory should be readable") {
        let entry = entry.expect("workspace crate entry should be readable");
        let crate_path = entry.path();
        if !crate_path.is_dir()
            || entry.file_name() == "watchdog"
            || !crate_path.join("Cargo.toml").is_file()
        {
            continue;
        }
        collect_production_rust_sources(&crate_path.join("src"), &mut rust_sources);
    }

    let mut violations = Vec::new();
    for path in rust_sources {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let code = rust_code_without_comments_or_strings(&source);
        let compact = rust_code_without_cfg_test_items(&code)
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();

        for forbidden in FORBIDDEN_APIS {
            if forbidden
                .needles
                .iter()
                .any(|needle| compact.contains(needle))
            {
                let relative = path.strip_prefix(&workspace).unwrap_or(&path);
                violations.push(format!(
                    "{}: {} is only allowed in watchdog",
                    relative.display(),
                    forbidden.description
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "workspace watchdog constraints were violated:\n{}",
        violations.join("\n")
    );
}

fn collect_production_rust_sources(root: &Path, sources: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries {
        let entry = entry.expect("source directory entry should be readable");
        let path = entry.path();
        if path.is_dir() {
            if !is_test_source(&path) {
                collect_production_rust_sources(&path, sources);
            }
        } else if path.extension().and_then(|value| value.to_str()) == Some("rs")
            && !is_test_source(&path)
        {
            sources.push(path);
        }
    }
}

fn is_test_source(path: &Path) -> bool {
    if path.components().any(|component| {
        matches!(
            component.as_os_str().to_str(),
            Some("test" | "tests" | "test_support")
        )
    }) {
        return true;
    }

    let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };
    stem == "tests"
        || stem.ends_with("_test")
        || stem.ends_with("_tests")
        || stem.contains("test_support")
}

/// Removes comments and string contents before API matching. Keeping other
/// tokens intact lets the test normalize whitespace without false positives
/// from documentation, diagnostics, or fixture strings.
fn rust_code_without_comments_or_strings(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut index = 0usize;

    while index < bytes.len() {
        if bytes[index..].starts_with(b"//") {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            output.push('\n');
            index = index.saturating_add(1);
            continue;
        }
        if bytes[index..].starts_with(b"/*") {
            index += 2;
            let mut depth = 1usize;
            while index < bytes.len() && depth > 0 {
                if bytes[index..].starts_with(b"/*") {
                    depth += 1;
                    index += 2;
                } else if bytes[index..].starts_with(b"*/") {
                    depth -= 1;
                    index += 2;
                } else {
                    if bytes[index] == b'\n' {
                        output.push('\n');
                    }
                    index += 1;
                }
            }
            continue;
        }

        if let Some((content_start, hashes)) = raw_string_start(bytes, index) {
            index = content_start;
            let mut terminator = Vec::with_capacity(hashes + 1);
            terminator.push(b'"');
            terminator.extend(std::iter::repeat_n(b'#', hashes));
            while index < bytes.len() && !bytes[index..].starts_with(&terminator) {
                if bytes[index] == b'\n' {
                    output.push('\n');
                }
                index += 1;
            }
            index = (index + terminator.len()).min(bytes.len());
            continue;
        }

        let string_prefix = if bytes[index] == b'"' {
            Some(1)
        } else if bytes[index..].starts_with(b"b\"") {
            Some(2)
        } else {
            None
        };
        if let Some(prefix_len) = string_prefix {
            index += prefix_len;
            let mut escaped = false;
            while index < bytes.len() {
                let byte = bytes[index];
                index += 1;
                if byte == b'\n' {
                    output.push('\n');
                }
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    break;
                }
            }
            continue;
        }

        output.push(bytes[index] as char);
        index += 1;
    }

    output
}

fn raw_string_start(bytes: &[u8], index: usize) -> Option<(usize, usize)> {
    let mut cursor = if bytes.get(index) == Some(&b'r') {
        index + 1
    } else if bytes.get(index) == Some(&b'b') && bytes.get(index + 1) == Some(&b'r') {
        index + 2
    } else {
        return None;
    };
    let hash_start = cursor;
    while bytes.get(cursor) == Some(&b'#') {
        cursor += 1;
    }
    (bytes.get(cursor) == Some(&b'"')).then_some((cursor + 1, cursor - hash_start))
}

/// Blanks items guarded by an exact `cfg(test)` attribute. This covers both
/// inline test modules and test-only helper functions that intentionally use
/// low-level APIs for fault injection.
fn rust_code_without_cfg_test_items(source: &str) -> String {
    let mut output = source.as_bytes().to_vec();
    let mut cursor = 0usize;

    while cursor < output.len() {
        let Some((attribute_start, attribute_end)) = next_cfg_test_attribute(&output, cursor)
        else {
            break;
        };
        let mut item_cursor = attribute_end;
        while output.get(item_cursor).is_some_and(u8::is_ascii_whitespace) {
            item_cursor += 1;
        }
        // Skip additional attributes attached to the same item.
        while output.get(item_cursor) == Some(&b'#') {
            let Some(end) = attribute_end_at(&output, item_cursor) else {
                break;
            };
            item_cursor = end;
            while output.get(item_cursor).is_some_and(u8::is_ascii_whitespace) {
                item_cursor += 1;
            }
        }

        let mut delimiter = item_cursor;
        while delimiter < output.len() && !matches!(output[delimiter], b'{' | b';') {
            delimiter += 1;
        }
        let item_end = if output.get(delimiter) == Some(&b'{') {
            matching_brace_end(&output, delimiter).unwrap_or(output.len())
        } else {
            delimiter.saturating_add(1).min(output.len())
        };
        for byte in &mut output[attribute_start..item_end] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        cursor = item_end;
    }

    String::from_utf8(output).expect("source was valid UTF-8 before test item filtering")
}

fn next_cfg_test_attribute(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut cursor = from;
    while cursor < bytes.len() {
        if bytes[cursor] != b'#' {
            cursor += 1;
            continue;
        }
        let Some(end) = attribute_end_at(bytes, cursor) else {
            cursor += 1;
            continue;
        };
        let open = bytes[cursor..end].iter().position(|byte| *byte == b'[')? + cursor;
        let content = bytes[open + 1..end - 1]
            .iter()
            .copied()
            .filter(|byte| !byte.is_ascii_whitespace())
            .collect::<Vec<_>>();
        if content == b"cfg(test)" {
            return Some((cursor, end));
        }
        cursor = end;
    }
    None
}

fn attribute_end_at(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'#') {
        return None;
    }
    let mut cursor = start + 1;
    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
        cursor += 1;
    }
    if bytes.get(cursor) != Some(&b'[') {
        return None;
    }
    let mut depth = 1usize;
    cursor += 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(cursor + 1);
                }
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn matching_brace_end(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, byte) in bytes[open..].iter().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn constraint_scanner_ignores_comments_and_string_literals() {
    let fixture = r##"
        // tokio::spawn(async {});
        const MESSAGE: &str = "std::panic::set_hook";
        const RAW: &str = r#"thread::Builder::new().spawn()"#;
        fn allowed() { watchdog.spawn_thread(); }
    "##;

    let code = rust_code_without_comments_or_strings(fixture);
    let compact = rust_code_without_cfg_test_items(&code)
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    assert!(!compact.contains("tokio::spawn"));
    assert!(!compact.contains("panic::set_hook"));
    assert!(!compact.contains("thread::Builder"));
    assert!(compact.contains("watchdog.spawn_thread()"));
}

#[test]
fn constraint_scanner_ignores_cfg_test_items() {
    let fixture = r#"
        #[cfg(test)]
        mod tests {
            fn inject_panic() { std::thread::spawn(|| panic!()); }
        }

        #[cfg(test)]
        fn test_helper() { tokio::spawn(async {}); }

        fn production() { watchdog.spawn_thread(); }
    "#;
    let code = rust_code_without_comments_or_strings(fixture);
    let compact = rust_code_without_cfg_test_items(&code)
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();

    assert!(!compact.contains("thread::spawn"));
    assert!(!compact.contains("tokio::spawn"));
    assert!(compact.contains("watchdog.spawn_thread()"));
}
