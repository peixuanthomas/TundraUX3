use crate::{
    display::{Kmscon, SessionDisplayBackend},
    pam::Pam,
    process,
};
use session_protocol::{
    SessionIdentity, SystemAction,
    greeter::{ClientMessage, PamStyle, ServerMessage},
};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::io::{AsRawFd, FromRawFd},
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
};
use zbus::{
    blocking::{Connection, Proxy},
    message::Header,
};
fn err(e: impl std::fmt::Display) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(e.to_string())
}
pub(crate) fn read<T: serde::de::DeserializeOwned>(reader: &mut impl BufRead) -> io::Result<T> {
    let mut b = zeroize::Zeroizing::new(Vec::new());
    let n = (&mut *reader).take(65537).read_until(b'\n', &mut b)?;
    if n == 0 || n > 65536 || b.last() != Some(&b'\n') {
        return Err(io::Error::other("invalid private channel frame"));
    }
    let parsed = serde_json::from_slice(b.as_slice()).map_err(io::Error::other);
    for byte in b.iter_mut() {
        unsafe { std::ptr::write_volatile(byte, 0) }
    }
    parsed
}
pub(crate) fn write(writer: &mut impl Write, value: &impl serde::Serialize) -> io::Result<()> {
    let mut bytes = zeroize::Zeroizing::new(serde_json::to_vec(value).map_err(io::Error::other)?);
    if bytes.len() > 65535 {
        return Err(io::Error::other("oversized private channel message"));
    }
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.flush()
}
fn peer_root(fd: i32) -> io::Result<()> {
    let mut peer = std::mem::MaybeUninit::<libc::ucred>::uninit();
    let mut size = size_of::<libc::ucred>() as libc::socklen_t;
    if unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            peer.as_mut_ptr().cast(),
            &mut size,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if unsafe { peer.assume_init().uid } != 0 {
        return Err(io::Error::other("private channel peer is not root"));
    }
    Ok(())
}
#[derive(Default)]
struct State {
    runtime: Option<crate::runtime::Runtime>,
}
struct Service {
    state: Arc<Mutex<State>>,
}
fn caller(connection: &Connection, header: &Header<'_>) -> zbus::fdo::Result<(u32, u32)> {
    let sender = header.sender().ok_or_else(|| err("missing D-Bus sender"))?;
    let p = Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .map_err(err)?;
    let uid = p
        .call("GetConnectionUnixUser", &(sender.as_str(),))
        .map_err(err)?;
    let pid = p
        .call("GetConnectionUnixProcessID", &(sender.as_str(),))
        .map_err(err)?;
    Ok((uid, pid))
}
fn root(connection: &Connection, header: &Header<'_>) -> zbus::fdo::Result<()> {
    if caller(connection, header)?.0 != 0 {
        return Err(zbus::fdo::Error::AccessDenied(
            "root service required".into(),
        ));
    }
    Ok(())
}
fn unavailable() -> zbus::fdo::Error {
    zbus::fdo::Error::NotSupported("Trusted VT/device broker is not available; secure desktop sessions, locks and consent cannot be enabled".into())
}
#[zbus::interface(name = "org.tundra.Session1")]
impl Service {
    fn get_snapshot(&self) -> zbus::fdo::Result<String> {
        let state = self.state.lock().map_err(err)?;
        serde_json::to_string(&state.runtime.as_ref().and_then(|r| r.snapshot.as_ref()))
            .map_err(err)
    }
    fn lock(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        self.with_user(&header, |r| r.lock())
    }
    fn logout(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        self.with_user(&header, |r| r.logout())
    }
    fn switch_user(&self, #[zbus(header)] header: Header<'_>) -> zbus::fdo::Result<()> {
        self.with_user(&header, |r| r.logout())
    }
    fn close_session(
        &self,
        identity_json: &str,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<()> {
        let c = Connection::system().map_err(err)?;
        root(&c, &header)?;
        let identity: SessionIdentity = serde_json::from_str(identity_json).map_err(err)?;
        let mut state = self.state.lock().map_err(err)?;
        state
            .runtime
            .as_mut()
            .ok_or_else(unavailable)?
            .close_for_update(&identity)
            .map_err(err)
    }
    fn request_consent(
        &self,
        sender: &str,
        identity_json: &str,
        action_json: &str,
        #[zbus(header)] header: Header<'_>,
    ) -> zbus::fdo::Result<bool> {
        let c = Connection::system().map_err(err)?;
        root(&c, &header)?;
        let identity: SessionIdentity = serde_json::from_str(identity_json).map_err(err)?;
        let action: SystemAction = serde_json::from_str(action_json).map_err(err)?;
        action.validate().map_err(err)?;
        if !sender.starts_with(':') || identity.uid == 0 {
            return Err(zbus::fdo::Error::AccessDenied("invalid origin".into()));
        }
        let p = Proxy::new(
            &c,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .map_err(err)?;
        let uid: u32 = p.call("GetConnectionUnixUser", &(sender,)).map_err(err)?;
        let pid: u32 = p
            .call("GetConnectionUnixProcessID", &(sender,))
            .map_err(err)?;
        let session = session_protocol::linux::session_for_pid(&c, pid).map_err(err)?;
        if uid != identity.uid
            || session.identity != identity
            || !session.active
            || session.remote
            || session.seat != "seat0"
            || !session_protocol::linux::account_in_group(
                &session_protocol::linux::account(uid).map_err(err)?,
                "tundra-admin",
            )
            .map_err(err)?
        {
            return Err(zbus::fdo::Error::AccessDenied(
                "origin is not active authorized administrator".into(),
            ));
        }
        self.state
            .lock()
            .map_err(err)?
            .runtime
            .as_mut()
            .ok_or_else(unavailable)?
            .consent(&identity, &action)
            .map_err(err)
    }
}
impl Service {
    fn with_user(
        &self,
        header: &Header<'_>,
        op: impl FnOnce(&mut crate::runtime::Runtime) -> io::Result<()>,
    ) -> zbus::fdo::Result<()> {
        let c = Connection::system().map_err(err)?;
        let (uid, pid) = caller(&c, header)?;
        let session = session_protocol::linux::session_for_pid(&c, pid).map_err(err)?;
        let mut state = self.state.lock().map_err(err)?;
        let runtime = state.runtime.as_mut().ok_or_else(unavailable)?;
        if uid != session.identity.uid || runtime.identity().map_err(err)? != &session.identity {
            return Err(zbus::fdo::Error::AccessDenied(
                "caller is outside managed session".into(),
            ));
        }
        op(runtime).map_err(err)
    }
}
/// Diagnostic worker has no D-Bus listener and accepts no unprivileged caller.
/// A root supervisor supplies a private socketpair; passwords never enter argv/env.
fn pam_worker(username: &str, fd: i32, mode: &str, frontend_fd: Option<i32>) -> io::Result<()> {
    peer_root(fd)?;
    let socket = unsafe { UnixStream::from_raw_fd(fd) };
    socket.set_read_timeout(Some(std::time::Duration::from_secs(120)))?;
    unsafe {
        libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
    let user = process::account_by_name(username)?;
    if user.uid == 0 {
        return Err(io::Error::other("root login is prohibited"));
    }
    let output = Arc::new(Mutex::new(socket.try_clone()?));
    let input = Arc::new(Mutex::new(BufReader::with_capacity(1, socket)));
    let out = output.clone();
    let inp = input.clone();
    let mut id = 0u64;
    let prompt = Box::new(move |style, text: &str| {
        id = id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("prompt ID exhausted"))?;
        let style = match style {
            1 => PamStyle::EchoOff,
            2 => PamStyle::EchoOn,
            3 => PamStyle::Error,
            4 => PamStyle::Info,
            _ => return Err(io::Error::other("unknown PAM prompt style")),
        };
        write(
            &mut *out.lock().map_err(|e| io::Error::other(e.to_string()))?,
            &ServerMessage::PamPrompt {
                id,
                style,
                text: text.to_owned(),
            },
        )?;
        match read::<ClientMessage>(&mut *inp.lock().map_err(|e| io::Error::other(e.to_string()))?)?
        {
            ClientMessage::PamResponse {
                id: actual,
                response,
            } if actual == id => Ok(response),
            _ => Err(io::Error::other(
                "authentication cancelled or stale response",
            )),
        }
    });
    let trusted = mode == "greeter";
    let mut pam = Pam::start(
        if trusted {
            "tundra-greeter"
        } else {
            "tundra-session"
        },
        &user,
        if trusted {
            crate::display::TRUSTED_VT
        } else {
            crate::display::USER_VT
        },
        prompt,
    )?;
    if trusted {
        pam.put("XDG_SESSION_CLASS=greeter")?;
    } else {
        pam.authenticate(&user)?;
        write(
            &mut *output.lock().map_err(|e| io::Error::other(e.to_string()))?,
            &serde_json::json!({"type":"Authenticated","uid":user.uid}),
        )?;
    }
    if mode != "authenticate" {
        pam.open(&user)?;
        let c = Connection::system().map_err(io::Error::other)?;
        let s = session_protocol::linux::session_for_pid(&c, std::process::id())
            .map_err(io::Error::other)?;
        if s.identity.uid != user.uid || s.seat != "seat0" || s.remote {
            return Err(io::Error::other("PAM/logind session identity mismatch"));
        }
        struct Cleanup(Option<SessionIdentity>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if let Some(identity) = &self.0 {
                    if let Err(e) = process::drain_session(identity) {
                        eprintln!("session process cleanup: {e}");
                    }
                }
            }
        }
        let mut cleanup = Cleanup(Some(s.identity.clone()));
        let environment = process::environment(&user, pam.environment())?;
        let frontend = frontend_fd.map(|fd| unsafe { UnixStream::from_raw_fd(fd) });
        let mut child = if matches!(mode, "desktop" | "greeter") {
            Some(Kmscon.spawn(&user, environment.clone(), frontend.as_ref())?)
        } else {
            None
        };
        // The diagnostic proves PAM/logind and environment establishment; it intentionally
        // does not launch an unsecured desktop. A future broker must gate display startup.
        write(
            &mut *output.lock().map_err(|e| io::Error::other(e.to_string()))?,
            &serde_json::json!({"type":"SessionOpened","identity":&s.identity,"environment":environment}),
        )?;
        loop {
            let fd = input
                .lock()
                .map_err(|e| io::Error::other(e.to_string()))?
                .get_ref()
                .as_raw_fd();
            let mut poll = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            if unsafe { libc::poll(&mut poll, 1, 250) } > 0 {
                match read::<ClientMessage>(
                    &mut *input.lock().map_err(|e| io::Error::other(e.to_string()))?,
                ) {
                    Ok(ClientMessage::Logout {} | ClientMessage::Cancel {}) | Err(_) => break,
                    _ => return Err(io::Error::other("invalid worker control")),
                }
            }
            if let Some(child) = &mut child {
                if child.try_wait()?.is_some() {
                    break;
                }
            }
        }
        process::drain_session(&s.identity)?;
        cleanup.0 = None;
        drop(cleanup);
        if let Some(child) = &mut child {
            let _ = child.kill();
            let _ = child.wait();
        }

        pam.close()?;
    }
    write(
        &mut *output.lock().map_err(|e| io::Error::other(e.to_string()))?,
        &ServerMessage::Complete {},
    )?;
    Ok(())
}
pub fn run() -> io::Result<()> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "sessiond requires a system service root identity",
        ));
    }
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--pam-worker") {
        if !(5..=6).contains(&args.len()) {
            return Err(io::Error::other(
                "usage: --pam-worker USER PRIVATE_FD authenticate|session",
            ));
        }
        let fd: i32 = args[3].parse().map_err(io::Error::other)?;
        if fd < 3 {
            return Err(io::Error::other(
                "worker requires private socket descriptor",
            ));
        }
        if !matches!(
            args[4].as_str(),
            "authenticate" | "session" | "desktop" | "greeter"
        ) {
            return Err(io::Error::other("invalid worker mode"));
        }
        let front = if args.len() == 6 {
            Some(args[5].parse::<i32>().map_err(io::Error::other)?)
        } else {
            None
        };
        if (args[4] == "greeter") != front.is_some() {
            return Err(io::Error::other("greeter requires private frontend fd"));
        }
        return pam_worker(&args[2], fd, &args[4], front);
    }
    if args.len() > 2 || (args.len() == 2 && args[1] != "--seat") {
        return Err(io::Error::other("unsupported sessiond arguments"));
    }
    let state = Arc::new(Mutex::new(State {
        runtime: if args.len() == 2 {
            Some(crate::runtime::Runtime::start()?)
        } else {
            None
        },
    }));
    let _connection = zbus::blocking::connection::Builder::system()
        .map_err(io::Error::other)?
        .name(session_protocol::SESSION_BUS)
        .map_err(io::Error::other)?
        .serve_at(
            session_protocol::SESSION_PATH,
            Service {
                state: state.clone(),
            },
        )
        .map_err(io::Error::other)?
        .build()
        .map_err(io::Error::other)?;
    eprintln!(
        "sessiond ready; --seat enables managed VT sessions, default mode exposes discovery only"
    );
    loop {
        if let Some(runtime) = &mut state
            .lock()
            .map_err(|e| io::Error::other(e.to_string()))?
            .runtime
        {
            runtime.poll()?;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_frames_reject_oversize_truncation_and_stale_protocol() {
        assert!(
            read::<ClientMessage>(&mut std::io::Cursor::new(b"{\"type\":\"Cancel\"}")).is_err()
        );
        assert!(read::<ClientMessage>(&mut std::io::Cursor::new(vec![b'x'; 65537])).is_err());
        assert!(
            read::<ClientMessage>(&mut std::io::Cursor::new(
                b"{\"type\":\"Cancel\",\"uid\":0}\n"
            ))
            .is_err()
        );
        assert!(matches!(
            read::<ClientMessage>(&mut std::io::Cursor::new(b"{\"type\":\"Cancel\"}\n")).unwrap(),
            ClientMessage::Cancel {}
        ));
    }
    #[test]
    fn no_session_is_never_reported_as_locked() {
        let service = Service {
            state: Arc::new(Mutex::new(State::default())),
        };
        assert_eq!(service.get_snapshot().unwrap(), "null");
    }
}
