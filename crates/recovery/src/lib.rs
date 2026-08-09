//! The deliberately small recovery program run inside the bundled WezTerm.
//! It accepts only the launcher-produced, already redacted handoff projection;
//! it never opens watchdog incident reports itself.

#![deny(unsafe_code)]

use base64::Engine as _;
use image::{ImageBuffer, ImageEncoder, Luma};
use qrcodegen::{QrCode, QrCodeEcc};
use serde::Deserialize;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

pub const HANDOFF_SCHEMA_VERSION: u32 = 1;
pub const RESTART_EXIT_CODE: i32 = 74;
pub const MAX_HANDOFF_BYTES: usize = 16 * 1024;
pub const MAX_CAPSULE_BYTES: usize = 1_200;
const MAX_TEXT_FIELD: usize = 240;
const MAX_TRACE_FRAMES: usize = 8;

/// Writes the only credential accepted by the outer launcher as proof that
/// the recovery UI reached its locked Enter transition. A WezTerm window exit
/// without this atomically-published file is never interpreted as a restart.
pub fn write_restart_request(path: &Path, incident_id: &str) -> Result<(), RecoveryError> {
    if !path.is_absolute() || path.file_name().is_none() {
        return Err(RecoveryError::Invalid(
            "recovery outcome path is invalid".to_owned(),
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| RecoveryError::Invalid("recovery outcome path is invalid".to_owned()))?;
    fs::create_dir_all(parent)
        .map_err(|_| RecoveryError::Io("could not create recovery outcome directory".to_owned()))?;
    let incident_id = safe_field(incident_id, 96);
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "incident_id": incident_id,
        "origin": "recovery",
        "kind": "restart",
        "code": RESTART_EXIT_CODE,
    }))
    .map_err(|_| RecoveryError::Invalid("could not encode recovery outcome".to_owned()))?;
    let temporary = restart_temporary_path(path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| RecoveryError::Io("could not create recovery outcome".to_owned()))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| RecoveryError::Io("could not persist recovery outcome".to_owned()))?;
    drop(file);
    if path.exists() {
        fs::remove_file(path)
            .map_err(|_| RecoveryError::Io("could not replace recovery outcome".to_owned()))?;
    }
    fs::rename(&temporary, path)
        .map_err(|_| RecoveryError::Io("could not publish recovery outcome".to_owned()))
}

fn restart_temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("recovery-outcome.json");
    path.with_file_name(format!(".{name}.{}.tmp", std::process::id()))
}
const DETAILS_UNAVAILABLE: &str = "Detailed report unavailable";
const REDACTED: &str = "[redacted]";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RecoveryHandoffV1 {
    pub schema_version: u32,
    pub incident_id: String,
    pub session_id: String,
    /// RFC 3339 UTC time serialized by the watchdog's `DateTime<Utc>`.
    pub occurred_at: String,
    pub failure: RecoveryProcessFailureV1,
    pub components: RecoveryComponentVersionsV1,
    pub restart_count: u32,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub traceback_frames: Vec<String>,
    #[serde(default)]
    pub report_available: bool,
}

/// JSON-compatible mirror of `watchdog::RecoveryProcessFailureV1`.  Keeping
/// this local means the recovery binary has no access to the full watchdog
/// report machinery.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RecoveryProcessFailureV1 {
    pub source: String,
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
}

/// JSON-compatible mirror of `watchdog::RecoveryComponentVersionsV1`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RecoveryComponentVersionsV1 {
    pub tundra: String,
    pub shell: String,
    pub wezterm: String,
}

impl RecoveryHandoffV1 {
    /// Parses the bounded private handoff.  The format intentionally does not
    /// contain the full watchdog JSON, environment, argv, or unredacted stderr.
    pub fn from_json(bytes: &[u8]) -> Result<Self, RecoveryError> {
        if bytes.len() > MAX_HANDOFF_BYTES {
            return Err(RecoveryError::TooLarge);
        }
        let mut value: Self = serde_json::from_slice(bytes)
            .map_err(|_| RecoveryError::Invalid("recovery handoff JSON is invalid".to_owned()))?;
        if value.schema_version != HANDOFF_SCHEMA_VERSION {
            return Err(RecoveryError::Invalid(format!(
                "unsupported handoff schema {}",
                value.schema_version
            )));
        }
        value.sanitize();
        Ok(value)
    }

