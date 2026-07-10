use crate::app::AppRunOutcome;
use crate::app_state::BottomHudPrompt;
use crate::config::{Config, LocationDisplay};
use crate::error::{ConfigError, TerminalError, WeatherAssetError};
use crate::render::TerminalRenderer;
use crate::theme::ThemeRegistry;
use crate::{app, geolocation};
use crossterm::{
    cursor, execute,
    style::ResetColor,
    terminal::{LeaveAlternateScreen, disable_raw_mode},
};
use std::fmt;
use std::io;
use std::panic;

#[derive(Debug, Clone, PartialEq)]
pub struct LaunchLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub city: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaunchOptions {
    pub prefer_config_location: bool,
    pub location_override: Option<LaunchLocation>,
    pub timezone_id: Option<String>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            prefer_config_location: true,
            location_override: None,
            timezone_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellLockscreenResult {
    Started,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchRunMode {
    Cli,
    ShellLockscreen,
}

impl LaunchRunMode {
    fn bottom_hud_prompt(self) -> BottomHudPrompt {
        match self {
            Self::Cli => BottomHudPrompt::Quit,
            Self::ShellLockscreen => BottomHudPrompt::Start,
        }
    }
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
    Config(ConfigError),
    Terminal(TerminalError),
    Assets(WeatherAssetError),
    Runtime(io::Error),
    Run(io::Error),
    Cleanup(io::Error),
    Signal(io::Error),
}

impl fmt::Display for WeathrRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(error) => write!(formatter, "failed to load weathr config: {error}"),
            Self::Terminal(error) => write!(formatter, "{}", error.user_friendly_message()),
            Self::Assets(error) => write!(formatter, "failed to load weathr ASCII assets: {error}"),
            Self::Runtime(error) => write!(formatter, "failed to start weathr runtime: {error}"),
            Self::Run(error) => write!(formatter, "weathr render loop failed: {error}"),
            Self::Cleanup(error) => write!(formatter, "failed to restore terminal: {error}"),
            Self::Signal(error) => write!(formatter, "failed to listen for Ctrl+C: {error}"),
        }
    }
}

impl std::error::Error for WeathrRunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Config(error) => Some(error),
            Self::Terminal(error) => Some(error),
            Self::Assets(error) => Some(error),
            Self::Runtime(error)
            | Self::Run(error)
            | Self::Cleanup(error)
            | Self::Signal(error) => Some(error),
        }
    }
}

impl From<ConfigError> for WeathrRunError {
    fn from(value: ConfigError) -> Self {
        Self::Config(value)
    }
}

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

pub fn run_default_blocking() -> Result<(), WeathrRunError> {
    run_blocking_with_options(LaunchOptions::default())
}

pub fn run_blocking_with_options(options: LaunchOptions) -> Result<(), WeathrRunError> {
    install_panic_restore_hook();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(WeathrRunError::Runtime)?;

    runtime
        .block_on(run_with_options(options, LaunchRunMode::Cli))
        .map(|_| ())
}

pub fn run_shell_lockscreen_blocking_with_options(
    options: LaunchOptions,
) -> Result<ShellLockscreenResult, WeathrRunError> {
    install_panic_restore_hook();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(WeathrRunError::Runtime)?;

    runtime.block_on(run_with_options(options, LaunchRunMode::ShellLockscreen))
}

async fn run_with_options(
    options: LaunchOptions,
    mode: LaunchRunMode,
) -> Result<ShellLockscreenResult, WeathrRunError> {
    let mut config = match Config::load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("Warning: could not load weathr config: {error}");
            Config::default()
        }
    };

    let timezone_id = options.timezone_id.clone();
    apply_launch_location(&mut config, &options);

    let mut theme_registry = ThemeRegistry::new();
    let theme_id = config.normalized_theme();
    if theme_registry.set_active(theme_id).is_err() {
        eprintln!(
            "Warning: theme '{}' is not registered, falling back to 'default'.",
            theme_id
        );
    }

    let mut renderer = TerminalRenderer::new()?;
    let (term_width, term_height) = renderer.get_size();
    let mut app = app::App::new_with_bottom_hud_prompt(
        &config,
        term_width,
        term_height,
        theme_registry,
        timezone_id,
        mode.bottom_hud_prompt(),
    )?;

    renderer.init()?;

    let run_result = tokio::select! {
        result = app.run_with_outcome(&mut renderer) => {
            result.map(ShellLockscreenResult::from).map_err(WeathrRunError::Run)
        },
        signal = tokio::signal::ctrl_c() => {
            signal
                .map(|_| ShellLockscreenResult::Cancelled)
                .map_err(WeathrRunError::Signal)
        },
    };
    let cleanup_result = renderer.cleanup().map_err(WeathrRunError::Cleanup);

    match (run_result, cleanup_result) {
        (Ok(result), Ok(())) => Ok(result),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn apply_launch_location(config: &mut Config, options: &LaunchOptions) {
    let geo_loc = if let Some(location) = &options.location_override {
        geolocation::GeoLocation {
            latitude: location.latitude,
            longitude: location.longitude,
            city: location.city.clone(),
        }
    } else if options.prefer_config_location && !config.location.auto {
        geolocation::GeoLocation {
            latitude: config.location.latitude,
            longitude: config.location.longitude,
            city: config.location.city.clone(),
        }
    } else {
        geolocation::fallback_location()
    };

    config.location.latitude = geo_loc.latitude;
    config.location.longitude = geo_loc.longitude;
    config.location.city = geo_loc.city;
    config.location.auto = false;
    config.location.hide = false;
    config.location.display = if config.location.city.is_some() {
        LocationDisplay::City
    } else {
        LocationDisplay::Coordinates
    };
}

fn install_panic_restore_hook() {
    let default_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show, ResetColor);
        default_hook(panic_info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_lockscreen_result_distinguishes_space_and_cancel() {
        assert_eq!(
            ShellLockscreenResult::from(AppRunOutcome::Space),
            ShellLockscreenResult::Started
        );
        assert_eq!(
            ShellLockscreenResult::from(AppRunOutcome::Cancelled),
            ShellLockscreenResult::Cancelled
        );
    }

    #[test]
    fn launch_mode_selects_shell_start_prompt() {
        assert_eq!(
            LaunchRunMode::Cli.bottom_hud_prompt(),
            BottomHudPrompt::Quit
        );
        assert_eq!(
            LaunchRunMode::ShellLockscreen.bottom_hud_prompt(),
            BottomHudPrompt::Start
        );
    }
}
