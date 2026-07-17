use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::PlatformError;

/// Default hard limit for document reads performed by the editor.
///
/// [`read_document_bytes`] intentionally remains unlimited for compatibility.
/// New interactive open flows should use [`read_document_bytes_limited`] with
/// this limit so a malformed or unexpectedly large file cannot force an
/// unbounded allocation.
pub const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024 * 1024;

const DOCUMENT_READ_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentFingerprint {
    pub len: u64,
    pub modified: Option<SystemTime>,
    pub content_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentBytes {
    pub bytes: Vec<u8>,
    pub fingerprint: DocumentFingerprint,
}

/// A UTF-8-safe suffix of a document.
///
/// `start_byte` is the byte position in the source document where `bytes`
/// begins. `total_bytes` is the source length. `truncated` is true when bytes
/// before the returned window were omitted, including when a partial first
/// line was discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentReadWindow {
    pub bytes: Vec<u8>,
    pub start_byte: u64,
    pub total_bytes: u64,
    pub truncated: bool,
}

/// Failure from a conditional document save.
///
/// `ExternalModification` is deliberately distinct from an I/O error so an
/// editor can offer reload/save-as/overwrite choices without guessing from an
/// error string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentWriteError {
    ExternalModification {
        path: PathBuf,
        expected: Option<DocumentFingerprint>,
        actual: Option<DocumentFingerprint>,
    },
    Platform(PlatformError),
}

