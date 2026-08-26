use std::env;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

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

#[derive(Debug)]
pub struct TerminalControlHandler {
    #[cfg(windows)]
    installed: bool,
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

            Self { installed }
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
}

#[cfg(any(unix, windows))]
use ratatui_image::FontSize;
#[cfg(any(unix, windows))]
use ratatui_image::picker::ProtocolType;
#[cfg(any(unix, windows))]
use ratatui_image::picker::cap_parser::{
    Parser as CapabilityParser, QueryStdioOptions, Response as CapabilityResponse,
};
#[cfg(any(unix, windows))]
use std::io::IsTerminal;
#[cfg(any(unix, windows))]
use std::time::{Duration, Instant};

#[cfg(any(unix, windows))]
const TERMINAL_CAPABILITY_QUERY_TIMEOUT: Duration = Duration::from_secs(1);
#[cfg(any(unix, windows))]
const DEFAULT_TERMINAL_FONT_SIZE: FontSize = FontSize::new(10, 20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalGraphicsProtocol {
    Kitty,
    Sixel,
    Iterm2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalGraphicsProbeStatus {
    Verified(TerminalGraphicsProtocol),
    Unsupported,
    NoResponse { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalCellSize {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalGraphicsCapabilities {
    pub status: TerminalGraphicsProbeStatus,
    pub cell_size: Option<TerminalCellSize>,
    pub is_tmux: bool,
    pub text_sizing_protocol: bool,
}

/// Performs the terminal graphics handshake and maps all parser-specific
/// values to platform-owned capability types.
pub fn probe_terminal_graphics_capabilities() -> TerminalGraphicsCapabilities {
    #[cfg(any(unix, windows))]
    {
        if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            return no_response_capabilities(
                "terminal graphics detection requires interactive stdin and stdout",
            );
        }
        match query_terminal_capabilities(TERMINAL_CAPABILITY_QUERY_TIMEOUT) {
            Ok(query) => terminal_probe_from_query(query),
            Err(error) => no_response_capabilities(error.to_string()),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        TerminalGraphicsCapabilities {
            status: TerminalGraphicsProbeStatus::Unsupported,
            cell_size: None,
            is_tmux: false,
            text_sizing_protocol: false,
        }
    }
}

fn no_response_capabilities(reason: impl Into<String>) -> TerminalGraphicsCapabilities {
    TerminalGraphicsCapabilities {
        status: TerminalGraphicsProbeStatus::NoResponse {
            reason: reason.into(),
        },
        cell_size: None,
        is_tmux: false,
        text_sizing_protocol: false,
    }
}

#[cfg(any(unix, windows))]
struct TerminalCapabilityQuery {
    protocol_type: ProtocolType,
    font_size: FontSize,
    is_tmux: bool,
    complete: bool,
    had_unverified_graphics_hint: bool,
    text_sizing_protocol: bool,
}

#[cfg(any(unix, windows))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum TerminalGraphicsCompatibility {
    #[default]
    Standard,
    WezTerm,
}

#[cfg(any(unix, windows))]
fn terminal_graphics_compatibility() -> TerminalGraphicsCompatibility {
    terminal_graphics_compatibility_with(|name| std::env::var(name).ok())
}

#[cfg(any(unix, windows))]
fn terminal_graphics_compatibility_with(
    value: impl Fn(&str) -> Option<String>,
) -> TerminalGraphicsCompatibility {
    let wezterm_executable = value("WEZTERM_EXECUTABLE").is_some_and(|value| !value.is_empty());
    let wezterm_program = value("TERM_PROGRAM").is_some_and(|value| value.contains("WezTerm"));
    if wezterm_executable || wezterm_program {
        TerminalGraphicsCompatibility::WezTerm
    } else {
        TerminalGraphicsCompatibility::Standard
    }
}

#[cfg(any(unix, windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Iterm2GraphicsCapabilities {
    file: bool,
    sixel: bool,
}

#[cfg(unix)]
fn query_terminal_capabilities(timeout: Duration) -> std::io::Result<TerminalCapabilityQuery> {
    use std::io;
    use std::os::fd::AsRawFd;

    let is_tmux = std::env::var_os("TMUX").is_some_and(|value| !value.is_empty());
    let compatibility = terminal_graphics_compatibility();
    write_terminal_capability_query(is_tmux, compatibility)?;

    let stdin = io::stdin();
    let fd = stdin.as_raw_fd();
    let deadline = Instant::now() + timeout;
    let mut responses = TerminalResponseCollector::new();

    while !responses.complete {
        let Some(timeout_ms) = deadline_wait_millis(deadline, Instant::now()) else {
            break;
        };
        let mut descriptor = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let poll_result =
            unsafe { libc::poll(&mut descriptor, 1, timeout_ms.min(i32::MAX as u32) as i32) };
        if poll_result == 0 {
            break;
        }
        if poll_result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if descriptor.revents & libc::POLLIN == 0 {
            break;
        }

        if deadline_wait_millis(deadline, Instant::now()).is_none() {
            break;
        }
        let mut byte = 0_u8;
        let read_result = unsafe { libc::read(fd, (&mut byte as *mut u8).cast(), 1) };
        if read_result == 0 {
            break;
        }
        if read_result < 0 {
            let error = io::Error::last_os_error();
            if matches!(
                error.kind(),
                io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
            ) {
                continue;
            }
            return Err(error);
        }

        responses.push_byte(byte);
    }

    let native_font_size = response_font_size(&responses.responses)
        .is_none()
        .then(native_terminal_font_size)
        .flatten();
    Ok(interpret_terminal_capability_responses(
        responses,
        is_tmux,
        native_font_size,
        compatibility,
    ))
}

#[cfg(windows)]
fn query_terminal_capabilities(timeout: Duration) -> std::io::Result<TerminalCapabilityQuery> {
    use std::io;
    use windows_sys::Win32::Foundation::{
        INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, INPUT_RECORD, KEY_EVENT, PeekConsoleInputW, ReadConsoleInputW,
        STD_INPUT_HANDLE,
    };
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    let is_tmux = std::env::var_os("TMUX").is_some_and(|value| !value.is_empty());
    let compatibility = terminal_graphics_compatibility();
    let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    if input.is_null() || input == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let _mode_guard = WindowsConsoleModeGuard::enable_virtual_terminal_input(input)?;
    write_terminal_capability_query(is_tmux, compatibility)?;

    let deadline = Instant::now() + timeout;
    let mut responses = TerminalResponseCollector::new();
    while !responses.complete {
        let Some(timeout_ms) = deadline_wait_millis(deadline, Instant::now()) else {
            break;
        };
        match unsafe { WaitForSingleObject(input, timeout_ms) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => break,
            WAIT_FAILED => {
                return Err(io::Error::last_os_error());
            }
            result => {
                return Err(io::Error::other(format!(
                    "unexpected console input wait result {result}"
                )));
            }
        }
        if deadline_wait_millis(deadline, Instant::now()).is_none() {
            break;
        }

        // Peek and then remove exactly one already-signaled record. No API in
        // this section can wait once the deadline check above has passed.
        let mut record = INPUT_RECORD::default();
        let mut peeked = 0;
        if unsafe { PeekConsoleInputW(input, &mut record, 1, &mut peeked) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if peeked == 0 {
            continue;
        }
        if deadline_wait_millis(deadline, Instant::now()).is_none() {
            break;
        }
        let mut read = 0;
        if unsafe { ReadConsoleInputW(input, &mut record, 1, &mut read) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if read != 1 {
            continue;
        }
        let (is_key_down, unicode_char) = if record.EventType == KEY_EVENT as u16 {
            let key = unsafe { record.Event.KeyEvent };
            (key.bKeyDown != 0, unsafe { key.uChar.UnicodeChar })
        } else {
            (false, 0)
        };
        if let Some(byte) = windows_vt_byte_from_record(
            record.EventType == KEY_EVENT as u16,
            is_key_down,
            unicode_char,
        ) {
            responses.push_byte(byte);
        }
    }

    let native_font_size = response_font_size(&responses.responses)
        .is_none()
        .then(native_terminal_font_size)
        .flatten();
    Ok(interpret_terminal_capability_responses(
        responses,
        is_tmux,
        native_font_size,
        compatibility,
    ))
}

#[cfg(unix)]
fn native_terminal_font_size() -> Option<FontSize> {
    let mut window = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut window) } < 0
        || window.ws_col == 0
        || window.ws_row == 0
        || window.ws_xpixel == 0
        || window.ws_ypixel == 0
    {
        return None;
    }
    font_size_from_terminal_geometry(
        window.ws_xpixel,
        window.ws_ypixel,
        window.ws_col,
        window.ws_row,
    )
}

#[cfg(any(unix, test))]
fn font_size_from_terminal_geometry(
    pixel_width: u16,
    pixel_height: u16,
    columns: u16,
    rows: u16,
) -> Option<FontSize> {
    if pixel_width == 0 || pixel_height == 0 || columns == 0 || rows == 0 {
        return None;
    }
    valid_font_size(FontSize::new(pixel_width / columns, pixel_height / rows))
}

#[cfg(windows)]
fn native_terminal_font_size() -> Option<FontSize> {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        CONSOLE_FONT_INFOEX, GetCurrentConsoleFontEx, GetStdHandle, STD_OUTPUT_HANDLE,
    };

    let output = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
    if output.is_null() || output == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut font = CONSOLE_FONT_INFOEX {
        cbSize: std::mem::size_of::<CONSOLE_FONT_INFOEX>() as u32,
        ..Default::default()
    };
    if unsafe { GetCurrentConsoleFontEx(output, 0, &mut font) } == 0
        || font.dwFontSize.X <= 0
        || font.dwFontSize.Y <= 0
    {
        return None;
    }
    valid_font_size(FontSize::new(
        font.dwFontSize.X as u16,
        font.dwFontSize.Y as u16,
    ))
}

#[cfg(windows)]
struct WindowsConsoleModeGuard {
    input: windows_sys::Win32::Foundation::HANDLE,
    original_mode: windows_sys::Win32::System::Console::CONSOLE_MODE,
}

#[cfg(windows)]
impl WindowsConsoleModeGuard {
    fn enable_virtual_terminal_input(
        input: windows_sys::Win32::Foundation::HANDLE,
    ) -> std::io::Result<Self> {
        use windows_sys::Win32::System::Console::{
            ENABLE_VIRTUAL_TERMINAL_INPUT, GetConsoleMode, SetConsoleMode,
        };

        let mut original_mode = 0;
        if unsafe { GetConsoleMode(input, &mut original_mode) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if unsafe { SetConsoleMode(input, original_mode | ENABLE_VIRTUAL_TERMINAL_INPUT) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self {
            input,
            original_mode,
        })
    }
}

#[cfg(windows)]
impl Drop for WindowsConsoleModeGuard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleMode(self.input, self.original_mode);
        }
    }
}

