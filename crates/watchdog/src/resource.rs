//! Resources that must be stopped before waiting for background workers.
use crate::{AppWatchdog, ProcessWatchdog, WatchdogError};
use std::collections::BTreeMap;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

type Cleanup = Box<dyn FnMut() -> io::Result<()> + Send>;

#[derive(Default)]
pub(crate) struct Resources {
    next: AtomicU64,
    entries: Mutex<BTreeMap<u64, Arc<Resource>>>,
}

struct Resource {
    name: String,
    process: Weak<crate::runtime::Shared>,
    cleanup: Mutex<Option<Cleanup>>,
}

impl Resource {
    fn stop(&self) -> io::Result<()> {
        // Serialize shutdown and owner-drop. Retry failures, but never run a
        // successful cleanup twice. A cleanup panic must not bring down UX.
        let mut cleanup = self.cleanup.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(stop) = cleanup.as_mut() {
            if let Some(shared) = self.process.upgrade() {
                ProcessWatchdog { shared }.run_resource_cleanup(stop)?;
            } else {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(stop))
                    .unwrap_or_else(|_| Err(io::Error::other("resource cleanup panicked")))?;
            }
            cleanup.take();
        }
        Ok(())
    }
}

impl Resources {
    pub(crate) fn stop(&self) -> Vec<String> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        entries
            .into_iter()
            .rev()
            .filter_map(|(id, entry)| match entry.stop() {
                Ok(()) => {
                    self.entries
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&id);
                    None
                }
                Err(error) => Some(format!("{}: {error}", entry.name)),
            })
            .collect()
    }
}

/// The owner stops its resource on drop. Process shutdown does the same even
/// when the owner is still alive. Cleanup must not panic or wait on a worker
/// that itself needs watchdog shutdown to complete.
pub struct ManagedResource {
    id: u64,
    resource: Arc<Resource>,
    process: Weak<crate::runtime::Shared>,
}

impl ManagedResource {
    /// Consult even a stopping runtime so shutdown cannot start untracked work.
    /// Early startup callers must still own and clean up the resource locally.
    pub fn register_current(
        name: impl Into<String>,
        cleanup: impl FnMut() -> io::Result<()> + Send + 'static,
    ) -> Result<Option<Self>, WatchdogError> {
        crate::runtime::resource_process()
            .map(|process| process.manage_resource(name, cleanup))
            .transpose()
    }

    pub fn stop(&self) -> io::Result<()> {
        self.resource.stop()
    }
}

impl Drop for ManagedResource {
    fn drop(&mut self) {
        let result = self.stop();
        if let Some(shared) = self.process.upgrade() {
            match result {
                Ok(()) => {
                    shared
                        .resources
                        .entries
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&self.id);
                }
                Err(error) => {
                    ProcessWatchdog { shared }.report_resource_failure(&self.resource.name, &error)
                }
            }
        }
    }
}

impl ProcessWatchdog {
    /// Register before publishing a newly created resource to another owner.
    /// On rejection the caller remains responsible for cleaning it up.
    pub fn manage_resource(
        &self,
        name: impl Into<String>,
        cleanup: impl FnMut() -> io::Result<()> + Send + 'static,
    ) -> Result<ManagedResource, WatchdogError> {
        let mut entries = self
            .shared
            .resources
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !self.shared.running.load(Ordering::Acquire) {
            return Err(WatchdogError::RuntimeStopped);
        }
        let id = self.shared.resources.next.fetch_add(1, Ordering::Relaxed);
        let resource = Arc::new(Resource {
            name: name.into(),
            process: Arc::downgrade(&self.shared),
            cleanup: Mutex::new(Some(Box::new(cleanup))),
        });
        entries.insert(id, resource.clone());
        Ok(ManagedResource {
            id,
            resource,
            process: Arc::downgrade(&self.shared),
        })
    }
}

impl AppWatchdog {
    pub fn manage_resource(
        &self,
        name: impl Into<String>,
        cleanup: impl FnMut() -> io::Result<()> + Send + 'static,
    ) -> Result<ManagedResource, WatchdogError> {
        self.process.manage_resource(
            format!("{}/{}", self.component_path(), name.into()),
            cleanup,
        )
    }
}
