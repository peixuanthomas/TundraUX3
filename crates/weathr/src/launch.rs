use crate::app::{App, AppRunOutcome};
use crate::app_state::BottomHudPrompt;
use crate::error::{TerminalError, WeatherAssetError};
use crate::render::TerminalRenderer;
use crate::theme::{Palette, ThemeRegistry};
use std::fmt;
use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use system_services::SystemSnapshot;
use tokio::sync::watch;
use watchdog::{AppCriticality, AppDescriptor, AppId, AppWatchdog, WatchdogError};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClockFormat {
    #[default]
    TwentyFourHour,
    TwelveHour,
}

/// Immutable service data and display preferences consumed by Weathr.
/// It intentionally has no network client, cache path or service handle.
#[derive(Clone)]
pub struct WeathrDisplayInput {
    pub snapshots: watch::Receiver<SystemSnapshot>,
    pub clock_format: ClockFormat,
    pub hide_hud: bool,
    pub palette: Palette,
    pub shutdown: Arc<AtomicBool>,
    pub minimum_terminal_size: Option<(u16, u16)>,
}

impl fmt::Debug for WeathrDisplayInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WeathrDisplayInput")
            .field("clock_format", &self.clock_format)
            .field("hide_hud", &self.hide_hud)
            .field("minimum_terminal_size", &self.minimum_terminal_size)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellLockscreenResult {
    Started,
    Cancelled,
}

impl From<AppRunOutcome> for ShellLockscreenResult {
    fn from(value: AppRunOutcome) -> Self {
        match value {
            AppRunOutcome::Space => Self::Started,
            AppRunOutcome::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Debug)]
pub enum WeathrRunError {
    Terminal(TerminalError),
    Assets(WeatherAssetError),
    WatchdogUnavailable,
    Watchdog(WatchdogError),
    /// Retained for host watchdogs that need to surface a typed unrecoverable
    /// display failure. The renderer itself does not install a panic hook.
    Panic {
        incident_id: String,
        reason: String,
    },
    Runtime(io::Error),
    Run(io::Error),
    Cleanup(io::Error),
}

impl fmt::Display for WeathrRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Terminal(error) => write!(formatter, "{}", error.user_friendly_message()),
            Self::Assets(error) => write!(formatter, "failed to load weathr ASCII assets: {error}"),
            Self::WatchdogUnavailable => formatter.write_str(
                "weathr requires the process watchdog to be installed before it is launched",
            ),
            Self::Watchdog(error) => write!(formatter, "weathr watchdog setup failed: {error}"),
            Self::Panic {
                incident_id,
                reason,
            } => write!(
                formatter,
                "weathr stopped after a panic ({incident_id}): {reason}"
            ),
            Self::Runtime(error) => write!(formatter, "failed to start weathr runtime: {error}"),
            Self::Run(error) => write!(formatter, "weathr render loop failed: {error}"),
            Self::Cleanup(error) => write!(formatter, "failed to restore terminal: {error}"),
        }
    }
}
impl std::error::Error for WeathrRunError {}
impl From<TerminalError> for WeathrRunError {
    fn from(value: TerminalError) -> Self {
        Self::Terminal(value)
    }
}
impl From<WeatherAssetError> for WeathrRunError {
    fn from(value: WeatherAssetError) -> Self {
        Self::Assets(value)
    }
}
impl From<WatchdogError> for WeathrRunError {
    fn from(value: WatchdogError) -> Self {
        Self::Watchdog(value)
    }
}

pub fn weathr_watchdog_descriptor() -> AppDescriptor {
    AppDescriptor::new(
        AppId::from_static("weathr"),
        "Weathr",
        env!("CARGO_PKG_VERSION"),
        AppCriticality::SessionCritical,
    )
}

pub fn restore_terminal_best_effort() {
    use crossterm::{
        cursor, execute,
        style::ResetColor,
        terminal::{LeaveAlternateScreen, disable_raw_mode},
    };
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show, ResetColor);
}

/// Blocks a terminal-owning caller until a snapshot-driven display session
/// exits. The caller owns `WeathrDisplayInput` and the system-services
/// lifecycle; Weathr never creates a network or cache runtime itself.
pub fn run_display_blocking(
    input: WeathrDisplayInput,
    watchdog: AppWatchdog,
) -> Result<ShellLockscreenResult, WeathrRunError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(WeathrRunError::Runtime)?;
    runtime.block_on(run_display(input, watchdog))
}

/// Renders one scene from snapshots. It performs no configuration lookup and
/// starts no weather, geolocation, cache or time request.
pub async fn run_display(
    input: WeathrDisplayInput,
    _watchdog: AppWatchdog,
) -> Result<ShellLockscreenResult, WeathrRunError> {
    run_display_inner(input, None).await
}

pub fn run_shell_lockscreen_managed_with_shutdown_and_assets(
    input: WeathrDisplayInput,
    _watchdog: AppWatchdog,
    ascii_assets: Arc<ascii_assets::AsciiAssetStore>,
) -> Result<ShellLockscreenResult, WeathrRunError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(WeathrRunError::Runtime)?;
    runtime.block_on(run_display_inner(input, Some(ascii_assets)))
}

async fn run_display_inner(
    input: WeathrDisplayInput,
    cached_store: Option<Arc<ascii_assets::AsciiAssetStore>>,
) -> Result<ShellLockscreenResult, WeathrRunError> {
    let mut registry = ThemeRegistry::new();
    registry.set_active_palette(input.palette);
    let dimensions = match cached_store.as_deref() {
        Some(store) => store.max_asset_dimensions(),
        None => ascii_assets::AsciiAssetStore::load_theme(registry.active().id)
            .map_err(WeatherAssetError::from)?
            .max_asset_dimensions(),
    };
    let minimum = minimum_terminal_size_for_assets(dimensions, input.minimum_terminal_size);
    let mut renderer = TerminalRenderer::new_with_minimum(minimum)?;
    let (width, height) = renderer.get_size();
    let mut app = App::new_with_bottom_hud_prompt_and_assets(
        width,
        height,
        registry,
        BottomHudPrompt::Start,
        input.snapshots,
        input.clock_format,
        input.hide_hud,
        cached_store.as_deref(),
    )?;
    renderer.init()?;
    let run_result = app
        .run_with_outcome_and_shutdown(&mut renderer, &input.shutdown)
        .await
        .map(ShellLockscreenResult::from)
        .map_err(WeathrRunError::Run);
    let cleanup = renderer.cleanup().map_err(WeathrRunError::Cleanup);
    match (run_result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn minimum_terminal_size_for_assets(
    dimensions: ascii_assets::AssetDimensions,
    explicit: Option<(u16, u16)>,
) -> (u16, u16) {
    let supplied = explicit.unwrap_or((0, 0));
    (
        u16::try_from(dimensions.width)
            .unwrap_or(u16::MAX)
            .max(supplied.0),
        u16::try_from(dimensions.height)
            .unwrap_or(u16::MAX)
            .max(supplied.1),
    )
}
