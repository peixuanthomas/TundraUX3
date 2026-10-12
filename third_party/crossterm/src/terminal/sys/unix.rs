//! UNIX related logic for terminal manipulation.

#[cfg(feature = "events")]
use crate::event::KeyboardEnhancementFlags;
use crate::terminal::{sys::file_descriptor::tty_fd, WindowSize};
#[cfg(feature = "libc")]
use libc::{
    cfmakeraw, ioctl, tcgetattr, tcsetattr, termios as Termios, winsize, STDERR_FILENO,
    STDIN_FILENO, STDOUT_FILENO, TCSANOW, TIOCGWINSZ,
};
use parking_lot::Mutex;
#[cfg(not(feature = "libc"))]
use rustix::{
    fd::AsFd,
    termios::{Termios, Winsize},
};

use std::{fs::File, io};
#[cfg(feature = "libc")]
use std::{
    mem,
    os::unix::io::{AsRawFd, RawFd},
};

// Some(Termios) -> we're in the raw mode and this is the previous mode
// None -> we're not in the raw mode
static TERMINAL_MODE_PRIOR_RAW_MODE: Mutex<Option<Termios>> = parking_lot::const_mutex(None);

pub(crate) fn is_raw_mode_enabled() -> bool {
    TERMINAL_MODE_PRIOR_RAW_MODE.lock().is_some()
}

#[cfg(feature = "libc")]
impl From<winsize> for WindowSize {
    fn from(size: winsize) -> WindowSize {
        WindowSize {
            columns: size.ws_col,
            rows: size.ws_row,
            width: size.ws_xpixel,
            height: size.ws_ypixel,
        }
    }
}
#[cfg(not(feature = "libc"))]
impl From<Winsize> for WindowSize {
    fn from(size: Winsize) -> WindowSize {
        WindowSize {
            columns: size.ws_col,
            rows: size.ws_row,
            width: size.ws_xpixel,
            height: size.ws_ypixel,
        }
    }
}

#[cfg(feature = "libc")]
pub(crate) fn window_size() -> io::Result<WindowSize> {
    let query = |fd| {
        let mut size: winsize = unsafe { mem::zeroed() };
        wrap_with_result(unsafe { ioctl(fd, TIOCGWINSZ as _, &mut size) })?;
        Ok::<_, io::Error>(WindowSize::from(size))
    };
    if let Ok(file) = File::open("/dev/tty") {
        if let Ok(size) = query(file.as_raw_fd()) {
            return Ok(size);
        }
    }
    query(STDOUT_FILENO)
        .or_else(|_| query(STDERR_FILENO))
        .or_else(|_| query(STDIN_FILENO))
}

#[cfg(not(feature = "libc"))]
pub(crate) fn window_size() -> io::Result<WindowSize> {
    use rustix::{
        stdio::{stderr, stdin, stdout},
        termios::tcgetwinsize,
    };
    if let Ok(file) = File::open("/dev/tty") {
        if let Ok(size) = tcgetwinsize(file.as_fd()) {
            return Ok(size.into());
        }
    }
    tcgetwinsize(stdout())
        .or_else(|_| tcgetwinsize(stderr()))
        .or_else(|_| tcgetwinsize(stdin()))
        .map(WindowSize::from)
        .map_err(io::Error::from)
}

pub(crate) fn size() -> io::Result<(u16, u16)> {
    window_size().map(|size| (size.columns, size.rows))
}

#[cfg(feature = "libc")]
pub(crate) fn enable_raw_mode() -> io::Result<()> {
    let mut original_mode = TERMINAL_MODE_PRIOR_RAW_MODE.lock();
    if original_mode.is_some() {
        return Ok(());
    }

    let tty = tty_fd()?;
    let fd = tty.raw_fd();
    let mut ios = get_terminal_attr(fd)?;
    let original_mode_ios = ios;
    raw_terminal_attr(&mut ios);
    set_terminal_attr(fd, &ios)?;
    // Keep it last - set the original mode only if we were able to switch to the raw mode
    *original_mode = Some(original_mode_ios);
    Ok(())
}

