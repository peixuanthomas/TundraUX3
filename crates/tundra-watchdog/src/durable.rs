//! Crash-durable file primitives shared by journals, run markers and reports.
//!
//! The Windows implementation is the only audited unsafe code in this crate. It
//! calls the documented wide-character file replacement APIs with NUL-terminated
//! paths owned by this function. Both paths are on the same directory/volume.

#[cfg(not(windows))]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMPORARY: AtomicU64 = AtomicU64::new(1);

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let (parent, temporary) = write_temporary(path, bytes)?;
    replace_file(&temporary, path)?;
    sync_parent(parent)
}

/// Creates `path` without ever replacing an existing operation journal.
pub(crate) fn atomic_create_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let (parent, temporary) = write_temporary(path, bytes)?;
    commit_new_file(&temporary, path)?;
    sync_parent(parent)
}

pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    fs::remove_file(path)?;
    if let Some(parent) = path.parent() {
        sync_parent(parent)?;
    }
    Ok(())
}

fn write_temporary<'a>(path: &'a Path, bytes: &[u8]) -> io::Result<(&'a Path, PathBuf)> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = temporary_path(parent, path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    Ok((parent, temporary))
}

fn temporary_path(parent: &Path, path: &Path) -> PathBuf {
    let sequence = NEXT_TEMPORARY.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    parent.join(format!(
        ".{}.tmp.{}.{}.{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("watchdog"),
        std::process::id(),
        nanos,
        sequence
    ))
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    // POSIX rename replaces a same-filesystem destination atomically.
    fs::rename(from, to)
}

#[cfg(not(windows))]
fn commit_new_file(from: &Path, to: &Path) -> io::Result<()> {
    // Creating a hard link is atomic and fails with AlreadyExists rather than
    // overwriting an older, still-unrecovered journal.
    fs::hard_link(from, to)?;
    let _ = fs::remove_file(from);
    Ok(())
}

#[cfg(not(windows))]
fn sync_parent(parent: &Path) -> io::Result<()> {
    File::open(parent)?.sync_all()
}

#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    use std::ptr;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_WRITE_THROUGH, MoveFileExW, ReplaceFileW,
    };

    let from = wide(from);
    let to = wide(to);
    let replaced = if to_path_exists(to.as_ptr()) {
        // SAFETY: `to` and `from` are live NUL-terminated UTF-16 buffers for
        // the duration of this call. Optional pointers are null as required.
        unsafe {
            ReplaceFileW(
                to.as_ptr(),
                from.as_ptr(),
                ptr::null(),
                0,
                ptr::null(),
                ptr::null(),
            )
        }
    } else {
        // SAFETY: both path buffers are valid and NUL-terminated. Omitting
        // REPLACE_EXISTING ensures this branch cannot overwrite a raced writer.
        unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) }
    };
    if replaced == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn commit_new_file(from: &Path, to: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    let from = wide(from);
    let to = wide(to);
    // SAFETY: both buffers are live NUL-terminated UTF-16 paths. Without
    // REPLACE_EXISTING Windows fails if `to` already exists.
    let moved = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if moved == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn to_path_exists(path: *const u16) -> bool {
    use windows_sys::Win32::Storage::FileSystem::{GetFileAttributesW, INVALID_FILE_ATTRIBUTES};
    // SAFETY: callers pass a live NUL-terminated UTF-16 buffer.
    unsafe { GetFileAttributesW(path) != INVALID_FILE_ATTRIBUTES }
}

#[cfg(windows)]
fn sync_parent(_parent: &Path) -> io::Result<()> {
    // MOVE_FILE_WRITE_THROUGH/ReplaceFileW provide the strongest replacement
    // durability available through these APIs; opening directories with std is
    // not supported on Windows.
    Ok(())
}
