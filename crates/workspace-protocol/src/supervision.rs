use super::{Hello, NativePath, RunId, SessionId, WorkerId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessState {
    Starting,
    Running,
    Unresponsive,
    Closing,
    ForceStopping,
    Exited { code: Option<i32>, forced: bool },
}

/// A child-owned process tree. The guardian validates ownership before admitting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessResource {
    UnixSession {
        leader_pid: u32,
        birth: u64,
    },
    UnixGroup {
        leader_pid: u32,
        process_group: u32,
        birth: u64,
    },
    LinuxDelegatedSession {
        leader_pid: u32,
        birth: u64,
    },
    WindowsJob {
        handle: u64,
        generation: u64,
    },
}

pub const MAX_RESOURCE_FRAME_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceToGuardian {
    Hello(Hello),
    Register {
        request: u64,
        resource: ProcessResource,
    },
    Unregister(ProcessResource),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianToResource {
    Welcome { worker: WorkerId },
    Registered { request: u64, accepted: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DesktopOperation {
    OpenPath {
        path: NativePath,
    },
    OpenWith {
        path: NativePath,
        application: NativePath,
    },
    OpenUri {
        uri: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorToWorker {
    Welcome {
        worker: WorkerId,
    },
    Ping {
        nonce: u64,
    },
    Close,
    Exit,
    AppStarted {
        worker: WorkerId,
        pid: u32,
        endpoint: String,
    },
    AppState {
        worker: WorkerId,
        state: ProcessState,
    },
    CleanupError {
        worker: WorkerId,
        error: String,
    },
    ServicesStarted {
        worker: WorkerId,
        pid: u32,
        endpoint: String,
    },
    RegisterWorker {
        worker: WorkerId,
        pid: u32,
    },
    UnregisterWorker {
        worker: WorkerId,
    },
    ResourceRegistered {
        request: u64,
        accepted: bool,
    },
    DesktopLaunched {
        request: u64,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerToSupervisor {
    Hello(Hello),
    Ready,
    /// Sent after an iteration on the worker's actual event-loop thread.
    Progress {
        counter: u64,
    },
    /// Carries that thread's current progress counter and the last ping nonce.
    Pong {
        nonce: u64,
        counter: u64,
    },
    Exiting {
        code: i32,
    },
    Finished(ShellCompletion),
    SpawnApp {
        session: SessionId,
        application: String,
    },
    CloseApp {
        worker: WorkerId,
    },
    AppClosing,
    ForceKill {
        worker: WorkerId,
    },
    Registered {
        worker: WorkerId,
    },
    RegisterResource {
        request: u64,
        resource: ProcessResource,
    },
    UnregisterResource {
        resource: ProcessResource,
    },
    LaunchDesktop {
        request: u64,
        operation: DesktopOperation,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellCompletion {
    Exit,
    Restart,
    Reset,
    UpdatePrepared(#[serde(with = "super::service::native_path")] PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchdogRequest {
    pub run: RunId,
    pub pid: u32,
    pub protocol: u32,
    pub action: WatchdogAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogAction {
    List,
    Kill { worker: WorkerId },
}

pub type WatchdogResponse = Result<WatchdogReply, WatchdogError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogReply {
    List(Vec<SessionInfo>),
    Killed { worker: WorkerId, code: Option<i32> },
    KillRequested { worker: WorkerId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionInfo {
    pub worker: WorkerId,
    pub pid: u32,
    pub application: String,
    pub state: ProcessState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogError {
    WrongRun,
    Version,
    NotFound,
    Unavailable,
}
