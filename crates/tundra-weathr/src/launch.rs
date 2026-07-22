use crate::app::AppRunOutcome;
use crate::app_state::BottomHudPrompt;
use crate::config::{Config, LocationDisplay, Provider};
use crate::error::{ConfigError, TerminalError, WeatherAssetError, WeatherError};
use crate::render::TerminalRenderer;
use crate::theme::ThemeRegistry;
use crate::weather::{OpenMeteoProvider, WeatherClient, WeatherData, WeatherLocation};
use crate::{app, geolocation};
use crossterm::{
    cursor, execute,
    style::ResetColor,
    terminal::{LeaveAlternateScreen, disable_raw_mode},
};
use std::fmt;
use std::io;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;
use tundra_watchdog::{
    AppCriticality, AppDescriptor, AppId, AppWatchdog, BoundaryKind, BoundarySpec, CaughtPanic,
    ProcessWatchdog, RecoveryOutcome, WatchdogError,
};

#[derive(Debug, Clone, PartialEq)]
pub struct LaunchLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub city: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaunchOptions {
    /// Whether Weathr should read its standalone config file.
    ///
    /// Embedders that supply their own settings should disable this so Weathr
    /// does not depend on a second config file under the user's config dir.
    pub load_config_file: bool,
    pub prefer_config_location: bool,
    /// A free-form address query. If search fails, Weathr falls back to
    /// `location_override` (or its normal configured/default location).
    pub location_query: Option<String>,
    pub location_override: Option<LaunchLocation>,
    pub timezone_id: Option<String>,
    pub minimum_terminal_size: Option<(u16, u16)>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            load_config_file: true,
            prefer_config_location: true,
            location_query: None,
            location_override: None,
            timezone_id: None,
            minimum_terminal_size: None,
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

const CLI_UI_MAX_RECOVERIES: usize = 1;

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
    WatchdogUnavailable,
    Watchdog(WatchdogError),
    Panic { incident_id: String, reason: String },
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
            Self::WatchdogUnavailable => None,
            Self::Watchdog(error) => Some(error),
            Self::Panic { .. } => None,
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
        match value {
            WeatherAssetError::Watchdog(error) => Self::Watchdog(error),
            error => Self::Assets(error),
        }
    }
}

impl From<WatchdogError> for WeathrRunError {
    fn from(value: WatchdogError) -> Self {
        Self::Watchdog(value)
    }
}

/// Returns the canonical watchdog identity used by every Weathr host.
///
/// Shell, CLI, and future hosts should register this descriptor instead of
/// duplicating its metadata so repeated registration remains conflict-free.
pub fn weathr_watchdog_descriptor() -> AppDescriptor {
    AppDescriptor::new(
        AppId::from_static("weathr"),
        "Weathr",
        env!("CARGO_PKG_VERSION"),
        AppCriticality::SessionCritical,
    )
}

/// Restores terminal state without installing or replacing a panic hook.
///
/// Process hosts can register this function as watchdog emergency cleanup.
pub fn restore_terminal_best_effort() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show, ResetColor);
}

fn global_weathr_watchdog() -> Result<AppWatchdog, WeathrRunError> {
    let process = ProcessWatchdog::global().ok_or(WeathrRunError::WatchdogUnavailable)?;
    process
        .register_app(weathr_watchdog_descriptor())
        .map_err(WeathrRunError::Watchdog)
}

fn finalize_ui_panic(caught: CaughtPanic, mode: &'static str) -> WeathrRunError {
    let incident_id = caught.incident_id().to_string();
    let reason = caught.payload().to_string();
    let _ = caught.finalize(RecoveryOutcome::Unrecoverable(format!(
        "weathr {mode} UI session stopped after panic"
    )));
    WeathrRunError::Panic {
        incident_id,
        reason,
    }
}

pub fn run_default_blocking() -> Result<(), WeathrRunError> {
    run_blocking_with_options(LaunchOptions::default())
}

pub fn run_blocking_with_options(options: LaunchOptions) -> Result<(), WeathrRunError> {
    run_blocking_managed(options, global_weathr_watchdog()?)
}

pub fn run_blocking_managed(
    options: LaunchOptions,
    watchdog: AppWatchdog,
) -> Result<(), WeathrRunError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(WeathrRunError::Runtime)?;

    run_cli_ui_with_recovery(&watchdog, || {
        runtime
            .block_on(run_with_options(
                options.clone(),
                LaunchRunMode::Cli,
                watchdog.clone(),
            ))
            .map(|_| ())
    })
}

fn run_cli_ui_with_recovery<T, F>(
    watchdog: &AppWatchdog,
    mut attempt: F,
) -> Result<T, WeathrRunError>
where
    F: FnMut() -> Result<T, WeathrRunError>,
{
    let mut recoveries = 0_usize;
    loop {
        let result = watchdog.run_boundary(
            BoundarySpec::new("cli-ui-session", BoundaryKind::UiSession).terminal_owner(),
            AssertUnwindSafe(&mut attempt),
        );
        match result {
            Ok(result) => return result,
            Err(caught) if recoveries < CLI_UI_MAX_RECOVERIES => {
                recoveries += 1;
                let _ = caught.finalize(RecoveryOutcome::RecoveredWithWarnings(
                    "the Weathr CLI UI was rebuilt once from configuration and cache".to_string(),
                ));
            }
            Err(caught) => return Err(finalize_ui_panic(caught, "CLI")),
        }
    }
}

