use super::*;
use platform::{
    linux::authorization::{Interaction, TextAgent},
    service::ServiceError,
};
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    process::{Command, Stdio},
};

pub(super) struct AuthTerminal {
    master: File,
    input: VecDeque<u8>,
}
impl AuthTerminal {
    fn open(columns: u16, rows: u16) -> io::Result<(Self, File)> {
        let mut master = -1;
        let mut slave = -1;
        let size = libc::winsize {
            ws_row: rows.max(1),
            ws_col: columns.max(1),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        if unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                &size,
            )
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
            if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        if unsafe { libc::fcntl(master.as_raw_fd(), libc::F_SETFL, libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // No response is echoed before the system password program takes over.
        let mut termios = std::mem::MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(slave.as_raw_fd(), termios.as_mut_ptr()) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut termios = unsafe { termios.assume_init() };
        termios.c_lflag &= !libc::ECHO;
        if unsafe { libc::tcsetattr(slave.as_raw_fd(), libc::TCSANOW, &termios) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((
            Self {
                master,
                input: VecDeque::new(),
            },
            slave,
        ))
    }
    pub(super) fn resize(&mut self, columns: u16, rows: u16) {
        let size = libc::winsize {
            ws_row: rows.max(1),
            ws_col: columns.max(1),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        unsafe {
            libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &size);
        }
    }
    pub(super) fn write(&mut self, bytes: &[u8]) {
        self.input.extend(bytes);
        self.flush();
    }
    fn flush(&mut self) {
        use zeroize::Zeroize;
        while !self.input.is_empty() {
            match self.master.write(self.input.as_slices().0) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    self.input.as_mut_slices().0[..count].zeroize();
                    self.input.drain(..count);
                }
            }
        }
    }
}
impl Drop for AuthTerminal {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.input.make_contiguous().zeroize();
    }
}

impl AutoAdminJob {
    fn open_terminal(&self) -> Result<File, ServiceError> {
        let (rows, columns) = self
            .0
            .display
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .parser
            .screen()
            .size();
        let (terminal, slave) =
            AuthTerminal::open(columns, rows).map_err(|_| ServiceError::ServiceUnavailable)?;
        *self.0.terminal.lock().unwrap_or_else(|e| e.into_inner()) = Some(terminal);
        Ok(slave)
    }
    pub fn poll_terminal(&self) {
        let mut bytes = Vec::new();
        if let Some(terminal) = self
            .0
            .terminal
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            terminal.flush();
            let mut chunk = [0u8; 8192];
            for _ in 0..8 {
                match terminal.master.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => bytes.extend_from_slice(&chunk[..count]),
                }
            }
        }
        if !bytes.is_empty() {
            self.emit(&OperationEvent::TerminalOutput { bytes });
        }
    }
    fn close_terminal(&self) {
        self.poll_terminal();
        *self.0.terminal.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

pub struct AutoAdminAuthorization {
    job: AutoAdminJob,
    agent: Mutex<Option<TextAgent>>,
    session: Option<platform::management::authorization::PrivilegeSession>,
}
impl AutoAdminAuthorization {
    pub fn new(job: AutoAdminJob) -> Self {
        Self {
            job,
            agent: Mutex::new(None),
            session: None,
        }
    }
    pub fn with_session(
        job: AutoAdminJob,
        session: platform::management::authorization::PrivilegeSession,
    ) -> Self {
        Self {
            job,
            agent: Mutex::new(None),
            session: Some(session),
        }
    }
}
impl Interaction for AutoAdminAuthorization {
    fn session_authorized(&self) -> bool {
        self.session.is_some()
    }
    fn account_operation(
        &self,
        operation: platform::linux::privilege_session::AccountOperation,
    ) -> Option<Result<(), ServiceError>> {
        self.session.as_ref().map(|session| {
            self.begin()?;
            session.execute(platform::linux::privilege_session::Request::Account(
                operation,
            ))
        })
    }
    fn embedded(&self) -> bool {
        true
    }
    fn begin(&self) -> Result<(), ServiceError> {
        if self.job.accepts_input() {
            Ok(())
        } else {
            Err(ServiceError::AuthorizationCancelled)
        }
    }
    fn fallback(&self) -> Result<(), ServiceError> {
        self.begin()?;
        let mut agent = self.agent.lock().unwrap_or_else(|e| e.into_inner());
        if agent.is_none() {
            let terminal = self.job.open_terminal()?;
            match TextAgent::register_embedded(terminal) {
                Ok(value) => *agent = Some(value),
                Err(error) => {
                    self.job.close_terminal();
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    fn finish(&self) {
        self.agent.lock().unwrap_or_else(|e| e.into_inner()).take();
        self.job.close_terminal();
    }
    fn cancelled(&self) -> bool {
        !self.job.accepts_input()
    }
    fn change_own_password(&self) -> Result<(), ServiceError> {
        self.begin()?;
        let tty = self.job.open_terminal()?;
        let result = (|| {
            let mut command = Command::new("/usr/bin/passwd");
            command
                .stdin(Stdio::from(
                    tty.try_clone()
                        .map_err(|_| ServiceError::ServiceUnavailable)?,
                ))
                .stdout(Stdio::from(
                    tty.try_clone()
                        .map_err(|_| ServiceError::ServiceUnavailable)?,
                ))
                .stderr(Stdio::from(tty));
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                        Err(io::Error::last_os_error())
                    } else {
                        Ok(())
                    }
                });
            }
            let mut child = command
                .spawn()
                .map_err(|_| ServiceError::ServiceUnavailable)?;
            *self.job.0.process.lock().unwrap_or_else(|e| e.into_inner()) =
                platform::management::termination::ProcessTree::child(child.id()).ok();
            if self.job.0.stop_requested.load(Ordering::Acquire)
                && let Some(process) = self
                    .job
                    .0
                    .process
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_mut()
            {
                let _ = process.signal(false);
            }
            let deadline = Instant::now() + Duration::from_secs(300);
            loop {
                let stopping = self.job.0.stop_requested.load(Ordering::Acquire);
                if stopping
                    && let Some(process) = self
                        .job
                        .0
                        .process
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .as_mut()
                {
                    let _ = process.signal(self.job.0.force_requested.load(Ordering::Acquire));
                }
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if stopping
                            && self
                                .job
                                .0
                                .process
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .as_ref()
                                .is_some_and(|process| process.has_live_children())
                        {
                            std::thread::sleep(Duration::from_millis(25));
                            continue;
                        }
                        if stopping {
                            return Err(ServiceError::AuthorizationCancelled);
                        }
                        return if status.success() {
                            Ok(())
                        } else {
                            Err(ServiceError::PermissionDenied)
                        };
                    }
                    Ok(None) if (stopping || Instant::now() < deadline) && self.job.running() => {
                        std::thread::sleep(Duration::from_millis(25))
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(ServiceError::AuthorizationCancelled);
                    }
                }
            }
        })();
        self.job
            .0
            .process
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        self.job.close_terminal();
        result
    }
}
impl Drop for AutoAdminAuthorization {
    fn drop(&mut self) {
        self.finish();
    }
}

#[cfg(feature = "test-support")]
impl AutoAdminJob {
    pub fn open_test_terminal(&self) -> Result<File, ServiceError> {
        self.open_terminal()
    }
    pub fn close_test_terminal(&self) {
        self.close_terminal();
    }
    pub fn track_test_child(&self, pid: u32) -> io::Result<()> {
        *self.0.process.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(platform::management::termination::ProcessTree::child(pid)?);
        Ok(())
    }
}
