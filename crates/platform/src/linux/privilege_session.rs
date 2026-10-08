//! A Shell-owned authorization session. No password, token file or public socket
//! is retained. Only the inherited private socket can issue fixed requests.
use crate::management::{HelperReady, ManagementCommand, ManagementError, helper};
use crate::service::ServiceError;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use zeroize::{Zeroize, Zeroizing};

const MAX_REQUEST: usize = 2 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
pub enum AccountOperation {
    Rename {
        username: String,
        display_name: String,
    },
    Password {
        username: String,
        password: String,
    },
    Locked {
        username: String,
        locked: bool,
    },
    Admin {
        username: String,
        admin: bool,
    },
    Delete {
        username: String,
    },
    Create {
        username: String,
        display_name: String,
        admin: bool,
        password: String,
    },
}
impl Drop for AccountOperation {
    fn drop(&mut self) {
        match self {
            Self::Password { password, .. } | Self::Create { password, .. } => password.zeroize(),
            _ => {}
        }
    }
}

#[derive(Serialize, Deserialize)]
pub enum Request {
    Start(ManagementCommand),
    Attach(PathBuf),
    Account(AccountOperation),
    Power { reboot: bool },
}

#[derive(Serialize, Deserialize)]
enum Response {
    Connected,
    Done,
    Failed(String),
    ServiceFailed(ServiceError),
}

fn failure(error: impl std::fmt::Display) -> ManagementError {
    ManagementError::Failed(error.to_string())
}

fn write_packet<T: Serialize>(stream: &mut UnixStream, value: &T) -> Result<(), ManagementError> {
    let bytes = Zeroizing::new(serde_json::to_vec(value).map_err(failure)?);
    if bytes.len() > MAX_REQUEST {
        return Err(ManagementError::InvalidInput(
            "Oversized session request".into(),
        ));
    }
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .map_err(failure)?;
    stream.write_all(&bytes).map_err(failure)
}

fn read_packet<T: serde::de::DeserializeOwned>(
    stream: &mut UnixStream,
) -> Result<T, ManagementError> {
    let mut size = [0; 4];
    stream.read_exact(&mut size).map_err(failure)?;
    let size = u32::from_be_bytes(size) as usize;
    if size == 0 || size > MAX_REQUEST {
        return Err(ManagementError::InvalidInput(
            "Oversized session request".into(),
        ));
    }
    let mut bytes = Zeroizing::new(vec![0; size]);
    stream.read_exact(&mut bytes).map_err(failure)?;
    // Do not echo malformed input: it may contain a new account password.
    serde_json::from_slice(&bytes)
        .map_err(|_| ManagementError::InvalidInput("Invalid session request".into()))
}

