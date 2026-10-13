use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Shared by the UI and its watchdog worker. Cancellation stays observable
/// after a result was queued, so a late success cannot start a replacement.
#[derive(Clone, Default)]
pub struct UpdateCancellation {
    requested: Arc<AtomicBool>,
    watchdog: Option<watchdog::ThreadCancellation>,
}

impl UpdateCancellation {
    pub fn cancel(&self) {
        self.requested.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.requested.load(Ordering::Acquire)
            || self
                .watchdog
                .as_ref()
                .is_some_and(|token| token.is_cancelled())
    }
    pub fn with_watchdog(&self, watchdog: watchdog::ThreadCancellation) -> Self {
        Self {
            requested: self.requested.clone(),
            watchdog: Some(watchdog),
        }
    }
    pub fn check(&self) -> Result<(), UpdateError> {
        if self.is_cancelled() {
            Err(UpdateError::cancelled())
        } else {
            Ok(())
        }
    }
}

pub(super) fn copy_stream(
    input: &mut impl Read,
    output: &mut impl std::io::Write,
    cancel: &UpdateCancellation,
) -> Result<u64, UpdateError> {
    let mut buffer = [0; 64 * 1024];
    let mut copied = 0;
    loop {
        cancel.check()?;
        let read = input.read(&mut buffer);
        cancel.check()?;
        let size = read?;
        if size == 0 {
            return Ok(copied);
        }
        output.write_all(&buffer[..size])?;
        copied += size as u64;
    }
}

pub(super) struct CancellableReader<'a, R>(pub R, pub &'a UpdateCancellation);

impl<R: Read> Read for CancellableReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.1.check().map_err(io::Error::other)?;
        self.0.read(buffer)
    }
}

impl<R: io::Seek> io::Seek for CancellableReader<'_, R> {
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        self.1.check().map_err(io::Error::other)?;
        self.0.seek(position)
    }
}

#[cfg(any(windows, target_os = "linux"))]
pub(super) fn copy_file(
    source: &Path,
    target: &Path,
    cancel: &UpdateCancellation,
) -> Result<(), UpdateError> {
    cancel.check()?;
    let mut input = File::open(source)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)?;
    copy_stream(&mut input, &mut output, cancel)?;
    output.sync_all()?;
    fs::set_permissions(target, input.metadata()?.permissions())?;
    cancel.check()
}

/// A guard owns only a directory freshly created by this operation. It never
/// derives a cleanup target from a user supplied source path or a shared root.
pub(super) struct WorkDirectory {
    pub(super) path: PathBuf,
    retained: bool,
}

impl WorkDirectory {
    pub(super) fn new(path: PathBuf) -> Self {
        Self {
            path,
            retained: false,
        }
    }
    pub(super) fn retain(&mut self) {
        self.retained = true;
    }
    pub(super) fn finish<T>(&mut self, result: Result<T, UpdateError>) -> Result<T, UpdateError> {
        match result {
            Ok(value) => {
                self.retain();
                Ok(value)
            }
            Err(error) => {
                self.retained = true;
                match cleanup_owned_directory(&self.path) {
                    Ok(()) => Err(error),
                    Err(cleanup) => Err(error.with_cleanup_failure(cleanup)),
                }
            }
        }
    }
}

impl Drop for WorkDirectory {
    fn drop(&mut self) {
        if !self.retained {
            let _ = cleanup_owned_directory(&self.path);
        }
    }
}

pub fn cleanup_update_work(path: &Path) -> Result<(), UpdateError> {
    cleanup_owned_directory(path)
}

fn cleanup_owned_directory(path: &Path) -> Result<(), UpdateError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(UpdateError::new(format!(
                "Could not inspect update files; retained {}: {error}",
                path.display()
            )));
        }
        Ok(_) => {}
    }
    platform::validate_no_follow_path(path, true).map_err(|error| {
        UpdateError::new(format!(
            "Could not clean update files; retained {}: {error}",
            path.display()
        ))
    })?;
    fs::remove_dir_all(path).map_err(|error| {
        UpdateError::new(format!(
            "Could not clean update files; retained {}: {error}",
            path.display()
        ))
    })
}

pub fn discard_staged_update(manifest_path: &Path) -> Result<(), UpdateError> {
    discard_staged_update_with_cleanup(manifest_path, &cleanup_owned_directory)
}

pub(super) fn discard_staged_update_with_cleanup(
    manifest_path: &Path,
    cleanup: &dyn Fn(&Path) -> Result<(), UpdateError>,
) -> Result<(), UpdateError> {
    let transaction_dir = mark_staged_update_cancelled(manifest_path)?;
    cleanup(&transaction_dir)
}

/// Save the short cancellation journal before scheduling potentially expensive
/// cleanup, including when the watchdog can no longer start a cleanup worker.
pub fn mark_staged_update_cancelled(manifest_path: &Path) -> Result<PathBuf, UpdateError> {
    let mut manifest = load_manifest(manifest_path).map_err(|error| {
        error
            .context(&format!(
                "Could not cancel update; retained {}",
                manifest_path.display()
            ))
            .with_retained_staging(manifest_path)
    })?;
    if !matches!(
        manifest.state,
        TransactionState::Prepared | TransactionState::Cancelled
    ) || manifest.cli_replaced
        || manifest.shell_replaced
    {
        return Err(UpdateError::new(format!(
            "Replacement has already started; retained {}",
            manifest.transaction_dir.display()
        ))
        .with_retained_staging(manifest_path));
    }
    // Persist cancellation before deleting any payload. A failed cleanup must
    // never make startup recovery apply this transaction again.
    manifest.state = TransactionState::Cancelled;
    write_manifest(manifest_path, &manifest).map_err(|error| {
        error
            .context(&format!(
                "Could not persist cancellation; retained {}",
                manifest_path.display()
            ))
            .with_retained_staging(manifest_path)
    })?;
    Ok(manifest.transaction_dir)
}