#[cfg(any(windows, test))]
fn windows_vt_byte_from_record(
    is_key_event: bool,
    is_key_down: bool,
    unicode_char: u16,
) -> Option<u8> {
    if !is_key_event || !is_key_down || unicode_char == 0 {
        return None;
    }
    u8::try_from(unicode_char).ok()
}

#[cfg(any(unix, windows))]
fn write_terminal_capability_query(
    is_tmux: bool,
    compatibility: TerminalGraphicsCompatibility,
) -> std::io::Result<()> {
    use std::io::{self, Write};

    let query = terminal_capability_query(is_tmux, compatibility);
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(query.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(Into::into)
}

#[cfg(any(unix, windows))]
struct TerminalResponseCollector {
    parser: CapabilityParser,
    responses: Vec<CapabilityResponse>,
    raw: Vec<u8>,
    complete: bool,
}

#[cfg(any(unix, windows))]
impl TerminalResponseCollector {
    fn new() -> Self {
        Self {
            parser: CapabilityParser::new(),
            responses: Vec::new(),
            raw: Vec::new(),
            complete: false,
        }
    }

    #[cfg(test)]
    fn push_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.push_byte(*byte);
            if self.complete {
                break;
            }
        }
    }

    fn push_byte(&mut self, byte: u8) {
        if self.raw.len() < 8 * 1024 {
            self.raw.push(byte);
        }
        for response in self.parser.push(char::from(byte)) {
            if response == CapabilityResponse::Status {
                self.complete = true;
                break;
            }
            self.responses.push(response);
        }
    }
}