// SCM_RIGHTS transfers a connected descriptor, not a pathname another process
// can open. The receiver marks it close-on-exec before making it available.
fn send_connection(channel: &UnixStream, stream: &UnixStream) -> Result<(), ManagementError> {
    let mut byte = b'F';
    let mut iov = libc::iovec {
        iov_base: (&mut byte as *mut u8).cast(),
        iov_len: 1,
    };
    let mut control = [0usize; 8];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = unsafe { libc::CMSG_SPACE(std::mem::size_of::<i32>() as u32) } as usize;
    unsafe {
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<i32>() as u32) as usize;
        std::ptr::write_unaligned(libc::CMSG_DATA(cmsg).cast::<i32>(), stream.as_raw_fd());
        if libc::sendmsg(channel.as_raw_fd(), &msg, libc::MSG_NOSIGNAL) != 1 {
            return Err(failure(std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

fn receive_connection(channel: &UnixStream) -> Result<UnixStream, ManagementError> {
    let mut byte = 0u8;
    let mut iov = libc::iovec {
        iov_base: (&mut byte as *mut u8).cast(),
        iov_len: 1,
    };
    let mut control = [0usize; 8];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = std::mem::size_of_val(&control);
    let count = unsafe { libc::recvmsg(channel.as_raw_fd(), &mut msg, libc::MSG_CMSG_CLOEXEC) };
    let mut descriptors = Vec::new();
    unsafe {
        let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_SOCKET && (*cmsg).cmsg_type == libc::SCM_RIGHTS {
                let length = (*cmsg).cmsg_len.saturating_sub(libc::CMSG_LEN(0) as usize);
                for i in 0..length / std::mem::size_of::<i32>() {
                    let fd = std::ptr::read_unaligned(libc::CMSG_DATA(cmsg).cast::<i32>().add(i));
                    descriptors.push(OwnedFd::from_raw_fd(fd));
                }
            }
            cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
        }
    }
    if count != 1 || byte != b'F' || msg.msg_flags & libc::MSG_CTRUNC != 0 || descriptors.len() != 1
    {
        return Err(failure("Invalid session connection"));
    }
    Ok(UnixStream::from(descriptors.pop().unwrap()))
}

pub struct Client {
    channel: UnixStream,
}
impl Client {
    /// Called only after the sudo-started broker sends its ready response.
    pub fn new(mut channel: UnixStream) -> Result<Self, ManagementError> {
        channel
            .set_read_timeout(Some(Duration::from_secs(60)))
            .map_err(failure)?;
        channel
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(failure)?;
        if !matches!(read_packet::<Response>(&mut channel)?, Response::Done) {
            return Err(failure("Authorization session did not start"));
        }
        Ok(Self { channel })
    }
    pub fn revoke_handle(&self) -> Result<UnixStream, ManagementError> {
        self.channel.try_clone().map_err(failure)
    }
    pub fn connect(&mut self, request: Request) -> Result<UnixStream, ManagementError> {
        if !matches!(request, Request::Start(_) | Request::Attach(_)) {
            return Err(failure("Not a connection request"));
        }
        write_packet(&mut self.channel, &request)?;
        match read_packet::<Response>(&mut self.channel)? {
            Response::Connected => receive_connection(&self.channel),
            // A missing/expired task is an ordinary operation failure, not a
            // broken authorization channel. Keep the session usable.
            Response::Failed(error) => Err(ManagementError::Conflict(error)),
            _ => Err(failure("Invalid connection response")),
        }
    }
    pub fn execute(
        &mut self,
        request: Request,
    ) -> Result<Result<(), ServiceError>, ManagementError> {
        write_packet(&mut self.channel, &request)?;
        match read_packet::<Response>(&mut self.channel)? {
            Response::Done => Ok(Ok(())),
            Response::ServiceFailed(error) => Ok(Err(error)),
            _ => Err(failure("Invalid operation response")),
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.channel.shutdown(std::net::Shutdown::Both);
    }
}

fn start_operation(command: &ManagementCommand, actor: u32) -> Result<UnixStream, ManagementError> {
    // The portable installation directory is user-writable. Re-execute the
    // already authorized running image, never a replaceable filesystem path.
    let mut child = Command::new("/proc/self/exe")
        .args(["__system-helper", &actor.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(failure)?;
    let mut request = Zeroizing::new(serde_json::to_vec(command).map_err(failure)?);
    request.push(b'\n');
    child
        .stdin
        .take()
        .ok_or_else(|| failure("Missing helper input"))?
        .write_all(&request)
        .map_err(failure)?;
    let output = child.wait_with_output().map_err(failure)?;
    if !output.status.success() {
        return Err(failure("Operation helper failed to start"));
    }
    let ready: HelperReady = serde_json::from_slice(&output.stdout).map_err(failure)?;
    helper::check_attach_path(&ready.socket, actor)?;
    helper::connect(&ready.socket, 0)
}

fn account_operation(actor: u32, operation: &AccountOperation) -> Result<(), ServiceError> {
    let accounts = super::accounts::Accounts::authorized_actor(actor)?;
    match operation {
        AccountOperation::Rename {
            username,
            display_name,
        } => accounts.rename(username, display_name),
        AccountOperation::Password { username, password } => accounts.password(username, password),
        AccountOperation::Locked { username, locked } => accounts.set_locked(username, *locked),
        AccountOperation::Admin { username, admin } => accounts.set_admin(username, *admin),
        AccountOperation::Delete { username } => accounts.delete(username),
        AccountOperation::Create {
            username,
            display_name,
            admin,
            password,
        } => accounts
            .create(username, display_name, *admin, password)
            .map(|_| ()),
    }
}

/// Internal CLI entry: the caller must have obtained system root authorization.
pub fn entry(actor: u32) -> Result<(), ManagementError> {
    helper::check_actor(actor)?;
    if unsafe { libc::geteuid() } != 0 {
        return Err(failure("Session requires root"));
    }
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(failure("Cannot protect authorization session"));
    }
    // stdout is a private full-duplex socket; stdin belongs only to sudo's
    // one password attempt and is closed immediately after writing it.
    let fd = unsafe { libc::fcntl(1, libc::F_DUPFD_CLOEXEC, 3) };
    if fd < 0 {
        return Err(failure(std::io::Error::last_os_error()));
    }
    let mut channel = unsafe { UnixStream::from_raw_fd(fd) };
    if helper::peer_uid(&channel).map_err(failure)? != actor {
        return Err(failure("Session channel does not belong to the actor"));
    }
    write_packet(&mut channel, &Response::Done)?;
    loop {
        let request: Request = match read_packet(&mut channel) {
            Ok(request) => request,
            // EOF or a malformed frame permanently ends authorization.
            Err(_) => return Ok(()),
        };
        match request {
            Request::Start(ref command) => match start_operation(command, actor) {
                Ok(stream) => {
                    write_packet(&mut channel, &Response::Connected)?;
                    send_connection(&channel, &stream)?;
                }
                Err(error) => write_packet(&mut channel, &Response::Failed(error.to_string()))?,
            },
            Request::Attach(ref path) => {
                match helper::check_attach_path(path, actor).and_then(|()| helper::connect(path, 0))
                {
                    Ok(stream) => {
                        write_packet(&mut channel, &Response::Connected)?;
                        send_connection(&channel, &stream)?;
                    }
                    Err(error) => write_packet(&mut channel, &Response::Failed(error.to_string()))?,
                }
            }
            Request::Account(ref operation) => {
                let result = account_operation(actor, operation);
                write_packet(
                    &mut channel,
                    &result
                        .map(|()| Response::Done)
                        .unwrap_or_else(Response::ServiceFailed),
                )?;
            }
            Request::Power { reboot } => {
                let result = super::power::execute(if reboot {
                    super::power::PowerAction::Reboot
                } else {
                    super::power::PowerAction::PowerOff
                });
                write_packet(
                    &mut channel,
                    &result
                        .map(|()| Response::Done)
                        .unwrap_or_else(Response::ServiceFailed),
                )?;
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/linux/privilege_session.rs"]
mod tests;
