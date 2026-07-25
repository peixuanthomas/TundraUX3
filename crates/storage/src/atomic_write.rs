use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(any(not(target_os = "linux"), test))]
use std::fs;
#[cfg(not(target_os = "linux"))]
use std::fs::OpenOptions;
#[cfg(all(unix, not(target_os = "linux")))]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
#[cfg(any(not(target_os = "linux"), test))]
use std::path::PathBuf;

use crate::error::StorageError;

#[cfg(target_os = "linux")]
pub(crate) fn create_dir(path: &Path, operation: &'static str) -> Result<(), StorageError> {
    unix_secure::open_directory(path, true)
        .and_then(|directory| directory.set_private_permissions())
        .map(|_| ())
        .map_err(|error| StorageError::Io {
            operation,
            path: path.to_path_buf(),
            message: error.to_string(),
        })
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn create_dir(path: &Path, operation: &'static str) -> Result<(), StorageError> {
    reject_symlink(path, operation)?;
    fs::create_dir_all(path).map_err(|error| StorageError::Io {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    tighten_directory(path, operation)
}

#[cfg(target_os = "linux")]
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    unix_secure::atomic_write(path, bytes)
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let parent = path.parent().ok_or_else(|| StorageError::MissingParent {
        path: path.to_path_buf(),
    })?;
    create_dir(parent, "create storage parent directory")?;
    reject_symlink(path, "inspect storage destination")?;

    for _ in 0..64 {
        let temp_path = temp_write_path(path);
        let mut file = match OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temp_path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(StorageError::Io {
                    operation: "create temporary storage file",
                    path: temp_path,
                    message: error.to_string(),
                });
            }
        };

        if let Err(error) = write_and_sync(&mut file, bytes) {
            let _ = fs::remove_file(&temp_path);
            return Err(StorageError::Io {
                operation: "write temporary storage file",
                path: temp_path,
                message: error.to_string(),
            });
        }

        drop(file);

        if let Err(error) = replace_file(&temp_path, path) {
            let _ = fs::remove_file(&temp_path);
            return Err(StorageError::Io {
                operation: "replace storage file",
                path: path.to_path_buf(),
                message: error.to_string(),
            });
        }

        sync_parent_directory(parent)?;

        return Ok(());
    }

    Err(StorageError::Io {
        operation: "create temporary storage file",
        path: parent.to_path_buf(),
        message: "could not create a unique temporary file".to_string(),
    })
}

/// Reject application-owned paths that have been replaced with a symbolic link.
///
/// Storage only calls this for paths under its own layout; deliberately keeping
/// the check here avoids applying this policy to documents opened by the editor.
#[cfg(not(unix))]
trait PrivateFileOptions {
    fn mode(&mut self, _mode: u32) -> &mut Self;
}

#[cfg(not(unix))]
impl PrivateFileOptions for OpenOptions {
    fn mode(&mut self, _mode: u32) -> &mut Self {
        self
    }
}

#[cfg(not(target_os = "linux"))]
fn reject_symlink(path: &Path, operation: &'static str) -> Result<(), StorageError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(StorageError::Io {
                operation,
                path: path.to_path_buf(),
                message: error.to_string(),
            });
        }
    };

    if metadata.file_type().is_symlink() {
        return Err(StorageError::Io {
            operation,
            path: path.to_path_buf(),
            message: "symbolic links are not allowed in application storage".to_string(),
        });
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub(crate) fn tighten_file(path: &Path, operation: &'static str) -> Result<(), StorageError> {
    unix_secure::tighten_file(path).map_err(|error| StorageError::Io {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn tighten_file(path: &Path, operation: &'static str) -> Result<(), StorageError> {
    reject_symlink(path, operation)?;

    #[cfg(unix)]
    {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(StorageError::Io {
                    operation,
                    path: path.to_path_buf(),
                    message: error.to_string(),
                });
            }
        };
        if !metadata.is_file() {
            return Err(StorageError::Io {
                operation,
                path: path.to_path_buf(),
                message: "application storage path is not a regular file".to_string(),
            });
        }
        if metadata.mode() & 0o777 != 0o600 {
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
                StorageError::Io {
                    operation: "tighten application file permissions",
                    path: path.to_path_buf(),
                    message: error.to_string(),
                }
            })?;
        }
    }

    Ok(())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn tighten_directory(path: &Path, operation: &'static str) -> Result<(), StorageError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| StorageError::Io {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if !metadata.is_dir() {
        return Err(StorageError::Io {
            operation,
            path: path.to_path_buf(),
            message: "application storage path is not a directory".to_string(),
        });
    }
    if metadata.mode() & 0o777 != 0o700 {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            StorageError::Io {
                operation: "tighten application directory permissions",
                path: path.to_path_buf(),
                message: error.to_string(),
            }
        })?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn tighten_directory(_path: &Path, _operation: &'static str) -> Result<(), StorageError> {
    Ok(())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn sync_parent_directory(parent: &Path) -> Result<(), StorageError> {
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| StorageError::Io {
            operation: "sync storage parent directory",
            path: parent.to_path_buf(),
            message: error.to_string(),
        })
}

