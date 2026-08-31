use crate::app::{App, AppInput, AppRunOutcome};
use crate::app_state::BottomHudPrompt;
use crate::assets::WeatherAsciiAssets;
use crate::error::{TerminalError, WeatherAssetError};
use crate::render::TerminalRenderer;
use crate::theme::{Palette, ThemeRegistry};
use std::fmt;
use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use system_services_model::SystemSnapshot;
use tokio::sync::watch;

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
    pub exit_semantic: ExitSemantic,
    pub first_frame_callback: Option<Arc<dyn Fn() -> io::Result<()> + Send + Sync>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitSemantic {
    Start,
    Quit,
}
impl ExitSemantic {
    fn prompt(self) -> BottomHudPrompt {
        match self {
            Self::Start => BottomHudPrompt::Start,
            Self::Quit => BottomHudPrompt::Quit,
        }
    }
    fn resolve(self, outcome: AppRunOutcome) -> ShellLockscreenResult {
        match outcome {
            AppRunOutcome::Cancelled => ShellLockscreenResult::Cancelled,
            AppRunOutcome::Space => match self {
                Self::Start => ShellLockscreenResult::Started,
                Self::Quit => ShellLockscreenResult::Quit,
            },
        }
    }
}

impl fmt::Debug for WeathrDisplayInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WeathrDisplayInput")
            .field("clock_format", &self.clock_format)
            .field("hide_hud", &self.hide_hud)
            .field("minimum_terminal_size", &self.minimum_terminal_size)
            .field("exit_semantic", &self.exit_semantic)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellLockscreenResult {
    Started,
    Quit,
    Cancelled,
}

#[derive(Debug)]
pub enum WeathrRunError {
    Terminal(TerminalError),
    Assets(WeatherAssetError),
    /// A display host could not complete setup before rendering started.
    Host(String),
    /// Retained for hosts that need to surface a typed unrecoverable display
    /// failure. The renderer does not install a panic hook.
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
            Self::Host(error) => write!(formatter, "weathr host setup failed: {error}"),
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
) -> Result<ShellLockscreenResult, WeathrRunError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(WeathrRunError::Runtime)?;
    runtime.block_on(run_display(input))
}

/// Renders one scene from snapshots. It performs no configuration lookup and
/// starts no weather, geolocation, cache or time request.
pub async fn run_display(
    input: WeathrDisplayInput,
) -> Result<ShellLockscreenResult, WeathrRunError> {
    run_display_inner(input).await
}

async fn run_display_inner(
    input: WeathrDisplayInput,
) -> Result<ShellLockscreenResult, WeathrRunError> {
    let mut registry = ThemeRegistry::new();
    registry.set_active_palette(input.palette);
    let assets = WeatherAsciiAssets::bundled()?;
    let dimensions = assets.max_dimensions();
    let minimum = minimum_terminal_size_for_assets(dimensions, input.minimum_terminal_size);
    let mut renderer = TerminalRenderer::new_with_minimum(minimum)?;
    let (width, height) = renderer.get_size();
    let mut app = App::new_with_bottom_hud_prompt_and_assets(AppInput {
        term_width: width,
        term_height: height,
        themes: registry,
        bottom_hud_prompt: input.exit_semantic.prompt(),
        snapshots: input.snapshots,
        clock_format: input.clock_format,
        hide_hud: input.hide_hud,
        assets,
    })?;
    renderer.init()?;
    let first_frame_callback = input.first_frame_callback;
    let run_result = app
        .run_with_outcome_and_shutdown(&mut renderer, &input.shutdown, first_frame_callback)
        .await
        .map(|outcome| input.exit_semantic.resolve(outcome))
        .map_err(WeathrRunError::Run);
    let cleanup = renderer.cleanup().map_err(WeathrRunError::Cleanup);
    match (run_result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn minimum_terminal_size_for_assets(
    dimensions: (usize, usize),
    explicit: Option<(u16, u16)>,
) -> (u16, u16) {
    let supplied = explicit.unwrap_or((0, 0));
    (
        u16::try_from(dimensions.0)
            .unwrap_or(u16::MAX)
            .max(supplied.0),
        u16::try_from(dimensions.1)
            .unwrap_or(u16::MAX)
            .max(supplied.1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn host_prompt_and_space_outcome_agree() {
        assert_eq!(ExitSemantic::Start.prompt(), BottomHudPrompt::Start);
        assert_eq!(
            ExitSemantic::Start.resolve(AppRunOutcome::Space),
            ShellLockscreenResult::Started
        );
        assert_eq!(ExitSemantic::Quit.prompt(), BottomHudPrompt::Quit);
        assert_eq!(
            ExitSemantic::Quit.resolve(AppRunOutcome::Space),
            ShellLockscreenResult::Quit
        );
    }
}