#[cfg(any(unix, windows))]
fn interpret_terminal_capability_responses(
    responses: TerminalResponseCollector,
    is_tmux: bool,
    native_font_size: Option<FontSize>,
    compatibility: TerminalGraphicsCompatibility,
) -> TerminalCapabilityQuery {
    let iterm2_graphics = parse_iterm2_graphics_capabilities(&responses.raw);
    let standard_protocol = standard_protocol_from_responses(&responses.responses);
    let iterm2_protocol = iterm2_graphics.and_then(|capabilities| {
        capabilities
            .sixel
            .then_some(ProtocolType::Sixel)
            .or_else(|| capabilities.file.then_some(ProtocolType::Iterm2))
    });
    let protocol_type = match compatibility {
        TerminalGraphicsCompatibility::Standard => standard_protocol.or(iterm2_protocol),
        // ratatui-image's Kitty backend uses Unicode placeholders, which
        // WezTerm does not implement correctly. WezTerm does support the
        // iTerm2 inline-image protocol; require a completed terminal handshake
        // before selecting that environment-backed compatibility path.
        TerminalGraphicsCompatibility::WezTerm => {
            iterm2_protocol.or_else(|| responses.complete.then_some(ProtocolType::Iterm2))
        }
    };
    let had_unverified_graphics_hint =
        protocol_type.is_none() && terminal_has_graphics_environment_hint(is_tmux);
    TerminalCapabilityQuery {
        protocol_type: protocol_type.unwrap_or(ProtocolType::Halfblocks),
        font_size: font_size_from_responses(&responses.responses, native_font_size),
        is_tmux,
        complete: responses.complete,
        had_unverified_graphics_hint,
        text_sizing_protocol: responses_support_text_sizing_protocol(&responses.responses),
    }
}