#[cfg(not(unix))]
fn sync_parent_directory(_parent: &Path) -> Result<(), StorageError> {
    Ok(())
}

#[cfg(target_os = "linux")]
mod unix_secure {
    use std::ffi::{CStr, CString, OsStr, OsString};
    use std::fs::File;
    use std::io;
    use std::mem::MaybeUninit;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Component, Path};

    use super::{StorageError, process, unix_nanos, write_and_sync};

    const PRIVATE_DIRECTORY_MODE: libc::mode_t = 0o700;
    const PRIVATE_FILE_MODE: libc::mode_t = 0o600;

    pub(super) struct Directory {
        fd: OwnedFd,
    }

    impl Directory {
        pub(super) fn set_private_permissions(&self) -> io::Result<()> {
            cvt(unsafe {
                // SAFETY: `fd` is an open directory descriptor owned by this value.
                libc::fchmod(self.fd.as_raw_fd(), PRIVATE_DIRECTORY_MODE)
            })
            .map(|_| ())
        }

        fn create_private_file(&self, name: &CStr) -> io::Result<File> {
            let fd = cvt(unsafe {
                // SAFETY: `name` is NUL-terminated, and the returned descriptor is
                // immediately adopted by `OwnedFd`.
                libc::openat(
                    self.fd.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_CLOEXEC
                        | libc::O_NOFOLLOW,
                    PRIVATE_FILE_MODE,
                )
            })?;
            let fd = unsafe {
                // SAFETY: a successful `openat` returns a new owned descriptor.
                OwnedFd::from_raw_fd(fd)
            };
            cvt(unsafe {
                // SAFETY: the descriptor is valid for the duration of this call.
                libc::fchmod(fd.as_raw_fd(), PRIVATE_FILE_MODE)
            })?;
            Ok(File::from(fd))
        }

        fn open_regular_file(&self, name: &CStr) -> io::Result<Option<OwnedFd>> {
            let Some(mode) = metadata_mode_at(self.fd.as_raw_fd(), name)? else {
                return Ok(None);
            };
            ensure_regular_not_symlink(mode)?;

            let fd = cvt(unsafe {
                // SAFETY: `name` is NUL-terminated and resolution is relative to
                // the already verified directory descriptor.
                libc::openat(
                    self.fd.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            })?;
            let fd = unsafe {
                // SAFETY: a successful `openat` returns a new owned descriptor.
                OwnedFd::from_raw_fd(fd)
            };
            ensure_fd_is_regular(fd.as_raw_fd())?;
            Ok(Some(fd))
        }

        fn validate_regular_or_missing(&self, name: &CStr) -> io::Result<()> {
            if let Some(mode) = metadata_mode_at(self.fd.as_raw_fd(), name)? {
                ensure_regular_not_symlink(mode)?;
            }
            Ok(())
        }

        fn rename(&self, from: &CStr, to: &CStr) -> io::Result<()> {
            cvt(unsafe {
                // SAFETY: both names are NUL-terminated direct children and both
                // directory descriptors remain valid throughout the call.
                libc::renameat(
                    self.fd.as_raw_fd(),
                    from.as_ptr(),
                    self.fd.as_raw_fd(),
                    to.as_ptr(),
                )
            })
            .map(|_| ())
        }

        fn unlink(&self, name: &CStr) -> io::Result<()> {
            cvt(unsafe {
                // SAFETY: `name` is a NUL-terminated direct child name.
                libc::unlinkat(self.fd.as_raw_fd(), name.as_ptr(), 0)
            })
            .map(|_| ())
        }

        fn sync(&self) -> io::Result<()> {
            cvt(unsafe {
                // SAFETY: `fd` remains an open directory descriptor.
                libc::fsync(self.fd.as_raw_fd())
            })
            .map(|_| ())
        }
    }

    pub(super) fn open_directory(path: &Path, create: bool) -> io::Result<Directory> {
        let start = if path.is_absolute() { c"/" } else { c"." };
        let mut current = open_directory_fd(libc::AT_FDCWD, start)?;
        let mut saw_normal_component = false;

        for component in path.components() {
            let name = match component {
                Component::RootDir | Component::CurDir => continue,
                Component::Normal(name) => name,
                Component::ParentDir => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "parent-directory components are not allowed in application storage",
                    ));
                }
                Component::Prefix(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "platform path prefixes are not valid Unix storage paths",
                    ));
                }
            };

            saw_normal_component = true;
            let name = c_name(name)?;
            current = open_or_create_child_directory(current.as_raw_fd(), &name, create)?;
        }

        if !saw_normal_component {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "application storage directory must not be the filesystem root",
            ));
        }

        Ok(Directory { fd: current })
    }

    pub(super) fn tighten_file(path: &Path) -> io::Result<()> {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "storage file has no parent")
        })?;
        let file_name = direct_child_name(path)?;
        let directory = open_directory(parent, false)?;

        if let Some(file) = directory.open_regular_file(&file_name)? {
            cvt(unsafe {
                // SAFETY: `file` is a verified regular-file descriptor.
                libc::fchmod(file.as_raw_fd(), PRIVATE_FILE_MODE)
            })?;
        }
        Ok(())
    }

    pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StorageError> {
        let parent = path.parent().ok_or_else(|| StorageError::MissingParent {
            path: path.to_path_buf(),
        })?;
        let destination = direct_child_name(path).map_err(|error| StorageError::Io {
            operation: "validate storage destination",
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        let directory = open_directory(parent, true)
            .and_then(|directory| {
                directory.set_private_permissions()?;
                Ok(directory)
            })
            .map_err(|error| StorageError::Io {
                operation: "open storage parent directory",
                path: parent.to_path_buf(),
                message: error.to_string(),
            })?;
        directory
            .validate_regular_or_missing(&destination)
            .map_err(|error| StorageError::Io {
                operation: "inspect storage destination",
                path: path.to_path_buf(),
                message: error.to_string(),
            })?;

        for _ in 0..64 {
            let temporary_name = temporary_name(path).map_err(|error| StorageError::Io {
                operation: "create temporary storage name",
                path: path.to_path_buf(),
                message: error.to_string(),
            })?;
            let temporary_path = parent.join(OsStr::from_bytes(temporary_name.to_bytes()));
            let mut file = match directory.create_private_file(&temporary_name) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(StorageError::Io {
                        operation: "create temporary storage file",
                        path: temporary_path,
                        message: error.to_string(),
                    });
                }
            };

            if let Err(error) = write_and_sync(&mut file, bytes) {
                drop(file);
                let _ = directory.unlink(&temporary_name);
                return Err(StorageError::Io {
                    operation: "write temporary storage file",
                    path: temporary_path,
                    message: error.to_string(),
                });
            }
            drop(file);

            if let Err(error) = directory.validate_regular_or_missing(&destination) {
                let _ = directory.unlink(&temporary_name);
                return Err(StorageError::Io {
                    operation: "inspect storage destination",
                    path: path.to_path_buf(),
                    message: error.to_string(),
                });
            }
            if let Err(error) = directory.rename(&temporary_name, &destination) {
                let _ = directory.unlink(&temporary_name);
                return Err(StorageError::Io {
                    operation: "replace storage file",
                    path: path.to_path_buf(),
                    message: error.to_string(),
                });
            }
            directory.sync().map_err(|error| StorageError::Io {
                operation: "sync storage parent directory",
                path: parent.to_path_buf(),
                message: error.to_string(),
            })?;
            return Ok(());
        }

        Err(StorageError::Io {
            operation: "create temporary storage file",
            path: parent.to_path_buf(),
            message: "could not create a unique temporary file".to_string(),
        })
    }

    fn open_or_create_child_directory(
        parent: RawFd,
        name: &CStr,
        create: bool,
    ) -> io::Result<OwnedFd> {
        for _ in 0..3 {
            match metadata_mode_at(parent, name)? {
                Some(mode) => {
                    ensure_directory_not_symlink(mode)?;
                    return open_directory_fd(parent, name);
                }
                None if !create => return Err(io::Error::from(io::ErrorKind::NotFound)),
                None => {
                    let result = unsafe {
                        // SAFETY: `parent` is open, and `name` is NUL-terminated.
                        libc::mkdirat(parent, name.as_ptr(), PRIVATE_DIRECTORY_MODE)
                    };
                    if result == 0 {
                        let directory = open_directory_fd(parent, name)?;
                        cvt(unsafe {
                            // SAFETY: the descriptor was just opened successfully.
                            libc::fchmod(directory.as_raw_fd(), PRIVATE_DIRECTORY_MODE)
                        })?;
                        return Ok(directory);
                    }
                    let error = io::Error::last_os_error();
                    if error.kind() != io::ErrorKind::AlreadyExists {
                        return Err(error);
                    }
                }
            }
        }

        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "directory entry changed repeatedly during secure creation",
        ))
    }

    fn open_directory_fd(parent: RawFd, name: &CStr) -> io::Result<OwnedFd> {
        let fd = cvt(unsafe {
            // SAFETY: `parent` is either AT_FDCWD or a valid directory descriptor;
            // `name` is NUL-terminated. O_NOFOLLOW rejects a symlink at this step.
            libc::openat(
                parent,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            )
        })?;
        Ok(unsafe {
            // SAFETY: a successful `openat` returns a new owned descriptor.
            OwnedFd::from_raw_fd(fd)
        })
    }

    fn metadata_mode_at(parent: RawFd, name: &CStr) -> io::Result<Option<libc::mode_t>> {
        let mut metadata = MaybeUninit::<libc::stat>::uninit();
        let result = unsafe {
            // SAFETY: `metadata` points to writable storage and `name` is
            // NUL-terminated. AT_SYMLINK_NOFOLLOW reports the entry itself.
            libc::fstatat(
                parent,
                name.as_ptr(),
                metadata.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result == 0 {
            let metadata = unsafe {
                // SAFETY: successful `fstatat` initialized the structure.
                metadata.assume_init()
            };
            return Ok(Some(metadata.st_mode));
        }

        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(error)
        }
    }

    fn ensure_fd_is_regular(fd: RawFd) -> io::Result<()> {
        let mut metadata = MaybeUninit::<libc::stat>::uninit();
        cvt(unsafe {
            // SAFETY: `metadata` points to writable storage and `fd` is open.
            libc::fstat(fd, metadata.as_mut_ptr())
        })?;
        let metadata = unsafe {
            // SAFETY: successful `fstat` initialized the structure.
            metadata.assume_init()
        };
        if metadata.st_mode & libc::S_IFMT == libc::S_IFREG {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "application storage path is not a regular file",
            ))
        }
    }

    fn ensure_regular_not_symlink(mode: libc::mode_t) -> io::Result<()> {
        match mode & libc::S_IFMT {
            libc::S_IFLNK => Err(symlink_error()),
            libc::S_IFREG => Ok(()),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "application storage path is not a regular file",
            )),
        }
    }

    fn ensure_directory_not_symlink(mode: libc::mode_t) -> io::Result<()> {
        match mode & libc::S_IFMT {
            libc::S_IFLNK => Err(symlink_error()),
            libc::S_IFDIR => Ok(()),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "application storage path is not a directory",
            )),
        }
    }

    fn symlink_error() -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "symbolic links are not allowed in application storage",
        )
    }

    fn direct_child_name(path: &Path) -> io::Result<CString> {
        let name = path.file_name().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "storage path must name a direct child",
            )
        })?;
        c_name(name)
    }

    fn temporary_name(path: &Path) -> io::Result<CString> {
        let file_name = path.file_name().unwrap_or_else(|| OsStr::new("document"));
        let mut bytes = Vec::with_capacity(file_name.as_bytes().len() + 64);
        bytes.push(b'.');
        bytes.extend_from_slice(file_name.as_bytes());
        bytes.extend_from_slice(format!(".tmp.{}.{}", process::id(), unix_nanos()).as_bytes());
        c_name(&OsString::from_vec(bytes))
    }

    fn c_name(name: &OsStr) -> io::Result<CString> {
        CString::new(name.as_bytes()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "storage path contains an embedded NUL byte",
            )
        })
    }

    fn cvt(result: libc::c_int) -> io::Result<libc::c_int> {
        if result == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(result)
        }
    }
}