#[cfg(not(feature = "libc"))]
pub(crate) fn enable_raw_mode() -> io::Result<()> {
    let mut original_mode = TERMINAL_MODE_PRIOR_RAW_MODE.lock();
    if original_mode.is_some() {
        return Ok(());
    }

    let tty = tty_fd()?;
    let mut ios = get_terminal_attr(&tty)?;
    let original_mode_ios = ios.clone();
    ios.make_raw();
    set_terminal_attr(&tty, &ios)?;
    // Keep it last - set the original mode only if we were able to switch to the raw mode
    *original_mode = Some(original_mode_ios);
    Ok(())
}

/// Reset the raw mode.
///
/// More precisely, reset the whole termios mode to what it was before the first call
/// to [enable_raw_mode]. If you don't mess with termios outside of crossterm, it's
/// effectively disabling the raw mode and doing nothing else.
#[cfg(feature = "libc")]
pub(crate) fn disable_raw_mode() -> io::Result<()> {
    let mut original_mode = TERMINAL_MODE_PRIOR_RAW_MODE.lock();
    if let Some(original_mode_ios) = original_mode.as_ref() {
        let tty = tty_fd()?;
        set_terminal_attr(tty.raw_fd(), original_mode_ios)?;
        // Keep it last - remove the original mode only if we were able to switch back
        *original_mode = None;
    }
    Ok(())
}

#[cfg(not(feature = "libc"))]
pub(crate) fn disable_raw_mode() -> io::Result<()> {
    let mut original_mode = TERMINAL_MODE_PRIOR_RAW_MODE.lock();
    if let Some(original_mode_ios) = original_mode.as_ref() {
        let tty = tty_fd()?;
        set_terminal_attr(&tty, original_mode_ios)?;
        // Keep it last - remove the original mode only if we were able to switch back
        *original_mode = None;
    }
    Ok(())
}

#[cfg(not(feature = "libc"))]
fn get_terminal_attr(fd: impl AsFd) -> io::Result<Termios> {
    let result = rustix::termios::tcgetattr(fd)?;
    Ok(result)
}

#[cfg(not(feature = "libc"))]
fn set_terminal_attr(fd: impl AsFd, termios: &Termios) -> io::Result<()> {
    rustix::termios::tcsetattr(fd, rustix::termios::OptionalActions::Now, termios)?;
    Ok(())
}

/// Queries the terminal's support for progressive keyboard enhancement.
///
/// On unix systems, this function will block and possibly time out while
/// [`crossterm::event::read`](crate::event::read) or [`crossterm::event::poll`](crate::event::poll) are being called.
#[cfg(feature = "events")]
pub fn supports_keyboard_enhancement() -> io::Result<bool> {
    query_keyboard_enhancement_flags().map(|flags| flags.is_some())
}

/// Queries the terminal's currently active keyboard enhancement flags.
///
/// On unix systems, this function will block and possibly time out while
/// [`crossterm::event::read`](crate::event::read) or [`crossterm::event::poll`](crate::event::poll) are being called.
#[cfg(feature = "events")]
pub fn query_keyboard_enhancement_flags() -> io::Result<Option<KeyboardEnhancementFlags>> {
    if is_raw_mode_enabled() {
        query_keyboard_enhancement_flags_raw()
    } else {
        query_keyboard_enhancement_flags_nonraw()
    }
}

#[cfg(feature = "events")]
fn query_keyboard_enhancement_flags_nonraw() -> io::Result<Option<KeyboardEnhancementFlags>> {
    enable_raw_mode()?;
    let flags = query_keyboard_enhancement_flags_raw();
    disable_raw_mode()?;
    flags
}