#[cfg(any(unix, windows))]
fn terminal_probe_from_query(query: TerminalCapabilityQuery) -> TerminalGraphicsCapabilities {
    let has_live_protocol_response = query.protocol_type != ProtocolType::Halfblocks;
    let protocol = match query.protocol_type {
        ProtocolType::Kitty => Some(TerminalGraphicsProtocol::Kitty),
        ProtocolType::Sixel => Some(TerminalGraphicsProtocol::Sixel),
        ProtocolType::Iterm2 => Some(TerminalGraphicsProtocol::Iterm2),
        ProtocolType::Halfblocks => None,
    };
    let status = match protocol {
        Some(protocol) if query.complete || has_live_protocol_response => {
            TerminalGraphicsProbeStatus::Verified(protocol)
        }
        _ if query.complete && query.had_unverified_graphics_hint => {
            TerminalGraphicsProbeStatus::NoResponse {
                reason: "terminal responded, but the hinted graphics protocol did not answer its capability query".to_string(),
            }
        }
        _ if query.complete => TerminalGraphicsProbeStatus::Unsupported,
        _ => TerminalGraphicsProbeStatus::NoResponse {
            reason: "terminal did not return the graphics capability query terminator".to_string(),
        },
    };
    TerminalGraphicsCapabilities {
        status,
        cell_size: Some(TerminalCellSize {
            width: query.font_size.width,
            height: query.font_size.height,
        }),
        is_tmux: query.is_tmux,
        text_sizing_protocol: query.text_sizing_protocol,
    }
}

