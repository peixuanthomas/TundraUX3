use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

fn unique_temp_dir() -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "tundra-platform-document-{}-{}-{}",
        process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT_TEMP_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).expect("create document test directory");
    fs::canonicalize(directory).expect("canonicalize document test directory")
}

#[test]
fn atomically_round_trips_document_and_fingerprint() {
    let directory = unique_temp_dir();
    let path = directory.join("note.md");
    let fingerprint = atomic_write_document(&path, b"# Tundra\n").unwrap();
    let loaded = read_document_bytes(&path).unwrap();
    assert_eq!(loaded.bytes, b"# Tundra\n");
    assert_eq!(loaded.fingerprint, fingerprint);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn limited_read_accepts_a_document_at_the_limit() {
    let directory = unique_temp_dir();
    let path = directory.join("bounded.log");
    let contents = b"exactly eight bytes";
    fs::write(&path, contents).unwrap();

    let loaded = read_document_bytes_limited(&path, contents.len() as u64).unwrap();

    assert_eq!(loaded.bytes, contents);
    assert_eq!(loaded.fingerprint.len, contents.len() as u64);
    assert_eq!(loaded.fingerprint, document_fingerprint(&path).unwrap());
    fs::remove_dir_all(directory).unwrap();
}

fn assert_monotonic_progress(progress: &[(u64, u64)], total: u64) {
    assert_eq!(progress.first(), Some(&(0, total)));
    assert_eq!(progress.last(), Some(&(total, total)));
    assert!(
        progress
            .iter()
            .all(|(_, observed_total)| *observed_total == total)
    );
    for pair in progress.windows(2) {
        assert!(pair[0].0 <= pair[1].0);
        assert!(pair[1].0 - pair[0].0 <= DOCUMENT_READ_CHUNK_BYTES as u64);
    }
}

#[test]
fn strict_limited_read_reports_monotonic_progress_and_supports_cancellation() {
    let directory = unique_temp_dir();
    let path = directory.join("strict-progress.log");
    let contents = vec![b'x'; DOCUMENT_READ_CHUNK_BYTES * 2 + 17];
    fs::write(&path, &contents).unwrap();
    let total = contents.len() as u64;
    let mut progress = Vec::new();

    let loaded = read_document_bytes_limited_with_progress(&path, total, |completed, total| {
        progress.push((completed, total));
        true
    })
    .unwrap();

    assert_eq!(loaded.bytes, contents);
    assert_monotonic_progress(&progress, total);

    let error =
        read_document_bytes_limited_with_progress(&path, total, |completed, _| completed == 0)
            .expect_err("returning false after the first chunk must cancel the read");
    assert!(matches!(
        &error,
        PlatformError::Interrupted {
            operation: "read document",
            path: interrupted_path,
            ..
        } if interrupted_path.as_deref() == Some(path.as_path())
    ));
    assert!(error.to_string().contains("cancelled after 65536"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn prefix_snapshot_reports_monotonic_progress_and_supports_cancellation() {
    let directory = unique_temp_dir();
    let path = directory.join("prefix-progress.log");
    let contents = vec![b'y'; DOCUMENT_READ_CHUNK_BYTES * 2 + 31];
    fs::write(&path, &contents).unwrap();
    let total = contents.len() as u64;
    let mut progress = Vec::new();

    let loaded =
        read_document_prefix_snapshot_limited_with_progress(&path, total, |completed, total| {
            progress.push((completed, total));
            true
        })
        .unwrap();

    assert_eq!(loaded.bytes, contents);
    assert_monotonic_progress(&progress, total);

    let error =
        read_document_prefix_snapshot_limited_with_progress(&path, total, |completed, _| {
            completed == 0
        })
        .expect_err("returning false after the first prefix chunk must cancel the read");
    assert!(matches!(error, PlatformError::Interrupted { .. }));
    assert!(error.to_string().contains("cancelled after 65536"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn limited_read_rejects_preflight_size_and_legacy_read_remains_compatible() {
    let directory = unique_temp_dir();
    let path = directory.join("bounded.log");
    let contents = b"larger than the configured test limit";
    fs::write(&path, contents).unwrap();

    let error = read_document_bytes_limited(&path, 8).expect_err("limit must be enforced");

    assert!(matches!(error, PlatformError::InvalidInput { .. }));
    assert!(error.to_string().contains(&contents.len().to_string()));
    assert!(error.to_string().contains("8 byte limit"));
    assert_eq!(read_document_bytes(&path).unwrap().bytes, contents);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn limited_read_rejects_a_sparse_file_before_reading_contents() {
    let directory = unique_temp_dir();
    let path = directory.join("sparse.log");
    File::create(&path).unwrap().set_len(4096).unwrap();

    let error = read_document_bytes_limited(&path, 32).expect_err("sparse size must count");

    assert!(matches!(error, PlatformError::InvalidInput { .. }));
    assert!(error.to_string().contains("4096 bytes"));
    assert!(error.to_string().contains("32 byte limit"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn limited_read_rejects_growth_beyond_the_preflight_limit() {
    let directory = unique_temp_dir();
    let path = directory.join("growing.log");
    fs::write(&path, b"start").unwrap();
    let file = File::open(&path).unwrap();
    let preflight = file.metadata().unwrap();
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b" grew")
        .unwrap();

    let error = read_open_document_snapshot(
        &path,
        file,
        preflight,
        5,
        true,
        DocumentReadConsistency::Stable,
    )
    .expect_err("growth after preflight must exceed the hard limit");

    assert!(matches!(error, PlatformError::InvalidInput { .. }));
    assert!(error.to_string().contains("10 bytes"));
    assert!(error.to_string().contains("5 byte limit"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn limited_read_rejects_truncation_after_preflight() {
    let directory = unique_temp_dir();
    let path = directory.join("shrinking.log");
    fs::write(&path, b"original").unwrap();
    let file = File::open(&path).unwrap();
    let preflight = file.metadata().unwrap();
    fs::write(&path, b"cut").unwrap();

    let error = read_open_document_snapshot(
        &path,
        file,
        preflight,
        64,
        true,
        DocumentReadConsistency::Stable,
    )
    .expect_err("truncation after preflight must be unstable");

    assert!(matches!(
        error,
        PlatformError::Io {
            operation: "read stable document",
            ..
        }
    ));
    assert!(error.to_string().contains("started at 8 bytes"));
    assert!(error.to_string().contains("ended at 3 bytes"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn prefix_snapshot_allows_append_beyond_the_limit_and_excludes_new_bytes() {
    let directory = unique_temp_dir();
    let path = directory.join("active.log");
    fs::write(&path, b"start").unwrap();
    let file = File::open(&path).unwrap();
    let preflight = file.metadata().unwrap();
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b" appended past the limit")
        .unwrap();

    let (bytes, fingerprint) = read_open_document_snapshot(
        &path,
        file,
        preflight,
        5,
        true,
        DocumentReadConsistency::AppendablePrefix,
    )
    .expect("append-only growth must not invalidate the opened prefix");

    assert_eq!(bytes.unwrap(), b"start");
    assert_eq!(fingerprint.len, 5);
    assert!(fs::metadata(&path).unwrap().len() > 5);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn prefix_snapshot_rejects_truncation_after_preflight() {
    let directory = unique_temp_dir();
    let path = directory.join("active.log");
    fs::write(&path, b"original").unwrap();
    let file = File::open(&path).unwrap();
    let preflight = file.metadata().unwrap();
    fs::write(&path, b"cut").unwrap();

    let error = read_open_document_snapshot(
        &path,
        file,
        preflight,
        64,
        true,
        DocumentReadConsistency::AppendablePrefix,
    )
    .expect_err("truncating an opened prefix must fail");

    assert!(matches!(
        error,
        PlatformError::Io {
            operation: "read stable document",
            ..
        }
    ));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn prefix_snapshot_rejects_path_replacement() {
    let directory = unique_temp_dir();
    let path = directory.join("active.log");
    let moved = directory.join("rotated.log");
    fs::write(&path, b"opened").unwrap();
    let file = File::open(&path).unwrap();
    let preflight = file.metadata().unwrap();
    fs::rename(&path, &moved).unwrap();
    fs::write(&path, b"newlog").unwrap();

    let error = read_open_document_snapshot(
        &path,
        file,
        preflight,
        64,
        true,
        DocumentReadConsistency::AppendablePrefix,
    )
    .expect_err("replacing the opened path must fail");

    assert!(matches!(
        error,
        PlatformError::Io {
            operation: "read document snapshot",
            ..
        }
    ));
    assert!(error.to_string().contains("path was replaced"));
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn limited_read_rechecks_the_no_follow_policy_after_preflight() {
    use std::os::unix::fs::symlink;

    let directory = unique_temp_dir();
    let path = directory.join("opened.log");
    let replacement = directory.join("replacement.log");
    fs::write(&path, b"opened").unwrap();
    fs::write(&replacement, b"replacement").unwrap();
    let file = File::open(&path).unwrap();
    let preflight = file.metadata().unwrap();
    fs::remove_file(&path).unwrap();
    symlink(&replacement, &path).unwrap();

    let error = read_open_document_snapshot(
        &path,
        file,
        preflight,
        64,
        true,
        DocumentReadConsistency::Stable,
    )
    .expect_err("a link introduced after preflight must be rejected");

    assert!(matches!(error, PlatformError::InvalidInput { .. }));
    assert!(error.to_string().contains("symbolic links"));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn tail_read_returns_complete_small_document() {
    let directory = unique_temp_dir();
    let path = directory.join("report.txt");
    let contents = b"first line\nsecond line\n";
    fs::write(&path, contents).unwrap();

    let tail = read_document_tail_bytes(&path, 4096).unwrap();

    assert_eq!(tail.bytes, contents);
    assert_eq!(tail.start_byte, 0);
    assert_eq!(tail.total_bytes, contents.len() as u64);
    assert!(!tail.truncated);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn tail_read_drops_a_partial_first_line() {
    let directory = unique_temp_dir();
    let path = directory.join("report.txt");
    let contents = b"first line\nsecond line\nthird line\n";
    fs::write(&path, contents).unwrap();

    let tail = read_document_tail_bytes(&path, 18).unwrap();

    assert_eq!(tail.bytes, b"third line\n");
    assert_eq!(tail.start_byte, "first line\nsecond line\n".len() as u64);
    assert_eq!(tail.total_bytes, contents.len() as u64);
    assert!(tail.truncated);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn tail_read_preserves_a_window_that_starts_on_a_line_boundary() {
    let directory = unique_temp_dir();
    let path = directory.join("report.txt");
    fs::write(&path, b"head\nkept\n").unwrap();

    let tail = read_document_tail_bytes(&path, b"kept\n".len()).unwrap();

    assert_eq!(tail.bytes, b"kept\n");
    assert_eq!(tail.start_byte, b"head\n".len() as u64);
    assert!(tail.truncated);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn tail_read_aligns_utf8_before_dropping_the_partial_line() {
    let directory = unique_temp_dir();
    let path = directory.join("report.txt");
    let contents = "header\n😀 partial line\nfinal line\n";
    fs::write(&path, contents).unwrap();
    let final_start = contents.find("final line").unwrap() as u64;

    let tail = read_document_tail_bytes(&path, 23).unwrap();

    assert_eq!(tail.bytes, b"final line\n");
    assert_eq!(tail.start_byte, final_start);
    assert!(std::str::from_utf8(&tail.bytes).is_ok());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn tail_read_rejects_invalid_utf8() {
    let directory = unique_temp_dir();
    let path = directory.join("report.txt");
    fs::write(&path, [b'o', b'k', b'\n', 0xff]).unwrap();

    let error = read_document_tail_bytes(&path, 16).expect_err("invalid UTF-8 must fail");

    assert!(matches!(error, PlatformError::InvalidInput { .. }));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn tail_read_rejects_an_invalid_continuation_at_the_window_start() {
    let directory = unique_temp_dir();
    let path = directory.join("report.txt");
    fs::write(&path, [b'a', 0x80, b'b']).unwrap();

    let error = read_document_tail_bytes(&path, 2).expect_err("invalid UTF-8 must fail");

    assert!(matches!(error, PlatformError::InvalidInput { .. }));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn tail_read_keeps_the_suffix_of_a_single_oversized_line() {
    let directory = unique_temp_dir();
    let path = directory.join("single-line.log");
    fs::write(&path, b"0123456789").unwrap();

    let tail = read_document_tail_bytes(&path, 5).unwrap();

    assert_eq!(tail.bytes, b"56789");
    assert_eq!(tail.start_byte, 5);
    assert!(tail.truncated);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn tail_read_recognizes_cr_only_line_endings() {
    let directory = unique_temp_dir();
    let path = directory.join("cr.log");
    let contents = b"first\rsecond\rthird\r";
    fs::write(&path, contents).unwrap();

    let tail = read_document_tail_bytes(&path, 10).unwrap();

    assert_eq!(tail.bytes, b"third\r");
    assert_eq!(tail.start_byte, b"first\rsecond\r".len() as u64);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn conditional_write_refuses_to_overwrite_an_external_edit() {
    let directory = unique_temp_dir();
    let path = directory.join("note.md");
    let expected = atomic_write_document(&path, b"opened\n").unwrap();
    fs::write(&path, b"external edit\n").unwrap();
    let actual = document_fingerprint(&path).unwrap();

    let error = atomic_write_document_if_unchanged(&path, b"editor edit\n", Some(expected))
        .expect_err("an external edit must win until the user resolves the conflict");
    assert_eq!(
        error,
        DocumentWriteError::ExternalModification {
            path: path.clone(),
            expected: Some(expected),
            actual: Some(actual),
        }
    );
    assert_eq!(fs::read(&path).unwrap(), b"external edit\n");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn conditional_new_write_never_clobbers_an_existing_target() {
    let directory = unique_temp_dir();
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("save-as.md");
    fs::write(&path, b"created elsewhere\n").unwrap();

    let error = atomic_write_document_if_unchanged(&path, b"editor edit\n", None)
        .expect_err("Save As must not overwrite a target that appeared concurrently");
    assert!(matches!(
        error,
        DocumentWriteError::ExternalModification {
            expected: None,
            actual: Some(_),
            ..
        }
    ));
    assert_eq!(fs::read(&path).unwrap(), b"created elsewhere\n");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn conditional_write_succeeds_with_the_opened_fingerprint() {
    let directory = unique_temp_dir();
    let path = directory.join("note.md");
    let expected = atomic_write_document(&path, b"opened\n").unwrap();
    let saved =
        atomic_write_document_if_unchanged(&path, b"editor edit\n", Some(expected)).unwrap();

    let loaded = read_document_bytes(&path).unwrap();
    assert_eq!(loaded.bytes, b"editor edit\n");
    assert_eq!(loaded.fingerprint, saved);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn conditional_streaming_write_succeeds_and_returns_the_written_fingerprint() {
    let directory = unique_temp_dir();
    let path = directory.join("streamed.md");
    let expected = atomic_write_document(&path, b"opened\n").unwrap();

    let saved = atomic_write_document_if_unchanged_with(&path, Some(expected), |writer| {
        writer.write_all(b"first chunk\n")?;
        writer.write_all(b"second chunk\n")
    })
    .unwrap();

    assert_eq!(fs::read(&path).unwrap(), b"first chunk\nsecond chunk\n");
    assert_eq!(saved, document_fingerprint(&path).unwrap());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn conditional_streaming_write_detects_a_conflict_after_streaming() {
    let directory = unique_temp_dir();
    let path = directory.join("streamed.md");
    let expected = atomic_write_document(&path, b"opened\n").unwrap();

    let error = atomic_write_document_if_unchanged_with(&path, Some(expected), |writer| {
        writer.write_all(b"editor edit\n")?;
        fs::write(&path, b"external edit\n")
    })
    .expect_err("the second expectation check must detect the external edit");

    assert!(matches!(
        error,
        DocumentWriteError::ExternalModification {
            expected: Some(actual_expected),
            actual: Some(_),
            ..
        } if actual_expected == expected
    ));
    assert_eq!(fs::read(&path).unwrap(), b"external edit\n");
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn streaming_write_error_keeps_the_target_and_cleans_up_the_temporary_file() {
    let directory = unique_temp_dir();
    let path = directory.join("streamed.md");
    fs::write(&path, b"original\n").unwrap();

    let error = atomic_write_document_with(&path, |writer| -> io::Result<()> {
        writer.write_all(b"partial replacement\n")?;
        Err(io::Error::other("injected stream failure"))
    })
    .expect_err("stream failure must abort the atomic write");

    assert!(matches!(
        error,
        PlatformError::DetailedIo {
            operation: "write temporary document",
            ..
        }
    ));
    assert_eq!(fs::read(&path).unwrap(), b"original\n");
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn replacement_preserves_existing_unix_permissions() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let directory = unique_temp_dir();
    let path = directory.join("private.md");
    let expected = atomic_write_document(&path, b"private\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let expected = document_fingerprint(&path).unwrap_or(expected);

    atomic_write_document_if_unchanged(&path, b"still private\n", Some(expected)).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o640);
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(unix)]
#[test]
fn rejects_symbolic_link_targets() {
    use std::os::unix::fs::symlink;

    let directory = unique_temp_dir();
    fs::create_dir_all(&directory).unwrap();
    let real = directory.join("real.md");
    fs::write(&real, "safe").unwrap();
    let link = directory.join("link.md");
    symlink(&real, &link).unwrap();
    assert!(read_document_bytes(&link).is_err());
    assert!(read_document_tail_bytes(&link, 1024).is_err());
    assert!(atomic_write_document(&link, b"unsafe").is_err());
    fs::remove_dir_all(directory).unwrap();
}
