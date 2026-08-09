use std::env;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
use std::ffi::c_void;

use crate::PlatformKind;
use crate::diagnostics::{CheckStatus, EnvironmentCheck};

pub const ENTER_FULLSCREEN_SEQUENCE: &str = "\x1B[?1049h\x1B[?25l\x1B[2J\x1B[H";
pub const EXIT_FULLSCREEN_SEQUENCE: &str = "\x1B[?25h\x1B[?1049l";

static TERMINAL_SHUTDOWN_REQUESTED: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn shared_shutdown_flag() -> &'static Arc<AtomicBool> {
    TERMINAL_SHUTDOWN_REQUESTED.get_or_init(|| Arc::new(AtomicBool::new(false)))
}

#[cfg(target_os = "linux")]
pub(crate) fn request_process_shutdown() {
    shared_shutdown_flag().store(true, Ordering::SeqCst);
}

pub fn with_terminal_fullscreen<W, T>(
    output: &mut W,
    body: impl FnOnce(&mut W) -> io::Result<T>,
) -> io::Result<T>
where
    W: Write,
{
    write!(output, "{ENTER_FULLSCREEN_SEQUENCE}")?;
    output.flush()?;

    let body_result = body(output);
    let exit_result = write!(output, "{EXIT_FULLSCREEN_SEQUENCE}").and_then(|_| output.flush());

    match (body_result, exit_result) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

pub fn terminal_environment_check(kind: PlatformKind) -> EnvironmentCheck {
    let wt_session = env::var("WT_SESSION").ok();
    terminal_environment_check_with(kind, wt_session.as_deref())
}

pub fn terminal_environment_check_with(
    kind: PlatformKind,
    wt_session: Option<&str>,
) -> EnvironmentCheck {
    terminal_environment_check_with_graphics_protocol(kind, wt_session, None)
}

/// Builds the terminal diagnostics result from an already-probed inline
/// graphics protocol. Merely identifying a terminal emulator is not enough:
/// the current UI requires capabilities such as native image rendering before
/// the terminal check can pass.
pub fn terminal_environment_check_with_graphics_protocol(
    kind: PlatformKind,
    wt_session: Option<&str>,
    graphics_protocol: Option<&str>,
) -> EnvironmentCheck {
    if let Some(protocol) = graphics_protocol.filter(|value| !value.trim().is_empty()) {
        return EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Pass,
            message: format!(
                "{} graphics protocol detected; image and advanced UI features are supported",
                protocol.trim()
            ),
        };
    }

    match kind {
        PlatformKind::Windows => {
            if is_windows_terminal_session(wt_session) {
                EnvironmentCheck {
                    label: "Terminal".to_string(),
                    status: CheckStatus::Warning,
                    message: "Windows Terminal detected, but no inline graphics protocol was detected; text-only UI is available"
                        .to_string(),
                }
            } else {
                EnvironmentCheck {
                    label: "Terminal".to_string(),
                    status: CheckStatus::Warning,
                    message: "No inline graphics protocol detected; this terminal is text-only and advanced UI features are unavailable"
                        .to_string(),
                }
            }
        }
        PlatformKind::Macos | PlatformKind::Linux => EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Warning,
            message: "No inline graphics protocol detected; this terminal is text-only and advanced UI features are unavailable"
                .to_string(),
        },
        PlatformKind::Unsupported => EnvironmentCheck {
            label: "Terminal".to_string(),
            status: CheckStatus::Warning,
            message: "No supported inline graphics protocol detected on this platform; only text UI can be assumed"
                .to_string(),
        },
    }
}