#[cfg(feature = "events")]
fn query_keyboard_enhancement_flags_raw() -> io::Result<Option<KeyboardEnhancementFlags>> {
    use crate::event::{
        filter::{KeyboardEnhancementFlagsFilter, PrimaryDeviceAttributesFilter},
        poll_internal, read_internal, InternalEvent,
    };
    use std::io::Write;
    use std::time::Duration;

    // This is the recommended method for testing support for the keyboard enhancement protocol.
    // We send a query for the flags supported by the terminal and then the primary device attributes
    // query. If we receive the primary device attributes response but not the keyboard enhancement
    // flags, none of the flags are supported.
    //
    // See <https://sw.kovidgoyal.net/kitty/keyboard-protocol/#detection-of-support-for-this-protocol>

    // ESC [ ? u        Query progressive keyboard enhancement flags (kitty protocol).
    // ESC [ c          Query primary device attributes.
    const QUERY: &[u8] = b"\x1B[?u\x1B[c";

    let result = File::options()
        .write(true)
        .open("/dev/tty")
        .and_then(|mut file| {
            file.write_all(QUERY)?;
            file.flush()
        });
    if result.is_err() {
        let mut stdout = io::stdout();
        stdout.write_all(QUERY)?;
        stdout.flush()?;
    }

    loop {
        match poll_internal(
            Some(Duration::from_millis(2000)),
            &KeyboardEnhancementFlagsFilter,
        ) {
            Ok(true) => {
                match read_internal(&KeyboardEnhancementFlagsFilter) {
                    Ok(InternalEvent::KeyboardEnhancementFlags(current_flags)) => {
                        // Flush the PrimaryDeviceAttributes out of the event queue.
                        read_internal(&PrimaryDeviceAttributesFilter).ok();
                        return Ok(Some(current_flags));
                    }
                    _ => return Ok(None),
                }
            }
            Ok(false) => {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "The keyboard enhancement status could not be read within a normal duration",
                ));
            }
            Err(_) => {}
        }
    }
}

#[cfg(feature = "libc")]
// Transform the given mode into an raw mode (non-canonical) mode.
fn raw_terminal_attr(termios: &mut Termios) {
    unsafe { cfmakeraw(termios) }
}

#[cfg(feature = "libc")]
fn get_terminal_attr(fd: RawFd) -> io::Result<Termios> {
    unsafe {
        let mut termios = mem::zeroed();
        wrap_with_result(tcgetattr(fd, &mut termios))?;
        Ok(termios)
    }
}

#[cfg(feature = "libc")]
fn set_terminal_attr(fd: RawFd, termios: &Termios) -> io::Result<()> {
    wrap_with_result(unsafe { tcsetattr(fd, TCSANOW, termios) })
}

