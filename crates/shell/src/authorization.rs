//! Main-loop handoff for system authentication. The suspended loop reads no input events.
use crate::terminal_session::TerminalGuard;
use platform::{
    linux::authorization::{Interaction, TextAgent, controlling_terminal},
    service::ServiceError,
};
use std::{
    io::{self, Write},
    os::fd::AsRawFd,
    sync::{Arc, mpsc},
    time::Duration,
};

enum Request {
    Begin(mpsc::Sender<Result<(), ServiceError>>),
    Fallback(mpsc::Sender<Result<(), ServiceError>>),
    End,
}
struct ChannelInteraction {
    sender: mpsc::Sender<Request>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}
impl Interaction for ChannelInteraction {
    fn begin(&self) -> Result<(), ServiceError> {
        let (sender, receiver) = mpsc::channel();
        self.sender
            .send(Request::Begin(sender))
            .map_err(|_| ServiceError::AuthorizationCancelled)?;
        receiver
            .recv_timeout(Duration::from_secs(300))
            .map_err(|_| ServiceError::AuthorizationCancelled)?
    }
    fn fallback(&self) -> Result<(), ServiceError> {
        let (sender, receiver) = mpsc::channel();
        self.sender
            .send(Request::Fallback(sender))
            .map_err(|_| ServiceError::AuthorizationCancelled)?;
        receiver
            .recv_timeout(Duration::from_secs(15))
            .map_err(|_| ServiceError::AuthorizationCancelled)?
    }
    fn cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Acquire)
    }
    fn finish(&self) {
        let _ = self.sender.send(Request::End);
    }
}

pub(crate) struct AuthorizationHost {
    receiver: mpsc::Receiver<Request>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}
impl AuthorizationHost {
    pub(crate) fn channel() -> (Self, Arc<dyn Interaction>) {
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        (
            Self {
                receiver,
                cancelled: cancelled.clone(),
            },
            Arc::new(ChannelInteraction { sender, cancelled }),
        )
    }
    pub(crate) fn power<W: Write>(
        &self,
        guard: &mut TerminalGuard<W>,
        action: platform::linux::power::PowerAction,
        watchdog: &watchdog::AppWatchdog,
        interaction: Arc<dyn Interaction>,
        stop: impl Fn() -> bool,
    ) -> Result<(), platform::PlatformError> {
        let (sender, receiver) = mpsc::channel();
        let _worker = watchdog
            .task_group("power-authorization")
            .spawn_thread(
                watchdog::TaskSpec::one_shot(watchdog::TaskId::from_static("logind-request")),
                move || {
                    let result = platform::linux::power::execute_with_interaction(
                        action,
                        Some(interaction.clone()),
                    );
                    let _ = sender.send(result);
                },
            )
            .map_err(|_| platform::PlatformError::from(ServiceError::ServiceUnavailable))?;
        loop {
            if stop() {
                self.cancelled
                    .store(true, std::sync::atomic::Ordering::Release);
                return Err(ServiceError::AuthorizationCancelled.into());
            }
            self.handle_pending(guard, &stop)
                .map_err(|_| platform::PlatformError::from(ServiceError::ServiceUnavailable))?;
            match receiver.recv_timeout(Duration::from_millis(25)) {
                Ok(result) => return result.map_err(Into::into),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(ServiceError::Unknown.into());
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Called only on the event-loop thread, before any crossterm poll/read.
    pub(crate) fn handle_pending<W: Write>(
        &self,
        guard: &mut TerminalGuard<W>,
        stop: impl Fn() -> bool,
    ) -> io::Result<bool> {
        let Ok(request) = self.receiver.try_recv() else {
            return Ok(false);
        };
        let Request::Begin(reply) = request else {
            if let Request::Fallback(reply) = request {
                let _ = reply.send(Err(ServiceError::AuthorizationCancelled));
            }
            return Ok(false);
        };
        let mut suspension = match Suspension::enter(guard) {
            Ok(value) => value,
            Err(error) => {
                let _ = reply.send(Err(ServiceError::ServiceUnavailable));
                return Err(error);
            }
        };
        if reply.send(Ok(())).is_err() {
            suspension.finish()?;
            return Ok(true);
        }
        let mut agent = None;
        loop {
            if stop() {
                self.cancelled
                    .store(true, std::sync::atomic::Ordering::Release);
                break;
            }
            match self.receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(Request::End) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(Request::Begin(reply)) => {
                    let _ = reply.send(Err(ServiceError::Busy));
                }
                Ok(Request::Fallback(reply)) => {
                    let result = TextAgent::register().map(|value| agent = Some(value));
                    let _ = reply.send(result);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if agent
                .as_mut()
                .is_some_and(|agent| !agent.alive().unwrap_or(false))
            {
                // The system reports the failed authentication to the worker. Restore the
                // terminal immediately after the agent exits, including unexpected EOF.
                break;
            }
        }
        drop(agent);
        suspension.finish()?;
        Ok(true)
    }
}

struct Suspension<'a, W: Write> {
    guard: &'a mut TerminalGuard<W>,
    normal: Option<(std::fs::File, libc::termios)>,
    resumed: bool,
}
impl<'a, W: Write> Suspension<'a, W> {
    fn enter(guard: &'a mut TerminalGuard<W>) -> io::Result<Self> {
        guard.restore()?;
        let mut suspension = Self {
            guard,
            normal: None,
            resumed: false,
        };
        // A graphical agent can still authorize a session without a controlling TTY.
        // Only the fallback requires a safe foreground controlling terminal.
        if let Ok(tty) = controlling_terminal() {
            let mut normal = std::mem::MaybeUninit::<libc::termios>::uninit();
            if unsafe { libc::tcgetattr(tty.as_raw_fd(), normal.as_mut_ptr()) } != 0 {
                return Err(io::Error::last_os_error());
            }
            suspension.normal = Some((tty, unsafe { normal.assume_init() }));
            let (tty, _) = suspension.normal.as_ref().unwrap();
            unsafe {
                libc::tcflush(tty.as_raw_fd(), libc::TCIFLUSH);
            }
        }
        Ok(suspension)
    }
    fn finish(&mut self) -> io::Result<()> {
        if self.resumed {
            return Ok(());
        }
        if let Some((tty, normal)) = &self.normal {
            // Restore canonical/echo state BEFORE crossterm records its new baseline.
            if unsafe { libc::tcsetattr(tty.as_raw_fd(), libc::TCSAFLUSH, normal) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        self.guard.resume()?;
        self.resumed = true;
        Ok(())
    }
}
impl<W: Write> Drop for Suspension<'_, W> {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

impl Drop for AuthorizationHost {
    fn drop(&mut self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Release);
    }
}
