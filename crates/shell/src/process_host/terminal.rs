use std::io;

#[cfg(unix)]
pub(super) use unix::{TerminalState, verify_parent};
#[cfg(windows)]
pub(super) use windows::{TerminalState, verify_parent};

#[cfg(unix)]
mod unix {
    use super::io;
    use std::fs::{File, OpenOptions};
    use std::io::{IsTerminal, Write};
    use std::mem::MaybeUninit;
    use std::os::fd::{AsFd, AsRawFd};
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::{Duration, Instant};

    pub(in crate::process_host) struct TerminalState {
        tty: File,
        output: Option<File>,
        mode: libc::termios,
        foreground: Option<libc::pid_t>,
        owned: bool,
        cleanup: Vec<u8>,
        cleanup_deadline: Option<Instant>,
    }

    impl TerminalState {
        pub(in crate::process_host) fn capture() -> io::Result<Option<Self>> {
            if !io::stdin().is_terminal() {
                return Ok(None);
            }
            let (tty, foreground, path) = match File::open("/dev/tty") {
                Ok(tty) => {
                    // SAFETY: the descriptor belongs to a live terminal.
                    let group = unsafe { libc::tcgetpgrp(tty.as_raw_fd()) };
                    if group < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    (tty, Some(group), Some(std::path::PathBuf::from("/dev/tty")))
                }
                // Tests and service managers can pass a PTY without assigning a controlling tty.
                Err(error) if error.raw_os_error() == Some(libc::ENXIO) => {
                    let input = io::stdin().as_fd().try_clone_to_owned()?;
                    let path = tty_path(input.as_raw_fd());
                    (input.into(), None, path)
                }
                Err(error) => return Err(error),
            };
            let output = path.and_then(|path| {
                OpenOptions::new()
                    .write(true)
                    .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY)
                    .open(path)
                    .ok()
            });
            let mut mode = MaybeUninit::uninit();
            // SAFETY: tcgetattr writes the mode into the supplied storage.
            let mode = unsafe {
                if libc::tcgetattr(tty.as_raw_fd(), mode.as_mut_ptr()) < 0 {
                    return Err(io::Error::last_os_error());
                }
                mode.assume_init()
            };
            Ok(Some(Self {
                tty,
                output,
                mode,
                foreground,
                owned: false,
                cleanup: Vec::new(),
                cleanup_deadline: None,
            }))
        }

        pub(in crate::process_host) fn hand_off(&mut self, shell_pid: u32) -> io::Result<()> {
            if let Some(foreground) = self.foreground {
                // SAFETY: getpgrp only reads this process's group.
                if foreground != unsafe { libc::getpgrp() } {
                    return Err(io::ErrorKind::PermissionDenied.into());
                }
                foreground_group(&self.tty, shell_pid as libc::pid_t)?;
            }
            self.owned = true;
            Ok(())
        }

        pub(in crate::process_host) fn restore(&mut self, abnormal: bool) -> io::Result<()> {
            if !self.owned {
                return Ok(());
            }
            if let Some(group) = self.foreground {
                foreground_group(&self.tty, group)?;
            }
            // Discard buffered UI input and output before restoring the invoking terminal.
            let discarded =
                abnormal && unsafe { libc::tcflush(self.tty.as_raw_fd(), libc::TCIOFLUSH) } == 0;
            let action = if discarded {
                libc::TCSAFLUSH
            } else {
                libc::TCSANOW
            };
            // SAFETY: the tty and its saved termios remain valid until restore returns.
            if unsafe { libc::tcsetattr(self.tty.as_raw_fd(), action, &self.mode) } != 0 {
                return Err(io::Error::last_os_error());
            }
            self.owned = false;
            if abnormal && self.output.is_some() {
                use crossterm::Command;
                use crossterm::cursor::{SetCursorStyle, Show};
                use crossterm::event::{
                    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture,
                };
                use crossterm::terminal::LeaveAlternateScreen;
                let mut cleanup =
                    String::from("\x1b[0m\x1b[?9001l\x1b[>4;0m\x1b[<u\x1b_Ga=d,d=a\x1b\\");
                let _ = DisableMouseCapture.write_ansi(&mut cleanup);
                let _ = DisableFocusChange.write_ansi(&mut cleanup);
                let _ = DisableBracketedPaste.write_ansi(&mut cleanup);
                let _ = SetCursorStyle::DefaultUserShape.write_ansi(&mut cleanup);
                let _ = Show.write_ansi(&mut cleanup);
                let _ = LeaveAlternateScreen.write_ansi(&mut cleanup);
                self.cleanup = cleanup.into_bytes();
                self.cleanup_deadline = Some(Instant::now() + Duration::from_millis(200));
                self.poll_cleanup();
            }
            Ok(())
        }