pub fn run_shell_lockscreen_blocking_with_options(
    options: LaunchOptions,
) -> Result<ShellLockscreenResult, WeathrRunError> {
    let watchdog = global_weathr_watchdog()?;
    let boundary = watchdog.clone();
    match boundary.run_boundary(
        BoundarySpec::new("standalone-shell-lockscreen-ui", BoundaryKind::UiSession)
            .terminal_owner(),
        AssertUnwindSafe(|| run_shell_lockscreen_managed(options, watchdog)),
    ) {
        Ok(result) => result,
        Err(caught) => Err(finalize_ui_panic(caught, "standalone Shell lockscreen")),
    }
}

/// Runs one Shell lockscreen UI session under the caller's supervisor.
///
/// A panic intentionally unwinds to the host boundary so the Shell can apply
/// its shared 60-second/two-recovery crash-loop policy.
pub fn run_shell_lockscreen_managed(
    options: LaunchOptions,
    watchdog: AppWatchdog,
) -> Result<ShellLockscreenResult, WeathrRunError> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(WeathrRunError::Runtime)?;

    runtime.block_on(run_with_options(
        options,
        LaunchRunMode::ShellLockscreen,
        watchdog,
    ))
}

async fn run_with_options(
    options: LaunchOptions,
    mode: LaunchRunMode,
    watchdog: AppWatchdog,
) -> Result<ShellLockscreenResult, WeathrRunError> {
    let mut config = load_config_for_launch(&options);

    let timezone_id = options.timezone_id.clone();
    apply_launch_location(&mut config, &options, Some(&watchdog)).await;

    let mut theme_registry = ThemeRegistry::new();
    let theme_id = config.normalized_theme();
    if theme_registry.set_active(theme_id).is_err() {
        eprintln!(
            "Warning: theme '{}' is not registered, falling back to 'default'.",
            theme_id
        );
    }

    let active_theme_id = theme_registry.active().id;
    let asset_dimensions = tundra_ascii_assets::AsciiAssetStore::load_theme(active_theme_id)
        .map_err(WeatherAssetError::from)?
        .max_asset_dimensions();
    let minimum_terminal_size =
        minimum_terminal_size_for_assets(asset_dimensions, options.minimum_terminal_size);

    let mut renderer = TerminalRenderer::new_with_minimum(minimum_terminal_size)?;
    let (term_width, term_height) = renderer.get_size();
    let mut app = app::App::new_with_bottom_hud_prompt(
        &config,
        term_width,
        term_height,
        theme_registry,
        timezone_id,
        mode.bottom_hud_prompt(),
        watchdog,
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

fn load_config_for_launch(options: &LaunchOptions) -> Config {
    if !options.load_config_file {
        return Config::default();
    }

    match Config::load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("Warning: could not load weathr config: {error}");
            Config::default()
        }
    }
}

/// Starts a fresh Open-Meteo request without acquiring terminal ownership.
///
/// The request intentionally bypasses the normal five-minute weather cache so
/// startup can refresh stale conditions while another component renders the
/// Banner. A successful response is persisted before this future completes,
/// allowing the later Weathr lockscreen to reuse it immediately.
pub async fn prefetch_weather(options: LaunchOptions) -> Result<WeatherData, WeatherError> {
    let mut config = load_config_for_launch(&options);
    apply_launch_location(&mut config, &options, None).await;
    let location = WeatherLocation {
        latitude: config.location.latitude,
        longitude: config.location.longitude,
        elevation: None,
    };
    let client = WeatherClient::new(Arc::new(OpenMeteoProvider::new()), Duration::from_secs(300));
    client
        .refresh_current_weather_for_startup(&location, &config.units, Provider::OpenMeteo)
        .await
}

fn minimum_terminal_size_for_assets(
    asset_dimensions: tundra_ascii_assets::AssetDimensions,
    requested_minimum: Option<(u16, u16)>,
) -> (u16, u16) {
    let (requested_width, requested_height) = requested_minimum.unwrap_or((
        crate::render::MIN_TERMINAL_WIDTH,
        crate::render::MIN_TERMINAL_HEIGHT,
    ));
    (
        requested_width.max(u16::try_from(asset_dimensions.width).unwrap_or(u16::MAX)),
        requested_height.max(u16::try_from(asset_dimensions.height).unwrap_or(u16::MAX)),
    )
}

