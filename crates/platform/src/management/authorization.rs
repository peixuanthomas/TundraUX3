//! Session authorization is an inherited connection, never a saved password.
use crate::linux::privilege_session::{Client, Request};
use crate::management::ManagementError;
use crate::service::ServiceError;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use zeroize::Zeroizing;

#[derive(Clone, Default)]
pub struct PrivilegeSession(Arc<State>);
#[derive(Default)]
struct State {
    session: Mutex<Option<Authorized>>,
    revoker: Mutex<Option<UnixStream>>,
    revoked: AtomicBool,
}
struct Authorized {
    client: Client,
    child: Child,
}
impl Drop for Authorized {
    fn drop(&mut self) {
        if let Ok(stream) = self.client.revoke_handle() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        // Only reap our broker; independently launched tasks remain alive.
        for _ in 0..50 {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}
impl std::fmt::Debug for PrivilegeSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrivilegeSession")
    }
}
impl PartialEq for PrivilegeSession {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for PrivilegeSession {}

impl PrivilegeSession {
    pub fn revoke(&self) {
        self.0.revoked.store(true, Ordering::Release);
        if let Some(stream) = self
            .0
            .revoker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }

    pub fn ensure(
        &self,
        mut password: impl FnMut() -> Result<Zeroizing<String>, ManagementError>,
    ) -> Result<(), ManagementError> {
        let mut session = self.0.session.lock().unwrap_or_else(|e| e.into_inner());
        if self.0.revoked.load(Ordering::Acquire) {
            return Err(ManagementError::Cancelled);
        }
        if session.is_some() {
            return Ok(());
        }
        // Same-user ptrace/proc-fd access must not expose the retained capability.
        if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
            return Err(ManagementError::PermissionDenied(
                "Cannot protect authorization session".into(),
            ));
        }
        match self.start(None) {
            Ok(authorized) => {
                *session = Some(authorized);
                return Ok(());
            }
            Err(ManagementError::PermissionDenied(_)) => {}
            Err(error) => return Err(error),
        }
        for _ in 0..3 {
            let secret = password()?;
            if self.0.revoked.load(Ordering::Acquire) {
                return Err(ManagementError::Cancelled);
            }
            let result = self.start(Some(&secret));
            drop(secret);
            if let Ok(authorized) = result {
                *session = Some(authorized);
                return Ok(());
            }
        }
        Err(ManagementError::PermissionDenied(
            "System authorization failed".into(),
        ))
    }

    fn start(&self, password: Option<&str>) -> Result<Authorized, ManagementError> {
        let failure = |error: std::io::Error| ManagementError::Failed(error.to_string());
        let actor = unsafe { libc::getuid() };
        let executable = std::env::current_exe()
            .map_err(failure)?
            .with_file_name("tundra-cli");
        if !executable.is_file() {
            return Err(ManagementError::Unavailable(
                "tundra-cli must be beside tundra-shell".into(),
            ));
        }
        let (channel, broker) = UnixStream::pair().map_err(failure)?;
        let mut launch = if actor == 0 {
            Command::new(&executable)
        } else {
            let mut launch = Command::new("/usr/bin/sudo");
            // -k with a command neither reads nor refreshes the shared sudo
            // timestamp. Authorization cannot spill into unrelated processes.
            launch.args(if password.is_some() {
                ["-S", "-k"]
            } else {
                ["-n", "-k"]
            });
            launch.args(["-p", "", "--"]).arg(&executable);
            launch
        };
        launch
            .arg("__privilege-session")
            .arg(actor.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::from(OwnedFd::from(broker)))
            .stderr(Stdio::null());
        {
            let mut revoker = self.0.revoker.lock().unwrap_or_else(|e| e.into_inner());
            if self.0.revoked.load(Ordering::Acquire) {
                return Err(ManagementError::Cancelled);
            }
            *revoker = Some(channel.try_clone().map_err(failure)?);
        }
        let mut child = launch.spawn().map_err(failure)?;
        let result = (|| {
            if let Some(mut stdin) = child.stdin.take() {
                if let Some(password) = password {
                    // Closing stdin limits sudo to this one attempt; the protocol
                    // uses the separate stdout socket after authentication.
                    stdin.write_all(password.as_bytes()).map_err(failure)?;
                    stdin.write_all(b"\n").map_err(failure)?;
                }
            }
            Client::new(channel)
        })();
        match result {
            Ok(client) if !self.0.revoked.load(Ordering::Acquire) => {
                Ok(Authorized { client, child })
            }
            result => {
                drop(result);
                if let Some(stream) = self
                    .0
                    .revoker
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                {
                    let _ = stream.shutdown(std::net::Shutdown::Both);
                }
                for _ in 0..50 {
                    if !matches!(child.try_wait(), Ok(None)) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(ManagementError::PermissionDenied(
                    "Authorization failed or was cancelled".into(),
                ))
            }
        }
    }

    pub fn connect(&self, request: Request) -> Result<UnixStream, ManagementError> {
        let mut session = self.0.session.lock().unwrap_or_else(|e| e.into_inner());
        if self.0.revoked.load(Ordering::Acquire) {
            return Err(ManagementError::Cancelled);
        }
        let result = session
            .as_mut()
            .ok_or(ManagementError::Cancelled)?
            .client
            .connect(request);
        if result
            .as_ref()
            .is_err_and(|error| !matches!(error, ManagementError::Conflict(_)))
        {
            self.revoke();
        }
        result
    }
    pub fn execute(&self, request: Request) -> Result<(), ServiceError> {
        let mut session = self.0.session.lock().unwrap_or_else(|e| e.into_inner());
        if self.0.revoked.load(Ordering::Acquire) {
            return Err(ServiceError::AuthorizationCancelled);
        }
        let result = session
            .as_mut()
            .ok_or(ServiceError::AuthorizationCancelled)?
            .client
            .execute(request);
        match result {
            Ok(result) => result,
            Err(_) => {
                self.revoke();
                Err(ServiceError::BackendDisconnected)
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/management/authorization_tests.rs"]
mod tests;
