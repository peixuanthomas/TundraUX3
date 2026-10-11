//! Read a bounded tail of an explicitly selected text log, without elevation.
use runtime_log::{
    LogContext, LogFileStatus, LogLevel, LogPhase, LogQuery, LogQueryResult, LogSourceState,
    RuntimeLogEvent,
};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};

pub fn query_log_file(query: &LogQuery, cancelled: &AtomicBool) -> LogQueryResult {
    let mut result = LogQueryResult::default();
    let Some(path) = &query.file_path else {
        return result;
    };
    if cancelled.load(Ordering::Relaxed) {
        result.state = LogSourceState::Cancelled;
        return result;
    }
    if !path.is_absolute() {
        result.state = LogSourceState::Unavailable;
        result
            .notices
            .push("Choose an absolute log file path.".into());
        return result;
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
        Ok(_) => {
            result.state = LogSourceState::Unavailable;
            result
                .notices
                .push("The log is not a regular file. Choose a text log file.".into());
            return result;
        }
        Err(error) => {
            failure(&mut result, error);
            return result;
        }
    };
    let mut file = match open_file(path) {
        Ok(file) => file,
        Err(error) => {
            failure(&mut result, error);
            return result;
        }
    };
    let opened = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) => {
            failure(&mut result, error);
            return result;
        }
    };
    if !opened.is_file() || identity(&metadata) != identity(&opened) {
        result.state = LogSourceState::Unavailable;
        result
            .notices
            .push("The log file was replaced. Refresh to read the new file.".into());
        return result;
    }
    let id = identity(&opened);
    result.file_status = Some(LogFileStatus {
        identity: id.clone(),
        length: opened.len(),
    });
    const MAX_BYTES: u64 = 8 * 1024 * 1024;
    let start = opened.len().saturating_sub(MAX_BYTES);
    if let Err(error) = file.seek(SeekFrom::Start(start)) {
        failure(&mut result, error);
        return result;
    }
    let mut bytes = Vec::new();
    if let Err(error) = file.take(MAX_BYTES).read_to_end(&mut bytes) {
        failure(&mut result, error);
        return result;
    }
    let first = if start > 0 {
        bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(bytes.len())
    } else {
        0
    };
    let time: chrono::DateTime<chrono::Utc> = opened
        .modified()
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        .into();
    let mut offset = start + first as u64;
    let limit = query.limit.min(10_000);
    let mut lines = std::collections::VecDeque::with_capacity(limit);
    for line in bytes[first..].split_inclusive(|byte| *byte == b'\n') {
        if cancelled.load(Ordering::Relaxed) {
            result.state = LogSourceState::Cancelled;
            result.events.clear();
            return result;
        }
        // Keep byte spans until the tail is selected. Most scanned records
        // will never be returned and need no owned text or event metadata.
        let decoded = String::from_utf8_lossy(line);
        let text = decoded.trim_end_matches(['\r', '\n']);
        if line.len() > 8192 {
            result.truncated = true;
        }
        if text.chars().take(8192).any(|ch| ch == '\u{fffd}') {
            result.damaged_records += 1;
        }
        if !text.is_empty() {
            if lines.len() == limit {
                lines.pop_front();
                result.truncated = true;
            }
            if limit != 0 {
                lines.push_back((offset, line));
            }
        }
        offset += line.len() as u64;
    }
    result.events.reserve(lines.len());
    for (offset, line) in lines.into_iter().rev() {
        if cancelled.load(Ordering::Relaxed) {
            result.state = LogSourceState::Cancelled;
            result.events.clear();
            return result;
        }
        let text = String::from_utf8_lossy(line)
            .trim_end_matches(['\r', '\n'])
            .chars()
            .take(8192)
            .collect::<String>();
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        line.hash(&mut hash);
        let mut event = RuntimeLogEvent::new(
            LogContext {
                app: "log-file".into(),
                module: "file".into(),
                operation: "read".into(),
                ..Default::default()
            },
            LogLevel::Info,
            LogPhase::Observed,
            runtime_log::sanitize_text(&text),
        );
        event.event_id = format!("file:{id}:{offset}:{:x}", hash.finish());
        event.source = query.source;
        event.timestamp = time;
        event.source_path = Some(path.clone());
        event.native_source = Some(format!("file; offset={offset}"));
        event.timestamp_note = Some(
            "File modification time; individual event time and severity were not parsed".into(),
        );
        result.events.push(event);
    }
    result.truncated = result.truncated || start > 0;
    if query.since.is_some()
        || query.until.is_some()
        || query.min_level.is_some()
        || query.module.is_some()
    {
        result.notices.push("Raw file records have no parsed event time or level. Clear event filters to inspect the file.".into());
        result.events.clear();
        result.state = LogSourceState::Partial;
    }
    if result.truncated {
        result.notices.push(
            "Showing a bounded tail of the file. Earlier records remain in the log file.".into(),
        );
    }
    result
}
fn failure(result: &mut LogQueryResult, error: std::io::Error) {
    result.state = if error.kind() == std::io::ErrorKind::PermissionDenied {
        LogSourceState::PermissionDenied
    } else {
        LogSourceState::Unavailable
    };
    result.notices.push(
        match error.kind() {
            std::io::ErrorKind::PermissionDenied => {
                "Log access denied. Check the file permissions."
            }
            std::io::ErrorKind::NotFound => {
                "The log file is missing. Check the path or wait for rotation to finish."
            }
            _ => "The log file could not be read. Check the file and refresh.",
        }
        .into(),
    );
}
fn open_file(path: &std::path::Path) -> std::io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
    }
    #[cfg(not(unix))]
    {
        File::open(path)
    }
}
fn identity(metadata: &fs::Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}", metadata.dev(), metadata.ino())
    }
    #[cfg(not(unix))]
    {
        format!(
            "{:?}",
            metadata
                .created()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tail_keeps_offsets_and_counts_damage_even_in_discarded_records() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tail.log");
        let bytes = b"bad\xff\n\r\nold\nlast\r\n";
        fs::write(&path, bytes).unwrap();
        for limit in [0, 1, 2, 200] {
            let query = LogQuery {
                file_path: Some(path.clone()),
                limit,
                ..Default::default()
            };
            let result = query_log_file(&query, &AtomicBool::new(false));
            assert_eq!(result.damaged_records, 1);
            assert_eq!(result.events.len(), limit.min(3));
            assert_eq!(result.truncated, limit < 3);
            if limit > 0 {
                assert_eq!(result.events[0].message, "last");
                assert_eq!(
                    result.events[0].native_source.as_deref(),
                    Some("file; offset=11")
                );
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                bytes[11..].hash(&mut hash);
                assert!(
                    result.events[0]
                        .event_id
                        .ends_with(&format!(":11:{:x}", hash.finish()))
                );
            }
        }
    }

    #[test]
    fn damage_and_text_limits_count_characters_not_utf8_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("unicode.log");
        let mut bytes = "中".repeat(8192).into_bytes();
        bytes.extend_from_slice(b"\xff\n");
        bytes.extend_from_slice("replacement: \u{fffd}\n".as_bytes());
        fs::write(&path, bytes).unwrap();
        let result = query_log_file(
            &LogQuery {
                file_path: Some(path),
                limit: 2,
                ..Default::default()
            },
            &AtomicBool::new(false),
        );
        assert_eq!(result.damaged_records, 1);
        assert!(result.truncated);
        assert_eq!(result.events.len(), 2);
        assert!(!result.events[1].message.contains('\u{fffd}'));
    }

    #[test]
    fn file_tail_is_bounded_and_detects_rotation_and_truncation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("example.log");
        fs::write(&path, "one\ntwo\nthree\n").unwrap();
        let query = LogQuery {
            file_path: Some(path.clone()),
            limit: 2,
            ..Default::default()
        };
        let first = query_log_file(&query, &AtomicBool::new(false));
        assert_eq!(
            first
                .events
                .iter()
                .map(|event| event.message.as_str())
                .collect::<Vec<_>>(),
            ["three", "two"]
        );
        assert!(first.truncated);
        fs::write(&path, "new\n").unwrap();
        let shorter = query_log_file(&query, &AtomicBool::new(false));
        assert!(
            shorter.file_status.as_ref().unwrap().length
                < first.file_status.as_ref().unwrap().length
        );
        fs::rename(&path, directory.path().join("rotated.log")).unwrap();
        fs::write(&path, "replacement\n").unwrap();
        let rotated = query_log_file(&query, &AtomicBool::new(false));
        assert_ne!(shorter.events[0].event_id, rotated.events[0].event_id);
        #[cfg(unix)]
        assert_ne!(
            first.file_status.unwrap().identity,
            rotated.file_status.unwrap().identity
        );
        fs::remove_file(&path).unwrap();
        assert_eq!(
            query_log_file(&query, &AtomicBool::new(false)).state,
            LogSourceState::Unavailable
        );
    }
}