        pub(in crate::process_host) fn poll_cleanup(&mut self) -> bool {
            if self
                .cleanup_deadline
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                self.cleanup.clear();
            }
            if !self.cleanup.is_empty()
                && let Some(output) = &mut self.output
            {
                match output.write(&self.cleanup) {
                    Ok(count) => {
                        self.cleanup.drain(..count);
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => self.cleanup.clear(),
                }
            }
            !self.cleanup.is_empty()
        }
    }

    fn tty_path(fd: std::os::fd::RawFd) -> Option<std::path::PathBuf> {
        use std::ffi::{CStr, OsStr};
        use std::os::unix::ffi::OsStrExt;
        let mut path = [0; 1024];
        // SAFETY: ttyname_r writes at most the supplied buffer length, including the NUL.
        if unsafe { libc::ttyname_r(fd, path.as_mut_ptr(), path.len()) } != 0 {
            return None;
        }
        let path = unsafe { CStr::from_ptr(path.as_ptr()) };
        Some(OsStr::from_bytes(path.to_bytes()).into())
    }

    impl Drop for TerminalState {
        fn drop(&mut self) {
            let _ = self.restore(true);
        }
    }

    fn foreground_group(tty: &File, group: libc::pid_t) -> io::Result<()> {
        // The guardian becomes a background group while the Shell uses the tty.
        // SAFETY: the signal sets are initialized by libc before use.
        unsafe {
            let mut ignored = MaybeUninit::uninit();
            libc::sigemptyset(ignored.as_mut_ptr());
            let mut ignored = ignored.assume_init();
            libc::sigaddset(&mut ignored, libc::SIGTTOU);
            let mut previous = MaybeUninit::uninit();
            let status = libc::pthread_sigmask(libc::SIG_BLOCK, &ignored, previous.as_mut_ptr());
            if status != 0 {
                return Err(io::Error::from_raw_os_error(status));
            }
            let result = if libc::tcsetpgrp(tty.as_raw_fd(), group) == 0 {
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            };
            libc::pthread_sigmask(libc::SIG_SETMASK, previous.as_ptr(), std::ptr::null_mut());
            result
        }
    }

    pub(in crate::process_host) fn verify_parent(pid: u32) -> io::Result<()> {
        // SAFETY: getppid has no preconditions.
        if pid == 0 || pid != unsafe { libc::getppid() } as u32 {
            Err(io::ErrorKind::PermissionDenied.into())
        } else {
            Ok(())
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::io;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Console::{
        CONSOLE_CURSOR_INFO, GetConsoleCursorInfo, GetConsoleMode, GetStdHandle, STD_ERROR_HANDLE,
        STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetConsoleActiveScreenBuffer, SetConsoleCursorInfo,
        SetConsoleMode,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    pub(in crate::process_host) struct TerminalState {
        modes: Vec<(HANDLE, u32)>,
        screen: HANDLE,
        cursor: CONSOLE_CURSOR_INFO,
        owned: bool,
    }

    impl TerminalState {
        pub(in crate::process_host) fn capture() -> io::Result<Option<Self>> {
            let mut modes = Vec::new();
            // SAFETY: these handles remain valid in the guardian, which never redirects its console.
            let screen = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
            for id in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
                let mut mode = 0;
                let handle = unsafe { GetStdHandle(id) };
                if unsafe { GetConsoleMode(handle, &mut mode) } != 0 {
                    modes.push((handle, mode));
                }
            }
            if modes.is_empty() {
                return Ok(None);
            }
            let mut cursor = CONSOLE_CURSOR_INFO::default();
            unsafe { GetConsoleCursorInfo(screen, &mut cursor) };
            Ok(Some(Self {
                modes,
                screen,
                cursor,
                owned: false,
            }))
        }

        pub(in crate::process_host) fn hand_off(&mut self, _shell_pid: u32) -> io::Result<()> {
            self.owned = true;
            Ok(())
        }

        pub(in crate::process_host) fn restore(&mut self, _abnormal: bool) -> io::Result<()> {
            if !self.owned {
                return Ok(());
            }
            // SAFETY: the saved handles still refer to this console and its original screen buffer.
            unsafe {
                SetConsoleActiveScreenBuffer(self.screen);
                if self.cursor.dwSize != 0 {
                    SetConsoleCursorInfo(self.screen, &self.cursor);
                }
                for &(handle, mode) in &self.modes {
                    if SetConsoleMode(handle, mode) == 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
            }
            self.owned = false;
            Ok(())
        }

        pub(in crate::process_host) fn poll_cleanup(&mut self) -> bool {
            false
        }
    }

    impl Drop for TerminalState {
        fn drop(&mut self) {
            let _ = self.restore(true);
        }
    }

    pub(in crate::process_host) fn verify_parent(pid: u32) -> io::Result<()> {
        if pid == 0 {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        // SAFETY: a successful snapshot returns an owned process-list handle.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        // SAFETY: Windows writes a PROCESSENTRY32W to the correctly sized entry.
        let mut present = unsafe { Process32FirstW(snapshot.as_raw_handle(), &mut entry) };
        while present != 0 {
            if entry.th32ProcessID == std::process::id() {
                if entry.th32ParentProcessID == pid {
                    return Ok(());
                }
                break;
            }
            present = unsafe { Process32NextW(snapshot.as_raw_handle(), &mut entry) };
        }
        Err(io::ErrorKind::PermissionDenied.into())
    }
}