#[cfg(feature = "libc")]
fn wrap_with_result(result: i32) -> io::Result<()> {
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rustix::{
        process::setsid,
        pty::{grantpt, openpt, ptsname, unlockpt, OpenptFlags},
        termios::{tcsetwinsize, Winsize},
    };
    use std::{
        ffi::OsStr,
        fs::File,
        os::unix::{ffi::OsStrExt, process::CommandExt},
        path::Path,
        process::{Command, Stdio},
    };

    // Test size lookup through /dev/tty, stdout, stderr, and stdin using private PTYs, without
    // requiring an interactive terminal. Each parent configures a subprocess that runs an ignored
    // assertion test. The standard-descriptor cases share the same child test; controlling-terminal
    // setup and the error case have their own children. Subprocesses isolate descriptor and session
    // changes from the other tests.
    //
    // The parent collects child failures through the test harness's stdout, except when stdout is
    // the terminal under test: that case uses --nocapture and collects stderr. Each child prints a
    // completion marker after its assertions. The parent checks it because an incorrect --exact
    // test name can run zero tests and still exit successfully.

    /// Verify /dev/tty lookup by giving the child a private controlling terminal while redirecting
    /// all standard descriptors.
    #[test]
    fn size_uses_controlling_terminal() {
        let (pty, _terminal) = terminal_with_size();
        let test_name = "terminal::sys::unix::tests::check_controlling_terminal_size";
        let mut child = child_test(test_name);
        // Pass the endpoint path to the child so it can acquire a controlling terminal after exec.
        // Every standard descriptor is redirected, requiring /dev/tty.
        let name = ptsname(&pty, Vec::new()).unwrap();
        let terminal_path = OsStr::from_bytes(name.to_bytes());
        child
            .arg("--show-output")
            .env("CROSSTERM_TEST_TERMINAL_PATH", terminal_path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        detach_controlling_terminal(&mut child);

        let output = child.output().unwrap();
        drop(pty);

        let diagnostics = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(0), "{diagnostics}");
        let child_ran = diagnostics.contains("terminal size checked");
        assert!(child_ran, "missing completion marker:\n{diagnostics}");
    }

    /// Acquire the parent-provided terminal and check cell and pixel dimensions through both public
    /// APIs. Runs only in the isolated child.
    #[test]
    #[ignore = "launched by size_uses_controlling_terminal with isolated descriptors"]
    fn check_controlling_terminal_size() {
        assert_no_controlling_terminal();

        // Explicitly acquire this endpoint as the child's controlling terminal rather than relying
        // on platform-specific open behavior. Close the setup handle so the API must reopen the
        // terminal through /dev/tty.
        let terminal_path = std::env::var_os("CROSSTERM_TEST_TERMINAL_PATH").unwrap();
        let terminal = File::options()
            .read(true)
            .write(true)
            .open(terminal_path)
            .unwrap();
        rustix::process::ioctl_tiocsctty(&terminal).unwrap();
        drop(terminal);

        assert!(
            File::open("/dev/tty").is_ok(),
            "check the controlling terminal setup"
        );
        assert_eq!(crate::terminal::size().unwrap(), (81, 23));
        let window = crate::terminal::window_size().unwrap();
        assert_eq!((window.columns, window.rows), (81, 23));
        let pixel_size = (window.width, window.height);
        assert_eq!(pixel_size, (810, 460), "preserve pixel dimensions");

        println!("terminal size checked");
    }

    /// Verify stdout lookup with a PTY on stdout and no controlling terminal. Capture child
    /// diagnostics on stderr so stdout remains the terminal under test.
    #[test]
    fn size_uses_stdout_terminal() {
        let (pty, terminal) = terminal_with_size();
        let test_name = "terminal::sys::unix::tests::check_terminal_size";
        let mut child = child_test(test_name);
        child
            .arg("--nocapture")
            .stdin(Stdio::null())
            .stdout(Stdio::from(terminal))
            .stderr(Stdio::piped());
        // stdout must be the only usable terminal, so detach /dev/tty.
        detach_controlling_terminal(&mut child);

        let output = child.output().unwrap();
        drop(pty);

        let diagnostics = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(0), "{diagnostics}");
        let child_ran = diagnostics.contains("terminal size checked");
        assert!(child_ran, "missing completion marker:\n{diagnostics}");
    }

    /// Verify stderr fallback with stdout piped, stdin disconnected, and /dev/tty unavailable. The
    /// shared child checks the private PTY dimensions.
    #[test]
    fn size_uses_stderr_when_stdout_is_redirected() {
        let (pty, terminal) = terminal_with_size();
        let test_name = "terminal::sys::unix::tests::check_terminal_size";
        let mut child = child_test(test_name);
        child
            .arg("--show-output")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(terminal));
        // Detach /dev/tty so the query must use the configured standard descriptor. Inheriting an
        // already-open endpoint does not acquire a controlling terminal.
        detach_controlling_terminal(&mut child);

        let output = child.output().unwrap();
        drop(pty);

        let diagnostics = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(0), "{diagnostics}");
        let child_ran = diagnostics.contains("terminal size checked");
        assert!(child_ran, "missing completion marker:\n{diagnostics}");
    }

    /// Verify stdin fallback with stdout and stderr redirected and /dev/tty unavailable. The shared
    /// child checks the private PTY dimensions.
    #[test]
    fn size_uses_stdin_when_stdout_and_stderr_are_redirected() {
        let (pty, terminal) = terminal_with_size();
        let test_name = "terminal::sys::unix::tests::check_terminal_size";
        let mut child = child_test(test_name);
        child
            .arg("--show-output")
            .stdin(Stdio::from(terminal))
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // Detach /dev/tty so the query must use the configured standard descriptor. Inheriting an
        // already-open endpoint does not acquire a controlling terminal.
        detach_controlling_terminal(&mut child);

        let output = child.output().unwrap();
        drop(pty);

        let diagnostics = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(0), "{diagnostics}");
        let child_ran = diagnostics.contains("terminal size checked");
        assert!(child_ran, "missing completion marker:\n{diagnostics}");
    }

    /// Check both public APIs against the configured cell and pixel dimensions. The stdout, stderr,
    /// and stdin parents each supply a single usable terminal.
    #[test]
    #[ignore = "launched by the stdout, stderr, and stdin tests with isolated descriptors"]
    fn check_terminal_size() {
        assert_no_controlling_terminal();

        assert_eq!(crate::terminal::size().unwrap(), (81, 23));
        let window = crate::terminal::window_size().unwrap();
        assert_eq!((window.columns, window.rows), (81, 23));
        let pixel_size = (window.width, window.height);
        assert_eq!(pixel_size, (810, 460), "preserve pixel dimensions");

        // The stdout scenario captures stderr with --nocapture; the other scenarios capture the
        // harness output on stdout. Emit the marker on both streams.
        println!("terminal size checked");
        eprintln!("terminal size checked");
    }

    /// Verify failure when the child has no controlling terminal and all standard descriptors are
    /// connected to pipes or /dev/null.
    #[test]
    fn size_returns_error_without_a_terminal() {
        let test_name = "terminal::sys::unix::tests::check_no_terminal_size";
        let mut child = child_test(test_name);
        child
            .arg("--show-output")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // Detach /dev/tty so no terminal remains available to the child.
        detach_controlling_terminal(&mut child);

        let output = child.output().unwrap();

        let diagnostics = String::from_utf8_lossy(&output.stdout);
        assert_eq!(output.status.code(), Some(0), "{diagnostics}");
        let child_ran = diagnostics.contains("terminal size checked");
        assert!(child_ran, "missing completion marker:\n{diagnostics}");
    }

    /// Check that both public APIs fail and size returns the final descriptor error. Runs only in
    /// the child with no usable terminal.
    #[test]
    #[ignore = "launched by size_returns_error_without_a_terminal with isolated descriptors"]
    fn check_no_terminal_size() {
        assert_no_controlling_terminal();

        // The errno for /dev/null varies by OS; compare with the final query directly.
        let expected = rustix::termios::tcgetwinsize(rustix::stdio::stdin()).unwrap_err();
        let error = crate::terminal::size().unwrap_err();
        assert_eq!(error.raw_os_error(), Some(expected.raw_os_error()));
        assert!(crate::terminal::window_size().is_err());

        println!("terminal size checked");
    }

    // Launch only the named assertion test in this same binary. It is ignored during normal runs
    // because its parent must configure the child descriptors and session.
    fn child_test(test_name: &str) -> Command {
        let executable = std::env::current_exe().unwrap();
        let mut child = Command::new(executable);
        child.args(["--ignored", "--exact", test_name]);
        child
    }

    // Verify that setsid detached the child from any controlling terminal. Otherwise /dev/tty could
    // satisfy the size query and hide a broken standard-descriptor fallback. A failure here
    // indicates subprocess setup failure, before testing size lookup.
    fn assert_no_controlling_terminal() {
        let tty = File::open("/dev/tty");
        assert!(tty.is_err(), "setsid must detach the controlling terminal");
    }

    // Configure a pre-exec hook that starts the child in a new session without a controlling
    // terminal. The parent session stays unchanged. The /dev/tty test explicitly acquires its
    // private terminal in the child after exec.
    fn detach_controlling_terminal(child: &mut Command) {
        // SAFETY: The pre_exec hook only calls async-signal-safe setsid.
        unsafe {
            child.pre_exec(|| setsid().map(|_| ()).map_err(std::io::Error::from));
        }
    }

    // Create a private terminal with known cell and pixel dimensions. The parent must keep the PTY
    // open until the child exits. Opening the endpoint here lets descriptor tests inherit it
    // without opening a terminal device in the child's new session.
    fn terminal_with_size() -> (rustix::fd::OwnedFd, File) {
        let pty = openpt(OpenptFlags::RDWR).unwrap();
        grantpt(&pty).unwrap();
        unlockpt(&pty).unwrap();
        let name = ptsname(&pty, Vec::new()).unwrap();
        let path = Path::new(OsStr::from_bytes(name.to_bytes()));
        let terminal = File::options().read(true).write(true).open(path).unwrap();
        let size = Winsize {
            ws_col: 81,
            ws_row: 23,
            ws_xpixel: 810,
            ws_ypixel: 460,
        };
        tcsetwinsize(&terminal, size).unwrap();
        (pty, terminal)
    }
}