#[cfg(any(unix, windows))]
fn terminal_capability_query(
    is_tmux: bool,
    compatibility: TerminalGraphicsCompatibility,
) -> String {
    let blacklist_protocols = match compatibility {
        TerminalGraphicsCompatibility::Standard => Vec::new(),
        TerminalGraphicsCompatibility::WezTerm => {
            vec![ProtocolType::Kitty, ProtocolType::Sixel]
        }
    };
    let standard_query = CapabilityParser::query(
        is_tmux,
        QueryStdioOptions {
            text_sizing_protocol: true,
            blacklist_protocols,
            ..Default::default()
        },
    );
    let (start, escape, end) = CapabilityParser::tmux_start_escape_end(is_tmux);
    let final_status_query = format!("{escape}[5n{end}");
    let standard_commands = standard_query
        .strip_suffix(&final_status_query)
        .expect("ratatui-image capability query ends with a status query");
    debug_assert!(standard_commands.starts_with(start));
    format!("{standard_commands}{escape}]1337;Capabilities{escape}\\{final_status_query}")
}

#[cfg(any(unix, windows))]
fn terminal_has_graphics_environment_hint(is_tmux: bool) -> bool {
    terminal_has_graphics_environment_hint_with(is_tmux, |name| std::env::var(name).ok())
}

#[cfg(any(unix, windows))]
fn terminal_has_graphics_environment_hint_with(
    is_tmux: bool,
    value: impl Fn(&str) -> Option<String>,
) -> bool {
    let nonempty = |name| value(name).is_some_and(|value| !value.is_empty());
    if is_tmux && (nonempty("ITERM_SESSION_ID") || nonempty("WEZTERM_EXECUTABLE")) {
        return true;
    }
    value("TERM_PROGRAM").is_some_and(|term_program| {
        [
            "iTerm",
            "WezTerm",
            "mintty",
            "vscode",
            "Tabby",
            "Hyper",
            "rio",
            "Bobcat",
            "WarpTerminal",
        ]
        .iter()
        .any(|hint| term_program.contains(hint))
    }) || value("LC_TERMINAL").is_some_and(|terminal| terminal.contains("iTerm"))
}

#[cfg(any(unix, windows))]
fn parse_iterm2_graphics_capabilities(response: &[u8]) -> Option<Iterm2GraphicsCapabilities> {
    const PREFIX: &[u8] = b"\x1b]1337;Capabilities=";
    let start = response
        .windows(PREFIX.len())
        .position(|window| window == PREFIX)?
        + PREFIX.len();
    let tail = &response[start..];
    let end = tail
        .iter()
        .position(|byte| *byte == b'\x07')
        .or_else(|| tail.windows(2).position(|window| window == b"\x1b\\"))?;
    let features = tail[..end]
        .iter()
        .copied()
        .take_while(u8::is_ascii_alphanumeric)
        .collect::<Vec<_>>();
    Some(Iterm2GraphicsCapabilities {
        file: iterm2_feature_present(&features, b"F"),
        sixel: iterm2_feature_present(&features, b"Sx"),
    })
}

#[cfg(any(unix, windows))]
fn iterm2_feature_present(features: &[u8], expected: &[u8]) -> bool {
    let mut start = 0;
    for end in 1..=features.len() {
        if end == features.len() || features[end].is_ascii_uppercase() {
            if &features[start..end] == expected {
                return true;
            }
            start = end;
        }
    }
    false
}

#[cfg(any(unix, windows))]
fn standard_protocol_from_responses(responses: &[CapabilityResponse]) -> Option<ProtocolType> {
    if responses.contains(&CapabilityResponse::Kitty) {
        Some(ProtocolType::Kitty)
    } else if responses.contains(&CapabilityResponse::Sixel) {
        Some(ProtocolType::Sixel)
    } else {
        None
    }
}

#[cfg(any(unix, windows))]
fn responses_support_text_sizing_protocol(responses: &[CapabilityResponse]) -> bool {
    let positions = responses
        .iter()
        .filter_map(|response| match response {
            CapabilityResponse::CursorPositionReport(x, y) => Some((*x, *y)),
            _ => None,
        })
        .collect::<Vec<_>>();
    matches!(positions.as_slice(), [(x1, _), (x2, _), (x3, _)] if *x2 == x1.saturating_add(2) && *x3 == x2.saturating_add(2))
}