async fn apply_launch_location(
    config: &mut Config,
    options: &LaunchOptions,
    watchdog: Option<&AppWatchdog>,
) {
    let searched_location = if let Some(query) = options
        .location_query
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty())
    {
        let result = match watchdog {
            Some(watchdog) => geolocation::search_address_managed(watchdog, query).await,
            None => geolocation::search_address(query).await,
        };
        match result {
            Ok(location) => Some(location),
            Err(error) => {
                eprintln!(
                    "Warning: weather location search for {query:?} failed: {}",
                    error.user_friendly_message().replace('\n', " ")
                );
                None
            }
        }
    } else {
        None
    };

    let geo_loc = if let Some(location) = searched_location {
        location
    } else if let Some(location) = &options.location_override {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tundra_watchdog::{WatchdogConfig, WatchdogRuntime};

    fn test_watchdog(
        name: &str,
    ) -> (
        WatchdogRuntime,
        ProcessWatchdog,
        AppWatchdog,
        std::path::PathBuf,
    ) {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test clock is after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tundra-weathr-{name}-{}-{suffix}",
            std::process::id()
        ));
        let config = WatchdogConfig::new(
            root.join("crashes"),
            root.join("fallback"),
            root.join("state"),
            "tundra-weathr-test",
            env!("CARGO_PKG_VERSION"),
        );
        let (runtime, process) =
            WatchdogRuntime::start_isolated(config).expect("test watchdog starts");
        let app = process
            .register_app(weathr_watchdog_descriptor())
            .expect("test Weathr app registers");
        (runtime, process, app, root)
    }

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

    #[test]
    fn standalone_launch_keeps_config_file_loading_enabled() {
        assert!(LaunchOptions::default().load_config_file);
    }

    #[test]
    fn embedded_launch_can_skip_the_standalone_config_file() {
        let options = LaunchOptions {
            load_config_file: false,
            ..LaunchOptions::default()
        };

        let config = load_config_for_launch(&options);
        assert_eq!(config.location.latitude, crate::config::default_latitude());
        assert_eq!(
            config.location.longitude,
            crate::config::default_longitude()
        );
        assert!(config.location.auto);
        assert_eq!(config.normalized_theme(), crate::config::DEFAULT_THEME);
    }

    #[test]
    fn watchdog_descriptor_is_stable_for_every_host() {
        let descriptor = weathr_watchdog_descriptor();

        assert_eq!(descriptor.id.as_str(), "weathr");
        assert_eq!(descriptor.display_name, "Weathr");
        assert_eq!(descriptor.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(descriptor.criticality, AppCriticality::SessionCritical);
    }

    #[test]
    fn cli_ui_rebuild_is_limited_to_one_recovery() {
        assert_eq!(CLI_UI_MAX_RECOVERIES, 1);
    }

    #[test]
    fn cli_ui_rebuilds_once_after_a_panic() {
        let (runtime, process, watchdog, root) = test_watchdog("cli-rebuild");
        let mut attempts = 0;

        let result = run_cli_ui_with_recovery(&watchdog, || {
            attempts += 1;
            if attempts == 1 {
                panic!("first UI failed");
            }
            Ok(())
        });

        assert!(result.is_ok());
        assert_eq!(attempts, 2);
        let incidents = process.drain_incidents();
        assert_eq!(incidents.len(), 1);
        assert!(matches!(
            &incidents[0].recovery,
            RecoveryOutcome::RecoveredWithWarnings(_)
        ));
        runtime.shutdown().expect("test watchdog shuts down");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn second_cli_ui_panic_stops_the_crash_loop() {
        let (runtime, process, watchdog, root) = test_watchdog("cli-crash-loop");
        let mut attempts = 0;

        let error = run_cli_ui_with_recovery::<(), _>(&watchdog, || {
            attempts += 1;
            panic!("UI failed repeatedly");
        })
        .expect_err("second UI panic must stop recovery");

        assert_eq!(attempts, 2);
        assert!(matches!(error, WeathrRunError::Panic { .. }));
        let incidents = process.drain_incidents();
        assert_eq!(incidents.len(), 2);
        assert!(
            incidents
                .iter()
                .any(|incident| matches!(&incident.recovery, RecoveryOutcome::Unrecoverable(_)))
        );
        runtime.shutdown().expect("test watchdog shuts down");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn resize_size_error_remains_one_actionable_line_after_launch_wrapping() {
        let error = WeathrRunError::Run(io::Error::other(TerminalError::TooSmall {
            width: 107,
            height: 20,
            min_width: 108,
            min_height: 20,
        }))
        .to_string();

        assert_eq!(error.lines().count(), 1);
        assert!(error.contains("107x20"));
        assert!(error.contains("108x20"));
        assert!(error.contains("resize"));
    }

    #[test]
    fn launch_terminal_minimum_tracks_assets_and_an_explicit_shell_floor() {
        assert_eq!(
            minimum_terminal_size_for_assets(
                tundra_ascii_assets::AssetDimensions {
                    width: 137,
                    height: 23,
                },
                None,
            ),
            (137, 23)
        );
        assert_eq!(
            minimum_terminal_size_for_assets(
                tundra_ascii_assets::AssetDimensions {
                    width: 66,
                    height: 10,
                },
                Some((108, 20)),
            ),
            (108, 20)
        );
    }
}
