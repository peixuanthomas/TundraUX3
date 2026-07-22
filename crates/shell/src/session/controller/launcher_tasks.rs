use super::super::*;
#[derive(Debug)]
pub(in crate::session) enum LauncherRefreshEvent {
    ItemChecked {
        request_id: u64,
        id: String,
        result: Result<LauncherItemStatus, String>,
    },
    Finished {
        request_id: u64,
        error: Option<String>,
    },
}

pub(in crate::session) struct ShellLauncherTaskShared {
    pub(in crate::session) platform: std::sync::Arc<dyn Platform>,
    pub(in crate::session) task_group: watchdog::ManagedTaskGroup,
    pub(in crate::session) event_tx: std::sync::mpsc::Sender<LauncherRefreshEvent>,
    pub(in crate::session) event_rx:
        std::sync::Mutex<std::sync::mpsc::Receiver<LauncherRefreshEvent>>,
    pub(in crate::session) workers:
        std::sync::Mutex<std::collections::BTreeMap<u64, watchdog::ManagedThreadHandle<()>>>,
    pub(in crate::session) next_request_id: std::sync::atomic::AtomicU64,
}

impl Drop for ShellLauncherTaskShared {
    fn drop(&mut self) {
        if let Ok(workers) = self.workers.get_mut() {
            for worker in workers.values() {
                worker.cancel();
            }
        }
    }
}

/// Cloneable handle for full Launcher integrity scans performed away from the
/// terminal event/render thread.
#[derive(Clone)]
pub(in crate::session) struct ShellLauncherTaskRuntime {
    pub(in crate::session) shared: std::sync::Arc<ShellLauncherTaskShared>,
}

impl std::fmt::Debug for ShellLauncherTaskRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShellLauncherTaskRuntime")
            .finish_non_exhaustive()
    }
}

impl PartialEq for ShellLauncherTaskRuntime {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for ShellLauncherTaskRuntime {}

pub(in crate::session) static NEXT_LAUNCHER_RUNTIME_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);
pub(in crate::session) const MAX_CONCURRENT_LAUNCHER_REFRESHES: usize = 2;

impl ShellLauncherTaskRuntime {
    pub(in crate::session) fn new_managed(
        platform: std::sync::Arc<dyn Platform>,
        watchdog: watchdog::AppWatchdog,
    ) -> Self {
        use std::sync::atomic::Ordering;

        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let runtime_id = NEXT_LAUNCHER_RUNTIME_ID
            .fetch_add(1, Ordering::Relaxed)
            .max(1);
        Self {
            shared: std::sync::Arc::new(ShellLauncherTaskShared {
                platform,
                task_group: watchdog.task_group(&format!("launcher-integrity-{runtime_id}")),
                event_tx,
                event_rx: std::sync::Mutex::new(event_rx),
                workers: std::sync::Mutex::new(std::collections::BTreeMap::new()),
                next_request_id: std::sync::atomic::AtomicU64::new(1),
            }),
        }
    }

    pub(in crate::session) fn submit(
        &self,
        entries: Vec<storage::LauncherEntryRecord>,
    ) -> Result<u64, String> {
        use std::sync::atomic::Ordering;

        let mut workers = self
            .shared
            .workers
            .lock()
            .map_err(|_| "Launcher refresh task registry is unavailable".to_string())?;
        if workers.len() >= MAX_CONCURRENT_LAUNCHER_REFRESHES {
            return Err("Previous Launcher refreshes are still finishing".to_string());
        }
        let request_id = self
            .shared
            .next_request_id
            .fetch_add(1, Ordering::Relaxed)
            .max(1);
        let task_id = watchdog::TaskId::new(format!("refresh-{}", request_id % 64))
            .map_err(|error| format!("invalid Launcher refresh task: {error}"))?;
        let platform = std::sync::Arc::clone(&self.shared.platform);
        let events = self.shared.event_tx.clone();
        let worker = self
            .shared
            .task_group
            .spawn_thread(watchdog::TaskSpec::one_shot(task_id), move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    for entry in &entries {
                        let result = app::launcher::verify_launcher_entry(entry, platform.as_ref())
                            .map_err(|error| error.to_string());
                        if events
                            .send(LauncherRefreshEvent::ItemChecked {
                                request_id,
                                id: entry.id.clone(),
                                result,
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                }));
                match result {
                    Ok(()) => {
                        let _ = events.send(LauncherRefreshEvent::Finished {
                            request_id,
                            error: None,
                        });
                    }
                    Err(payload) => {
                        let _ = events.send(LauncherRefreshEvent::Finished {
                            request_id,
                            error: Some("Launcher refresh worker panicked".to_string()),
                        });
                        std::panic::resume_unwind(payload);
                    }
                }
            })
            .map_err(|error| format!("Could not start Launcher refresh: {error}"))?;
        workers.insert(request_id, worker);
        Ok(request_id)
    }

    pub(in crate::session) fn drain_events(&self) -> Vec<LauncherRefreshEvent> {
        let Ok(receiver) = self.shared.event_rx.lock() else {
            return Vec::new();
        };
        let events = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
        drop(receiver);
        if let Ok(mut workers) = self.shared.workers.lock() {
            for event in &events {
                if let LauncherRefreshEvent::Finished { request_id, .. } = event {
                    workers.remove(request_id);
                }
            }
        }
        events
    }
}