fn write_and_sync(file: &mut File, bytes: &[u8]) -> Result<(), std::io::Error> {
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(not(target_os = "linux"))]
fn temp_write_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "document".into());

    parent.join(format!(
        ".{file_name}.tmp.{}.{}",
        process::id(),
        unix_nanos()
    ))
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

#[cfg(all(not(target_os = "linux"), not(windows)))]
fn replace_file(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    fs::rename(from, to)
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
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
    let result = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
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

    fn test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tundra-storage-{name}-{}-{}",
            process::id(),
            unix_nanos()
        ))
    }

    #[test]
    fn atomic_write_replaces_contents() {
        let directory = test_path("replace");
        let path = directory.join("state.json");

        atomic_write(&path, b"first").expect("initial write");
        atomic_write(&path, b"second").expect("replacement write");

        assert_eq!(fs::read(&path).expect("read replacement"), b"second");
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    #[test]
    fn application_directories_and_files_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let directory = test_path("permissions");
        let path = directory.join("users.json");
        atomic_write(&path, b"{}").expect("write private file");

        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        tighten_file(&path, "test tighten").expect("tighten existing file");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_storage_destination_is_rejected() {
        use std::os::unix::fs::symlink;

        let directory = test_path("symlink");
        fs::create_dir_all(&directory).unwrap();
        let target = directory.join("outside");
        fs::write(&target, b"keep").unwrap();
        let path = directory.join("state.json");
        symlink(&target, &path).unwrap();

        let error = atomic_write(&path, b"replacement").expect_err("symlink must be rejected");
        assert!(error.to_string().contains("symbolic links"));
        assert_eq!(fs::read(&target).unwrap(), b"keep");
        let _ = fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_storage_directory_is_rejected() {
        use std::os::unix::fs::symlink;

        let directory = test_path("symlink-dir");
        let target = test_path("symlink-target");
        fs::create_dir_all(&target).unwrap();
        symlink(&target, &directory).unwrap();

        let error = create_dir(&directory, "test directory").expect_err("symlink must be rejected");
        assert!(error.to_string().contains("symbolic links"));
        let _ = fs::remove_file(&directory);
        let _ = fs::remove_dir_all(target);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn symlinked_ancestor_cannot_redirect_atomic_write() {
        use std::os::unix::fs::symlink;

        let base = test_path("ancestor-symlink");
        let application_root = base.join("application");
        let attacker_root = base.join("attacker");
        fs::create_dir_all(&application_root).unwrap();
        fs::create_dir_all(&attacker_root).unwrap();
        let redirected_component = application_root.join("state");
        symlink(&attacker_root, &redirected_component).unwrap();
        let destination = redirected_component.join("nested").join("users.json");

        let error =
            atomic_write(&destination, b"secret").expect_err("ancestor symlink must be rejected");
        assert!(error.to_string().contains("symbolic links"));
        assert!(
            !attacker_root.join("nested").exists(),
            "write must not create entries below the symlink target"
        );
        let _ = fs::remove_dir_all(base);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn symlinked_ancestor_cannot_be_traversed_when_tightening_file() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let base = test_path("tighten-ancestor-symlink");
        let application_root = base.join("application");
        let attacker_root = base.join("attacker");
        fs::create_dir_all(&application_root).unwrap();
        fs::create_dir_all(&attacker_root).unwrap();
        let attacker_file = attacker_root.join("users.json");
        fs::write(&attacker_file, b"outside").unwrap();
        fs::set_permissions(&attacker_file, fs::Permissions::from_mode(0o644)).unwrap();
        symlink(&attacker_root, application_root.join("state")).unwrap();
        let redirected_file = application_root.join("state").join("users.json");

        let error = tighten_file(&redirected_file, "test tighten")
            .expect_err("ancestor symlink must be rejected");
        assert!(error.to_string().contains("symbolic links"));
        assert_eq!(
            fs::metadata(&attacker_file).unwrap().permissions().mode() & 0o777,
            0o644
        );
        let _ = fs::remove_dir_all(base);
    }
}