#[cfg(any(unix, windows))]
fn font_size_from_responses(
    responses: &[CapabilityResponse],
    native_font_size: Option<FontSize>,
) -> FontSize {
    response_font_size(responses)
        .or_else(|| native_font_size.and_then(valid_font_size))
        .unwrap_or(DEFAULT_TERMINAL_FONT_SIZE)
}

#[cfg(any(unix, windows))]
fn response_font_size(responses: &[CapabilityResponse]) -> Option<FontSize> {
    responses.iter().find_map(|response| match response {
        CapabilityResponse::CellSize(Some((width, height))) => {
            valid_font_size(FontSize::new(*width, *height))
        }
        _ => None,
    })
}

#[cfg(any(unix, windows, test))]
fn valid_font_size(font_size: FontSize) -> Option<FontSize> {
    (font_size.width > 0 && font_size.height > 0).then_some(font_size)
}

#[cfg(any(unix, windows))]
fn deadline_wait_millis(deadline: Instant, now: Instant) -> Option<u32> {
    let remaining = deadline.checked_duration_since(now)?;
    if remaining.is_zero() {
        return None;
    }
    Some(remaining.as_millis().max(1).min(u32::MAX as u128) as u32)
}

#[cfg(test)]
mod graphics_tests {
    use super::*;

    fn query(protocol_type: ProtocolType, complete: bool, hinted: bool) -> TerminalCapabilityQuery {
        TerminalCapabilityQuery {
            protocol_type,
            font_size: DEFAULT_TERMINAL_FONT_SIZE,
            is_tmux: false,
            complete,
            had_unverified_graphics_hint: hinted,
            text_sizing_protocol: false,
        }
    }

    #[test]
    fn capability_query_orders_confirmations_before_one_final_terminator() {
        for (compatibility, expects_standard_queries) in [
            (TerminalGraphicsCompatibility::Standard, true),
            (TerminalGraphicsCompatibility::WezTerm, false),
        ] {
            for is_tmux in [false, true] {
                let query = terminal_capability_query(is_tmux, compatibility);
                let (_, escape, end) = CapabilityParser::tmux_start_escape_end(is_tmux);
                let iterm = query.find("]1337;Capabilities").expect("iTerm query");
                let status = format!("{escape}[5n");
                let terminator = query.rfind(&status).expect("final status query");

                assert!(iterm < terminator);
                assert_eq!(query.matches(&status).count(), 1);
                assert!(query.ends_with(&format!("{status}{end}")));
                assert_eq!(query.contains("_Gi=31"), expects_standard_queries);
                assert_eq!(query.contains("[c"), expects_standard_queries);
                if expects_standard_queries {
                    assert!(query.find("_Gi=31").unwrap() < query.find("[c").unwrap());
                    assert!(query.find("[c").unwrap() < iterm);
                }
            }
        }
    }

    #[test]
    fn wezterm_completed_handshake_falls_back_to_iterm2() {
        for is_tmux in [false, true] {
            let mut responses = TerminalResponseCollector::new();
            responses.push_bytes(b"\x1b[0n");
            let interpreted = interpret_terminal_capability_responses(
                responses,
                is_tmux,
                Some(DEFAULT_TERMINAL_FONT_SIZE),
                TerminalGraphicsCompatibility::WezTerm,
            );
            assert_eq!(interpreted.protocol_type, ProtocolType::Iterm2);
        }
    }

    #[test]
    fn protocol_priority_and_text_sizing_match_live_response_semantics() {
        let raw = b"\x1b[?64;4c\x1b[1;1R\x1b_Gi=31;OK\x1b\\\x1b[1;3R\x1b[2;5R\x1b[0n";
        let mut parser = CapabilityParser::new();
        let responses = raw
            .iter()
            .flat_map(|byte| parser.push(char::from(*byte)))
            .collect::<Vec<_>>();
        assert_eq!(
            standard_protocol_from_responses(&responses),
            Some(ProtocolType::Kitty)
        );
        assert_eq!(
            standard_protocol_from_responses(&[CapabilityResponse::Sixel]),
            Some(ProtocolType::Sixel)
        );
        assert!(responses_support_text_sizing_protocol(&responses));
        assert!(!responses_support_text_sizing_protocol(&[
            CapabilityResponse::CursorPositionReport(1, 1),
            CapabilityResponse::CursorPositionReport(3, 1),
            CapabilityResponse::CursorPositionReport(4, 1),
        ]));
    }

