use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::process::{Child, Command, ExitStatus};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;
use workspace_protocol::ProcessResource;

#[cfg(unix)]
#[allow(unsafe_code)]
mod unix;
#[cfg(windows)]
#[allow(unsafe_code)]
mod windows;

#[cfg(unix)]
use unix as native;
#[cfg(target_os = "macos")]
pub use unix::OwnedProcessSession;
#[cfg(target_os = "linux")]
pub(super) use unix::linux_delegated_session;
#[cfg(all(test, unix))]
pub(super) use unix::test_process_exited;
#[cfg(unix)]
pub(super) use unix::{unix_group, unix_session};
#[cfg(windows)]
use windows as native;

pub(super) struct WorkerProcess {
    child: Child,
    tree: Arc<native::WorkerTree>,
    resources: Arc<Mutex<Resources>>,
    retiring: BTreeSet<ProcessResource>,
    retirement: Option<mpsc::Receiver<io::Result<Vec<ProcessResource>>>>,
    cleanup: Option<mpsc::Receiver<io::Result<()>>>,
    cleanup_finished: bool,
}

type Resources = BTreeMap<ProcessResource, (u32, Arc<Mutex<native::ResourceTree>>)>;

impl WorkerProcess {
    pub(super) fn spawn(command: &mut Command) -> io::Result<Self> {
        let (child, tree) = native::WorkerTree::spawn(command)?;
        Ok(Self {
            child,
            tree: Arc::new(tree),
            resources: Arc::new(Mutex::new(BTreeMap::new())),
            retiring: BTreeSet::new(),
            retirement: None,
            cleanup: None,
            cleanup_finished: false,
        })
    }

    pub(super) fn pid(&self) -> u32 {
        self.child.id()
    }

    pub(super) fn root_exited(&mut self) -> io::Result<bool> {
        self.tree.root_exited(&mut self.child)
    }

    #[cfg(unix)]
    pub(super) fn owns_process(&self, pid: u32) -> io::Result<bool> {
        self.tree.owns_pid(pid)
    }

    pub(super) fn register(&mut self, owner: u32, resource: ProcessResource) -> io::Result<()> {
        let mut resources = self
            .resources
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match resources.entry(resource) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((
                    owner,
                    Arc::new(Mutex::new(self.tree.register(
                        &self.child,
                        owner,
                        resource,
                    )?)),
                ));
            }
            std::collections::btree_map::Entry::Occupied(entry) if entry.get().0 != owner => {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            std::collections::btree_map::Entry::Occupied(_) => {}
        }
        Ok(())
    }

    pub(super) fn unregister(&mut self, owner: u32, resource: ProcessResource) -> io::Result<()> {
        let resources = self
            .resources
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some((registered_owner, _)) = resources.get(&resource) {
            if *registered_owner != owner {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            self.retiring.insert(resource);
        }
        Ok(())
    }

    pub(super) fn check_retired(&mut self) -> io::Result<()> {
        if let Some(retirement) = &self.retirement {
            match retirement.try_recv() {
                Ok(Ok(finished)) => {
                    let mut resources = self
                        .resources
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    for id in finished {
                        resources.remove(&id);
                    }
                }
                Ok(Err(error)) => {
                    self.retirement = None;
                    return Err(error);
                }
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
            self.retirement = None;
        }
        if self.retiring.is_empty() {
            return Ok(());
        }
        let retired = {
            let resources = self
                .resources
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.retiring
                .iter()
                .filter_map(|id| resources.get(id).map(|(_, tree)| (*id, tree.clone())))
                .collect::<Vec<_>>()
        };
        let (report, retirement) = mpsc::channel();
        thread::Builder::new()
            .name("watchdog-resource-retirement".into())
            .spawn(move || {
                let result = (|| {
                    let mut finished = Vec::new();
                    for (id, resource) in retired {
                        let mut resource =
                            resource.lock().unwrap_or_else(|error| error.into_inner());
                        if resource.finished()? {
                            finished.push(id);
                        }
                    }
                    Ok(finished)
                })();
                let _ = report.send(result);
            })?;
        self.retiring.clear();
        self.retirement = Some(retirement);
        Ok(())
    }

    pub(super) fn request_stop(&mut self) -> io::Result<()> {
        let result = self.tree.request_stop(&mut self.child);
        let cleanup = self.start_cleanup();
        result.and(cleanup)
    }

    fn start_cleanup(&mut self) -> io::Result<()> {
        if self.cleanup.is_some() || self.cleanup_finished {
            return Ok(());
        }
        let (report, cleanup) = mpsc::channel();
        let tree = self.tree.clone();
        let resources = self.resources.clone();
        thread::Builder::new()
            .name("watchdog-process-cleanup".into())
            .spawn(move || {
                clean_tree(&tree, resources, report);
            })?;
        self.cleanup = Some(cleanup);
        Ok(())
    }

    pub(super) fn poll_cleanup(&mut self) -> io::Result<()> {
        if let Some(cleanup) = &self.cleanup {
            match cleanup.try_recv() {
                Ok(Ok(())) => {
                    self.cleanup_finished = true;
                    self.cleanup = None;
                }
                Ok(Err(error)) => return Err(error),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.cleanup = None;
                    return Err(io::Error::other(
                        "process cleanup stopped before verifying exit",
                    ));
                }
            }
        }
        Ok(())
    }

    pub(super) fn try_reap(&mut self) -> io::Result<Option<ExitStatus>> {
        self.poll_cleanup()?;
        if self.cleanup_finished {
            self.child.try_wait()
        } else {
            Ok(None)
        }
    }
}

fn clean_tree(
    tree: &native::WorkerTree,
    resources: Arc<Mutex<Resources>>,
    report: mpsc::Sender<io::Result<()>>,
) {
    let mut last_error = None;
    loop {
        let mut failure = None;
        let resources_finished = {
            let mut resources = resources.lock().unwrap_or_else(|error| error.into_inner());
            resources.retain(|_, (_, resource)| {
                let mut resource = match resource.try_lock() {
                    Ok(locked) => locked,
                    Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
                    Err(std::sync::TryLockError::WouldBlock) => return true,
                };
                match resource.stop_and_check() {
                    Ok(alive) => alive,
                    Err(error) => {
                        failure.get_or_insert(error);
                        true
                    }
                }
            });
            resources.is_empty()
        };
        match tree.stop_descendants_and_check() {
            Ok(true) if resources_finished => {
                let _ = report.send(Ok(()));
                return;
            }
            Ok(_) => {}
            Err(error) => {
                failure.get_or_insert(error);
            }
        }
        if let Some(error) = failure {
            let message = error.to_string();
            if last_error.as_ref() != Some(&message) {
                last_error = Some(message);
                if report.send(Err(error)).is_err() {
                    return;
                }
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
}
