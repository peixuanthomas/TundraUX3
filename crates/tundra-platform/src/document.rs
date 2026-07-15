use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
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

fn stable_hash(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
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
        assert!(atomic_write_document(&link, b"unsafe").is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