    #[test]
    fn cell_geometry_priority_is_response_then_native_then_default() {
        let interpret = |responses, native| {
            interpret_terminal_capability_responses(
                TerminalResponseCollector {
                    parser: CapabilityParser::new(),
                    responses,
                    raw: Vec::new(),
                    complete: true,
                },
                false,
                native,
                TerminalGraphicsCompatibility::Standard,
            )
            .font_size
        };
        let response = interpret(
            vec![CapabilityResponse::CellSize(Some((7, 13)))],
            Some(FontSize::new(8, 16)),
        );
        assert_eq!((response.width, response.height), (7, 13));
        let native = interpret(Vec::new(), Some(FontSize::new(8, 16)));
        assert_eq!((native.width, native.height), (8, 16));
        let invalid_response = interpret(
            vec![CapabilityResponse::CellSize(Some((0, 13)))],
            Some(FontSize::new(9, 18)),
        );
        assert_eq!((invalid_response.width, invalid_response.height), (9, 18));
        for invalid_native in [FontSize::new(0, 16), FontSize::new(8, 0)] {
            let fallback = interpret(Vec::new(), Some(invalid_native));
            assert_eq!(
                (fallback.width, fallback.height),
                (
                    DEFAULT_TERMINAL_FONT_SIZE.width,
                    DEFAULT_TERMINAL_FONT_SIZE.height
                )
            );
        }
        for geometry in [
            (0, 320, 80, 20),
            (640, 0, 80, 20),
            (79, 320, 80, 20),
            (640, 19, 80, 20),
        ] {
            assert!(
                font_size_from_terminal_geometry(geometry.0, geometry.1, geometry.2, geometry.3)
                    .is_none()
            );
        }
    }

    #[test]
    fn deadline_wait_never_waits_after_deadline() {
        let now = Instant::now();
        assert_eq!(deadline_wait_millis(now, now), None);
        assert_eq!(
            deadline_wait_millis(now + Duration::from_nanos(1), now),
            Some(1)
        );
        assert_eq!(
            deadline_wait_millis(now + Duration::from_millis(25), now),
            Some(25)
        );
        assert_eq!(
            deadline_wait_millis(now, now + Duration::from_millis(1)),
            None
        );
    }

    #[test]
    fn probe_status_preserves_verified_unsupported_and_no_response() {
        assert_eq!(
            terminal_probe_from_query(query(ProtocolType::Kitty, false, false)).status,
            TerminalGraphicsProbeStatus::Verified(TerminalGraphicsProtocol::Kitty)
        );
        assert_eq!(
            terminal_probe_from_query(query(ProtocolType::Halfblocks, true, false)).status,
            TerminalGraphicsProbeStatus::Unsupported
        );
        assert!(matches!(
            terminal_probe_from_query(query(ProtocolType::Halfblocks, false, false)).status,
            TerminalGraphicsProbeStatus::NoResponse { .. }
        ));
        assert!(matches!(
            terminal_probe_from_query(query(ProtocolType::Halfblocks, true, true)).status,
            TerminalGraphicsProbeStatus::NoResponse { .. }
        ));
    }

    #[test]
    fn response_collection_stops_at_status_and_preserves_geometry() {
        let mut input = b"\x1b[6;9;17t\x1b_Gi=31;OK\x1b\\\x1b[0nx".iter().copied();
        let mut responses = TerminalResponseCollector::new();
        while !responses.complete {
            responses.push_byte(input.next().expect("complete response"));
        }
        assert_eq!(input.next(), Some(b'x'));
        let capabilities = terminal_probe_from_query(interpret_terminal_capability_responses(
            responses,
            false,
            None,
            TerminalGraphicsCompatibility::Standard,
        ));
        assert_eq!(
            capabilities.cell_size,
            Some(TerminalCellSize {
                width: 17,
                height: 9
            })
        );
    }

