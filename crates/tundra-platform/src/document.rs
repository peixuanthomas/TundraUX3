use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::PlatformError;

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

pub fn read_document_bytes(path: &Path) -> Result<DocumentBytes, PlatformError> {
    validate_no_follow_path(path, true)?;
    let mut file = File::open(path).map_err(|error| PlatformError::Io {
        operation: "open document",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| PlatformError::Io {
            operation: "read document",
            path: Some(path.to_path_buf()),
            message: error.to_string(),
        })?;
    let metadata = file.metadata().map_err(|error| PlatformError::Io {
        operation: "read document metadata",
        path: Some(path.to_path_buf()),
        message: error.to_string(),
    })?;
    if !metadata.is_file() {
        return Err(PlatformError::InvalidInput {
            message: format!("document is not a regular file: {}", path.display()),
        });
    }
    let fingerprint = DocumentFingerprint {
        len: metadata.len(),
        modified: metadata.modified().ok(),
        content_hash: stable_hash(&bytes),
    };
    Ok(DocumentBytes { bytes, fingerprint })
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
    read_document_bytes(path).map(|document| document.fingerprint)
}

pub fn atomic_write_document(
    path: &Path,
    bytes: &[u8],
) -> Result<DocumentFingerprint, PlatformError> {
    match atomic_write_document_impl(path, bytes, WriteExpectation::Unchecked) {
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
    atomic_write_document_impl(path, bytes, WriteExpectation::Exact(expected))
}

fn atomic_write_document_impl(
    path: &Path,
    bytes: &[u8],
    expectation: WriteExpectation,
) -> Result<DocumentFingerprint, DocumentWriteError> {
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
        let mut file = match OpenOptions::new()
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

        let write_result = file.write_all(bytes).and_then(|_| {
            if let Some(permissions) = target_permissions.clone() {
                file.set_permissions(permissions)?;
            }
            file.sync_all()
        });
        if let Err(error) = write_result {
            drop(file);
            let _ = fs::remove_file(&temporary);
            return Err(PlatformError::Io {
                operation: "write temporary document",
                path: Some(temporary),
                message: error.to_string(),
            }
            .into());
        }
        drop(file);

        if let Err(error) = verify_write_expectation(path, expectation) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        let install_result = match expectation {
            WriteExpectation::Exact(None) => install_new_file(&temporary, path),
            WriteExpectation::Unchecked | WriteExpectation::Exact(Some(_)) => {
                replace_file(&temporary, path)
            }
        };
        if let Err(error) = install_result {
            let _ = fs::remove_file(&temporary);
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
        return document_fingerprint(path).map_err(DocumentWriteError::Platform);
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

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hasher = StableHasher::new();
    hasher.write(bytes);
    hasher.finish()
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
