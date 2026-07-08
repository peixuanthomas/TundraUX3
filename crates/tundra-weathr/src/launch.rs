use crate::config::{Config, LocationDisplay};
use crate::error::{ConfigError, TerminalError};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchOptions {
    pub prefer_config_location: bool,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            prefer_config_location: true,
        }
    }
}

#[derive(Debug)]
pub enum WeathrRunError {
    Config(ConfigError),
    Terminal(TerminalError),
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

pub fn run_default_blocking() -> Result<(), WeathrRunError> {
    install_panic_restore_hook();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(WeathrRunError::Runtime)?;

    runtime.block_on(run_with_options(LaunchOptions::default()))
}

async fn run_with_options(options: LaunchOptions) -> Result<(), WeathrRunError> {
    let mut config = match Config::load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("Warning: could not load weathr config: {error}");
            Config::default()
        }
    };

    apply_launch_location(&mut config, options);

    let mut theme_registry = ThemeRegistry::new();
    let theme_id = config.normalized_theme();
    if theme_registry.set_active(theme_id).is_err() {
        eprintln!(
            "Warning: theme '{}' is not registered, falling back to 'default'.",
            theme_id
        );
    }

    let mut renderer = TerminalRenderer::new()?;
    renderer.init()?;

    let (term_width, term_height) = renderer.get_size();
    let mut app = app::App::new(&config, term_width, term_height, theme_registry);

    let run_result = tokio::select! {
        result = app.run(&mut renderer) => result.map_err(WeathrRunError::Run),
        signal = tokio::signal::ctrl_c() => signal.map_err(WeathrRunError::Signal),
    };
    let cleanup_result = renderer.cleanup().map_err(WeathrRunError::Cleanup);

    match (run_result, cleanup_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error),
    }
}

fn apply_launch_location(config: &mut Config, options: LaunchOptions) {
    let geo_loc = if options.prefer_config_location && !config.location.auto {
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