impl ShellSession {
    pub(in crate::session) fn poll_launcher_background_tasks(&mut self) {
        let events = self
            .launcher_task_runtime
            .as_ref()
            .map(ShellLauncherTaskRuntime::drain_events)
            .unwrap_or_default();
        for event in events {
            match event {
                LauncherRefreshEvent::ItemChecked {
                    request_id,
                    id,
                    result,
                } if self.launcher_refresh_request == Some(request_id) => {
                    self.update_launcher_state(|state| match result {
                        Ok(status) => state.set_item_status(&id, status),
                        Err(error) => {
                            state.set_item_status(&id, LauncherItemStatus::Unsupported);
                            state.error = Some(error);
                        }
                    });
                }
                LauncherRefreshEvent::Finished { request_id, error }
                    if self.launcher_refresh_request == Some(request_id) =>
                {
                    self.launcher_refresh_request = None;
                    self.update_launcher_state(|state| {
                        if let Some(error) = error {
                            state.error = Some(error);
                        } else if state.error.is_none() {
                            state.message = Some("Launcher refresh complete".to_string());
                        }
                    });
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod launcher_task_workflow_tests {
    use super::*;

    #[test]
    fn managed_refresh_publishes_full_integrity_results() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tundra-launcher-refresh-{}-{unique}",
            std::process::id()
        ));
        let documents = root.join("Documents");
        std::fs::create_dir_all(&documents).expect("test documents");
        let executable = documents.join("program.exe");
        std::fs::write(&executable, b"approved content").expect("test executable");
        let executable = std::fs::canonicalize(executable).expect("canonical executable");
        let metadata = std::fs::metadata(&executable).expect("executable metadata");

        let app_paths = platform::build_windows_app_paths(
            root.join("Roaming"),
            root.join("Local"),
            root.join("Temp"),
        )
        .expect("test app paths");
        let user_dirs = platform::UserDirs::new(
            root.join("Desktop"),
            documents,
            root.join("Downloads"),
            root.join("Pictures"),
            root.join("Videos"),
            root.join("Music"),
            root.join("Roaming"),
        )
        .expect("test user dirs");
        let platform = platform::mock::MockPlatform::new(user_dirs, app_paths)
            .with_kind(platform::PlatformKind::Windows);
        platform.set_file_attributes(
            executable.clone(),
            platform::FileAttributes {
                path: executable.clone(),
                is_file: true,
                is_dir: false,
                len: metadata.len(),
                readonly: false,
                modified: metadata.modified().ok(),
                hidden: false,
                system: false,
                archive: false,
                symlink: false,
                junction: false,
                reparse_point: false,
                shortcut: false,
            },
        );
        platform.set_file_open_policy(
            executable.clone(),
            platform::FileOpenPolicy::launcher_required(
                platform::ExecutableKind::NativeBinary,
                "test policy",
            ),
        );
        let fingerprint =
            app::launcher::fingerprint_file(&executable).expect("approved fingerprint");
        let entry = storage::LauncherEntryRecord {
            id: "program".to_string(),
            path: executable.to_string_lossy().into_owned(),
            executable_kind: Some(storage::LauncherExecutableKind::NativeBinary),
            fingerprint: Some(fingerprint),
            added_by_user_id: "admin".to_string(),
            added_at_epoch_ms: 0,
        };
        let watchdog = default_editor_watchdog().expect("test watchdog");
        let runtime =
            ShellLauncherTaskRuntime::new_managed(std::sync::Arc::new(platform), watchdog);

        let request_id = runtime.submit(vec![entry]).expect("submit refresh");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut status = None;
        let mut finished = false;
        while std::time::Instant::now() < deadline && !finished {
            for event in runtime.drain_events() {
                match event {
                    LauncherRefreshEvent::ItemChecked {
                        request_id: event_request_id,
                        id,
                        result,
                    } => {
                        assert_eq!(event_request_id, request_id);
                        assert_eq!(id, "program");
                        status = Some(result.expect("integrity result"));
                    }
                    LauncherRefreshEvent::Finished {
                        request_id: event_request_id,
                        error,
                    } => {
                        assert_eq!(event_request_id, request_id);
                        assert_eq!(error, None);
                        finished = true;
                    }
                }
            }
            if !finished {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        assert!(finished, "Launcher refresh did not finish");
        assert_eq!(status, Some(LauncherItemStatus::Ready));
        drop(runtime);
        platform::cleanup_temp_path(&root).expect("cleanup test fixture");
    }
}
