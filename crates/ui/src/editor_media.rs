use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(any(unix, windows))]
use std::time::{Duration, Instant};

use image::{DynamicImage, ImageReader, RgbaImage};
use ratatui::Frame;
use ratatui::layout::{Rect, Size};
use ratatui_image::FontSize;
use ratatui_image::Image;
use ratatui_image::Resize;
use ratatui_image::picker::ProtocolType;
#[cfg(any(unix, windows))]
use ratatui_image::picker::cap_parser::{
    Parser as CapabilityParser, QueryStdioOptions, Response as CapabilityResponse,
};
use ratatui_image::protocol::Protocol;
use ratatui_image::protocol::iterm2::Iterm2;
use ratatui_image::protocol::kitty::Kitty;
use ratatui_image::protocol::sixel::Sixel;

pub const EDITOR_IMAGE_MAX_PIXELS: u64 = 20_000_000;
#[cfg(any(unix, windows))]
const TERMINAL_CAPABILITY_QUERY_TIMEOUT: Duration = Duration::from_secs(1);
const DEFAULT_TERMINAL_FONT_SIZE: FontSize = FontSize::new(10, 20);
static NEXT_KITTY_IMAGE_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorGraphicsProtocol {
    Kitty,
    Sixel,
    Iterm2,
}