impl fmt::Display for DocumentWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExternalModification { path, .. } => write!(
                formatter,
                "document changed outside the editor before it could be saved: {}",
                path.display()
            ),
            Self::Platform(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for DocumentWriteError {}

impl From<PlatformError> for DocumentWriteError {
    fn from(value: PlatformError) -> Self {
        Self::Platform(value)
    }
}

#[derive(Debug, Clone, Copy)]
enum WriteExpectation {
    Unchecked,
    Exact(Option<DocumentFingerprint>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocumentReadConsistency {
    Stable,
    AppendablePrefix,
}

pub fn read_document_bytes(path: &Path) -> Result<DocumentBytes, PlatformError> {
    read_document_bytes_limited(path, u64::MAX)
}

/// Reads a stable, regular-file snapshot without allocating more than
/// `max_bytes` for document contents.
///
/// The file size is checked before allocation, reading is bounded to that
/// preflight size, and metadata plus the no-follow path policy are rechecked
/// after the read. A file that grows beyond `max_bytes` is rejected with an
/// error containing both the observed and configured sizes. Other concurrent
/// size or modification changes are reported as an unstable read.
pub fn read_document_bytes_limited(
    path: &Path,
    max_bytes: u64,
) -> Result<DocumentBytes, PlatformError> {
    read_document_bytes_limited_with_progress(path, max_bytes, |_, _| true)
}

/// Progress-reporting and cooperatively cancellable variant of
/// [`read_document_bytes_limited`].
///
/// `progress` is called after preflight with `(0, total_bytes)` and after each
/// chunk of at most 64 KiB. Returning `false` stops before the next chunk and
/// returns [`PlatformError::Interrupted`].
pub fn read_document_bytes_limited_with_progress<F>(
    path: &Path,
    max_bytes: u64,
    mut progress: F,
) -> Result<DocumentBytes, PlatformError>
where
    F: FnMut(u64, u64) -> bool,
{
    let (bytes, fingerprint) = read_document_snapshot_with_progress(
        path,
        max_bytes,
        true,
        DocumentReadConsistency::Stable,
        &mut progress,
    )?;
    Ok(DocumentBytes {
        bytes: bytes.expect("document snapshot requested owned bytes"),
        fingerprint,
    })
}

/// Reads the prefix that existed when a regular file was opened.
///
/// Unlike [`read_document_bytes_limited`], appends that happen after preflight
/// are permitted and are deliberately excluded from the returned bytes. The
/// preflight length is still bounded by `max_bytes`; truncation, replacement,
/// no-follow violations, and detectable rewrites within the original prefix
/// are rejected. This is intended for append-only log snapshots.
pub fn read_document_prefix_snapshot_limited(
    path: &Path,
    max_bytes: u64,
) -> Result<DocumentBytes, PlatformError> {
    read_document_prefix_snapshot_limited_with_progress(path, max_bytes, |_, _| true)
}

/// Progress-reporting and cooperatively cancellable variant of
/// [`read_document_prefix_snapshot_limited`].
///
/// The progress total is the preflight prefix length, so appends do not move
/// the goal while the snapshot is being read.
pub fn read_document_prefix_snapshot_limited_with_progress<F>(
    path: &Path,
    max_bytes: u64,
    mut progress: F,
) -> Result<DocumentBytes, PlatformError>
where
    F: FnMut(u64, u64) -> bool,
{
    let (bytes, fingerprint) = read_document_snapshot_with_progress(
        path,
        max_bytes,
        true,
        DocumentReadConsistency::AppendablePrefix,
        &mut progress,
    )?;
    Ok(DocumentBytes {
        bytes: bytes.expect("document prefix snapshot requested owned bytes"),
        fingerprint,
    })
}

fn read_document_snapshot(
    path: &Path,
    max_bytes: u64,
    collect_bytes: bool,
    consistency: DocumentReadConsistency,
) -> Result<(Option<Vec<u8>>, DocumentFingerprint), PlatformError> {
    read_document_snapshot_with_progress(
        path,
        max_bytes,
        collect_bytes,
        consistency,
        &mut |_, _| true,
    )
}

fn read_document_snapshot_with_progress<F>(
    path: &Path,
    max_bytes: u64,
    collect_bytes: bool,
    consistency: DocumentReadConsistency,
    progress: &mut F,
) -> Result<(Option<Vec<u8>>, DocumentFingerprint), PlatformError>
where
    F: FnMut(u64, u64) -> bool,
{
    validate_no_follow_path(path, true)?;
    let file = File::open(path).map_err(|error| PlatformError::Io {
        operation: "open document",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    let initial_metadata = file.metadata().map_err(|error| PlatformError::Io {
        operation: "read document metadata",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    if !initial_metadata.is_file() {
        return Err(PlatformError::InvalidInput {
            message: format!("document is not a regular file: {}", path.display()),
        });
    }
    validate_no_follow_path(path, true)?;

    read_open_document_snapshot_with_progress(
        path,
        file,
        initial_metadata,
        max_bytes,
        collect_bytes,
        consistency,
        progress,
    )
}

#[cfg(test)]
fn read_open_document_snapshot(
    path: &Path,
    file: File,
    initial_metadata: fs::Metadata,
    max_bytes: u64,
    collect_bytes: bool,
    consistency: DocumentReadConsistency,
) -> Result<(Option<Vec<u8>>, DocumentFingerprint), PlatformError> {
    read_open_document_snapshot_with_progress(
        path,
        file,
        initial_metadata,
        max_bytes,
        collect_bytes,
        consistency,
        &mut |_, _| true,
    )
}

fn read_open_document_snapshot_with_progress<F>(
    path: &Path,
    mut file: File,
    initial_metadata: fs::Metadata,
    max_bytes: u64,
    collect_bytes: bool,
    consistency: DocumentReadConsistency,
    progress: &mut F,
) -> Result<(Option<Vec<u8>>, DocumentFingerprint), PlatformError>
where
    F: FnMut(u64, u64) -> bool,
{
    let initial_len = initial_metadata.len();
    ensure_document_size_within_limit(path, initial_len, max_bytes)?;
    if !progress(0, initial_len) {
        return Err(document_read_interrupted(path, 0, initial_len));
    }
    let initial_modified = initial_metadata.modified().ok();
    let mut bytes = if collect_bytes {
        let capacity = usize::try_from(initial_len).map_err(|_| PlatformError::InvalidInput {
            message: format!(
                "document is too large to fit in memory on this platform: {} ({} bytes)",
                path.display(),
                initial_len
            ),
        })?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|error| PlatformError::InvalidInput {
                message: format!(
                    "could not reserve memory to read document: {} ({} bytes): {error}",
                    path.display(),
                    initial_len
                ),
            })?;
        Some(bytes)
    } else {
        None
    };
    let mut hasher = StableHasher::new();
    let mut remaining = initial_len;
    let mut chunk = [0_u8; DOCUMENT_READ_CHUNK_BYTES];
    while remaining > 0 {
        let requested = remaining.min(DOCUMENT_READ_CHUNK_BYTES as u64) as usize;
        let read = file
            .read(&mut chunk[..requested])
            .map_err(|error| PlatformError::Io {
                operation: "read document",
                path: Some(path.to_path_buf()),
                message: error.to_string(),
            })?;
        if read == 0 {
            let actual_len = file.metadata().ok().map(|metadata| metadata.len());
            return Err(document_changed_while_reading(
                path,
                initial_len,
                actual_len,
            ));
        }
        hasher.write(&chunk[..read]);
        if let Some(bytes) = bytes.as_mut() {
            bytes.extend_from_slice(&chunk[..read]);
        }
        remaining -= read as u64;
        let completed = initial_len - remaining;
        if !progress(completed, initial_len) {
            return Err(document_read_interrupted(path, completed, initial_len));
        }
    }

    let has_extra_byte = if consistency == DocumentReadConsistency::Stable {
        let mut extra = [0_u8; 1];
        file.read(&mut extra).map_err(|error| PlatformError::Io {
            operation: "verify document length",
            path: Some(path.to_path_buf()),
            message: error.to_string(),
        })? != 0
    } else {
        false
    };
    let final_metadata = file.metadata().map_err(|error| PlatformError::Io {
        operation: "verify document metadata",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    let final_len = final_metadata.len();
    match consistency {
        DocumentReadConsistency::Stable => {
            if final_len > max_bytes {
                return Err(document_size_limit_error(path, final_len, max_bytes));
            }
            if has_extra_byte
                || final_len != initial_len
                || final_metadata.modified().ok() != initial_modified
            {
                return Err(document_changed_while_reading(
                    path,
                    initial_len,
                    Some(final_len),
                ));
            }
        }
        DocumentReadConsistency::AppendablePrefix => {
            if final_len < initial_len {
                return Err(document_changed_while_reading(
                    path,
                    initial_len,
                    Some(final_len),
                ));
            }
            verify_document_prefix_samples(
                &mut file,
                bytes
                    .as_deref()
                    .expect("appendable prefix reads always collect bytes"),
                path,
            )?;
        }
    }
    validate_no_follow_path(path, true)?;
    let path_metadata = fs::symlink_metadata(path).map_err(|error| PlatformError::Io {
        operation: "verify document path metadata",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    match consistency {
        DocumentReadConsistency::Stable => {
            if !path_metadata.is_file()
                || path_metadata.len() != final_len
                || path_metadata.modified().ok() != final_metadata.modified().ok()
            {
                return Err(document_changed_while_reading(
                    path,
                    initial_len,
                    Some(path_metadata.len()),
                ));
            }
        }
        DocumentReadConsistency::AppendablePrefix => {
            if !path_metadata.is_file()
                || !open_file_refers_to_path(&file, path).map_err(|error| PlatformError::Io {
                    operation: "verify document identity",
                    path: Some(path.to_path_buf()),
                    message: error.to_string(),
                })?
            {
                return Err(document_replaced_while_reading(path));
            }
            if path_metadata.len() < initial_len {
                return Err(document_changed_while_reading(
                    path,
                    initial_len,
                    Some(path_metadata.len()),
                ));
            }
            let append_observed = final_len > initial_len || path_metadata.len() > initial_len;
            if !append_observed
                && (final_metadata.modified().ok() != initial_modified
                    || path_metadata.modified().ok() != initial_modified)
            {
                return Err(document_changed_while_reading(
                    path,
                    initial_len,
                    Some(path_metadata.len()),
                ));
            }
        }
    }

    let fingerprint = DocumentFingerprint {
        len: initial_len,
        modified: match consistency {
            DocumentReadConsistency::Stable => final_metadata.modified().ok(),
            DocumentReadConsistency::AppendablePrefix => initial_modified,
        },
        content_hash: hasher.finish(),
    };
    Ok((bytes, fingerprint))
}

fn ensure_document_size_within_limit(
    path: &Path,
    observed_bytes: u64,
    max_bytes: u64,
) -> Result<(), PlatformError> {
    if observed_bytes > max_bytes {
        Err(document_size_limit_error(path, observed_bytes, max_bytes))
    } else {
        Ok(())
    }
}

fn document_size_limit_error(path: &Path, observed_bytes: u64, max_bytes: u64) -> PlatformError {
    PlatformError::InvalidInput {
        message: format!(
            "document is too large to read: {} ({} bytes exceeds the {} byte limit)",
            path.display(),
            observed_bytes,
            max_bytes
        ),
    }
}

fn document_changed_while_reading(
    path: &Path,
    initial_len: u64,
    actual_len: Option<u64>,
) -> PlatformError {
    let actual = actual_len
        .map(|len| format!("{len} bytes"))
        .unwrap_or_else(|| "an unknown size".to_string());
    PlatformError::Io {
        operation: "read stable document",
        path: Some(path.to_path_buf()),
        message: format!(
            "document changed while it was being read (started at {initial_len} bytes, ended at {actual})"
        ),
    }
}

fn document_read_interrupted(path: &Path, completed_bytes: u64, total_bytes: u64) -> PlatformError {
    PlatformError::Interrupted {
        operation: "read document",
        path: Some(path.to_path_buf()),
        message: format!("document read cancelled after {completed_bytes} of {total_bytes} bytes"),
    }
}

fn document_replaced_while_reading(path: &Path) -> PlatformError {
    PlatformError::Io {
        operation: "read document snapshot",
        path: Some(path.to_path_buf()),
        message: "document path was replaced while it was being read".to_string(),
    }
}

fn verify_document_prefix_samples(
    file: &mut File,
    expected: &[u8],
    path: &Path,
) -> Result<(), PlatformError> {
    if expected.is_empty() {
        return Ok(());
    }
    let sample_len = expected.len().min(DOCUMENT_READ_CHUNK_BYTES);
    verify_document_prefix_sample(file, expected, 0, sample_len, path)?;
    if expected.len() > sample_len {
        verify_document_prefix_sample(
            file,
            expected,
            expected.len() - sample_len,
            sample_len,
            path,
        )?;
    }
    Ok(())
}

fn verify_document_prefix_sample(
    file: &mut File,
    expected: &[u8],
    start: usize,
    len: usize,
    path: &Path,
) -> Result<(), PlatformError> {
    file.seek(SeekFrom::Start(start as u64))
        .map_err(|error| PlatformError::Io {
            operation: "verify document snapshot",
            path: Some(path.to_path_buf()),
            message: error.to_string(),
        })?;
    let mut actual = vec![0_u8; len];
    file.read_exact(&mut actual)
        .map_err(|error| PlatformError::Io {
            operation: "verify document snapshot",
            path: Some(path.to_path_buf()),
            message: error.to_string(),
        })?;
    if actual != expected[start..start + len] {
        return Err(PlatformError::Io {
            operation: "read document snapshot",
            path: Some(path.to_path_buf()),
            message: "document contents changed within the opened snapshot while it was being read"
                .to_string(),
        });
    }
    Ok(())
}

fn open_file_refers_to_path(file: &File, path: &Path) -> io::Result<bool> {
    let open_file = same_file::Handle::from_file(file.try_clone()?)?;
    let path_file = same_file::Handle::from_path(path)?;
    Ok(open_file == path_file)
}

/// Reads at most `max_bytes` from the end of a regular UTF-8 document.
///
/// When the requested byte window begins inside a UTF-8 code point, the
/// leading code-point fragment is omitted. If the file was truncated and the
/// window contains a line ending, a leading partial line is also omitted so
/// callers never display a misleading fragment of an earlier line. A single
/// line longer than the window is retained as a tail instead of becoming an
/// empty document.
pub fn read_document_tail_bytes(
    path: &Path,
    max_bytes: usize,
) -> Result<DocumentReadWindow, PlatformError> {
    validate_no_follow_path(path, true)?;
    let mut file = File::open(path).map_err(|error| PlatformError::Io {
        operation: "open document tail",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    let metadata = file.metadata().map_err(|error| PlatformError::Io {
        operation: "read document tail metadata",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    if !metadata.is_file() {
        return Err(PlatformError::InvalidInput {
            message: format!("document is not a regular file: {}", path.display()),
        });
    }

    let total_bytes = metadata.len();
    let initial_modified = metadata.modified().ok();
    let requested_start = total_bytes.saturating_sub(max_bytes as u64);
    let prefix_len = requested_start.min(3) as usize;
    let read_start = requested_start.saturating_sub(prefix_len as u64);
    let read_len = total_bytes.saturating_sub(read_start);
    let capacity = usize::try_from(read_len).map_err(|_| PlatformError::InvalidInput {
        message: format!("document tail is too large to read: {}", path.display()),
    })?;
    file.seek(SeekFrom::Start(read_start))
        .map_err(|error| PlatformError::Io {
            operation: "seek document tail",
            path: Some(path.to_path_buf()),
            message: error.to_string(),
        })?;
    let mut bytes = Vec::with_capacity(capacity);
    Read::by_ref(&mut file)
        .take(read_len)
        .read_to_end(&mut bytes)
        .map_err(|error| PlatformError::Io {
            operation: "read document tail",
            path: Some(path.to_path_buf()),
            message: error.to_string(),
        })?;

    let utf8_start = utf8_tail_start(&bytes, prefix_len, path)?;
    let mut start_byte = read_start + utf8_start as u64;
    if utf8_start > 0 {
        bytes.drain(..utf8_start);
    }
    if requested_start > 0
        && !starts_after_line_ending(&mut file, start_byte, path)?
        && let Some(discarded) = first_line_ending_end(&bytes)
    {
        bytes.drain(..discarded);
        start_byte += discarded as u64;
    }

    let final_metadata = file.metadata().map_err(|error| PlatformError::Io {
        operation: "verify document tail metadata",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    if final_metadata.len() != total_bytes || final_metadata.modified().ok() != initial_modified {
        return Err(PlatformError::Io {
            operation: "read stable document tail",
            path: Some(path.to_path_buf()),
            message: "document changed while it was being read".to_string(),
        });
    }
    validate_no_follow_path(path, true)?;

    Ok(DocumentReadWindow {
        bytes,
        start_byte,
        total_bytes,
        truncated: start_byte > 0,
    })
}

fn utf8_tail_start(
    bytes: &[u8],
    requested_index: usize,
    path: &Path,
) -> Result<usize, PlatformError> {
    for candidate in 0..=requested_index.min(bytes.len()) {
        let Ok(text) = std::str::from_utf8(&bytes[candidate..]) else {
            continue;
        };
        let requested = requested_index.saturating_sub(candidate).min(text.len());
        if text.is_char_boundary(requested) {
            return Ok(candidate + requested);
        }
        if let Some(next) =
            (requested.saturating_add(1)..=text.len()).find(|index| text.is_char_boundary(*index))
        {
            return Ok(candidate + next);
        }
    }
    Err(PlatformError::InvalidInput {
        message: format!("document tail is not valid UTF-8: {}", path.display()),
    })
}

fn starts_after_line_ending(
    file: &mut File,
    start_offset: u64,
    path: &Path,
) -> Result<bool, PlatformError> {
    if start_offset == 0 {
        return Ok(true);
    }
    file.seek(SeekFrom::Start(start_offset - 1))
        .and_then(|_| {
            let mut previous = [0_u8; 1];
            file.read_exact(&mut previous)
                .map(|_| matches!(previous[0], b'\n' | b'\r'))
        })
        .map_err(|error| PlatformError::Io {
            operation: "inspect document tail line boundary",
            path: Some(path.to_path_buf()),
            message: error.to_string(),
        })
}

fn first_line_ending_end(bytes: &[u8]) -> Option<usize> {
    let index = bytes
        .iter()
        .position(|byte| matches!(*byte, b'\n' | b'\r'))?;
    Some(
        if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            index + 2
        } else {
            index + 1
        },
    )
}

pub fn document_fingerprint(path: &Path) -> Result<DocumentFingerprint, PlatformError> {
    read_document_snapshot(path, u64::MAX, false, DocumentReadConsistency::Stable)
        .map(|(_, fingerprint)| fingerprint)
}

pub fn atomic_write_document(
    path: &Path,
    bytes: &[u8],
) -> Result<DocumentFingerprint, PlatformError> {
    atomic_write_document_with(path, |writer| writer.write_all(bytes))
}

/// Atomically writes a document using a streaming callback.
///
/// The callback may write any number of chunks to the supplied writer. The
/// platform hashes and counts those chunks as they are written, applies the
/// existing target permissions, syncs the temporary file, atomically installs
/// it, and syncs the parent directory.
pub fn atomic_write_document_with<F>(
    path: &Path,
    write: F,
) -> Result<DocumentFingerprint, PlatformError>
where
    F: FnOnce(&mut dyn Write) -> io::Result<()>,
{
    match atomic_write_document_impl(path, WriteExpectation::Unchecked, write) {
        Ok(fingerprint) => Ok(fingerprint),
        Err(DocumentWriteError::Platform(error)) => Err(error),
        Err(DocumentWriteError::ExternalModification { .. }) => {
            unreachable!("unchecked document writes do not perform conflict detection")
        }
    }
}

/// Atomically saves only while the target still has `expected` contents.
///
/// Passing `Some(fingerprint)` protects an opened document from overwriting a
/// later external edit. Passing `None` means the target is expected not to
/// exist, which protects Save As from clobbering a file created in the
/// meantime.
pub fn atomic_write_document_if_unchanged(
    path: &Path,
    bytes: &[u8],
    expected: Option<DocumentFingerprint>,
) -> Result<DocumentFingerprint, DocumentWriteError> {
    atomic_write_document_if_unchanged_with(path, expected, |writer| writer.write_all(bytes))
}

/// Streaming variant of [`atomic_write_document_if_unchanged`].
///
/// The expectation is checked both before invoking `write` and immediately
/// before installation. If the callback fails, the temporary file is removed
/// and the original target is left untouched.
pub fn atomic_write_document_if_unchanged_with<F>(
    path: &Path,
    expected: Option<DocumentFingerprint>,
    write: F,
) -> Result<DocumentFingerprint, DocumentWriteError>
where
    F: FnOnce(&mut dyn Write) -> io::Result<()>,
{
    atomic_write_document_impl(path, WriteExpectation::Exact(expected), write)
}

fn atomic_write_document_impl<F>(
    path: &Path,
    expectation: WriteExpectation,
    write: F,
) -> Result<DocumentFingerprint, DocumentWriteError>
where
    F: FnOnce(&mut dyn Write) -> io::Result<()>,
{
    validate_no_follow_path(path, false)?;
    let parent = path.parent().ok_or_else(|| PlatformError::InvalidInput {
        message: format!("document path has no parent: {}", path.display()),
    })?;
    fs::create_dir_all(parent).map_err(|error| PlatformError::Io {
        operation: "create document parent directory",
        path: Some(parent.to_path_buf()),
        message: error.to_string(),
    })?;
    validate_no_follow_path(parent, true)?;
    verify_write_expectation(path, expectation)?;
    let target_permissions = existing_permissions(path)?;

    for attempt in 0..64_u8 {
        let temporary = temporary_path(path, attempt);
        let file = match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(PlatformError::Io {
                    operation: "create temporary document",
                    path: Some(temporary),
                    message: error.to_string(),
                }
                .into());
            }
        };
        let mut temporary_document = TemporaryDocument::new(temporary.clone(), file);

        let (write_result, written_len, content_hash) = {
            let mut writer = FingerprintingWriter::new(temporary_document.file_mut());
            let result = write(&mut writer).and_then(|_| writer.flush());
            let written_len = writer.len;
            let content_hash = writer.hasher.finish();
            (result, written_len, content_hash)
        };
        let write_result = write_result.and_then(|_| {
            if let Some(permissions) = target_permissions.clone() {
                temporary_document.file_mut().set_permissions(permissions)?;
            }
            temporary_document.file_mut().sync_all()
        });
        if let Err(error) = write_result {
            return Err(PlatformError::Io {
                operation: "write temporary document",
                path: Some(temporary),
                message: error.to_string(),
            }
            .into());
        }
        temporary_document.close();

        verify_write_expectation(path, expectation)?;
        validate_no_follow_path(path, false)?;
        let install_result = match expectation {
            WriteExpectation::Exact(None) => install_new_file(&temporary, path),
            WriteExpectation::Unchecked | WriteExpectation::Exact(Some(_)) => {
                replace_file(&temporary, path)
            }
        };
        if let Err(error) = install_result {
            if matches!(expectation, WriteExpectation::Exact(_))
                && let Err(conflict) = verify_write_expectation(path, expectation)
            {
                return Err(conflict);
            }
            return Err(PlatformError::Io {
                operation: "replace document",
                path: Some(path.to_path_buf()),
                message: error.to_string(),
            }
            .into());
        }
        sync_parent_directory(parent).map_err(|error| PlatformError::Io {
            operation: "sync document parent directory",
            path: Some(parent.to_path_buf()),
            message: error.to_string(),
        })?;
        return fingerprint_installed_document(path, written_len, content_hash)
            .map_err(DocumentWriteError::Platform);
    }

    Err(PlatformError::Io {
        operation: "create temporary document",
        path: Some(parent.to_path_buf()),
        message: "could not reserve a unique temporary filename".to_string(),
    }
    .into())
}

fn verify_write_expectation(
    path: &Path,
    expectation: WriteExpectation,
) -> Result<(), DocumentWriteError> {
    let WriteExpectation::Exact(expected) = expectation else {
        return Ok(());
    };
    let actual = optional_document_fingerprint(path)?;
    if actual == expected {
        Ok(())
    } else {
        Err(DocumentWriteError::ExternalModification {
            path: path.to_path_buf(),
            expected,
            actual,
        })
    }
}

fn optional_document_fingerprint(
    path: &Path,
) -> Result<Option<DocumentFingerprint>, PlatformError> {
    match fs::symlink_metadata(path) {
        Ok(_) => document_fingerprint(path).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PlatformError::Io {
            operation: "inspect document before save",
            path: Some(path.to_path_buf()),
            message: error.to_string(),
        }),
    }
}

fn existing_permissions(path: &Path) -> Result<Option<fs::Permissions>, PlatformError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PlatformError::Io {
            operation: "read document permissions",
            path: Some(path.to_path_buf()),
            message: error.to_string(),
        }),
    }
}

pub fn validate_no_follow_path(path: &Path, must_exist: bool) -> Result<(), PlatformError> {
    let mut current = Some(path);
    let mut checked_target = false;
    while let Some(candidate) = current {
        match fs::symlink_metadata(candidate) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || metadata_is_reparse_point(&metadata) {
                    return Err(PlatformError::InvalidInput {
                        message: format!(
                            "symbolic links and reparse points are not valid document paths: {}",
                            candidate.display()
                        ),
                    });
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && (!checked_target || !must_exist) => {}
            Err(error) => {
                return Err(PlatformError::Io {
                    operation: "inspect document path",
                    path: Some(candidate.to_path_buf()),
                    message: error.to_string(),
                });
            }
        }
        checked_target = true;
        current = candidate.parent();
    }
    Ok(())
}

fn temporary_path(path: &Path, attempt: u8) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "document".into());
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    parent.join(format!(
        ".{name}.tundra.{}.{}.{}.tmp",
        process::id(),
        timestamp,
        attempt
    ))
}

struct TemporaryDocument {
    path: PathBuf,
    file: Option<File>,
}

impl TemporaryDocument {
    fn new(path: PathBuf, file: File) -> Self {
        Self {
            path,
            file: Some(file),
        }
    }

    fn file_mut(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("temporary document file is still open")
    }

    fn close(&mut self) {
        self.file.take();
    }
}

impl Drop for TemporaryDocument {
    fn drop(&mut self) {
        self.file.take();
        let _ = fs::remove_file(&self.path);
    }
}

struct FingerprintingWriter<'a> {
    inner: &'a mut File,
    hasher: StableHasher,
    len: u64,
}

impl<'a> FingerprintingWriter<'a> {
    fn new(inner: &'a mut File) -> Self {
        Self {
            inner,
            hasher: StableHasher::new(),
            len: 0,
        }
    }
}

impl Write for FingerprintingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(bytes)?;
        self.len = self
            .len
            .checked_add(written as u64)
            .ok_or_else(|| io::Error::other("document length overflow while writing"))?;
        self.hasher.write(&bytes[..written]);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn fingerprint_installed_document(
    path: &Path,
    written_len: u64,
    content_hash: u64,
) -> Result<DocumentFingerprint, PlatformError> {
    validate_no_follow_path(path, true)?;
    let metadata = fs::symlink_metadata(path).map_err(|error| PlatformError::Io {
        operation: "verify saved document metadata",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    if !metadata.is_file() {
        return Err(PlatformError::InvalidInput {
            message: format!("saved document is not a regular file: {}", path.display()),
        });
    }
    if metadata.len() != written_len {
        return Err(PlatformError::Io {
            operation: "verify saved document",
            path: Some(path.to_path_buf()),
            message: format!(
                "saved document length changed unexpectedly (wrote {written_len} bytes, found {} bytes)",
                metadata.len()
            ),
        });
    }
    Ok(DocumentFingerprint {
        len: written_len,
        modified: metadata.modified().ok(),
        content_hash,
    })
}

struct StableHasher(u64);

impl StableHasher {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(Self::PRIME);
        }
    }

    fn finish(self) -> u64 {
        self.0
    }
}

#[cfg(not(windows))]
fn metadata_is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(windows)]
fn metadata_is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn install_new_file(from: &Path, to: &Path) -> std::io::Result<()> {
    // Creating a hard link is an atomic no-clobber operation on the same
    // filesystem. The temporary file always lives beside the destination.
    fs::hard_link(from, to)?;
    fs::remove_file(from)
}

#[cfg(windows)]
fn install_new_file(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    fn wide_null(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    let from = wide_null(from.as_os_str());
    let to = wide_null(to.as_os_str());
    let result = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn sync_parent_directory(parent: &Path) -> std::io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(windows)]
fn sync_parent_directory(_parent: &Path) -> std::io::Result<()> {
    // Both ReplaceFileW and MoveFileExW below request write-through semantics.
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> std::io::Result<()> {
    use core::ffi::c_void;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    const REPLACEFILE_WRITE_THROUGH: u32 = 0x1;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
        fn ReplaceFileW(
            replaced_file_name: *const u16,
            replacement_file_name: *const u16,
            backup_file_name: *const u16,
            replace_flags: u32,
            exclude: *mut c_void,
            reserved: *mut c_void,
        ) -> i32;
    }

    fn wide_null(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    let target_exists = to.try_exists()?;
    let from = wide_null(from.as_os_str());
    let to = wide_null(to.as_os_str());
    let result = if target_exists {
        unsafe {
            ReplaceFileW(
                to.as_ptr(),
                from.as_ptr(),
                ptr::null(),
                REPLACEFILE_WRITE_THROUGH,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        }
    } else {
        unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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

        let loaded = read_document_prefix_snapshot_limited_with_progress(
            &path,
            total,
            |completed, total| {
                progress.push((completed, total));
                true
            },
        )
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
            PlatformError::Io {
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
}
