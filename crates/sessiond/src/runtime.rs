use crate::{
    display::{Kmscon, SessionDisplayBackend, VtGate, TRUSTED_VT},
    linux::{read, write},
    process,
};
use session_protocol::{
    greeter::{ClientMessage, ServerMessage},
    SessionIdentity, SessionSnapshot, SessionState, SystemAction, SystemUser,
};
use std::{
    io::{self, BufReader},
    os::unix::{io::AsRawFd, net::UnixStream, process::CommandExt},
    process::{Child, Command},
    time::{Duration, Instant},
};
use zbus::blocking::{Connection, Proxy};
pub struct Worker {
    child: Child,
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    pub identity: SessionIdentity,
}
impl Worker {
    fn start(
        user: &SystemUser,
        mode: &str,
        frontend: Option<&UnixStream>,
        mut prompt: impl FnMut(ServerMessage) -> io::Result<ClientMessage>,
    ) -> io::Result<Self> {
        let (parent, child) = UnixStream::pair()?;
        let fd = child.as_raw_fd();
        let front = frontend.map(AsRawFd::as_raw_fd);
        let mut cmd = Command::new(std::env::current_exe()?);
        cmd.env_clear().env("PATH", "/usr/bin:/bin").args([
            "--pam-worker",
            &user.username,
            &fd.to_string(),
            mode,
        ]);
        if let Some(f) = front {
            cmd.arg(f.to_string());
        }
        unsafe {
            cmd.pre_exec(move || {
                if libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 4u32) < 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                for f in [Some(fd), front].into_iter().flatten() {
                    if libc::fcntl(f, libc::F_SETFD, 0) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let process = cmd.spawn()?;
        drop(child);
        parent.set_read_timeout(Some(Duration::from_secs(120)))?;
        let mut verified = false;
        let mut worker = Self {
            child: process,
            reader: BufReader::with_capacity(1, parent.try_clone()?),
            writer: parent,
            identity: SessionIdentity {
                uid: user.uid,
                logind_session_id: String::new(),
            },
        };
        loop {
            let msg: serde_json::Value = read(&mut worker.reader)?;
            match msg["type"].as_str() {
                Some("PamPrompt") => {
                    let server = serde_json::from_value(msg).map_err(io::Error::other)?;
                    let mut response = prompt(server)?;
                    let result = write(&mut worker.writer, &response);
                    if let ClientMessage::PamResponse { response, .. } = &mut response {
                        use zeroize::Zeroize;
                        response.zeroize();
                    }
                    result?;
                }
                Some("SessionOpened") => {
                    worker.identity = serde_json::from_value(msg["identity"].clone())
                        .map_err(io::Error::other)?;
                    if worker.identity.uid != user.uid {
                        return Err(io::Error::other("worker UID mismatch"));
                    }
                    break;
                }
                Some("Authenticated") => {
                    if msg["uid"].as_u64() != Some(u64::from(user.uid)) {
                        return Err(io::Error::other("authenticated UID changed"));
                    }
                    verified = true;
                }
                Some("Complete") if mode == "authenticate" && verified => break,
                _ => return Err(io::Error::other("invalid PAM worker reply")),
            }
        }
        Ok(worker)
    }
    fn close(&mut self) -> io::Result<()> {
        let _ = write(&mut self.writer, &ClientMessage::Logout {});
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.child.try_wait()? {
                if status.success() {
                    break;
                } else {
                    return Err(io::Error::other("PAM worker cleanup failed"));
                }
            }
            if Instant::now() > deadline {
                return Err(io::Error::other(
                    "PAM cleanup timed out; maintenance blocked",
                ));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = write(&mut self.writer, &ClientMessage::Logout {});
    }
}
pub struct Runtime {
    pub snapshot: Option<SessionSnapshot>,
    user: Option<SystemUser>,
    desktop: Option<Worker>,
    greeter: Worker,
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    gate: VtGate,
    maintenance: bool,
    next_id: u64,
}
impl Runtime {
    pub fn start() -> io::Result<Self> {
        crate::display::ensure_reserved_vts_available()?;
        if std::fs::read_to_string("/proc/sys/dev/tty/legacy_tiocsti")?.trim() != "0" {
            return Err(io::Error::other(
                "legacy TIOCSTI must be disabled before starting trusted seat",
            ));
        }
        let user = process::account_by_name("tundra-greeter")?;
        let (front, client) = UnixStream::pair()?;
        let greeter = Worker::start(&user, "greeter", Some(&client), |_| {
            Err(io::Error::other("greeter PAM must not ask credentials"))
        })?;
        drop(client);
        front.set_read_timeout(Some(Duration::from_secs(120)))?;
        let mut gate = VtGate::new()?;
        Kmscon.activate(TRUSTED_VT)?;
        gate.secure()?;
        let mut runtime = Self {
            snapshot: None,
            user: None,
            desktop: None,
            greeter,
            reader: BufReader::with_capacity(1, front.try_clone()?),
            writer: front,
            gate,
            maintenance: std::path::Path::new("/run/tundra/maintenance-ready").exists(),
            next_id: 0,
        };
        if !matches!(
            read::<ClientMessage>(&mut runtime.reader)?,
            ClientMessage::Ready {}
        ) {
            return Err(io::Error::other(
                "trusted frontend did not acknowledge initial rendering",
            ));
        }
        if !runtime.maintenance {
            write(&mut runtime.writer, &ServerMessage::Login { message: None })?;
        }
        Ok(runtime)
    }
    fn exchange(&mut self, msg: ServerMessage) -> io::Result<ClientMessage> {
        write(&mut self.writer, &msg)?;
        read(&mut self.reader)
    }
    fn transition(&mut self, next: SessionState) -> io::Result<()> {
        let s = self
            .snapshot
            .as_mut()
            .ok_or_else(|| io::Error::other("no managed session"))?;
        s.transition(s.revision, next).map_err(io::Error::other)
    }
    pub fn identity(&self) -> io::Result<&SessionIdentity> {
        self.snapshot
            .as_ref()
            .map(|s| &s.identity)
            .ok_or_else(|| io::Error::other("no managed session"))
    }
    fn healthy(&mut self) -> io::Result<()> {
        if !self.greeter.alive() {
            return Err(io::Error::other("trusted greeter worker exited"));
        }
        Ok(())
    }
    pub fn lock(&mut self) -> io::Result<()> {
        self.healthy()?;
        self.transition(SessionState::Locking)?;
        self.gate.secure()?;
        self.set_locked(true)?;
        self.transition(SessionState::Locked)?;
        write(
            &mut self.writer,
            &ServerMessage::Locked {
                username: self.user.as_ref().unwrap().username.clone(),
            },
        )
    }
    fn resume(&mut self) -> io::Result<()> {
        self.gate.restore()?;
        let c = Connection::system().map_err(io::Error::other)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let pid = self
                .desktop
                .as_ref()
                .ok_or_else(|| io::Error::other("no user worker"))?
                .child
                .id();
            if session_protocol::linux::session_for_pid(&c, pid)
                .is_ok_and(|s| s.active && self.identity().is_ok_and(|id| id == &s.identity))
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                self.gate.secure()?;
                return Err(io::Error::other("user seat reactivation failed"));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn set_locked(&self, locked: bool) -> io::Result<()> {
        let c = Connection::system().map_err(io::Error::other)?;
        let manager = Proxy::new(
            &c,
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
        )
        .map_err(io::Error::other)?;
        let path: zbus::zvariant::OwnedObjectPath = manager
            .call(
                "GetSession",
                &(self.identity()?.logind_session_id.as_str(),),
            )
            .map_err(io::Error::other)?;
        let p = Proxy::new(
            &c,
            "org.freedesktop.login1",
            path,
            "org.freedesktop.login1.Session",
        )
        .map_err(io::Error::other)?;
        p.call::<_, _, ()>("SetLockedHint", &(locked,))
            .map_err(io::Error::other)
    }
    pub fn logout(&mut self) -> io::Result<()> {
        self.gate.secure()?;
        if self.snapshot.is_some() {
            self.transition(SessionState::Closing)?;
            if let Some(w) = &mut self.desktop {
                w.close()?;
            }
            self.desktop = None;
            self.transition(SessionState::Ended)?;
            self.snapshot = None;
            self.user = None;
        }
        if !self.maintenance {
            write(&mut self.writer, &ServerMessage::Login { message: None })?;
        }
        Ok(())
    }
    pub fn close_for_update(&mut self, identity: &SessionIdentity) -> io::Result<()> {
        if self.identity()? != identity {
            return Err(io::Error::other("session changed"));
        }
        self.maintenance = true;
        self.logout()?;
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::create_dir_all("/run/tundra")?;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open("/run/tundra/maintenance-ready")?;
        use std::io::Write;
        f.write_all(b"sessions-closed\n")?;
        f.sync_all()
    }
    pub fn consent(
        &mut self,
        identity: &SessionIdentity,
        action: &SystemAction,
    ) -> io::Result<bool> {
        self.healthy()?;
        if self.identity()? != identity
            || self.snapshot.as_ref().unwrap().state != SessionState::Active
        {
            return Err(io::Error::other(
                "consent requires active originating session",
            ));
        }
        self.gate.secure()?;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("request IDs exhausted"))?;
        let id = self.next_id;
        let response = self.exchange(ServerMessage::Consent {
            id,
            title: "System authorization".into(),
            description: match action {
                SystemAction::PowerOff => "Shut down this computer".into(),
                SystemAction::Reboot => "Restart this computer".into(),
                SystemAction::ReadSystemLogs { max_records, .. } => {
                    format!("Read up to {max_records} restricted system log records")
                }
                SystemAction::InstallUpdate { release_id } => {
                    format!("Log out and install verified system update {release_id}")
                }
            },
        });
        let approved =
            matches!(response,Ok(ClientMessage::Consent{id:actual,approved:true}) if actual==id);
        self.healthy()?;
        self.resume()?;
        Ok(approved)
    }
    pub fn poll(&mut self) -> io::Result<()> {
        self.healthy()?;
        if self
            .snapshot
            .as_ref()
            .is_some_and(|s| s.state == SessionState::Active)
            && self.gate.active()? == TRUSTED_VT
        {
            // The physical trusted-VT shortcut is a secure attention path.
            self.lock()?;
        }
        if self.desktop.as_mut().is_some_and(|w| !w.alive()) {
            self.logout()?;
        }
        let mut p = libc::pollfd {
            fd: self.reader.get_ref().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut p, 1, 0) } <= 0 {
            return Ok(());
        }
        let message: ClientMessage = read(&mut self.reader)?;
        match message {
            ClientMessage::Login { username } if self.snapshot.is_none() && !self.maintenance => {
                let result = (|| {
                    let user = process::account_by_name(&username)?;
                    if user.uid == 0 || user.username == "tundra-greeter" {
                        return Err(io::Error::other("account cannot start desktop"));
                    }
                    for group in ["input", "tty"] {
                        if session_protocol::linux::account_in_group(&user, group)? {
                            return Err(io::Error::other(
                                "account has raw input/terminal group access",
                            ));
                        }
                    }
                    let worker = Worker::start(&user, "desktop", None, |m| self.exchange(m))?;
                    self.snapshot = Some(SessionSnapshot {
                        identity: worker.identity.clone(),
                        state: SessionState::Opening,
                        revision: 0,
                    });
                    self.user = Some(user);
                    self.desktop = Some(worker);
                    self.resume()?;
                    self.transition(SessionState::Active)
                })();
                if let Err(e) = result {
                    write(
                        &mut self.writer,
                        &ServerMessage::Login {
                            message: Some(e.to_string()),
                        },
                    )?;
                }
            }
            ClientMessage::Unlock {}
                if self
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.state == SessionState::Locked) =>
            {
                self.transition(SessionState::Unlocking)?;
                let user = self.user.as_ref().unwrap().clone();
                let result = Worker::start(&user, "authenticate", None, |m| self.exchange(m));
                if result.is_ok() {
                    self.resume()?;
                    self.set_locked(false)?;
                    self.transition(SessionState::Active)?;
                } else {
                    self.transition(SessionState::Locked)?;
                    write(
                        &mut self.writer,
                        &ServerMessage::Locked {
                            username: user.username,
                        },
                    )?;
                }
            }
            ClientMessage::Logout {} => self.logout()?,
            ClientMessage::Cancel {} => {}
            _ => return Err(io::Error::other("unsolicited trusted channel response")),
        }
        Ok(())
    }
}