pub fn is_windows_terminal_session(wt_session: Option<&str>) -> bool {
    wt_session
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

/// Dispatches pending desktop messages for the thread that installed a
/// [`TerminalControlHandler`].
///
/// Windows only delivers session-end broadcasts to a window on the thread's
/// message queue. GUI launchers must call this regularly from their existing
/// supervisory loop; this function never blocks and never creates a thread.
/// On non-Windows platforms it is intentionally a no-op.
pub fn pump_desktop_shutdown_events() {
    #[cfg(windows)]
    unsafe {
        let mut message = Msg::default();
        while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

#[derive(Debug)]
pub struct TerminalControlHandler {
    #[cfg(windows)]
    installed: bool,
    #[cfg(windows)]
    shutdown_window: Option<ShutdownWindow>,
    #[cfg(unix)]
    signal_ids: Vec<signal_hook::SigId>,
}

impl TerminalControlHandler {
    pub fn install() -> Self {
        shared_shutdown_flag().store(false, Ordering::SeqCst);

        #[cfg(windows)]
        {
            let installed =
                unsafe { SetConsoleCtrlHandler(Some(handle_console_control), true.into()) != 0 };

            // A GUI subsystem executable has no useful console-control event
            // source during logoff/shutdown. Keep the console handler as a
            // fallback, then add a hidden *top-level* window: message-only
            // windows do not receive WM_QUERYENDSESSION broadcasts.
            Self {
                installed,
                shutdown_window: ShutdownWindow::create(),
            }
        }

        #[cfg(unix)]
        {
            use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};
            let signal_ids = [SIGINT, SIGTERM, SIGHUP]
                .into_iter()
                .filter_map(|signal| {
                    signal_hook::flag::register(signal, Arc::clone(shared_shutdown_flag())).ok()
                })
                .collect();
            Self { signal_ids }
        }

        #[cfg(not(any(windows, unix)))]
        {
            Self {}
        }
    }

    pub fn shutdown_requested(&self) -> bool {
        pump_desktop_shutdown_events();
        shared_shutdown_flag().load(Ordering::SeqCst)
    }

    /// Returns the process-wide termination source shared by the lock screen
    /// and the main Shell session.
    pub fn shutdown_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(shared_shutdown_flag())
    }
}

impl Drop for TerminalControlHandler {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            if let Some(window) = self.shutdown_window.take() {
                unsafe {
                    DestroyWindow(window.handle);
                }
            }
            if self.installed {
                unsafe {
                    SetConsoleCtrlHandler(Some(handle_console_control), false.into());
                }
            }
        }

        #[cfg(unix)]
        {
            for signal_id in self.signal_ids.drain(..) {
                signal_hook::low_level::unregister(signal_id);
            }
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn handle_console_control(control_type: u32) -> i32 {
    match control_type {
        CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT
        | CTRL_SHUTDOWN_EVENT => {
            if let Some(shutdown) = TERMINAL_SHUTDOWN_REQUESTED.get() {
                shutdown.store(true, Ordering::SeqCst);
            }
            true.into()
        }
        _ => false.into(),
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct ShutdownWindow {
    handle: Hwnd,
}

#[cfg(windows)]
impl ShutdownWindow {
    fn create() -> Option<Self> {
        unsafe {
            let instance = GetModuleHandleW(std::ptr::null());
            if instance.is_null() || !register_shutdown_window_class(instance) {
                return None;
            }

            let handle = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                SHUTDOWN_WINDOW_CLASS.as_ptr(),
                SHUTDOWN_WINDOW_TITLE.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                instance,
                std::ptr::null_mut(),
            );
            (!handle.is_null()).then_some(Self { handle })
        }
    }
}

#[cfg(windows)]
unsafe fn register_shutdown_window_class(instance: Hinstance) -> bool {
    let class = WndClassW {
        style: 0,
        lpfn_wnd_proc: Some(shutdown_window_proc),
        cb_cls_extra: 0,
        cb_wnd_extra: 0,
        h_instance: instance,
        h_icon: std::ptr::null_mut(),
        h_cursor: std::ptr::null_mut(),
        hbr_background: std::ptr::null_mut(),
        lpsz_menu_name: std::ptr::null(),
        lpsz_class_name: SHUTDOWN_WINDOW_CLASS.as_ptr(),
    };
    let atom = unsafe { RegisterClassW(&class) };
    atom != 0 || unsafe { GetLastError() } == ERROR_CLASS_ALREADY_EXISTS
}

#[cfg(windows)]
unsafe extern "system" fn shutdown_window_proc(
    window: Hwnd,
    message: u32,
    w_param: Wparam,
    l_param: Lparam,
) -> Lresult {
    match message {
        WM_QUERYENDSESSION => {
            if let Some(shutdown) = TERMINAL_SHUTDOWN_REQUESTED.get() {
                shutdown.store(true, Ordering::SeqCst);
            }
            true.into()
        }
        WM_ENDSESSION => {
            if let Some(shutdown) = TERMINAL_SHUTDOWN_REQUESTED.get() {
                shutdown.store(true, Ordering::SeqCst);
            }
            0
        }
        _ => unsafe { DefWindowProcW(window, message, w_param, l_param) },
    }
}

#[cfg(windows)]
const WM_QUERYENDSESSION: u32 = 0x0011;
#[cfg(windows)]
const WM_ENDSESSION: u32 = 0x0016;
#[cfg(windows)]
const PM_REMOVE: u32 = 0x0001;
#[cfg(windows)]
const WS_POPUP: u32 = 0x8000_0000;
#[cfg(windows)]
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
#[cfg(windows)]
const ERROR_CLASS_ALREADY_EXISTS: u32 = 1410;

#[cfg(windows)]
const SHUTDOWN_WINDOW_CLASS: &[u16] = &[
    b'T' as u16,
    b'u' as u16,
    b'n' as u16,
    b'd' as u16,
    b'r' as u16,
    b'a' as u16,
    b'U' as u16,
    b'X' as u16,
    b'3' as u16,
    b'.' as u16,
    b'S' as u16,
    b'h' as u16,
    b'u' as u16,
    b't' as u16,
    b'd' as u16,
    b'o' as u16,
    b'w' as u16,
    b'n' as u16,
    b'W' as u16,
    b'a' as u16,
    b't' as u16,
    b'c' as u16,
    b'h' as u16,
    b'e' as u16,
    b'r' as u16,
    b'.' as u16,
    b'v' as u16,
    b'1' as u16,
    0,
];
#[cfg(windows)]
const SHUTDOWN_WINDOW_TITLE: &[u16] = &[
    b'T' as u16,
    b'u' as u16,
    b'n' as u16,
    b'd' as u16,
    b'r' as u16,
    b'a' as u16,
    0,
];

#[cfg(windows)]
type Hwnd = *mut c_void;
#[cfg(windows)]
type Hinstance = *mut c_void;
#[cfg(windows)]
type Wparam = usize;
#[cfg(windows)]
type Lparam = isize;
#[cfg(windows)]
type Lresult = isize;

#[cfg(windows)]
#[repr(C)]
struct WndClassW {
    style: u32,
    lpfn_wnd_proc: Option<unsafe extern "system" fn(Hwnd, u32, Wparam, Lparam) -> Lresult>,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: Hinstance,
    h_icon: *mut c_void,
    h_cursor: *mut c_void,
    hbr_background: *mut c_void,
    lpsz_menu_name: *const u16,
    lpsz_class_name: *const u16,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct Point {
    x: i32,
    y: i32,
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    w_param: Wparam,
    l_param: Lparam,
    time: u32,
    point: Point,
    l_private: u32,
}

#[cfg(windows)]
const CTRL_C_EVENT: u32 = 0;
#[cfg(windows)]
const CTRL_BREAK_EVENT: u32 = 1;
#[cfg(windows)]
const CTRL_CLOSE_EVENT: u32 = 2;
#[cfg(windows)]
const CTRL_LOGOFF_EVENT: u32 = 5;
#[cfg(windows)]
const CTRL_SHUTDOWN_EVENT: u32 = 6;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn SetConsoleCtrlHandler(
        handler_routine: Option<unsafe extern "system" fn(u32) -> i32>,
        add: i32,
    ) -> i32;
    fn GetModuleHandleW(module_name: *const u16) -> Hinstance;
    fn GetLastError() -> u32;
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassW(window_class: *const WndClassW) -> u16;
    fn CreateWindowExW(
        extended_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: *mut c_void,
        instance: Hinstance,
        parameter: *mut c_void,
    ) -> Hwnd;
    fn DestroyWindow(window: Hwnd) -> i32;
    fn DefWindowProcW(window: Hwnd, message: u32, w_param: Wparam, l_param: Lparam) -> Lresult;
    fn PeekMessageW(
        message: *mut Msg,
        window: Hwnd,
        min_filter: u32,
        max_filter: u32,
        remove: u32,
    ) -> i32;
    fn TranslateMessage(message: *const Msg) -> i32;
    fn DispatchMessageW(message: *const Msg) -> Lresult;
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn session_end_broadcasts_set_the_shared_shutdown_flag() {
        let handler = TerminalControlHandler::install();
        assert!(!handler.shutdown_requested());

        unsafe {
            shutdown_window_proc(std::ptr::null_mut(), WM_QUERYENDSESSION, 0, 0);
        }
        assert!(handler.shutdown_requested());

        shared_shutdown_flag().store(false, Ordering::SeqCst);
        unsafe {
            shutdown_window_proc(std::ptr::null_mut(), WM_ENDSESSION, 1, 0);
        }
        assert!(handler.shutdown_requested());
        shared_shutdown_flag().store(false, Ordering::SeqCst);
    }

    #[test]
    fn shutdown_window_class_can_be_registered_repeatedly() {
        let first = ShutdownWindow::create();
        let second = ShutdownWindow::create();
        assert!(first.is_some());
        assert!(second.is_some());
    }
}
