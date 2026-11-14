use super::WorkerConnection;
use platform::{DesktopOpen, PlatformError};
use std::io;
use std::sync::mpsc;
use std::time::Duration;
use watchdog::{
    AppCriticality, AppDescriptor, AppId, TaskId, TaskSpec, WatchdogConfig, WatchdogRuntime,
};
use workspace_protocol::{DesktopOperation, NativePath, WorkerId};

const DESKTOP_QUEUE: usize = 8;

pub(super) fn install() -> io::Result<()> {
    platform::install_desktop_open(|operation| {
        let operation = match operation {
            DesktopOpen::Path(path) => DesktopOperation::OpenPath {
                path: NativePath(path),
            },
            DesktopOpen::With { path, application } => DesktopOperation::OpenWith {
                path: NativePath(path),
                application: NativePath(application),
            },
            DesktopOpen::Uri(uri) => DesktopOperation::OpenUri { uri },
        };
        WorkerConnection::current()
            .expect("the guardian connection is installed before desktop forwarding")
            .launch_desktop(operation)
            .map_err(|error| PlatformError::Native {
                operation: "ShellExecuteW",
                message: error.to_string(),
            })
    })
}

pub(super) struct DesktopRelay {
    requests: mpsc::SyncSender<Job>,
    pub(super) replies: mpsc::Receiver<Reply>,
    _watchdog: WatchdogRuntime,
}

struct Job {
    worker: WorkerId,
    request: u64,
    operation: DesktopOperation,
}

pub(super) struct Reply {
    pub(super) worker: WorkerId,
    pub(super) request: u64,
    pub(super) error: Option<String>,
}

impl DesktopRelay {
    pub(super) fn start(paths: &platform::AppPaths) -> io::Result<Self> {
        let reports = paths.logs_path().join("crashes");
        let fallback = paths.temp_path().join("watchdog");
        let (runtime, watchdog) = WatchdogRuntime::start(
            WatchdogConfig::new(
                reports,
                fallback,
                paths.data_path(),
                "tundra-guardian",
                env!("CARGO_PKG_VERSION"),
            )
            .with_unclean_exit_tracking(false),
        )
        .map_err(io::Error::other)?;
        let desktop = watchdog
            .register_app(AppDescriptor::new(
                AppId::from_static("guardian-desktop"),
                "Tundra desktop launch",
                env!("CARGO_PKG_VERSION"),
                AppCriticality::Optional,
            ))
            .map_err(io::Error::other)?;
        let (requests, inbox) = mpsc::sync_channel::<Job>(DESKTOP_QUEUE);
        let (outbox, replies) = mpsc::sync_channel(DESKTOP_QUEUE);
        let group = desktop.task_group("desktop-launches");
        group
            .spawn_thread_with_cancellation(
                TaskSpec::one_shot(TaskId::from_static("launch")),
                move |cancellation| {
                    let platform = platform::native_platform();
                    while !cancellation.is_cancelled() {
                        let job = match inbox.recv_timeout(Duration::from_millis(50)) {
                            Ok(job) => job,
                            Err(mpsc::RecvTimeoutError::Timeout) => continue,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        };
                        let result = match job.operation {
                            DesktopOperation::OpenPath { path } => platform.open_path(&path.0),
                            DesktopOperation::OpenWith { path, application } => {
                                platform.open_with(&path.0, &application.0)
                            }
                            DesktopOperation::OpenUri { uri } => platform.open_uri(&uri),
                        };
                        let _ = outbox.try_send(Reply {
                            worker: job.worker,
                            request: job.request,
                            error: result.err().map(|error| error.to_string()),
                        });
                    }
                },
            )
            .map_err(io::Error::other)?;
        Ok(Self {
            requests,
            replies,
            _watchdog: runtime,
        })
    }

    pub(super) fn enqueue(
        &self,
        worker: WorkerId,
        request: u64,
        operation: DesktopOperation,
    ) -> bool {
        self.requests
            .try_send(Job {
                worker,
                request,
                operation,
            })
            .is_ok()
    }
}