    pub fn generic(reason: impl AsRef<str>) -> Self {
        // Error strings are an untrusted boundary too: operating-system I/O
        // errors can include the path they failed to open. Sanitize the reason
        // before interpolation, then sanitize the completed projection again.
        let reason = safe_field(reason.as_ref(), MAX_TEXT_FIELD / 2);
        let reason = if reason.is_empty() {
            "unknown error".to_owned()
        } else {
            reason
        };
        let mut handoff = Self {
            schema_version: HANDOFF_SCHEMA_VERSION,
            incident_id: "unavailable".to_owned(),
            session_id: "unavailable".to_owned(),
            occurred_at: "unknown".to_owned(),
            failure: RecoveryProcessFailureV1 {
                source: "recovery handoff".to_owned(),
                exit_code: None,
                signal: Some("unknown".to_owned()),
            },
            components: RecoveryComponentVersionsV1 {
                tundra: "unknown".to_owned(),
                shell: "unknown".to_owned(),
                wezterm: "unknown".to_owned(),
            },
            restart_count: 0,
            summary: format!("{DETAILS_UNAVAILABLE}: {reason}"),
            traceback_frames: Vec::new(),
            report_available: false,
        };
        handoff.sanitize();
        handoff
    }

    /// Binds the display and Enter credential to the incident selected by the
    /// outer launcher. A missing, corrupt, or mismatched handoff is reduced to
    /// the generic projection while retaining that safe incident identifier,
    /// so report loss never turns Enter into an unauthenticated restart.
    pub fn bound_to_incident(mut self, expected_incident_id: &str) -> Self {
        let expected_incident_id = safe_field(expected_incident_id, 96);
        if self.incident_id != expected_incident_id {
            self = Self::generic("recovery handoff incident mismatch");
        }
        self.incident_id = expected_incident_id;
        self.sanitized_copy()
    }

    pub fn capsule(&self) -> String {
        // Public fields make the mirror convenient for JSON, but callers can
        // mutate them after parsing. Never trust an earlier sanitization pass.
        let handoff = self.sanitized_copy();
        let mut frames = handoff.traceback_frames.clone();
        let mut truncated = false;
        loop {
            let mut result = String::from("TUNDRA-PANIC-CAPSULE/1\n");
            let _ = writeln!(result, "Incident: {}", handoff.incident_id);
            let _ = writeln!(result, "Session: {}", handoff.session_id);
            let _ = writeln!(result, "UTC: {}", handoff.occurred_at);
            let _ = writeln!(result, "Source: {}", handoff.failure.source);
            let _ = writeln!(result, "Exit: {}", failure_status(&handoff.failure));
            let _ = writeln!(result, "Restarts: {}", handoff.restart_count);
            let _ = writeln!(
                result,
                "Versions: tundra={} shell={} wezterm={}",
                handoff.components.tundra, handoff.components.shell, handoff.components.wezterm
            );
            let _ = writeln!(result, "Summary: {}", handoff.summary);
            for (index, frame) in frames.iter().enumerate() {
                let _ = writeln!(result, "Frame {}: {}", index + 1, frame);
            }
            if truncated {
                result.push_str("Trace: truncated\n");
            }
            let _ = writeln!(
                result,
                "Full details: Diagnostics > Logs > {}",
                handoff.incident_id
            );
            if result.len() <= MAX_CAPSULE_BYTES || frames.is_empty() {
                return truncate_utf8(&result, MAX_CAPSULE_BYTES).to_owned();
            }
            frames.pop();
            truncated = true;
        }
    }

    fn sanitize(&mut self) {
        self.incident_id = safe_field(&self.incident_id, 96);
        self.session_id = safe_field(&self.session_id, 96);
        self.occurred_at = safe_field(&self.occurred_at, 64);
        self.failure.source = safe_field(&self.failure.source, 96);
        self.failure.signal = self
            .failure
            .signal
            .as_deref()
            .map(|value| safe_field(value, 96));
        self.components.tundra = safe_field(&self.components.tundra, 64);
        self.components.shell = safe_field(&self.components.shell, 64);
        self.components.wezterm = safe_field(&self.components.wezterm, 64);
        self.summary = safe_field(&self.summary, MAX_TEXT_FIELD);
        self.traceback_frames = self
            .traceback_frames
            .iter()
            .take(MAX_TRACE_FRAMES)
            .map(|frame| normalized_frame(frame))
            .collect();
        if !self.report_available {
            self.summary = DETAILS_UNAVAILABLE.to_owned();
            self.traceback_frames.clear();
        }
    }