impl EditorGraphicsProtocol {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Kitty => "Kitty",
            Self::Sixel => "Sixel",
            Self::Iterm2 => "iTerm2",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalGraphicsProbeStatus {
    Verified(EditorGraphicsProtocol),
    Unsupported,
    NoResponse { reason: String },
}

impl TerminalGraphicsProbeStatus {
    pub const fn protocol(&self) -> Option<EditorGraphicsProtocol> {
        match self {
            Self::Verified(protocol) => Some(*protocol),
            Self::Unsupported | Self::NoResponse { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TerminalGraphicsProbe {
    status: TerminalGraphicsProbeStatus,
    picker: Option<EditorImagePicker>,
    text_sizing_protocol: bool,
}

impl TerminalGraphicsProbe {
    pub fn verified(picker: EditorImagePicker) -> Self {
        Self {
            status: TerminalGraphicsProbeStatus::Verified(picker.protocol()),
            picker: Some(picker),
            text_sizing_protocol: false,
        }
    }

    pub fn unsupported() -> Self {
        Self {
            status: TerminalGraphicsProbeStatus::Unsupported,
            picker: None,
            text_sizing_protocol: false,
        }
    }

    pub fn no_response(reason: impl Into<String>) -> Self {
        Self {
            status: TerminalGraphicsProbeStatus::NoResponse {
                reason: reason.into(),
            },
            picker: None,
            text_sizing_protocol: false,
        }
    }

    pub fn status(&self) -> &TerminalGraphicsProbeStatus {
        &self.status
    }

    pub fn picker(&self) -> Option<&EditorImagePicker> {
        self.picker.as_ref()
    }

    pub const fn protocol(&self) -> Option<EditorGraphicsProtocol> {
        self.status.protocol()
    }

    pub const fn text_sizing_protocol(&self) -> bool {
        self.text_sizing_protocol
    }

    fn with_text_sizing_protocol(mut self, supported: bool) -> Self {
        self.text_sizing_protocol = supported;
        self
    }
}

#[derive(Debug, Clone)]
pub struct EditorImagePicker {
    font_size: FontSize,
    protocol: EditorGraphicsProtocol,
    is_tmux: bool,
}

impl EditorImagePicker {
    /// Query after entering the alternate screen and before the event loop starts.
    /// Half-block rendering is intentionally treated as unsupported: the Editor
    /// contract requires raw Markdown fallback when no graphics protocol exists.
    pub fn detect_stdio() -> Result<Option<Self>, EditorMediaError> {
        match Self::probe_stdio() {
            TerminalGraphicsProbe {
                picker: Some(picker),
                ..
            } => Ok(Some(picker)),
            TerminalGraphicsProbe {
                status: TerminalGraphicsProbeStatus::Unsupported,
                ..
            } => Ok(None),
            TerminalGraphicsProbe {
                status: TerminalGraphicsProbeStatus::NoResponse { reason },
                ..
            } => Err(EditorMediaError::TerminalQuery(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                reason,
            ))),
            TerminalGraphicsProbe {
                status: TerminalGraphicsProbeStatus::Verified(_),
                picker: None,
                ..
            } => unreachable!("verified terminal graphics probes always carry a picker"),
        }
    }

    /// Performs a live capability handshake and preserves the distinction
    /// between an explicit text-only response and a terminal that never
    /// answered the query.
    pub fn probe_stdio() -> TerminalGraphicsProbe {
        #[cfg(any(unix, windows))]
        {
            match query_terminal_capabilities(TERMINAL_CAPABILITY_QUERY_TIMEOUT) {
                Ok(query) => terminal_probe_from_query(query),
                Err(error) => TerminalGraphicsProbe::no_response(error.to_string()),
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            TerminalGraphicsProbe::unsupported()
        }
    }

    pub fn protocol(&self) -> EditorGraphicsProtocol {
        self.protocol
    }

    pub fn prepare_path(
        &self,
        path: &Path,
        area: Rect,
    ) -> Result<PreparedEditorImage, EditorMediaError> {
        let image = ImageReader::open(path)
            .map_err(|error| EditorMediaError::Decode(error.to_string()))?
            .with_guessed_format()
            .map_err(|error| EditorMediaError::Decode(error.to_string()))?
            .decode()
            .map_err(|error| EditorMediaError::Decode(error.to_string()))?;
        self.prepare(image, area)
    }

    pub fn prepare_bytes(
        &self,
        bytes: &[u8],
        area: Rect,
    ) -> Result<PreparedEditorImage, EditorMediaError> {
        let image = image::load_from_memory(bytes)
            .map_err(|error| EditorMediaError::Decode(error.to_string()))?;
        self.prepare(image, area)
    }

    pub fn prepare(
        &self,
        image: DynamicImage,
        area: Rect,
    ) -> Result<PreparedEditorImage, EditorMediaError> {
        let pixels = u64::from(image.width()).saturating_mul(u64::from(image.height()));
        if pixels > EDITOR_IMAGE_MAX_PIXELS {
            return Err(EditorMediaError::TooLarge {
                width: image.width(),
                height: image.height(),
            });
        }
        let resize = Resize::Fit(None);
        let size = resize.size_for(&image, self.font_size, area.as_size());
        let image = resize.resize(&image, self.font_size, size, None);
        let protocol = match self.protocol {
            EditorGraphicsProtocol::Kitty => Protocol::Kitty(
                Kitty::new(image, size, next_kitty_image_id(), self.is_tmux)
                    .map_err(EditorMediaError::Protocol)?,
            ),
            EditorGraphicsProtocol::Sixel => Protocol::Sixel(
                Sixel::new(image, size, self.is_tmux).map_err(EditorMediaError::Protocol)?,
            ),
            EditorGraphicsProtocol::Iterm2 => Protocol::ITerm2(
                Iterm2::new(image, size, self.is_tmux).map_err(EditorMediaError::Protocol)?,
            ),
        };
        Ok(PreparedEditorImage {
            protocol,
            kind: self.protocol,
        })
    }

    /// Prepares a native RGBA icon for rendering through the detected terminal
    /// graphics protocol. `rgba` must contain exactly four bytes per pixel.
    ///
    /// This is intentionally owned input: platform icon APIs commonly hand out
    /// temporary buffers, while a prepared terminal image may outlive that API
    /// call until the next render pass.
    pub fn prepare_rgba(
        &self,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
        area: Rect,
    ) -> Result<PreparedEditorImage, EditorMediaError> {
        self.prepare(rgba_image(width, height, rgba)?, area)
    }

    fn from_terminal_capabilities(
        protocol_type: ProtocolType,
        font_size: FontSize,
        is_tmux: bool,
    ) -> Option<Self> {
        let protocol = match protocol_type {
            ProtocolType::Halfblocks => return None,
            ProtocolType::Kitty => EditorGraphicsProtocol::Kitty,
            ProtocolType::Sixel => EditorGraphicsProtocol::Sixel,
            ProtocolType::Iterm2 => EditorGraphicsProtocol::Iterm2,
        };
        Some(Self {
            font_size,
            protocol,
            is_tmux,
        })
    }
}

fn next_kitty_image_id() -> u32 {
    NEXT_KITTY_IMAGE_ID
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| {
            Some(id.saturating_add(1))
        })
        .expect("atomic image id update is infallible")
}

pub struct PreparedEditorImage {
    protocol: Protocol,
    kind: EditorGraphicsProtocol,
}

impl PreparedEditorImage {
    pub fn protocol(&self) -> EditorGraphicsProtocol {
        self.kind
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect) {
        frame.render_widget(Image::new(&self.protocol), area);
    }

    /// Render a fixed-size image centered inside a larger allocation.
    ///
    /// `ratatui-image` preserves the image aspect ratio but anchors the resulting
    /// protocol area at the allocation's left edge. Launcher and Home tiles
    /// allocate the whole tile width to an icon, so center the actual protocol
    /// footprint here.
    pub fn render_centered(&self, frame: &mut Frame<'_>, area: Rect) {
        let centered = centered_protocol_area(area, self.protocol.size());
        frame.render_widget(Image::new(&self.protocol), centered);
    }
}

#[derive(Debug)]
pub enum EditorMediaError {
    Protocol(ratatui_image::errors::Errors),
    TerminalQuery(std::io::Error),
    Decode(String),
    TooLarge {
        width: u32,
        height: u32,
    },
    InvalidRgbaLength {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
}

impl fmt::Display for EditorMediaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(error) => error.fmt(formatter),
            Self::TerminalQuery(error) => {
                write!(
                    formatter,
                    "could not query terminal image capabilities: {error}"
                )
            }
            Self::Decode(message) => write!(formatter, "could not decode image: {message}"),
            Self::TooLarge { width, height } => write!(
                formatter,
                "image dimensions {width}x{height} exceed the Editor safety limit"
            ),
            Self::InvalidRgbaLength {
                width,
                height,
                expected,
                actual,
            } => write!(
                formatter,
                "RGBA buffer for {width}x{height} image has {actual} bytes; expected {expected}"
            ),
        }
    }
}

impl std::error::Error for EditorMediaError {}

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
    let wezterm_executable =
        std::env::var_os("WEZTERM_EXECUTABLE").is_some_and(|value| !value.is_empty());
    let wezterm_program =
        std::env::var("TERM_PROGRAM").is_ok_and(|value| value.contains("WezTerm"));
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
fn query_terminal_capabilities(
    timeout: Duration,
) -> Result<TerminalCapabilityQuery, EditorMediaError> {
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
            return Err(EditorMediaError::TerminalQuery(error));
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
            return Err(EditorMediaError::TerminalQuery(error));
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
fn query_terminal_capabilities(
    timeout: Duration,
) -> Result<TerminalCapabilityQuery, EditorMediaError> {
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
        return Err(EditorMediaError::TerminalQuery(io::Error::last_os_error()));
    }
    let _mode_guard = WindowsConsoleModeGuard::enable_virtual_terminal_input(input)
        .map_err(EditorMediaError::TerminalQuery)?;
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
                return Err(EditorMediaError::TerminalQuery(io::Error::last_os_error()));
            }
            result => {
                return Err(EditorMediaError::TerminalQuery(io::Error::other(format!(
                    "unexpected console input wait result {result}"
                ))));
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
            return Err(EditorMediaError::TerminalQuery(io::Error::last_os_error()));
        }
        if peeked == 0 {
            continue;
        }
        if deadline_wait_millis(deadline, Instant::now()).is_none() {
            break;
        }
        let mut read = 0;
        if unsafe { ReadConsoleInputW(input, &mut record, 1, &mut read) } == 0 {
            return Err(EditorMediaError::TerminalQuery(io::Error::last_os_error()));
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
) -> Result<(), EditorMediaError> {
    use std::io::{self, Write};

    let query = terminal_capability_query(is_tmux, compatibility);
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(query.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(EditorMediaError::TerminalQuery)
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
fn terminal_probe_from_query(query: TerminalCapabilityQuery) -> TerminalGraphicsProbe {
    let has_live_protocol_response = query.protocol_type != ProtocolType::Halfblocks;
    let text_sizing_protocol = query.text_sizing_protocol;
    let picker = EditorImagePicker::from_terminal_capabilities(
        query.protocol_type,
        query.font_size,
        query.is_tmux,
    );
    let probe = match picker {
        Some(picker) if query.complete || has_live_protocol_response => {
            TerminalGraphicsProbe::verified(picker)
        }
        _ if query.complete && query.had_unverified_graphics_hint => {
            TerminalGraphicsProbe::no_response(
                "terminal responded, but the hinted graphics protocol did not answer its capability query",
            )
        }
        _ if query.complete => TerminalGraphicsProbe::unsupported(),
        _ => TerminalGraphicsProbe::no_response(
            "terminal did not return the graphics capability query terminator",
        ),
    };
    probe.with_text_sizing_protocol(text_sizing_protocol)
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
    let nonempty = |name| std::env::var_os(name).is_some_and(|value| !value.is_empty());
    if is_tmux && (nonempty("ITERM_SESSION_ID") || nonempty("WEZTERM_EXECUTABLE")) {
        return true;
    }
    std::env::var("TERM_PROGRAM").is_ok_and(|term_program| {
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
    }) || std::env::var("LC_TERMINAL").is_ok_and(|terminal| terminal.contains("iTerm"))
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

fn centered_protocol_area(allocation: Rect, protocol_size: Size) -> Rect {
    let width = protocol_size.width.min(allocation.width);
    let height = protocol_size.height.min(allocation.height);
    Rect::new(
        allocation
            .x
            .saturating_add(allocation.width.saturating_sub(width) / 2),
        allocation
            .y
            .saturating_add(allocation.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

fn rgba_image(width: u32, height: u32, rgba: Vec<u8>) -> Result<DynamicImage, EditorMediaError> {
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if pixels > EDITOR_IMAGE_MAX_PIXELS {
        return Err(EditorMediaError::TooLarge { width, height });
    }
    // The pixel limit guarantees this conversion and multiplication are safe on
    // every supported target, including 32-bit builds.
    let expected = usize::try_from(pixels.saturating_mul(4)).expect("bounded RGBA byte count");
    let actual = rgba.len();
    if actual != expected {
        return Err(EditorMediaError::InvalidRgbaLength {
            width,
            height,
            expected,
            actual,
        });
    }
    let image = RgbaImage::from_raw(width, height, rgba).expect("validated RGBA dimensions");
    Ok(DynamicImage::ImageRgba8(image))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halfblocks_are_reported_as_unsupported() {
        assert!(
            EditorImagePicker::from_terminal_capabilities(
                ProtocolType::Halfblocks,
                DEFAULT_TERMINAL_FONT_SIZE,
                false,
            )
            .is_none()
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn capability_query_orders_iterm_confirmation_before_one_final_terminator() {
        for is_tmux in [false, true] {
            let query = terminal_capability_query(is_tmux, TerminalGraphicsCompatibility::Standard);
            let (_, escape, end) = CapabilityParser::tmux_start_escape_end(is_tmux);
            let kitty = query.find("_Gi=31").expect("kitty query");
            let sixel = query.find("[c").expect("sixel query");
            let iterm = query
                .find("]1337;Capabilities")
                .expect("iTerm2 confirmation query");
            let status = format!("{escape}[5n");
            let terminator = query.rfind(&status).expect("final status query");

            assert!(kitty < sixel);
            assert!(sixel < iterm);
            assert!(iterm < terminator);
            assert_eq!(query.matches(&status).count(), 1);
            assert!(query.ends_with(&format!("{status}{end}")));
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn wezterm_uses_iterm2_instead_of_kitty_unicode_placeholders() {
        for is_tmux in [false, true] {
            let query = terminal_capability_query(is_tmux, TerminalGraphicsCompatibility::WezTerm);
            assert!(!query.contains("_Gi=31"));
            assert!(!query.contains("[c"));
            assert!(query.contains("]1337;Capabilities"));

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

    #[cfg(any(unix, windows))]
    #[test]
    fn standard_responses_prefer_kitty_and_report_text_sizing() {
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
        assert!(responses_support_text_sizing_protocol(&responses));
        assert!(responses.contains(&CapabilityResponse::Status));
        assert_eq!(
            standard_protocol_from_responses(&[CapabilityResponse::Sixel]),
            Some(ProtocolType::Sixel)
        );
        assert!(!responses_support_text_sizing_protocol(&[
            CapabilityResponse::CursorPositionReport(1, 1),
            CapabilityResponse::CursorPositionReport(3, 1),
            CapabilityResponse::CursorPositionReport(4, 1),
        ]));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn terminal_probe_distinguishes_unsupported_from_no_response() {
        let unsupported = terminal_probe_from_query(TerminalCapabilityQuery {
            protocol_type: ProtocolType::Halfblocks,
            font_size: DEFAULT_TERMINAL_FONT_SIZE,
            is_tmux: false,
            complete: true,
            had_unverified_graphics_hint: false,
            text_sizing_protocol: false,
        });
        assert_eq!(
            unsupported.status(),
            &TerminalGraphicsProbeStatus::Unsupported
        );

        let no_response = terminal_probe_from_query(TerminalCapabilityQuery {
            protocol_type: ProtocolType::Halfblocks,
            font_size: DEFAULT_TERMINAL_FONT_SIZE,
            is_tmux: false,
            complete: false,
            had_unverified_graphics_hint: false,
            text_sizing_protocol: false,
        });
        assert!(matches!(
            no_response.status(),
            TerminalGraphicsProbeStatus::NoResponse { .. }
        ));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn live_protocol_response_verifies_without_the_final_terminator() {
        let verified = terminal_probe_from_query(TerminalCapabilityQuery {
            protocol_type: ProtocolType::Kitty,
            font_size: DEFAULT_TERMINAL_FONT_SIZE,
            is_tmux: false,
            complete: false,
            had_unverified_graphics_hint: false,
            text_sizing_protocol: false,
        });
        assert_eq!(
            verified.status(),
            &TerminalGraphicsProbeStatus::Verified(EditorGraphicsProtocol::Kitty)
        );
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn iterm2_capability_response_reports_graphics_protocol_support() {
        assert_eq!(
            parse_iterm2_graphics_capabilities(b"\x1b]1337;Capabilities=AFN\x1b\\"),
            Some(Iterm2GraphicsCapabilities {
                file: true,
                sixel: false,
            })
        );
        assert_eq!(
            parse_iterm2_graphics_capabilities(b"\x1b]1337;Capabilities=ASxN\x07"),
            Some(Iterm2GraphicsCapabilities {
                file: false,
                sixel: true,
            })
        );
        assert_eq!(
            parse_iterm2_graphics_capabilities(b"\x1b]1337;Capabilities=FooN\x07"),
            Some(Iterm2GraphicsCapabilities {
                file: false,
                sixel: false,
            })
        );
        assert_eq!(parse_iterm2_graphics_capabilities(b"\x1b[?1;0c"), None);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn unconfirmed_environment_graphics_hint_is_no_response() {
        let probe = terminal_probe_from_query(TerminalCapabilityQuery {
            protocol_type: ProtocolType::Halfblocks,
            font_size: DEFAULT_TERMINAL_FONT_SIZE,
            is_tmux: false,
            complete: true,
            had_unverified_graphics_hint: true,
            text_sizing_protocol: false,
        });
        assert!(matches!(
            probe.status(),
            TerminalGraphicsProbeStatus::NoResponse { .. }
        ));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn measured_font_size_reaches_prepared_protocol_geometry() {
        let picker = EditorImagePicker::from_terminal_capabilities(
            ProtocolType::Kitty,
            FontSize::new(5, 10),
            false,
        )
        .expect("verified Kitty picker");
        let prepared = picker
            .prepare(DynamicImage::new_rgba8(100, 100), Rect::new(0, 0, 40, 40))
            .expect("prepare measured image");
        assert_eq!(prepared.protocol.size(), Size::new(20, 10));

        let default_picker = EditorImagePicker::from_terminal_capabilities(
            ProtocolType::Kitty,
            DEFAULT_TERMINAL_FONT_SIZE,
            false,
        )
        .expect("verified Kitty picker");
        let default_prepared = default_picker
            .prepare(DynamicImage::new_rgba8(100, 100), Rect::new(0, 0, 40, 40))
            .expect("prepare default image");
        assert_eq!(default_prepared.protocol.size(), Size::new(10, 5));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn native_protocol_factory_selects_each_verified_protocol() {
        for (protocol_type, expected) in [
            (ProtocolType::Kitty, EditorGraphicsProtocol::Kitty),
            (ProtocolType::Sixel, EditorGraphicsProtocol::Sixel),
            (ProtocolType::Iterm2, EditorGraphicsProtocol::Iterm2),
        ] {
            let picker = EditorImagePicker::from_terminal_capabilities(
                protocol_type,
                DEFAULT_TERMINAL_FONT_SIZE,
                true,
            )
            .expect("verified native protocol");
            assert_eq!(picker.protocol(), expected);
            assert!(picker.is_tmux);
            let prepared = picker
                .prepare(DynamicImage::new_rgba8(1, 1), Rect::new(0, 0, 1, 1))
                .expect("prepare selected native protocol");
            assert!(matches!(
                (&prepared.protocol, protocol_type),
                (Protocol::Kitty(_), ProtocolType::Kitty)
                    | (Protocol::Sixel(_), ProtocolType::Sixel)
                    | (Protocol::ITerm2(_), ProtocolType::Iterm2)
            ));
        }
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn response_interpretation_preserves_measured_cell_size() {
        let mut responses = TerminalResponseCollector::new();
        responses.push_bytes(b"\x1b[6;9;17t\x1b_Gi=31;OK\x1b\\\x1b[0n");
        let query = interpret_terminal_capability_responses(
            responses,
            false,
            None,
            TerminalGraphicsCompatibility::Standard,
        );
        assert_eq!(query.protocol_type, ProtocolType::Kitty);
        assert_eq!((query.font_size.width, query.font_size.height), (17, 9));
        assert!(query.complete);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn cell_geometry_priority_is_response_then_native_then_default() {
        let interpret_font_size = |capability_responses: Vec<CapabilityResponse>,
                                   native_font_size| {
            interpret_terminal_capability_responses(
                TerminalResponseCollector {
                    parser: CapabilityParser::new(),
                    responses: capability_responses,
                    raw: Vec::new(),
                    complete: true,
                },
                false,
                native_font_size,
                TerminalGraphicsCompatibility::Standard,
            )
            .font_size
        };

        let from_response = interpret_font_size(
            vec![CapabilityResponse::CellSize(Some((7, 13)))],
            Some(FontSize::new(8, 16)),
        );
        assert_eq!((from_response.width, from_response.height), (7, 13));

        let from_native = interpret_font_size(Vec::new(), Some(FontSize::new(8, 16)));
        assert_eq!((from_native.width, from_native.height), (8, 16));

        let after_invalid_response = interpret_font_size(
            vec![CapabilityResponse::CellSize(Some((0, 13)))],
            Some(FontSize::new(9, 18)),
        );
        assert_eq!(
            (after_invalid_response.width, after_invalid_response.height),
            (9, 18)
        );

        for invalid_native in [FontSize::new(0, 16), FontSize::new(8, 0)] {
            let fallback = interpret_font_size(Vec::new(), Some(invalid_native));
            assert_eq!(
                (fallback.width, fallback.height),
                (
                    DEFAULT_TERMINAL_FONT_SIZE.width,
                    DEFAULT_TERMINAL_FONT_SIZE.height,
                )
            );
        }

        assert!(font_size_from_terminal_geometry(0, 320, 80, 20).is_none());
        assert!(font_size_from_terminal_geometry(640, 0, 80, 20).is_none());
        assert!(font_size_from_terminal_geometry(79, 320, 80, 20).is_none());
        assert!(font_size_from_terminal_geometry(640, 19, 80, 20).is_none());
        let divided = font_size_from_terminal_geometry(640, 320, 80, 20)
            .expect("positive native terminal geometry");
        assert_eq!((divided.width, divided.height), (8, 16));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn native_cell_geometry_fallback_reaches_prepared_protocol_size() {
        let font_size = font_size_from_responses(&[], Some(FontSize::new(8, 16)));
        let picker =
            EditorImagePicker::from_terminal_capabilities(ProtocolType::Kitty, font_size, false)
                .expect("verified Kitty picker");
        let prepared = picker
            .prepare(DynamicImage::new_rgba8(100, 100), Rect::new(0, 0, 40, 40))
            .expect("prepare native-geometry image");

        assert_eq!(prepared.protocol.size(), Size::new(13, 7));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn deadline_wait_decision_never_waits_after_deadline() {
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

    #[cfg(any(unix, windows))]
    #[test]
    fn non_character_windows_record_yields_no_probe_byte_at_deadline() {
        assert_eq!(windows_vt_byte_from_record(true, true, 0), None);
        assert_eq!(windows_vt_byte_from_record(false, true, b'x'.into()), None);
        assert_eq!(windows_vt_byte_from_record(true, false, b'x'.into()), None);
        assert_eq!(
            windows_vt_byte_from_record(true, true, b'\x1b'.into()),
            Some(b'\x1b')
        );

        let deadline = Instant::now();
        assert_eq!(deadline_wait_millis(deadline, deadline), None);
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn probe_consumes_through_status_without_consuming_input_suffix() {
        let mut input = b"\x1b_Gi=31;OK\x1b\\\x1b[0nx".iter().copied();
        let mut responses = TerminalResponseCollector::new();
        while !responses.complete {
            responses.push_byte(input.next().expect("complete capability response"));
        }

        assert!(responses.responses.contains(&CapabilityResponse::Kitty));
        assert_eq!(input.next(), Some(b'x'));
    }

    #[test]
    fn rgba_preparation_rejects_wrong_buffer_length() {
        let error = rgba_image(2, 3, vec![0; 23]).unwrap_err();
        assert!(matches!(
            error,
            EditorMediaError::InvalidRgbaLength {
                width: 2,
                height: 3,
                expected: 24,
                actual: 23,
            }
        ));
    }

    #[test]
    fn rgba_preparation_constructs_an_rgba_image() {
        let image = rgba_image(2, 1, vec![255; 8]).expect("valid RGBA bytes");
        assert_eq!(image.width(), 2);
        assert_eq!(image.height(), 1);
    }

    #[test]
    fn protocol_footprint_is_centered_and_clamped_inside_its_allocation() {
        assert_eq!(
            centered_protocol_area(
                Rect::new(10, 5, 20, 6),
                Size {
                    width: 8,
                    height: 4,
                },
            ),
            Rect::new(16, 6, 8, 4)
        );
        assert_eq!(
            centered_protocol_area(
                Rect::new(10, 5, 4, 2),
                Size {
                    width: 8,
                    height: 4,
                },
            ),
            Rect::new(10, 5, 4, 2)
        );
    }
}