    #[test]
    fn environment_compatibility_is_injected_without_process_mutation() {
        let values = |name: &str| match name {
            "TERM_PROGRAM" => Some("WezTerm".to_string()),
            _ => None,
        };
        assert_eq!(
            terminal_graphics_compatibility_with(values),
            TerminalGraphicsCompatibility::WezTerm
        );
        assert!(terminal_has_graphics_environment_hint_with(false, |name| {
            (name == "LC_TERMINAL").then(|| "iTerm2".to_string())
        }));
        assert!(terminal_has_graphics_environment_hint_with(true, |name| {
            (name == "ITERM_SESSION_ID").then(|| "session".to_string())
        }));
    }

    #[test]
    fn response_parsers_cover_iterm_text_sizing_and_native_geometry() {
        assert_eq!(
            parse_iterm2_graphics_capabilities(b"\x1b]1337;Capabilities=AFN\x1b\\"),
            Some(Iterm2GraphicsCapabilities {
                file: true,
                sixel: false
            })
        );
        assert_eq!(
            parse_iterm2_graphics_capabilities(b"prefix\x1b]1337;Capabilities=ASxN\x07suffix"),
            Some(Iterm2GraphicsCapabilities {
                file: false,
                sixel: true
            })
        );
        assert_eq!(
            parse_iterm2_graphics_capabilities(b"\x1b]1337;Capabilities=FooN\x07"),
            Some(Iterm2GraphicsCapabilities {
                file: false,
                sixel: false
            })
        );
        assert_eq!(parse_iterm2_graphics_capabilities(b"\x1b[?1;0c"), None);
        assert!(responses_support_text_sizing_protocol(&[
            CapabilityResponse::CursorPositionReport(1, 1),
            CapabilityResponse::CursorPositionReport(3, 1),
            CapabilityResponse::CursorPositionReport(5, 1),
        ]));
        let size = font_size_from_terminal_geometry(640, 320, 80, 20).unwrap();
        assert_eq!((size.width, size.height), (8, 16));
    }

    #[test]
    fn windows_console_record_mapping_only_accepts_key_down_bytes() {
        assert_eq!(windows_vt_byte_from_record(false, true, b'x'.into()), None);
        assert_eq!(windows_vt_byte_from_record(true, false, b'x'.into()), None);
        assert_eq!(
            windows_vt_byte_from_record(true, true, b'x'.into()),
            Some(b'x')
        );
        assert_eq!(windows_vt_byte_from_record(true, true, 0x100), None);
    }

    #[test]
    fn ui_source_and_manifest_do_not_own_terminal_probing() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui");
        fn rust_source(directory: &std::path::Path, output: &mut String) {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    rust_source(&path, output);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    output.push_str(&std::fs::read_to_string(path).unwrap());
                }
            }
        }
        let mut source = String::new();
        rust_source(&root.join("src"), &mut source);
        for forbidden in [
            "std::env",
            "stdin()",
            "stdout()",
            "libc::",
            "GetStdHandle",
            "prepare_path",
            "ImageReader::open",
        ] {
            assert!(
                !source.contains(forbidden),
                "UI retained forbidden boundary: {forbidden}"
            );
        }
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        assert!(!manifest.contains("libc ="));
        assert!(!manifest.contains("windows-sys"));
        assert!(!manifest.contains("platform ="));
        assert!(!manifest.contains("system-services ="));
        let platform_source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/terminal.rs"),
        )
        .unwrap();
        assert!(platform_source.contains("probe_terminal_graphics_capabilities"));
        assert!(platform_source.contains("CapabilityParser"));
        for declaration in [
            "const TERMINAL_CAPABILITY_QUERY_TIMEOUT: Duration",
            "const DEFAULT_TERMINAL_FONT_SIZE: FontSize",
        ] {
            let declaration = platform_source.find(declaration).unwrap();
            let prefix = &platform_source[..declaration];
            assert!(
                prefix.ends_with("#[cfg(any(unix, windows))]\n"),
                "target-specific declaration lost its matching cfg: {declaration}"
            );
        }
    }
}