    fn sanitized_copy(&self) -> Self {
        let mut handoff = self.clone();
        handoff.sanitize();
        handoff
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum RecoveryError {
    Missing,
    TooLarge,
    Invalid(String),
    Io(String),
}

impl std::fmt::Display for RecoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => formatter.write_str("no recovery handoff was supplied"),
            Self::TooLarge => formatter.write_str("recovery handoff is too large"),
            Self::Invalid(message) | Self::Io(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RecoveryError {}

pub fn read_handoff(
    path: Option<&Path>,
    inline: Option<&str>,
) -> Result<RecoveryHandoffV1, RecoveryError> {
    let bytes = if let Some(path) = path {
        let display = path.to_string_lossy();
        if display.len() > 4096 {
            return Err(RecoveryError::Invalid(
                "handoff path is too long".to_owned(),
            ));
        }
        let metadata = fs::metadata(path)
            .map_err(|_| RecoveryError::Io("could not inspect the recovery handoff".to_owned()))?;
        if metadata.len() as usize > MAX_HANDOFF_BYTES {
            return Err(RecoveryError::TooLarge);
        }
        fs::read(path)
            .map_err(|_| RecoveryError::Io("could not read the recovery handoff".to_owned()))?
    } else if let Some(inline) = inline {
        if inline.len() > MAX_HANDOFF_BYTES {
            return Err(RecoveryError::TooLarge);
        }
        inline.as_bytes().to_vec()
    } else {
        return Err(RecoveryError::Missing);
    };
    RecoveryHandoffV1::from_json(&bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryState {
    AwaitingRestart,
    Restarting,
}

impl RecoveryState {
    /// Enter transitions exactly once. Escape and every other key are ignored.
    pub fn on_key(self, byte: u8) -> Self {
        match (self, byte) {
            (Self::AwaitingRestart, b'\r' | b'\n') => Self::Restarting,
            _ => self,
        }
    }
}

pub fn terminal_panic_page(
    handoff: &RecoveryHandoffV1,
    state: RecoveryState,
    columns: u16,
) -> String {
    let handoff = handoff.sanitized_copy();
    let width = usize::from(columns.max(48));
    let inner_width = width.saturating_sub(2);
    // Reserve the right rail for the pixel QR on wide WezTerm windows. The
    // frame still spans the full screen, preserving the kernel-panic panel.
    let content_width = if width >= 96 {
        inner_width.saturating_sub(42)
    } else {
        inner_width
    };
    let mut page = String::new();
    page.push_str("\x1b[2J\x1b[H\x1b[?25l\x1b[40m\x1b[37m");
    frame_rule(&mut page, inner_width);
    panic_line(
        &mut page,
        inner_width,
        "TUNDRAUX3 SESSION PANIC",
        "\x1b[1;97;41m",
    );
    panic_line(
        &mut page,
        inner_width,
        "SYSTEM HALTED — AUTOMATIC RECOVERY LIMIT REACHED",
        "\x1b[1;97;41m",
    );
    frame_rule(&mut page, inner_width);
    frame_text_line(&mut page, inner_width, "", "\x1b[37m");
    detail_line(
        &mut page,
        inner_width,
        content_width,
        "UTC",
        &handoff.occurred_at,
    );
    detail_line(
        &mut page,
        inner_width,
        content_width,
        "INCIDENT",
        &handoff.incident_id,
    );
    detail_line(
        &mut page,
        inner_width,
        content_width,
        "SESSION",
        &handoff.session_id,
    );
    detail_line(
        &mut page,
        inner_width,
        content_width,
        "FAULT",
        &handoff.failure.source,
    );
    detail_line(
        &mut page,
        inner_width,
        content_width,
        "EXIT",
        &failure_status(&handoff.failure),
    );
    detail_line(
        &mut page,
        inner_width,
        content_width,
        "RECOVERY ATTEMPTS",
        &handoff.restart_count.to_string(),
    );
    detail_line(
        &mut page,
        inner_width,
        content_width,
        "VERSIONS",
        &format!(
            "tundra {} | shell {} | wezterm {}",
            handoff.components.tundra, handoff.components.shell, handoff.components.wezterm
        ),
    );
    frame_text_line(&mut page, inner_width, "", "\x1b[37m");
    frame_text_line(&mut page, inner_width, "SUMMARY", "\x1b[1;93m");
    wrapped_line(
        &mut page,
        inner_width,
        content_width,
        &handoff.summary,
        "\x1b[37m",
    );
    if !handoff.traceback_frames.is_empty() {
        frame_text_line(&mut page, inner_width, "", "\x1b[37m");
        frame_text_line(&mut page, inner_width, "TRACE PREVIEW", "\x1b[1;93m");
        for frame in &handoff.traceback_frames {
            wrapped_line(
                &mut page,
                inner_width,
                content_width,
                &format!("  > {frame}"),
                "\x1b[37m",
            );
        }
    }
    frame_text_line(&mut page, inner_width, "", "\x1b[37m");
    frame_rule(&mut page, inner_width);
    match state {
        RecoveryState::AwaitingRestart => frame_text_line(
            &mut page,
            inner_width,
            "PRESS ENTER TO RESTART TUNDRAUX3",
            "\x1b[1;93m",
        ),
        RecoveryState::Restarting => {
            frame_text_line(&mut page, inner_width, "RESTARTING...", "\x1b[1;97;41m")
        }
    }
    frame_rule(&mut page, inner_width);
    page
}

/// iTerm2 inline-image protocol, which bundled WezTerm renders as pixels.
/// The output remains an offline PNG generated from the same QR matrix.
pub fn wezterm_pixel_qr(capsule: &str, module_pixels: u32) -> Result<String, RecoveryError> {
    let qr = make_qr(capsule)?;
    let module_pixels = module_pixels.max(2);
    let quiet = 4_u32;
    let modules = qr.size() as u32 + quiet * 2;
    let dimension = modules * module_pixels;
    let mut image = ImageBuffer::<Luma<u8>, Vec<u8>>::from_pixel(dimension, dimension, Luma([255]));
    for y in 0..qr.size() {
        for x in 0..qr.size() {
            if qr.get_module(x, y) {
                let left = (x as u32 + quiet) * module_pixels;
                let top = (y as u32 + quiet) * module_pixels;
                for pixel_y in top..top + module_pixels {
                    for pixel_x in left..left + module_pixels {
                        image.put_pixel(pixel_x, pixel_y, Luma([0]));
                    }
                }
            }
        }
    }
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(&image, dimension, dimension, image::ExtendedColorType::L8)
        .map_err(|error| RecoveryError::Io(format!("could not encode QR PNG: {error}")))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(png);
    Ok(format!(
        "\x1b]1337;File=inline=1;width={dimension}px;height={dimension}px;preserveAspectRatio=1:{encoded}\x07"
    ))
}

pub fn use_wezterm_pixels() -> bool {
    if std::env::var("TUNDRA_RECOVERY_QR_MODE").ok().as_deref() == Some("terminal") {
        return false;
    }
    std::env::var("WEZTERM_EXECUTABLE").is_ok()
        || std::env::var("TERM_PROGRAM").ok().as_deref() == Some("WezTerm")
}

pub fn render_qr(capsule: &str) -> Result<String, RecoveryError> {
    if use_wezterm_pixels() {
        wezterm_pixel_qr(capsule, 4)
    } else {
        Err(RecoveryError::Invalid(
            "pixel QR rendering is unavailable".to_owned(),
        ))
    }
}

/// Positions WezTerm's pixel image in the reserved right rail. Small windows
/// show a stable resize hint instead of falling back to font-dependent block
/// glyphs or wrapping an unscannable code across the panic page.
pub fn render_qr_for_layout(
    capsule: &str,
    columns: u16,
    rows: u16,
    pixel_width: u16,
    pixel_height: u16,
    prefer_wezterm_pixels: bool,
) -> Result<String, RecoveryError> {
    if prefer_wezterm_pixels && columns >= 96 && rows >= 28 {
        let qr = make_qr(capsule)?;
        let modules = u32::try_from(qr.size()).unwrap_or_default() + 8;
        let terminal_width = if pixel_width == 0 {
            u32::from(columns).saturating_mul(8)
        } else {
            u32::from(pixel_width)
        };
        let terminal_height = if pixel_height == 0 {
            u32::from(rows).saturating_mul(16)
        } else {
            u32::from(pixel_height)
        };
        let cell_width = (terminal_width / u32::from(columns.max(1))).max(1);
        let cell_height = (terminal_height / u32::from(rows.max(1))).max(1);
        let rail_pixels = cell_width.saturating_mul(38);
        let vertical_pixels = terminal_height.saturating_sub(cell_height.saturating_mul(6));
        let module_pixels = (rail_pixels / modules)
            .min(vertical_pixels / modules)
            .min(6);
        if module_pixels >= 2 {
            let column = columns.saturating_sub(39).max(1);
            return Ok(format!(
                "\x1b7\x1b[5;{column}H{}\x1b8",
                wezterm_pixel_qr(capsule, module_pixels)?
            ));
        }
    }

    Ok("\n\x1b[1;93mPIXEL QR UNAVAILABLE — ENLARGE THE WINDOW TO VIEW THE OFFLINE CAPSULE\x1b[0m\n".to_owned())
}

pub fn run(handoff: RecoveryHandoffV1) -> Result<i32, RecoveryError> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(RecoveryError::Invalid(
            "recovery mode requires a terminal PTY".to_owned(),
        ));
    }
    let mut output = io::stdout();
    let window = crossterm::terminal::window_size().ok();
    let (columns, rows) = window
        .as_ref()
        .map(|window| (window.columns, window.rows))
        .or_else(|| crossterm::terminal::size().ok())
        .unwrap_or((100, 36));
    let (pixel_width, pixel_height) = window
        .map(|window| (window.width, window.height))
        .unwrap_or((0, 0));
    let capsule = handoff.capsule();
    crossterm::terminal::enable_raw_mode().map_err(|error| RecoveryError::Io(error.to_string()))?;
    let result = (|| {
        write!(
            output,
            "{}\n{}",
            terminal_panic_page(&handoff, RecoveryState::AwaitingRestart, columns),
            render_qr_for_layout(
                &capsule,
                columns,
                rows,
                pixel_width,
                pixel_height,
                use_wezterm_pixels(),
            )?
        )
        .map_err(|error| RecoveryError::Io(error.to_string()))?;
        output
            .flush()
            .map_err(|error| RecoveryError::Io(error.to_string()))?;
        loop {
            if let crossterm::event::Event::Key(key) =
                crossterm::event::read().map_err(|error| RecoveryError::Io(error.to_string()))?
            {
                use crossterm::event::{KeyCode, KeyEventKind};
                if key.kind == KeyEventKind::Press && matches!(key.code, KeyCode::Enter) {
                    write!(
                        output,
                        "{}",
                        terminal_panic_page(&handoff, RecoveryState::Restarting, columns)
                    )
                    .map_err(|error| RecoveryError::Io(error.to_string()))?;
                    output
                        .flush()
                        .map_err(|error| RecoveryError::Io(error.to_string()))?;
                    return Ok(RESTART_EXIT_CODE);
                }
            }
        }
    })();
    let _ = crossterm::terminal::disable_raw_mode();
    let _ = write!(output, "\x1b[?25h\x1b[0m");
    let _ = output.flush();
    result
}

fn make_qr(capsule: &str) -> Result<QrCode, RecoveryError> {
    if capsule.len() > MAX_CAPSULE_BYTES {
        return Err(RecoveryError::TooLarge);
    }
    QrCode::encode_text(capsule, QrCodeEcc::High)
        .map_err(|error| RecoveryError::Invalid(format!("could not encode panic QR: {error}")))
}

fn failure_status(failure: &RecoveryProcessFailureV1) -> String {
    match (failure.exit_code, failure.signal.as_deref()) {
        (Some(code), Some(signal)) => format!("exit {code}; {signal}"),
        (Some(code), None) => format!("exit {code}"),
        (None, Some(signal)) => signal.to_owned(),
        (None, None) => "unknown".to_owned(),
    }
}

fn panic_line(output: &mut String, width: usize, text: &str, style: &str) {
    let text = display_text(text, width);
    let padding = width.saturating_sub(text.chars().count());
    let _ = writeln!(
        output,
        "\x1b[31m|\x1b[0m{style}{text}{}\x1b[0m\x1b[31m|\x1b[0m",
        " ".repeat(padding)
    );
}

fn detail_line(
    output: &mut String,
    frame_width: usize,
    content_width: usize,
    label: &str,
    value: &str,
) {
    let label = format!("{label:<18}");
    let value = display_text(
        value,
        content_width.saturating_sub(label.chars().count() + 1),
    );
    let visible = label.chars().count() + 1 + value.chars().count();
    let padding = frame_width.saturating_sub(visible);
    let _ = writeln!(
        output,
        "\x1b[31m|\x1b[0m\x1b[1;93m{label}\x1b[0m \x1b[37m{value}{}\x1b[0m\x1b[31m|\x1b[0m",
        " ".repeat(padding)
    );
}

fn wrapped_line(
    output: &mut String,
    frame_width: usize,
    content_width: usize,
    text: &str,
    style: &str,
) {
    // Values were sanitized before display; the renderer still removes every
    // control character at this boundary so untrusted text cannot create an
    // ANSI sequence, change colors, or escape the panic frame.
    frame_text_line(
        output,
        frame_width,
        &display_text(text, content_width),
        style,
    );
}

fn frame_rule(output: &mut String, inner_width: usize) {
    let _ = writeln!(output, "\x1b[31m+{}+\x1b[0m", "=".repeat(inner_width));
}

fn frame_text_line(output: &mut String, width: usize, text: &str, style: &str) {
    let text = display_text(text, width);
    let padding = width.saturating_sub(text.chars().count());
    let _ = writeln!(
        output,
        "\x1b[31m|\x1b[0m{style}{text}{}\x1b[0m\x1b[31m|\x1b[0m",
        " ".repeat(padding)
    );
}

fn display_text(value: &str, max: usize) -> String {
    let safe: String = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    truncate_utf8(&safe, max).to_owned()
}

fn safe_field(value: &str, max: usize) -> String {
    let compact: String = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let lowered = compact.to_ascii_lowercase();
    if contains_absolute_path(&lowered) || contains_sensitive_content(&lowered) {
        REDACTED.to_owned()
    } else {
        truncate_utf8(&compact, max).to_owned()
    }
}

fn contains_sensitive_content(lowered: &str) -> bool {
    [
        "token=",
        "token:",
        "token ",
        "secret",
        "api key",
        "api_key",
        "api-key",
        "apikey",
        "authorization",
        "bearer ",
        "password",
        "passwd",
        "credential",
        "clipboard",
        "argv",
        "command line",
        "environment",
        "env=",
        "username",
        "user=",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
}

fn contains_absolute_path(lowered: &str) -> bool {
    // Drive-absolute paths are recognized at any position, with either slash
    // style: `C:\...`, `c:/...`, and text such as `at D:\...`.
    if lowered.as_bytes().windows(3).any(|window| {
        window[0].is_ascii_alphabetic() && window[1] == b':' && matches!(window[2], b'/' | b'\\')
    }) {
        return true;
    }

    // UNC, device, and extended-length Windows paths. The input has already
    // been whitespace-collapsed, so a doubled separator remains unambiguous.
    if lowered.contains("\\\\") || lowered.starts_with("//") || lowered.contains(" //") {
        return true;
    }

    // Recognize an absolute POSIX path wherever a path token can begin while
    // avoiding ordinary punctuation such as the slash in `and/or`.
    let bytes = lowered.as_bytes();
    bytes.iter().enumerate().any(|(index, byte)| {
        if *byte != b'/' {
            return false;
        }
        let boundary = index == 0
            || bytes[index - 1].is_ascii_whitespace()
            || matches!(
                bytes[index - 1],
                b'(' | b'[' | b'{' | b'=' | b':' | b'\'' | b'"'
            );
        let path_body = bytes
            .get(index + 1)
            .is_some_and(|next| next.is_ascii_alphanumeric() || matches!(next, b'.' | b'_' | b'-'));
        boundary && path_body
    })
}

fn normalized_frame(value: &str) -> String {
    let value = safe_field(value, MAX_TEXT_FIELD);
    if value == "[redacted]" {
        value
    } else {
        // The projection contains symbol-level frames only. Separators are
        // removed defensively so accidentally supplied paths cannot leak.
        value
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&value)
            .to_owned()
    }
}

fn truncate_utf8(value: &str, max: usize) -> &str {
    if value.len() <= max {
        return value;
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handoff() -> RecoveryHandoffV1 {
        RecoveryHandoffV1::from_json(
            br#"{
          "schema_version": 1,
          "incident_id": "INC-20260809-001",
          "session_id": "session-1",
          "occurred_at": "2026-08-09T12:00:00Z",
          "failure": { "source": "tundra-shell", "exit_code": null, "signal": "abort" },
          "components": { "tundra": "0.1.1", "shell": "0.1.1", "wezterm": "2026.01" },
          "restart_count": 3,
          "summary": "render task aborted",
          "traceback_frames": ["tundra::render::draw", "tundra::runtime::tick"],
          "report_available": true
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn capsule_is_bounded_and_has_required_offline_fields() {
        let mut value = handoff();
        value.traceback_frames = (0..8).map(|_| "x".repeat(240)).collect();
        let capsule = value.capsule();
        assert!(capsule.len() <= MAX_CAPSULE_BYTES);
        assert!(capsule.starts_with("TUNDRA-PANIC-CAPSULE/1\n"));
        assert!(capsule.contains("Full details: Diagnostics > Logs > INC-20260809-001"));
        assert!(capsule.contains("Trace: truncated"));
    }

    #[test]
    fn final_iterm2_png_round_trips_to_the_capsule() {
        let capsule = handoff().capsule();
        let module_pixels = 4_u32;
        let protocol = wezterm_pixel_qr(&capsule, module_pixels).unwrap();
        let encoded = protocol
            .strip_prefix("\u{1b}]1337;File=inline=1;")
            .and_then(|value| value.strip_suffix('\u{7}'))
            .and_then(|value| value.split_once(':').map(|(_, encoded)| encoded))
            .expect("bounded iTerm2 image payload");
        let png = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("decode final PNG bytes");
        let image = image::load_from_memory(&png)
            .expect("parse final PNG")
            .to_luma8();
        assert_eq!(image.width(), image.height());
        assert_eq!(image.width() % module_pixels, 0);

        let quiet_zone = 4_u32;
        let total_modules = image.width() / module_pixels;
        let size = usize::try_from(total_modules - quiet_zone * 2).unwrap();
        let grid = rqrr::Grid::new(rqrr::SimpleGrid::from_func(size, |x, y| {
            let pixel_x =
                (u32::try_from(x).unwrap() + quiet_zone) * module_pixels + module_pixels / 2;
            let pixel_y =
                (u32::try_from(y).unwrap() + quiet_zone) * module_pixels + module_pixels / 2;
            image.get_pixel(pixel_x, pixel_y).0[0] < 128
        }));
        let (_, content) = grid.decode().expect("decode QR from final PNG pixels");
        assert_eq!(content, capsule);
    }

    #[test]
    fn wide_wezterm_layout_places_pixel_qr_in_right_rail() {
        let capsule = handoff().capsule();
        let output = render_qr_for_layout(&capsule, 120, 36, 960, 576, true).unwrap();
        assert!(output.starts_with("\u{1b}7\u{1b}[5;81H"));
        assert!(output.contains("\u{1b}]1337;File=inline=1"));
        assert!(output.contains("px;height="));
        assert!(output.ends_with("\u{1b}8"));

        let modules = (make_qr(&capsule).unwrap().size() as u32 + 8) * 3;
        let exact = wezterm_pixel_qr(&capsule, 3).unwrap();
        assert!(exact.contains(&format!("width={modules}px;height={modules}px")));
    }

    #[test]
    fn narrow_layout_never_wraps_an_oversized_qr() {
        let output = render_qr_for_layout(&handoff().capsule(), 48, 20, 384, 320, true).unwrap();
        assert!(output.contains("PIXEL QR UNAVAILABLE"));
        assert!(!output.contains("\u{1b}]1337;File="));
        assert!(!output.contains('█'));
    }

    #[test]
    fn page_has_exact_headings_and_restart_state() {
        let page = terminal_panic_page(&handoff(), RecoveryState::AwaitingRestart, 100);
        assert!(page.contains("TUNDRAUX3 SESSION PANIC"));
        assert!(page.contains("SYSTEM HALTED — AUTOMATIC RECOVERY LIMIT REACHED"));
        assert!(page.contains("PRESS ENTER TO RESTART TUNDRAUX3"));
        let restarting = terminal_panic_page(&handoff(), RecoveryState::Restarting, 100);
        assert!(restarting.contains("RESTARTING..."));
    }

    #[test]
    fn page_keeps_every_visible_line_inside_the_red_panic_frame() {
        let mut value = handoff();
        value.summary = "bad\u{1b}[1;31m ANSI must remain textless".to_owned();
        let page = terminal_panic_page(&value, RecoveryState::AwaitingRestart, 100);

        // The old implementation built styled detail text and then replaced
        // ESC with '?', visibly rendering fragments such as "?[1;93m".
        assert!(!page.contains("?[1;"));
        assert!(page.contains("\x1b[31m"));
        assert!(page.contains("\x1b[1;93m"));
        assert!(page.contains("\x1b[37m"));

        let lines = visible_lines(&page);
        assert!(!lines.is_empty());
        for line in &lines {
            let framed = (line.starts_with('+') && line.ends_with('+'))
                || (line.starts_with('|') && line.ends_with('|'));
            assert!(framed, "unframed panic-page line: {line:?}");
        }
        let enter = lines
            .iter()
            .find(|line| line.contains("PRESS ENTER TO RESTART TUNDRAUX3"))
            .expect("restart prompt inside the frame");
        assert!(enter.starts_with('|') && enter.ends_with('|'));
    }

    #[test]
    fn only_enter_can_request_one_restart() {
        assert_eq!(
            RecoveryState::AwaitingRestart.on_key(0x1b),
            RecoveryState::AwaitingRestart
        );
        assert_eq!(
            RecoveryState::AwaitingRestart.on_key(b'\r'),
            RecoveryState::Restarting
        );
        assert_eq!(
            RecoveryState::Restarting.on_key(b'\r'),
            RecoveryState::Restarting
        );
    }

    #[test]
    fn enter_restart_credential_is_atomic_and_incident_bound() {
        let root = std::env::temp_dir().join(format!(
            "tundra-recovery-outcome-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("restart.json");
        write_restart_request(&path, "incident-42").unwrap();
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["incident_id"], "incident-42");
        assert_eq!(value["origin"], "recovery");
        assert_eq!(value["kind"], "restart");
        assert_eq!(value["code"], RESTART_EXIT_CODE);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn privacy_redacts_paths_tokens_and_terminal_controls() {
        let json = br#"{
          "schema_version": 1, "incident_id": "INC", "session_id": "s",
          "occurred_at": "now", "failure": { "source": "C:\\Users\\alice\\secret", "exit_code": null, "signal": "TOKEN=abc" },
          "components": { "tundra": "v", "shell": "v", "wezterm": "v" }, "restart_count": 3,
          "summary": "Bearer abc\u001b[31m", "traceback_frames": ["/home/alice/app.rs"]
        }"#;
        let handoff = RecoveryHandoffV1::from_json(json).unwrap();
        let combined = format!(
            "{}{}",
            terminal_panic_page(&handoff, RecoveryState::AwaitingRestart, 100),
            handoff.capsule()
        );
        assert!(!combined.contains("alice"));
        assert!(!combined.contains("abc"));
        assert!(!handoff.summary.contains('\u{1b}'));
        assert!(combined.contains("[redacted]"));
    }

    #[test]
    fn malicious_handoff_is_resanitized_at_each_render_boundary() {
        let mut value = handoff();
        value.summary = "API Key: abc123 at C:/USERS/Alice/private.txt".to_owned();
        value.failure.source = r"\\SERVER\share\alice\dump".to_owned();
        value.traceback_frames = vec!["TOKEN: raw-secret".to_owned()];

        let page = terminal_panic_page(&value, RecoveryState::AwaitingRestart, 100);
        let capsule = value.capsule();
        let combined = format!("{page}{capsule}");
        for forbidden in ["abc123", "Alice", "SERVER", "raw-secret", "private.txt"] {
            assert!(!combined.contains(forbidden), "leaked {forbidden}");
        }
        assert!(combined.matches(REDACTED).count() >= 3);
    }

    #[test]
    fn absolute_paths_and_secret_labels_are_case_insensitively_redacted() {
        for malicious in [
            r"C:\Users\alice\file.txt",
            "d:/DATA/private.log",
            r"\\server\share\dump.txt",
            r"\\?\C:\long\path",
            "at /HOME/alice/file",
            "panic in /tmp/report.txt",
            "source=/VAR/log/tundra.log",
            "(/opt/tundra/private)",
            "TOKEN: value",
            "Secret value",
            "API KEY = value",
            "Api-Key: value",
            "Authorization: bearer value",
        ] {
            assert_eq!(
                safe_field(malicious, 240),
                REDACTED,
                "did not redact {malicious}"
            );
        }
    }

    #[test]
    fn missing_and_corrupt_reports_use_a_resanitized_minimal_handoff() {
        let missing_error = read_handoff(
            Some(Path::new(r"C:\Users\Alice\missing-handoff.json")),
            None,
        )
        .unwrap_err();
        let missing = RecoveryHandoffV1::generic(missing_error.to_string());
        let missing_output = format!(
            "{}{}",
            terminal_panic_page(&missing, RecoveryState::AwaitingRestart, 100),
            missing.capsule()
        );
        assert!(missing_output.contains(DETAILS_UNAVAILABLE));
        assert!(!missing_output.contains("Alice"));
        assert!(missing.traceback_frames.is_empty());

        let corrupt_error =
            read_handoff(None, Some(r#"{"summary":"token: bad at /tmp/x""#)).unwrap_err();
        let corrupt = RecoveryHandoffV1::generic(corrupt_error.to_string());
        let corrupt_output = corrupt.capsule();
        assert!(corrupt_output.contains(DETAILS_UNAVAILABLE));
        assert!(!corrupt_output.contains("token"));
        assert!(!corrupt_output.contains("/tmp"));
    }

    #[test]
    fn missing_or_mismatched_handoff_stays_bound_to_the_launcher_incident() {
        let missing =
            RecoveryHandoffV1::generic("handoff missing").bound_to_incident("panic-20260809-7");
        assert_eq!(missing.incident_id, "panic-20260809-7");
        assert!(!missing.report_available);
        assert!(missing.capsule().contains("Incident: panic-20260809-7"));

        let mut mismatched = handoff();
        mismatched.summary = "must not cross incident boundaries".to_owned();
        let rebound = mismatched.bound_to_incident("panic-20260809-8");
        assert_eq!(rebound.incident_id, "panic-20260809-8");
        assert!(!rebound.report_available);
        assert_eq!(rebound.summary, DETAILS_UNAVAILABLE);
        assert!(rebound.traceback_frames.is_empty());
    }

    #[test]
    fn unavailable_report_cannot_smuggle_summary_or_traceback() {
        let json = br#"{
          "schema_version": 1, "incident_id": "INC", "session_id": "s",
          "occurred_at": "now", "failure": { "source": "shell", "exit_code": 1, "signal": null },
          "components": { "tundra": "v", "shell": "v", "wezterm": "v" }, "restart_count": 3,
          "summary": "token: stolen", "traceback_frames": ["/var/private/trace"],
          "report_available": false
        }"#;
        let value = RecoveryHandoffV1::from_json(json).unwrap();
        assert_eq!(value.summary, DETAILS_UNAVAILABLE);
        assert!(value.traceback_frames.is_empty());
        assert!(!value.capsule().contains("stolen"));
    }

    #[test]
    fn rejects_oversized_handoff_before_parsing() {
        assert_eq!(
            RecoveryHandoffV1::from_json(&vec![b'x'; MAX_HANDOFF_BYTES + 1]),
            Err(RecoveryError::TooLarge)
        );
    }

    fn visible_lines(page: &str) -> Vec<String> {
        let mut plain = String::new();
        let mut characters = page.chars().peekable();
        while let Some(character) = characters.next() {
            if character != '\u{1b}' {
                plain.push(character);
                continue;
            }

            // This renderer emits CSI sequences only. Consume a complete CSI
            // sequence so the structural assertion examines terminal cells,
            // not color/cursor control bytes.
            if characters.next_if_eq(&'[').is_some() {
                for next in characters.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
        }
        plain.lines().map(str::to_owned).collect()
    }
}
