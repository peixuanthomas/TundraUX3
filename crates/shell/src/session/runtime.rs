use super::*;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use watchdog::{
    AppCriticality, AppDescriptor, AppId, AppWatchdog, BoundaryKind, BoundarySpec, CaughtPanic,
    ComponentId, IncidentKind, IncidentReceipt, ManagedThreadHandle, PanicAction, ProcessWatchdog,
    RecoveryOutcome, ReplaySafety, RestartPolicy, RuntimeSnapshot, TaskId, TaskKind, TaskSpec,
};

const MAX_READY_TERMINAL_EVENTS_PER_FRAME: usize = 4_096;
const COMMAND_LINE_REFRESH_INTERVAL: Duration = Duration::from_millis(16);
const BACKGROUND_POLL_INTERVAL: Duration = Duration::from_millis(250);

pub fn run_without_animation(output: &mut impl Write) -> io::Result<()> {
    run_not_fullscreen_without_animation(output)
}

pub fn run_not_fullscreen_without_animation(output: &mut impl Write) -> io::Result<()> {
    run_not_fullscreen_without_animation_with_loader(output, load_validated_runtime_ascii_assets)
}

pub(super) fn run_not_fullscreen_without_animation_with_loader(
    output: &mut impl Write,
    load_assets: impl FnOnce() -> io::Result<ui::RuntimeAsciiAssets>,
) -> io::Result<()> {
    let ascii_assets = load_assets()?;
    render_static_banner_with_assets(output, &ascii_assets)?;
    write_smoke_loop_message(output)
}

pub fn run_with_banner_animation(output: &mut impl Write) -> io::Result<()> {
    run_not_fullscreen(output)
}

pub fn run_not_fullscreen(output: &mut impl Write) -> io::Result<()> {
    run_not_fullscreen_with_loader(output, load_validated_runtime_ascii_assets)
}

pub(super) fn run_not_fullscreen_with_loader(
    output: &mut impl Write,
    load_assets: impl FnOnce() -> io::Result<ui::RuntimeAsciiAssets>,
) -> io::Result<()> {
    let ascii_assets = load_assets()?;
    run_not_fullscreen_with_assets(output, &ascii_assets)
}

pub(super) fn run_not_fullscreen_with_assets(
    output: &mut impl Write,
    ascii_assets: &ui::RuntimeAsciiAssets,
) -> io::Result<()> {
    display_startup_banner_with_assets(output, ascii_assets)?;
    write_smoke_loop_message(output)
}

pub fn run_shell_blocking(output: &mut impl Write) -> io::Result<()> {
    let process = ProcessWatchdog::global().ok_or_else(|| {
        io::Error::other("the process watchdog must be installed before starting tundra-shell")
    })?;
    run_shell_blocking_managed(output, process)
}

pub fn run_shell_blocking_managed(
    output: &mut impl Write,
    process: ProcessWatchdog,
) -> io::Result<()> {
    match run_shell_blocking_managed_with_outcome(output, process)? {
        ShellRunOutcome::Exit => Ok(()),
        ShellRunOutcome::RestartRequested => Err(restart_requires_binary_entrypoint()),
        ShellRunOutcome::ResetRequested => Err(reset_requires_binary_entrypoint()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellRunOutcome {
    Exit,
    RestartRequested,
    ResetRequested,
}

pub fn run_shell_blocking_managed_with_outcome(
    output: &mut impl Write,
    process: ProcessWatchdog,
) -> io::Result<ShellRunOutcome> {
    run_fullscreen_blocking_managed_with_outcome(output, process)
}

pub fn run_fullscreen_once_without_animation(output: &mut impl Write) -> io::Result<()> {
    run_fullscreen_once_without_animation_with_loader(output, load_validated_runtime_ascii_assets)
}

pub(super) fn run_fullscreen_once_without_animation_with_loader(
    output: &mut impl Write,
    load_assets: impl FnOnce() -> io::Result<ui::RuntimeAsciiAssets>,
) -> io::Result<()> {
    let ascii_assets = load_assets()?;
    with_fullscreen(output, |output| {
        render_static_banner_with_assets(output, &ascii_assets)?;
        write_smoke_loop_message(output)
    })
}

pub fn run_fullscreen_blocking(output: &mut impl Write) -> io::Result<()> {
    let process = ProcessWatchdog::global().ok_or_else(|| {
        io::Error::other("the process watchdog must be installed before starting tundra-shell")
    })?;
    run_fullscreen_blocking_managed(output, process)
}

pub fn run_frost_animation_preview(output: &mut impl Write) -> io::Result<()> {
    run_frost_animation_preview_with_color(output, storage::BorderColor::White)
}

pub fn run_frost_animation_preview_with_color(
    output: &mut impl Write,
    color: storage::BorderColor,
) -> io::Result<()> {
    let ascii_assets = load_validated_runtime_ascii_assets()?;
    with_fullscreen(output, |output| {
        display_startup_banner_with_assets_colored(output, &ascii_assets, ui_theme_color(color))
    })
}

pub fn run_matrix_animation_preview(output: &mut impl Write) -> io::Result<()> {
    run_matrix_animation_preview_with_color(output, storage::BorderColor::White)
}

pub fn run_matrix_animation_preview_with_color(
    output: &mut impl Write,
    color: storage::BorderColor,
) -> io::Result<()> {
    let ascii_assets = load_validated_runtime_ascii_assets()?;
    with_fullscreen(output, |output| {
        display_first_run_banner_with_assets_colored(output, &ascii_assets, ui_theme_color(color))
    })
}

pub fn run_fullscreen_blocking_managed(
    output: &mut impl Write,
    process: ProcessWatchdog,
) -> io::Result<()> {
    match run_fullscreen_blocking_managed_with_outcome(output, process)? {
        ShellRunOutcome::Exit => Ok(()),
        ShellRunOutcome::RestartRequested => Err(restart_requires_binary_entrypoint()),
        ShellRunOutcome::ResetRequested => Err(reset_requires_binary_entrypoint()),
    }
}

fn restart_requires_binary_entrypoint() -> io::Error {
    io::Error::new(
        io::ErrorKind::Interrupted,
        "the binary entry point must restart Shell",
    )
}

fn reset_requires_binary_entrypoint() -> io::Error {
    io::Error::new(
        io::ErrorKind::Interrupted,
        "Command Line requested a storage reset; the binary entry point must restart Shell",
    )
}

pub fn run_fullscreen_blocking_managed_with_outcome(
    output: &mut impl Write,
    process: ProcessWatchdog,
) -> io::Result<ShellRunOutcome> {
    let config = ShellLaunchConfig::default();
    let platform: std::sync::Arc<dyn Platform> = std::sync::Arc::from(platform::native_platform());
    let mut ascii_assets = match load_startup_runtime_ascii_assets(output, platform.as_ref())? {
        StartupAssetLoadOutcome::Loaded(assets) => assets,
        StartupAssetLoadOutcome::Restart => return Ok(ShellRunOutcome::RestartRequested),
        StartupAssetLoadOutcome::Exit => return Ok(ShellRunOutcome::Exit),
    };
    let terminal_size_requirement = ShellTerminalSizeRequirement::from_assets(&ascii_assets);
    checked_current_terminal_size(terminal_size_requirement)?;
    let terminal_control = TerminalControlHandler::install();
    let shell_watchdog = process
        .register_app(shell_watchdog_descriptor())
        .map_err(io::Error::other)?;
    let weathr_watchdog = process
        .register_app(weathr_watchdog_descriptor())
        .map_err(io::Error::other)?;
    let diagnostics_watchdog = process
        .register_app(app::diagnostics::diagnostics_watchdog_descriptor())
        .map_err(io::Error::other)?;
    let initial_startup = prepare_shell_startup(platform.as_ref()).map_err(io::Error::other)?;
    // Storage is initialized at this point, but login has not opened yet.
    // Keep this single service runtime alive across lockscreen/session cycles.
    let (system_services, _system_snapshots) = system_services::SystemServicesRuntime::start(
        system_services_config_for_startup(&initial_startup),
        shell_watchdog.clone(),
    );
    let (time_sync_sender, time_sync_receiver) = mpsc::channel();
    let time_sync_watchdog = shell_watchdog.child_component(ComponentId::from_static("time-sync"));
    // Both background jobs must be live before the blocking frost animation so
    // normal login can consume time calibration and prefetched weather data.
    let time_sync_worker = spawn_time_sync_worker(
        time_sync_sender,
        &time_sync_watchdog,
        system_services.clone(),
    )
    .map_err(io::Error::other)?;
    let (terminal_graphics_sender, terminal_graphics_receiver) = mpsc::sync_channel(1);
    let _terminal_graphics_worker =
        spawn_terminal_graphics_probe_worker(terminal_graphics_sender, &shell_watchdog)
            .map_err(io::Error::other)?;
    with_fullscreen(output, |output| {
        display_startup_banner_with_assets_colored(
            output,
            &ascii_assets,
            initial_startup.app_config.border_color,
        )
    })?;
    let terminal_graphics_probe = terminal_graphics_receiver.recv().unwrap_or_else(|_| {
        ui::TerminalGraphicsProbe::no_response(
            "terminal graphics detection worker stopped without returning a result",
        )
    });
    let mut initial_startup = Some(initial_startup);
    let mut cached_time_sync = None;
    let mut force_lockscreen = false;
    let mut show_terminal_graphics_notice = true;
    let mut session_recoveries = VecDeque::new();
    let mut explorer_task_runtime: Option<ShellExplorerTaskRuntime> = None;
    let mut diagnostics_task_runtime: Option<ShellDiagnosticsTaskRuntime> = None;
    // Linux installs its logind subscriptions lazily through this poll. Do it
    // before the first Weathr lockscreen so PrepareForShutdown can drive the
    // same process-wide shutdown flag used by the main Shell and lockscreen.
    let _ = platform.poll_lifecycle_event();

    loop {
        let mut startup = match initial_startup.take() {
            Some(startup) => startup,
            None => prepare_shell_startup(platform.as_ref()).map_err(io::Error::other)?,
        };
        let _ = system_services.reconfigure(system_services_config_for_startup(&startup));
        if explorer_task_runtime.is_none()
            && let Some(storage) = startup.storage_manager.as_ref()
        {
            let explorer_watchdog = process
                .register_app(app::explorer_tasks::explorer_watchdog_descriptor())
                .map_err(io::Error::other)?;
            explorer_task_runtime = Some(ShellExplorerTaskRuntime::new_managed(
                storage.clone(),
                explorer_watchdog,
            ));
        }
        if diagnostics_task_runtime.is_none()
            && let Some(storage) = startup.storage_manager.as_ref()
        {
            diagnostics_task_runtime = Some(ShellDiagnosticsTaskRuntime::new_managed(
                storage.clone(),
                process.clone(),
                diagnostics_watchdog.clone(),
            ));
        }
        if force_lockscreen || should_show_startup_lockscreen(&startup) {
            let lockscreen_input = weathr::WeathrDisplayInput {
                snapshots: system_services.subscribe(),
                clock_format: weathr::ClockFormat::TwentyFourHour,
                hide_hud: false,
                palette: weathr::theme::catalogue::DEFAULT_PALETTE,
                shutdown: terminal_control.shutdown_flag(),
                minimum_terminal_size: Some(terminal_size_requirement.as_terminal_size()),
                exit_semantic: weathr::ExitSemantic::Start,
            };
            let lockscreen_result = weathr_watchdog.run_boundary(
                BoundarySpec::new("shell-lockscreen-ui-session", BoundaryKind::UiSession)
                    .terminal_owner(),
                AssertUnwindSafe(|| weathr::run_display_blocking(lockscreen_input)),
            );
            match lockscreen_result {
                Ok(Ok(weathr::ShellLockscreenResult::Started)) => {}
                Ok(Ok(weathr::ShellLockscreenResult::Quit)) => return Ok(ShellRunOutcome::Exit),
                Ok(Ok(weathr::ShellLockscreenResult::Cancelled)) => {
                    return Ok(ShellRunOutcome::Exit);
                }
                Ok(Err(error)) => return Err(io::Error::other(error)),
                Err(caught) => {
                    recover_session_panic(
                        caught,
                        "Weathr lockscreen",
                        &mut session_recoveries,
                        platform.as_ref(),
                    )?;
                    force_lockscreen = true;
                    continue;
                }
            }
            startup = prepare_shell_startup(platform.as_ref()).map_err(io::Error::other)?;
        }

        let session_result = shell_watchdog.run_boundary(
            BoundarySpec::new("shell.fullscreen-session", BoundaryKind::UiSession).terminal_owner(),
            AssertUnwindSafe(|| {
                run_fullscreen_shell_session(FullscreenShellSessionInput {
                    output,
                    config,
                    startup,
                    ascii_assets: ascii_assets.clone(),
                    platform: std::sync::Arc::clone(&platform),
                    time_sync_receiver: &time_sync_receiver,
                    cached_time_sync: &mut cached_time_sync,
                    time_sync_worker: &time_sync_worker,
                    system_services: &system_services,
                    terminal_control: &terminal_control,
                    shell_watchdog: &shell_watchdog,
                    process_watchdog: &process,
                    explorer_task_runtime: explorer_task_runtime.clone(),
                    diagnostics_task_runtime: diagnostics_task_runtime.clone(),
                    terminal_graphics_probe: &terminal_graphics_probe,
                    show_terminal_graphics_notice: std::mem::take(
                        &mut show_terminal_graphics_notice,
                    ),
                })
            }),
        );
        match session_result {
            Ok(Ok((outcome, refreshed_ascii_assets))) => {
                ascii_assets = refreshed_ascii_assets;
                match outcome {
                    FullscreenShellSessionOutcome::Exit => return Ok(ShellRunOutcome::Exit),
                    FullscreenShellSessionOutcome::RestartRequested => {
                        return Ok(ShellRunOutcome::RestartRequested);
                    }
                    FullscreenShellSessionOutcome::ReturnToLockscreen => {
                        force_lockscreen = true;
                    }
                    FullscreenShellSessionOutcome::ResetRequested => {
                        return Ok(ShellRunOutcome::ResetRequested);
                    }
                }
            }
            Ok(Err(error)) => return Err(error),
            Err(caught) => {
                recover_session_panic(
                    caught,
                    "Shell UI",
                    &mut session_recoveries,
                    platform.as_ref(),
                )?;
                force_lockscreen = true;
            }
        }
        if diagnostics_task_runtime
            .as_ref()
            .is_some_and(ShellDiagnosticsTaskRuntime::restart_required)
        {
            return Ok(ShellRunOutcome::Exit);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FullscreenShellSessionOutcome {
    Exit,
    RestartRequested,
    ReturnToLockscreen,
    ResetRequested,
}

#[derive(Debug, Clone)]
pub(super) enum CachedTimeSyncResult {
    Success {
        utc: DateTime<Utc>,
        received_at: Instant,
    },
    Failure,
}

#[derive(Debug)]
pub(super) struct LauncherIconRequest {
    pub(super) id: String,
    pub(super) path: std::path::PathBuf,
}

#[derive(Debug)]
pub(super) struct LauncherIconResult {
    pub(super) id: String,
    pub(super) icon: Result<Option<PlatformIcon>, String>,
}

pub(super) struct CachedLauncherIcon {
    pub(super) area: Rect,
    pub(super) image: ui::PreparedEditorImage,
}

pub(super) struct LauncherIconRuntime {
    pub(super) picker: ui::EditorImagePicker,
    pub(super) requests: mpsc::Sender<LauncherIconRequest>,
    pub(super) results: mpsc::Receiver<LauncherIconResult>,
    pub(super) pending: HashSet<String>,
    pub(super) unavailable: HashSet<String>,
    pub(super) source_icons: HashMap<String, PlatformIcon>,
    pub(super) prepared: HashMap<String, CachedLauncherIcon>,
    pub(super) home_unavailable: HashSet<String>,
    pub(super) home_prepared: HashMap<String, CachedLauncherIcon>,
    pub(super) _worker: ManagedThreadHandle<()>,
}

impl LauncherIconRuntime {
    fn spawn(
        platform: std::sync::Arc<dyn Platform>,
        picker: ui::EditorImagePicker,
        watchdog: &AppWatchdog,
    ) -> Result<Self, String> {
        let (request_sender, request_receiver) = mpsc::channel::<LauncherIconRequest>();
        let (result_sender, result_receiver) = mpsc::channel::<LauncherIconResult>();
        let group = watchdog
            .child_component(ComponentId::from_static("launcher-icons"))
            .task_group("native-icons");
        let worker = group
            .spawn_thread(
                TaskSpec {
                    id: TaskId::from_static("loader"),
                    kind: TaskKind::LongRunning,
                    panic_action: PanicAction::ReportOnly,
                    replay_safety: ReplaySafety::Never,
                    restart_policy: RestartPolicy::never(),
                },
                move || {
                    while let Ok(request) = request_receiver.recv() {
                        let icon = platform
                            .file_icon(&request.path, 128)
                            .map_err(|error| error.to_string());
                        if result_sender
                            .send(LauncherIconResult {
                                id: request.id,
                                icon,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                },
            )
            .map_err(|error| error.to_string())?;
        Ok(Self {
            picker,
            requests: request_sender,
            results: result_receiver,
            pending: HashSet::new(),
            unavailable: HashSet::new(),
            source_icons: HashMap::new(),
            prepared: HashMap::new(),
            home_unavailable: HashSet::new(),
            home_prepared: HashMap::new(),
            _worker: worker,
        })
    }

    fn poll_results(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.results.try_recv() {
            changed = true;
            self.pending.remove(&result.id);
            match result.icon {
                Ok(Some(icon)) => {
                    self.source_icons.insert(result.id, icon);
                }
                Ok(None) | Err(_) => {
                    self.unavailable.insert(result.id);
                }
            }
        }
        changed
    }

    fn sync(&mut self, model: &ui::LauncherViewModel, main: Rect) {
        self.poll_results();
        let ids = model
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>();
        self.pending.retain(|id| ids.contains(id.as_str()));
        self.unavailable.retain(|id| ids.contains(id.as_str()));
        self.source_icons.retain(|id, _| ids.contains(id.as_str()));
        self.prepared.retain(|id, _| ids.contains(id.as_str()));
        if model.view_mode != app::launcher::LauncherViewMode::LargeIcons {
            return;
        }

        let layout = ui::launcher_layout(main, model);
        for item_layout in &layout.items {
            let Some(item) = model.items.get(item_layout.index) else {
                continue;
            };
            let needs_prepare = self
                .prepared
                .get(&item.id)
                .is_none_or(|cached| cached.area != item_layout.icon_area);
            if item.is_builtin() {
                if needs_prepare && !self.unavailable.contains(&item.id) {
                    self.prepared.remove(&item.id);
                    let prepared = model
                        .item_graphic_bytes(item)
                        .ok_or_else(|| "Launcher icon asset is not cached".to_string())
                        .and_then(|bytes| {
                            self.picker
                                .prepare_bytes(bytes, item_layout.icon_area)
                                .map_err(|error| error.to_string())
                        });
                    match prepared {
                        Ok(image) => {
                            self.prepared.insert(
                                item.id.clone(),
                                CachedLauncherIcon {
                                    area: item_layout.icon_area,
                                    image,
                                },
                            );
                        }
                        Err(_) => {
                            self.unavailable.insert(item.id.clone());
                        }
                    }
                }
                continue;
            }
            if needs_prepare
                && let Some(icon) = self.source_icons.get(&item.id)
                && let Ok(image) = self.picker.prepare_rgba(
                    icon.width(),
                    icon.height(),
                    icon.rgba().to_vec(),
                    item_layout.icon_area,
                )
            {
                self.prepared.insert(
                    item.id.clone(),
                    CachedLauncherIcon {
                        area: item_layout.icon_area,
                        image,
                    },
                );
            }
            if !self.source_icons.contains_key(&item.id)
                && !self.pending.contains(&item.id)
                && !self.unavailable.contains(&item.id)
                && self
                    .requests
                    .send(LauncherIconRequest {
                        id: item.id.clone(),
                        path: std::path::PathBuf::from(&item.path),
                    })
                    .is_ok()
            {
                self.pending.insert(item.id.clone());
            }
        }
    }

    fn sync_home(&mut self, model: &ui::HomeViewModel, main: Rect) {
        let labels = model
            .entries()
            .iter()
            .map(|entry| entry.label.as_str())
            .collect::<HashSet<_>>();
        self.home_unavailable
            .retain(|label| labels.contains(label.as_str()));
        self.home_prepared
            .retain(|label, _| labels.contains(label.as_str()));

        for (entry, tile) in model
            .entries()
            .iter()
            .zip(ui::home_entry_tile_areas(main, model.entries().len()))
        {
            let icon_area = ui::home_entry_icon_area(tile);
            if icon_area.width == 0 || icon_area.height == 0 {
                continue;
            }
            let needs_prepare = self
                .home_prepared
                .get(&entry.label)
                .is_none_or(|cached| cached.area != icon_area);
            if !needs_prepare || self.home_unavailable.contains(&entry.label) {
                continue;
            }

            self.home_prepared.remove(&entry.label);
            let prepared = model
                .home_icon_image_bytes_for_label(&entry.label)
                .ok_or_else(|| "Home icon asset is not cached".to_string())
                .and_then(|bytes| {
                    self.picker
                        .prepare_bytes(bytes, icon_area)
                        .map_err(|error| error.to_string())
                });
            match prepared {
                Ok(image) => {
                    self.home_prepared.insert(
                        entry.label.clone(),
                        CachedLauncherIcon {
                            area: icon_area,
                            image,
                        },
                    );
                }
                Err(_) => {
                    self.home_unavailable.insert(entry.label.clone());
                }
            }
        }
    }
}

impl ui::LauncherIconRenderer for LauncherIconRuntime {
    fn render_icon(&self, item_id: &str, frame: &mut ratatui::Frame<'_>, area: Rect) -> bool {
        let Some(icon) = self.prepared.get(item_id) else {
            return false;
        };
        icon.image.render_centered(frame, area);
        true
    }
}

impl ui::HomeIconRenderer for LauncherIconRuntime {
    fn render_icon(&self, entry_label: &str, frame: &mut ratatui::Frame<'_>, area: Rect) -> bool {
        let Some(icon) = self.home_prepared.get(entry_label) else {
            return false;
        };
        icon.image.render_centered(frame, area);
        true
    }
}

#[derive(Debug)]
pub(super) struct TimedTimeSyncResult {
    pub(super) result: TimeSyncResult,
    pub(super) received_at: Instant,
}

pub(super) struct TimeSyncWorker {
    pub(super) control_sender: tokio::sync::mpsc::UnboundedSender<TimeSyncControl>,
    pub(super) handle: Option<ManagedThreadHandle<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TimeSyncControl {
    Refresh,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeSyncWakeup {
    Control(TimeSyncControl),
    SnapshotChanged,
    Closed,
}

pub(super) const THEME_RELOAD_INTERVAL: Duration = Duration::from_millis(250);
pub(super) const THEME_RELOAD_NOTIFICATION_KEY: &str = "shell.theme-reload";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ThemeFileSignature {
    pub(super) modified: Option<SystemTime>,
    pub(super) byte_len: u64,
}

pub(super) struct UserThemeReloader {
    pub(super) storage: Option<StorageManager>,
    pub(super) last_observed: Option<Result<ThemeFileSignature, String>>,
    pub(super) active_user_id: Option<String>,
    pub(super) next_check: Instant,
}

impl UserThemeReloader {
    fn new(storage: Option<StorageManager>, now: Instant) -> Self {
        let last_observed = storage.as_ref().map(users_file_signature);
        Self {
            storage,
            last_observed,
            active_user_id: None,
            next_check: now.checked_add(THEME_RELOAD_INTERVAL).unwrap_or(now),
        }
    }

    fn poll_at(&mut self, now: Instant, theme: &mut ui::TundraTheme, state: &mut ShellSession) {
        let active_user_id = state.auth_session().map(|session| session.user_id.clone());
        let user_changed = self.active_user_id != active_user_id;
        if !user_changed && now < self.next_check {
            return;
        }
        self.next_check = now.checked_add(THEME_RELOAD_INTERVAL).unwrap_or(now);

        let Some(storage) = self.storage.as_ref() else {
            return;
        };

        if active_user_id.is_none() {
            let app_config = ShellAppConfig::default();
            theme.border_shape = app_config.border_shape;
            theme.border_color = app_config.border_color;
            theme.accent_color = app_config.accent_color;
            state
                .app
                .dispatch_at(app::AppCommand::SetActiveAppearance(None), now);
            self.active_user_id = None;
            state.notification_dismiss_modal_by_key(THEME_RELOAD_NOTIFICATION_KEY);
            state.finish_modal_focus_transition();
            return;
        }

        let observed = users_file_signature(storage);
        if !user_changed && self.last_observed.as_ref() == Some(&observed) {
            return;
        }
        self.last_observed = Some(observed.clone());

        let result = observed.and_then(|_| {
            let users = storage.load_users().map_err(|error| error.to_string())?;
            let user_id = active_user_id.as_deref().unwrap_or_default();
            users
                .users
                .iter()
                .find(|user| user.id == user_id)
                .map(|user| user.appearance.clone())
                .ok_or_else(|| format!("active user {user_id:?} is missing"))
        });
        self.active_user_id = active_user_id;
        match result {
            Ok(appearance) => {
                let app_config = ShellAppConfig::from_appearance(&appearance);
                theme.border_shape = app_config.border_shape;
                theme.border_color = app_config.border_color;
                theme.accent_color = app_config.accent_color;
                state
                    .app
                    .dispatch_at(app::AppCommand::SetActiveAppearance(Some(appearance)), now);
                state.notification_dismiss_modal_by_key(THEME_RELOAD_NOTIFICATION_KEY);
                state.finish_modal_focus_transition();
            }
            Err(error) => {
                let notification = ShellNotification::modal(
                    "Theme reload failed",
                    format!(
                        "Could not reload the active user's theme: {error}. The last valid theme is still active."
                    ),
                    ui::NotificationTone::Error,
                    vec![
                        ShellNotificationAction::new("ok", "OK")
                            .with_shortcut(InputKey::Escape)
                            .cancel(),
                    ],
                )
                .with_key(THEME_RELOAD_NOTIFICATION_KEY);
                state.notify_modal_with_options(notification);
            }
        }
    }
}

pub(super) fn users_file_signature(storage: &StorageManager) -> Result<ThemeFileSignature, String> {
    let path = &storage.layout().users_path;
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
    Ok(ThemeFileSignature {
        modified: metadata.modified().ok(),
        byte_len: metadata.len(),
    })
}

impl TimeSyncWorker {
    fn request_refresh(&self) {
        let _ = self.control_sender.send(TimeSyncControl::Refresh);
    }

    fn stop_and_join(&mut self) {
        let _ = self.control_sender.send(TimeSyncControl::Stop);
        if let Some(handle) = self.handle.take() {
            handle.cancel();
            let _ = handle.join();
        }
    }
}

impl Drop for TimeSyncWorker {
    fn drop(&mut self) {
        self.stop_and_join();
    }
}

impl CachedTimeSyncResult {
    pub(super) fn apply_to_state_at(&self, state: &mut ShellSession, now: Instant) {
        match self {
            Self::Success { utc, received_at } => {
                let elapsed = now.saturating_duration_since(*received_at);
                state.apply_time_sync_utc(*utc + elapsed);
            }
            Self::Failure => {
                state.apply_time_sync_failure_message("联网校准时间失败");
            }
        }
    }
}

pub(super) struct FullscreenShellSessionInput<'a, W> {
    output: &'a mut W,
    config: ShellLaunchConfig,
    startup: ShellStartupState,
    ascii_assets: ui::RuntimeAsciiAssets,
    platform: std::sync::Arc<dyn Platform>,
    time_sync_receiver: &'a mpsc::Receiver<TimedTimeSyncResult>,
    cached_time_sync: &'a mut Option<CachedTimeSyncResult>,
    time_sync_worker: &'a TimeSyncWorker,
    system_services: &'a system_services::SystemServicesHandle,
    terminal_control: &'a TerminalControlHandler,
    shell_watchdog: &'a AppWatchdog,
    process_watchdog: &'a ProcessWatchdog,
    explorer_task_runtime: Option<ShellExplorerTaskRuntime>,
    diagnostics_task_runtime: Option<ShellDiagnosticsTaskRuntime>,
    terminal_graphics_probe: &'a ui::TerminalGraphicsProbe,
    show_terminal_graphics_notice: bool,
}

pub(super) fn run_fullscreen_shell_session<W: Write>(
    input: FullscreenShellSessionInput<'_, W>,
) -> io::Result<(FullscreenShellSessionOutcome, ui::RuntimeAsciiAssets)> {
    let FullscreenShellSessionInput {
        output,
        config,
        startup,
        ascii_assets,
        platform,
        time_sync_receiver,
        cached_time_sync,
        time_sync_worker,
        system_services,
        terminal_control,
        shell_watchdog,
        process_watchdog,
        explorer_task_runtime,
        diagnostics_task_runtime,
        terminal_graphics_probe,
        show_terminal_graphics_notice,
    } = input;
    let terminal_size_requirement = ShellTerminalSizeRequirement::from_assets(&ascii_assets);
    let initial_size = checked_current_terminal_size(terminal_size_requirement)?;
    let mut guard = TerminalGuard::enter(output)?;
    if let Some(diagnostics) = diagnostics_task_runtime.as_ref() {
        diagnostics.set_terminal_graphics_probe(terminal_graphics_probe.status().clone());
    }
    let mut launcher_icons = terminal_graphics_probe
        .picker()
        .cloned()
        .and_then(|picker| {
            LauncherIconRuntime::spawn(std::sync::Arc::clone(&platform), picker, shell_watchdog)
                .ok()
        });
    let theme_storage = startup.storage_manager.clone();
    if startup.auth_bootstrap_required {
        display_first_run_banner_with_assets_colored(
            guard.terminal_mut().backend_mut(),
            &ascii_assets,
            startup.app_config.border_color,
        )?;
    }
    let mut theme = ui::TundraTheme::default_dark();
    let settings_services_config = system_services_config_for_startup(&startup);
    let mut state = ShellSession::new_with_runtime_services(
        config,
        initial_size,
        startup,
        ascii_assets,
        ShellRuntimeServices {
            explorer: explorer_task_runtime,
            diagnostics: diagnostics_task_runtime,
            editor: ShellEditorTaskRuntime::new_managed(shell_watchdog.clone()),
            settings: ShellSettingsTaskRuntime::new_managed_with_system_services(
                shell_watchdog.clone(),
                Some(system_services.clone()),
                settings_services_config,
            ),
        },
    );
    let mut system_status_snapshots = system_services.subscribe();
    state.apply_system_status_snapshot(app::AppSystemStatusSnapshot::from(
        &*system_status_snapshots.borrow_and_update(),
    ));
    state.set_terminal_image_support(launcher_icons.is_some());
    state.set_terminal_text_sizing_support(terminal_graphics_probe.text_sizing_protocol());
    if show_terminal_graphics_notice {
        state.apply_terminal_graphics_startup_policy(terminal_graphics_probe.status());
    }
    state.launcher_task_runtime = Some(ShellLauncherTaskRuntime::new_managed(
        std::sync::Arc::clone(&platform),
        shell_watchdog.clone(),
    ));
    let mut command_line_host = CommandLineHost::new(shell_watchdog.clone());
    let mut reset_requested = false;
    if let Some(cached) = cached_time_sync.as_ref() {
        cached.apply_to_state_at(&mut state, Instant::now());
    }
    let runtime_origin = Instant::now();
    let mut theme_reloader = UserThemeReloader::new(theme_storage, runtime_origin);
    let mut redraw = RedrawScheduler::new(
        runtime_origin,
        RedrawIdentity::from_session(&state),
        reduced_motion_enabled(&state),
    );
    let mut motion_effects = ShellMotionEffects::default();
    let mut shell_toast: Option<ui::components::Toast> = None;
    let mut terminal_size_error = None;
    let mut terminal_suspended = false;
    let mut last_background_poll = runtime_origin;

    loop {
        let state_before_polling = state.clone();
        let theme_before_polling = theme;
        drain_system_status_snapshot(&mut system_status_snapshots, &mut state);
        // logind signals are delivered by a backend worker and drained here so
        // neither D-Bus nor policy authorization can block terminal input.
        for _ in 0..16 {
            let lifecycle_event = match platform.poll_lifecycle_event() {
                Ok(event) => event,
                Err(error) => {
                    state.notify_alert_with_tone(
                        format!("Desktop lifecycle monitoring failed: {error}"),
                        ui::NotificationTone::Error,
                    );
                    break;
                }
            };
            let Some(lifecycle_event) = lifecycle_event else {
                break;
            };
            match lifecycle_event {
                PlatformLifecycleEvent::PrepareForShutdown => {
                    state.apply_input_with_platform(InputEvent::Shutdown, platform.as_ref());
                }
                PlatformLifecycleEvent::PrepareForSleep if !terminal_suspended => {
                    let _ = state.persist_editor_recovery_now(Instant::now());
                    motion_effects.cancel_for_bounds_change();
                    guard.restore()?;
                    terminal_suspended = true;
                }
                PlatformLifecycleEvent::Resumed => {
                    if terminal_suspended {
                        guard.resume()?;
                        terminal_suspended = false;
                    }
                    refresh_session_after_resume(&mut state, platform.as_ref(), time_sync_worker);
                }
                PlatformLifecycleEvent::PrepareForSleep => {}
            }
        }

        if terminal_suspended {
            if terminal_control.shutdown_requested() {
                state.apply_input_with_platform(InputEvent::Shutdown, platform.as_ref());
            }
            if state.shutdown_requested() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }

        if let Err(error) = terminal_size_requirement.validate(crossterm::terminal::size()?) {
            terminal_size_error = Some(io::Error::other(error));
            break;
        }

        let command_line_before_poll = (state.content_screen() == ShellScreen::CommandLine)
            .then(|| command_line_host.view_model());
        if state.content_screen() == ShellScreen::CommandLine {
            let username = state.current_home_username().unwrap_or("tundra");
            command_line_host.ensure_started(platform.as_ref(), username);
            match command_line_host.poll() {
                CommandLineHostEvent::None => {
                    let (width, height) = state.terminal_size();
                    if let Some(terminal_area) =
                        ui::command_line_terminal_area(Rect::new(0, 0, width, height))
                    {
                        command_line_host.resize_to_area(terminal_area);
                    }
                }
                CommandLineHostEvent::ExitToLauncher => state.close_command_line(),
                CommandLineHostEvent::ResetRequested => {
                    reset_requested = true;
                    break;
                }
            }
        }
        if command_line_before_poll
            .as_ref()
            .is_some_and(|before| *before != command_line_host.view_model())
        {
            redraw.request_redraw();
        }

        drain_time_sync_results(&mut state, time_sync_receiver, cached_time_sync);
        drain_watchdog_incidents(&mut state, process_watchdog);
        shell_watchdog.heartbeat(RuntimeSnapshot {
            screen: Some(format!("{:?}", state.active_screen())),
            terminal_size: Some(state.terminal_size()),
            ..RuntimeSnapshot::default()
        });
        let frame_now = Instant::now();
        if launcher_icons
            .as_mut()
            .is_some_and(LauncherIconRuntime::poll_results)
        {
            redraw.request_redraw();
        }
        theme_reloader.poll_at(frame_now, &mut theme, &mut state);
        let clock_snapshot = state.app.snapshot().clock;
        state.advance_clock_background_at(&clock_snapshot, frame_now);
        if session_render_state_changed(&state_before_polling, &state)
            || theme != theme_before_polling
        {
            redraw.request_redraw();
        }
        redraw.observe(
            frame_now,
            RedrawIdentity::from_session(&state),
            reduced_motion_enabled(&state),
        );
        if redraw.is_due(frame_now) {
            let active_screen = state.active_screen();
            let content_screen = state.content_screen();
            let chrome = state.to_shell_chrome_view_model();
            // Construct only the model that can be rendered this frame. Explorer,
            // Launcher, Editor, and Diagnostics models may clone sizable lists or
            // formatted content; rebuilding all of them for every Editor key made
            // input latency depend on unrelated background state.
            let home = matches!(content_screen, ShellScreen::Home | ShellScreen::ExitConfirm)
                .then(|| state.to_home_view_model());
            let clock = (content_screen == ShellScreen::Clock).then(|| {
                let terminal_cell_aspect_ratio = crossterm::terminal::window_size()
                    .map(|window| {
                        ui::TerminalCellAspectRatio::from_window_size(
                            window.columns,
                            window.rows,
                            window.width,
                            window.height,
                        )
                    })
                    .unwrap_or_default();
                state
                    .to_clock_view_model_at(&clock_snapshot, frame_now)
                    .with_terminal_cell_aspect_ratio(terminal_cell_aspect_ratio)
            });
            let time_sync_dialog = (content_screen != ShellScreen::CommandLine)
                .then(|| state.to_time_sync_dialog_view_model())
                .flatten();
            let setup =
                (content_screen == ShellScreen::FirstRunSetup).then(|| state.to_setup_view_model());
            let login = (content_screen == ShellScreen::Login)
                .then(|| state.to_login_view_model_at(frame_now));
            let bootstrap_admin = (content_screen == ShellScreen::BootstrapAdmin)
                .then(|| state.to_bootstrap_admin_view_model());
            let user_management = (content_screen == ShellScreen::UserManagement)
                .then(|| state.to_user_management_view_model());
            let explorer =
                (content_screen == ShellScreen::Explorer).then(|| state.to_explorer_view_model());
            let launcher =
                (content_screen == ShellScreen::Launcher).then(|| state.to_launcher_view_model());
            let command_line = (content_screen == ShellScreen::CommandLine).then(|| {
                let mut model = command_line_host.view_model();
                if let Some(username) = state.current_home_username() {
                    model = model.with_prompt_username(username);
                }
                model
            });
            let motion_frame = redraw.frame(frame_now);
            let motion_transitions = redraw.transitions(frame_now);
            let render_context = ui::RenderContext::from_theme_with_transitions(
                &theme,
                motion_frame,
                motion_transitions,
                shell_render_capabilities(terminal_graphics_probe),
            );
            let visible_toast = chrome
                .status
                .error
                .is_none()
                .then_some(chrome.status.toast.as_deref())
                .flatten();
            sync_shell_toast(&mut shell_toast, visible_toast, motion_frame);
            if shell_toast
                .as_ref()
                .is_some_and(|toast| !toast.is_visible(motion_frame))
            {
                shell_toast = None;
            }
            let graphical_icons_enabled = state.graphical_icons_enabled();
            if graphical_icons_enabled
                && let Some(icon_runtime) = launcher_icons.as_mut()
                && let ui::ShellLayout::Full { main, .. } =
                    ui::compute_shell_layout(render_context.page_area(Rect::new(
                        0,
                        0,
                        state.terminal_size().0,
                        state.terminal_size().1,
                    )))
            {
                if let Some(launcher) = launcher.as_ref() {
                    icon_runtime.sync(launcher, main);
                }
                if let Some(home) = home.as_ref() {
                    icon_runtime.sync_home(home, main);
                }
            }
            let editor =
                (content_screen == ShellScreen::Editor).then(|| state.to_editor_view_model());
            let settings = (content_screen == ShellScreen::Settings)
                .then(|| state.to_settings_view_model())
                .flatten();
            let diagnostics = (content_screen == ShellScreen::Diagnostics)
                .then(|| state.to_diagnostics_view_model());
            let system_status = (content_screen == ShellScreen::SystemStatus)
                .then(|| state.to_system_status_view_model())
                .flatten();
            let notification = (content_screen != ShellScreen::CommandLine)
                .then(|| state.to_notification_view_model())
                .flatten();
            let exit_confirmation = ui::ExitConfirmViewModel::new();

            state.refresh_hit_map_with_motion(motion_transitions);
            let terminal_area = Rect::new(0, 0, state.terminal_size().0, state.terminal_size().1);
            let page_area = render_context.page_area(terminal_area);
            let status_area = match ui::compute_shell_layout(page_area) {
                ui::ShellLayout::Full { status, .. } => Some(status),
                ui::ShellLayout::Compact(_) => None,
            };
            motion_effects.update(
                &state,
                terminal_area,
                page_area,
                status_area,
                render_context.theme,
                motion_frame.reduced_motion,
            );
            guard.terminal_mut().draw(|frame| {
                let area = frame.area();
                let page_area = render_context.page_area(area);
                match content_screen {
                    ShellScreen::FirstRunSetup => {
                        ui::render_setup_with_context(
                            frame,
                            page_area,
                            &chrome,
                            setup.as_ref().expect("Setup requires its view model"),
                            &render_context,
                        );
                    }
                    ShellScreen::Login => {
                        ui::render_login_with_context(
                            frame,
                            page_area,
                            &chrome,
                            login.as_ref().expect("Login requires its view model"),
                            &render_context,
                        );
                    }
                    ShellScreen::BootstrapAdmin => {
                        ui::render_bootstrap_admin_with_context(
                            frame,
                            page_area,
                            &chrome,
                            bootstrap_admin
                                .as_ref()
                                .expect("Bootstrap admin requires its view model"),
                            &render_context,
                        );
                    }
                    ShellScreen::UserManagement => {
                        ui::render_user_management_with_context(
                            frame,
                            page_area,
                            &chrome,
                            user_management
                                .as_ref()
                                .expect("User management requires its view model"),
                            &render_context,
                        );
                    }
                    ShellScreen::Explorer => {
                        ui::render_explorer_with_context(
                            frame,
                            page_area,
                            &chrome,
                            explorer.as_ref().expect("Explorer requires its view model"),
                            &render_context,
                        );
                    }
                    ShellScreen::Launcher => {
                        ui::render_launcher_with_context(
                            frame,
                            page_area,
                            &chrome,
                            launcher.as_ref().expect("Launcher requires its view model"),
                            &render_context,
                        );
                        if graphical_icons_enabled
                            && let Some(icons) = launcher_icons.as_ref()
                            && let ui::ShellLayout::Full { main, .. } =
                                ui::compute_shell_layout(page_area)
                        {
                            let model =
                                launcher.as_ref().expect("Launcher requires its view model");
                            for item_layout in ui::launcher_layout(main, model).items {
                                if let Some(item) = model.items.get(item_layout.index) {
                                    ui::LauncherIconRenderer::render_icon(
                                        icons,
                                        &item.id,
                                        frame,
                                        item_layout.icon_area,
                                    );
                                }
                            }
                        }
                    }
                    ShellScreen::CommandLine => {
                        ui::render_command_line_with_context(
                            frame,
                            page_area,
                            &chrome,
                            command_line
                                .as_ref()
                                .expect("Command Line requires its view model"),
                            &render_context,
                        );
                    }
                    ShellScreen::Editor => {
                        ui::render_editor_app_with_context(
                            frame,
                            page_area,
                            &chrome,
                            editor
                                .as_ref()
                                .expect("Editor content requires its view model"),
                            &render_context,
                        );
                    }
                    ShellScreen::Settings => {
                        ui::render_settings_with_context(
                            frame,
                            page_area,
                            &chrome,
                            settings
                                .as_ref()
                                .expect("Settings content requires its view model"),
                            &render_context,
                        );
                    }
                    ShellScreen::Diagnostics => {
                        ui::render_diagnostics_with_context(
                            frame,
                            page_area,
                            &chrome,
                            diagnostics
                                .as_ref()
                                .expect("Diagnostics requires its view model"),
                            &render_context,
                        );
                    }
                    ShellScreen::SystemStatus => {
                        ui::render_system_status_contextual(
                            frame,
                            page_area,
                            &chrome,
                            system_status
                                .as_ref()
                                .expect("System Status requires its view model"),
                            &render_context,
                        );
                    }
                    ShellScreen::Clock => {
                        ui::render_clock_with_context(
                            frame,
                            page_area,
                            &chrome,
                            clock.as_ref().expect("Clock requires its view model"),
                            &render_context,
                        );
                    }
                    ShellScreen::Home | ShellScreen::ExitConfirm => {
                        ui::render_home_with_context(
                            frame,
                            page_area,
                            &chrome,
                            home.as_ref().expect("Home requires its view model"),
                            &render_context,
                        );
                        if graphical_icons_enabled
                            && let Some(icons) = launcher_icons.as_ref()
                            && let ui::ShellLayout::Full { main, .. } =
                                ui::compute_shell_layout(page_area)
                        {
                            let model = home.as_ref().expect("Home requires its view model");
                            for (entry, tile) in model
                                .entries()
                                .iter()
                                .zip(ui::home_entry_tile_areas(main, model.entries().len()))
                            {
                                ui::HomeIconRenderer::render_icon(
                                    icons,
                                    &entry.label,
                                    frame,
                                    ui::home_entry_icon_area(tile),
                                );
                            }
                        }
                    }
                }

                if notification.is_none() && active_screen == ShellScreen::ExitConfirm {
                    ui::render_exit_confirmation_with_context(
                        frame,
                        area,
                        &exit_confirmation,
                        &render_context,
                    );
                }
                if notification.is_none()
                    && let Some(dialog) = time_sync_dialog.as_ref()
                {
                    ui::render_time_sync_failure_dialog_with_context(
                        frame,
                        area,
                        dialog,
                        &render_context,
                    );
                }
                if let Some(notification) = notification.as_ref() {
                    ui::render_notification_overlay_with_context(
                        frame,
                        area,
                        notification,
                        &render_context,
                    );
                }
                if notification.is_none()
                    && let Some(toast) = shell_toast.as_ref()
                    && let ui::ShellLayout::Full { status, .. } =
                        ui::compute_shell_layout(page_area)
                {
                    toast.render_frame(frame, status, &render_context);
                }
                motion_effects.process(motion_frame.delta, frame.buffer_mut(), &state);
            })?;
            let toast_requests_redraw = shell_toast
                .as_ref()
                .is_some_and(|toast| toast.requests_redraw(motion_frame));
            redraw.did_draw(frame_now);
            if toast_requests_redraw {
                redraw.request_animation_frame(frame_now);
            }
            if motion_effects.is_running() {
                redraw.request_animation_frame(frame_now);
            }
        }

        if terminal_control.shutdown_requested() {
            state.apply_input_with_platform(InputEvent::Shutdown, platform.as_ref());
        }
        if state.shutdown_requested() {
            break;
        }

        let poll_now = Instant::now();
        // The system-status watch receiver is intentionally polled even while
        // the user is idle, so snapshots reach AppState within the 250 ms
        // background cadence without a busy loop.
        let background_work_outstanding = if system_status_snapshots.has_changed().is_ok() {
            true
        } else {
            session_has_background_work(&state)
                || launcher_icons
                    .as_ref()
                    .is_some_and(|icons| !icons.pending.is_empty())
        };
        let background_poll_timeout = background_poll_timeout(
            background_work_outstanding,
            poll_now.saturating_duration_since(last_background_poll),
        );
        let state_poll_timeout = state.auth_poll_timeout(
            poll_now,
            state.notification_poll_timeout(poll_now, background_poll_timeout),
        );
        let redraw_timeout = redraw.poll_timeout(poll_now, Duration::MAX);
        let combined_timeout = state_poll_timeout.min(redraw_timeout);
        let (poll_timeout, command_line_timeout_is_state) = command_line_poll_timeout(
            state.content_screen() == ShellScreen::CommandLine,
            combined_timeout,
        );
        let state_timeout_wakeup =
            command_line_timeout_is_state && state_poll_timeout <= redraw_timeout;
        let mut action = None;
        let mut terminal_event_received = false;
        if let Some(input) = motion_effects.take_deferred_close() {
            action = Some(state.apply_input_with_platform(input, platform.as_ref()));
            redraw.request_redraw();
        }
        if event::poll(poll_timeout)? {
            terminal_event_received = true;
            let terminal_events = read_ready_terminal_event_batch(event::read()?)?;
            for terminal_event in terminal_events {
                let identity_before_input = RedrawIdentity::from_session(&state);
                if let event::Event::Resize(width, height) = &terminal_event
                    && let Err(error) = terminal_size_requirement.validate((*width, *height))
                {
                    terminal_size_error = Some(io::Error::other(error));
                    break;
                }
                if matches!(&terminal_event, event::Event::Resize(_, _)) {
                    motion_effects.cancel_for_bounds_change();
                }
                let input = crossterm_event_to_input(terminal_event);
                let command_line_captures = command_line_captures_input(&state, &input);
                if command_line_captures {
                    let (width, height) = state.terminal_size();
                    let terminal_area =
                        ui::command_line_terminal_area(Rect::new(0, 0, width, height));
                    match command_line_host.handle_input(&input, terminal_area) {
                        CommandLineHostEvent::None => {}
                        CommandLineHostEvent::ExitToLauncher => {
                            command_line_host.terminate();
                            state.close_command_line();
                        }
                        CommandLineHostEvent::ResetRequested => {
                            reset_requested = true;
                            break;
                        }
                    }
                    action = Some(ShellAction::Redraw);
                } else {
                    // Route against a clone solely to classify semantic cancellation before
                    // mutation; the real state still goes through the complete input preamble.
                    let semantic_cancel = motion_effects.has_interactive_overlay()
                        && state
                            .clone()
                            .route_input_at(input.clone(), Instant::now())
                            .command
                            .is_overlay_cancel_or_close();
                    match motion_effects.intercept_input(&input, semantic_cancel) {
                        MotionInputDisposition::Apply => {
                            action =
                                Some(state.apply_input_with_platform(input, platform.as_ref()));
                        }
                        MotionInputDisposition::Defer | MotionInputDisposition::Block => {
                            action = Some(ShellAction::Redraw);
                            redraw.request_animation_frame(Instant::now());
                        }
                    }
                }
                synchronize_motion_hit_map_after_input(
                    &mut state,
                    &mut redraw,
                    identity_before_input,
                    Instant::now(),
                );
                if action.is_some_and(|action| action != ShellAction::Redraw) {
                    break;
                }
            }
        } else if state_timeout_wakeup {
            action = Some(state.apply_input_with_platform(InputEvent::Tick, platform.as_ref()));
            if background_work_outstanding
                && poll_now.saturating_duration_since(last_background_poll)
                    >= BACKGROUND_POLL_INTERVAL
            {
                last_background_poll = Instant::now();
            }
        }

        if terminal_size_error.is_some() {
            break;
        }
        if reset_requested {
            break;
        }

        if terminal_event_received && action == Some(ShellAction::Redraw) {
            redraw.request_redraw();
        }
        if session_render_state_changed(&state_before_polling, &state) {
            redraw.request_redraw();
        }

        if action == Some(ShellAction::Exit) {
            break;
        }
        if action == Some(ShellAction::PowerOff) {
            // Interactive authorization may temporarily take over the
            // terminal. Recovery has already been persisted by the command
            // handler, so restore the user's terminal before asking logind or
            // the native platform service to power off.
            guard.restore()?;
            match platform.poweroff() {
                Ok(()) => break,
                Err(error) => {
                    guard.resume()?;
                    if let Ok((width, height)) = crossterm::terminal::size() {
                        let _ = state.apply_input_with_platform(
                            InputEvent::Resize { width, height },
                            platform.as_ref(),
                        );
                    }
                    state.show_exit_confirmation_modal(platform.as_ref());
                    state.notify_alert_with_tone(
                        format!("Power off failed: {error}"),
                        ui::NotificationTone::Error,
                    );
                }
            }
        }
    }

    command_line_host.terminate();
    guard.restore()?;
    drop(guard);

    if let Some(error) = terminal_size_error {
        return Err(error);
    }

    let outcome = if reset_requested {
        FullscreenShellSessionOutcome::ResetRequested
    } else if state.restart_requested {
        FullscreenShellSessionOutcome::RestartRequested
    } else if state.return_to_lockscreen_requested() {
        FullscreenShellSessionOutcome::ReturnToLockscreen
    } else {
        FullscreenShellSessionOutcome::Exit
    };
    Ok((outcome, state.ascii_assets.clone()))
}

pub(in crate::session) fn drain_system_status_snapshot(
    snapshots: &mut tokio::sync::watch::Receiver<system_services::SystemSnapshot>,
    state: &mut ShellSession,
) -> bool {
    if !snapshots.has_changed().unwrap_or(false) {
        return false;
    }
    apply_current_system_status_snapshot(snapshots, state);
    true
}

pub(in crate::session) fn apply_current_system_status_snapshot(
    snapshots: &mut tokio::sync::watch::Receiver<system_services::SystemSnapshot>,
    state: &mut ShellSession,
) {
    let snapshot = app::AppSystemStatusSnapshot::from(&*snapshots.borrow_and_update());
    state.apply_system_status_snapshot(snapshot);
}

fn read_ready_terminal_event_batch(first: event::Event) -> io::Result<Vec<event::Event>> {
    collect_ready_terminal_event_batch(first, || event::poll(Duration::ZERO), event::read)
}

fn command_line_poll_timeout(
    command_line_active: bool,
    state_poll_timeout: Duration,
) -> (Duration, bool) {
    if command_line_active && state_poll_timeout > COMMAND_LINE_REFRESH_INTERVAL {
        (COMMAND_LINE_REFRESH_INTERVAL, false)
    } else {
        (state_poll_timeout, true)
    }
}

fn reduced_motion_enabled(state: &ShellSession) -> bool {
    state.app.active_appearance().is_some_and(|appearance| {
        matches!(
            appearance.motion_preference,
            storage::MotionPreference::Reduced
        )
    })
}

fn sync_shell_toast(
    toast: &mut Option<ui::components::Toast>,
    visible_message: Option<&str>,
    frame: ui::MotionFrame,
) {
    match visible_message {
        Some(message) => match toast.as_mut() {
            Some(toast) if toast.message == message => {
                if toast.dismiss_at.is_some() {
                    toast.resume(frame);
                }
            }
            _ => {
                *toast = Some(ui::components::Toast::new(
                    message,
                    ui::components::ToastTone::Info,
                    frame,
                ));
            }
        },
        None => {
            if let Some(toast) = toast.as_mut()
                && toast.dismiss_at.is_none()
            {
                toast.dismiss(frame);
            }
        }
    }
}

fn synchronize_motion_hit_map_after_input(
    state: &mut ShellSession,
    redraw: &mut RedrawScheduler,
    identity_before_input: RedrawIdentity,
    now: Instant,
) {
    let identity_after_input = RedrawIdentity::from_session(state);
    if identity_before_input == identity_after_input {
        return;
    }
    redraw.observe(now, identity_after_input, reduced_motion_enabled(state));
    state.refresh_hit_map_with_motion(redraw.transitions(now));
}

fn session_render_state_changed(before: &ShellSession, after: &ShellSession) -> bool {
    let mut before = before.clone();
    let after = after.clone();
    before.ui.tick_count = after.ui.tick_count;
    before != after
}

fn session_has_background_work(state: &ShellSession) -> bool {
    state.content_screen() == ShellScreen::CommandLine
        || state.launcher_refresh_request.is_some()
        || state.editor_load_state.is_some()
        || state.editor_save_state.is_some()
        || state.diagnostics_scanning
        || state
            .settings_state
            .as_ref()
            .is_some_and(|settings| settings.time_sync_validation_request_id.is_some())
        || state
            .app
            .explorer_state()
            .is_some_and(|explorer| explorer.operation.is_some())
}

fn background_poll_timeout(outstanding: bool, elapsed: Duration) -> Duration {
    if outstanding {
        BACKGROUND_POLL_INTERVAL.saturating_sub(elapsed)
    } else {
        Duration::MAX
    }
}

fn shell_render_capabilities(
    terminal_graphics_probe: &ui::TerminalGraphicsProbe,
) -> ui::RenderCapabilities {
    let true_color = std::env::var("COLORTERM").is_ok_and(|value| {
        value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
    }) || std::env::var("TERM").is_ok_and(|value| {
        let value = value.to_ascii_lowercase();
        value.contains("truecolor") || value.contains("direct")
    }) || std::env::var_os("WT_SESSION").is_some();
    ui::RenderCapabilities {
        color: if true_color {
            ui::ColorCapability::TrueColor
        } else {
            ui::ColorCapability::Ansi
        },
        image_protocol: matches!(
            terminal_graphics_probe.status(),
            ui::TerminalGraphicsProbeStatus::Verified(_)
        ),
    }
}

fn command_line_captures_input(state: &ShellSession, input: &InputEvent) -> bool {
    if state.active_screen() != ShellScreen::CommandLine {
        return false;
    }

    match input {
        InputEvent::Key(_) | InputEvent::Paste(_) => true,
        InputEvent::Mouse(mouse) => {
            let continues_terminal_drag = matches!(
                mouse.kind,
                ui::MouseEventKind::Drag(ui::MouseButton::Left)
                    | ui::MouseEventKind::Up(ui::MouseButton::Left)
            );
            continues_terminal_drag
                || state.hit_map().layer_at(mouse.coordinates()) != Some(ShellHitLayer::ShellChrome)
        }
        _ => false,
    }
}

fn collect_ready_terminal_event_batch(
    first: event::Event,
    mut poll_ready: impl FnMut() -> io::Result<bool>,
    mut read_ready: impl FnMut() -> io::Result<event::Event>,
) -> io::Result<Vec<event::Event>> {
    // Crossterm's all-motion mode can enqueue many coordinates while one
    // Ratatui frame is being built, especially through a WSL ConPTY bridge.
    // Drain that ready backlog before drawing again and retain only the newest
    // point in each uninterrupted motion/resize run. Semantic boundaries such
    // as clicks, releases, wheel steps, and focus stay ordered. A key or paste
    // ends the batch immediately so a continuously-ready WSL mouse stream
    // cannot delay keyboard input after Crossterm has already decoded it.
    let mut events = Vec::new();
    let first_requires_dispatch = terminal_event_requires_immediate_dispatch(&first);
    push_coalesced_terminal_event(&mut events, first);
    if first_requires_dispatch {
        return Ok(events);
    }

    let mut raw_event_count = 1;
    while raw_event_count < MAX_READY_TERMINAL_EVENTS_PER_FRAME && poll_ready()? {
        let next = read_ready()?;
        let requires_dispatch = terminal_event_requires_immediate_dispatch(&next);
        push_coalesced_terminal_event(&mut events, next);
        raw_event_count += 1;
        if requires_dispatch {
            break;
        }
    }
    Ok(events)
}

fn terminal_event_requires_immediate_dispatch(event: &event::Event) -> bool {
    matches!(event, event::Event::Key(_) | event::Event::Paste(_))
}

fn push_coalesced_terminal_event(events: &mut Vec<event::Event>, next: event::Event) {
    if events
        .last()
        .is_some_and(|previous| terminal_event_can_replace(previous, &next))
    {
        *events
            .last_mut()
            .expect("the previous event was checked above") = next;
    } else {
        events.push(next);
    }
}

fn terminal_event_can_replace(previous: &event::Event, next: &event::Event) -> bool {
    match (previous, next) {
        (event::Event::Mouse(previous), event::Event::Mouse(next))
            if previous.modifiers == next.modifiers =>
        {
            match (&previous.kind, &next.kind) {
                (event::MouseEventKind::Moved, event::MouseEventKind::Moved) => true,
                (
                    event::MouseEventKind::Drag(previous_button),
                    event::MouseEventKind::Drag(next_button),
                ) => previous_button == next_button,
                _ => false,
            }
        }
        (event::Event::Resize(_, _), event::Event::Resize(_, _)) => true,
        _ => false,
    }
}

fn refresh_session_after_resume(
    state: &mut ShellSession,
    platform: &dyn Platform,
    time_sync_worker: &TimeSyncWorker,
) {
    if let Err(error) = platform.refresh_session() {
        state.notify_alert_with_tone(
            format!("Desktop session refresh failed: {error}"),
            ui::NotificationTone::Error,
        );
    }
    time_sync_worker.request_refresh();
    if let Ok((width, height)) = crossterm::terminal::size() {
        let _ = state.apply_input_with_platform(InputEvent::Resize { width, height }, platform);
    }
    let _ = state.apply_input_with_platform(InputEvent::FocusGained, platform);
    let _ = state.apply_input_with_platform(InputEvent::Tick, platform);
}

pub(super) const SESSION_RECOVERY_WINDOW: Duration = Duration::from_secs(60);
pub(super) const MAX_SESSION_RECOVERIES: usize = 2;

pub(super) fn reserve_session_recovery(recoveries: &mut VecDeque<Instant>, now: Instant) -> bool {
    while recoveries
        .front()
        .is_some_and(|at| now.saturating_duration_since(*at) > SESSION_RECOVERY_WINDOW)
    {
        recoveries.pop_front();
    }
    if recoveries.len() >= MAX_SESSION_RECOVERIES {
        return false;
    }
    recoveries.push_back(now);
    true
}

pub(super) fn recover_session_panic(
    caught: CaughtPanic,
    session_name: &str,
    recoveries: &mut VecDeque<Instant>,
    platform: &dyn Platform,
) -> io::Result<()> {
    let reason = caught.payload().to_string();
    if reserve_session_recovery(recoveries, Instant::now()) {
        let _ = caught.finalize(RecoveryOutcome::RecoveredWithWarnings(format!(
            "the {session_name} state was discarded; reauthentication is required"
        )));
        return Ok(());
    }

    let receipt = caught
        .finalize(RecoveryOutcome::Unrecoverable(format!(
            "automatic {session_name} recovery limit reached"
        )))
        .ok();
    let report = receipt
        .as_ref()
        .and_then(|receipt| receipt.text_report_path.as_ref())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "report path unavailable".to_string());
    let _ = platform.show_critical_error(
        "TundraUX3 could not recover",
        &format!("{session_name}: {reason}\n\nCrash report: {report}"),
    );
    Err(io::Error::other(format!(
        "{session_name} recovery limit reached after panic: {reason}"
    )))
}

pub(super) fn load_validated_runtime_ascii_assets() -> io::Result<ui::RuntimeAsciiAssets> {
    let ascii_assets = ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    checked_current_terminal_size(ShellTerminalSizeRequirement::from_assets(&ascii_assets))?;
    Ok(ascii_assets)
}

const DEFAULT_THEME_DOWNLOAD_URL: &str =
    "https://github.com/peixuanthomas/TundraUX3/releases/latest";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupAssetRecoveryChoice {
    AutoRestore,
    Download,
    Restart,
    Exit,
}

#[derive(Debug)]
enum StartupAssetLoadOutcome {
    Loaded(ui::RuntimeAsciiAssets),
    Restart,
    Exit,
}

fn load_startup_runtime_ascii_assets(
    output: &mut impl Write,
    platform: &dyn Platform,
) -> io::Result<StartupAssetLoadOutcome> {
    let root = ui::asset_root_for_recovery_from_env_or_current_exe().map_err(asset_io_error)?;
    resolve_startup_runtime_ascii_assets_at(
        &root,
        |report, last_error| prompt_startup_asset_recovery(output, report, last_error),
        || {
            platform
                .open_uri(DEFAULT_THEME_DOWNLOAD_URL)
                .map_err(|error| error.to_string())
        },
    )
}

fn resolve_startup_runtime_ascii_assets_at(
    root: &Path,
    mut choose: impl FnMut(
        &ui::DefaultThemeCheckReport,
        Option<&str>,
    ) -> io::Result<StartupAssetRecoveryChoice>,
    mut open_download: impl FnMut() -> Result<(), String>,
) -> io::Result<StartupAssetLoadOutcome> {
    let mut last_error = None;
    loop {
        let report = ui::check_default_theme(root);
        if report.is_ok() {
            let store = ui::AsciiAssetStore::load_with_root(root, ui::DEFAULT_THEME_ID)
                .map_err(asset_io_error)?;
            return Ok(StartupAssetLoadOutcome::Loaded(
                ui::RuntimeAsciiAssets::from_store(store),
            ));
        }

        match choose(&report, last_error.as_deref())? {
            StartupAssetRecoveryChoice::AutoRestore => {
                last_error = match ui::restore_default_theme(root) {
                    Ok(_) => None,
                    Err(error) => Some(error.to_string()),
                };
            }
            StartupAssetRecoveryChoice::Download => match open_download() {
                Ok(()) => return Ok(StartupAssetLoadOutcome::Exit),
                Err(error) => last_error = Some(format!("Could not open download page: {error}")),
            },
            StartupAssetRecoveryChoice::Restart => {
                return Ok(StartupAssetLoadOutcome::Restart);
            }
            StartupAssetRecoveryChoice::Exit => return Ok(StartupAssetLoadOutcome::Exit),
        }
    }
}

fn prompt_startup_asset_recovery(
    output: &mut impl Write,
    report: &ui::DefaultThemeCheckReport,
    last_error: Option<&str>,
) -> io::Result<StartupAssetRecoveryChoice> {
    let warnings = report.warning_checks();
    writeln!(output)?;
    writeln!(output, "TundraUX3 asset recovery mode")?;
    writeln!(
        output,
        "The default theme is incomplete or invalid ({} file{} affected).",
        warnings.len(),
        if warnings.len() == 1 { "" } else { "s" }
    )?;
    writeln!(output, "Asset root: {}", report.root.display())?;
    for check in warnings.iter().take(8) {
        writeln!(output, "  - {}: {}", check.key, check.message)?;
    }
    if warnings.len() > 8 {
        writeln!(output, "  - ... and {} more", warnings.len() - 8)?;
    }
    if let Some(error) = last_error {
        writeln!(output, "Previous recovery action failed: {error}")?;
    }
    writeln!(output)?;
    writeln!(
        output,
        "  [1] Automatically restore the built-in default theme"
    )?;
    writeln!(output, "  [2] Open the latest release download page")?;
    writeln!(output, "  [3] Restart TundraUX3")?;
    writeln!(output, "  [4] Close TundraUX3")?;

    loop {
        write!(output, "Choose 1, 2, 3, or 4: ")?;
        output.flush()?;
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input)? == 0 {
            return Ok(StartupAssetRecoveryChoice::Exit);
        }
        match input.trim().to_ascii_lowercase().as_str() {
            "1" | "a" | "auto" => return Ok(StartupAssetRecoveryChoice::AutoRestore),
            "2" | "d" | "download" => return Ok(StartupAssetRecoveryChoice::Download),
            "3" | "r" | "restart" => return Ok(StartupAssetRecoveryChoice::Restart),
            "4" | "q" | "quit" | "exit" => return Ok(StartupAssetRecoveryChoice::Exit),
            _ => writeln!(output, "Invalid choice.")?,
        }
    }
}

pub(super) fn spawn_time_sync_worker(
    sender: mpsc::Sender<TimedTimeSyncResult>,
    watchdog: &AppWatchdog,
    system_services: system_services::SystemServicesHandle,
) -> Result<TimeSyncWorker, watchdog::WatchdogError> {
    let (control_sender, mut control_receiver) = tokio::sync::mpsc::unbounded_channel();
    let group = watchdog.task_group("network-clock");
    let handle = group.spawn_thread(
        TaskSpec {
            id: TaskId::from_static("refresh-loop"),
            kind: TaskKind::LongRunning,
            panic_action: PanicAction::RestartTask,
            replay_safety: ReplaySafety::Idempotent,
            restart_policy: RestartPolicy::limited(
                3,
                Duration::from_secs(5 * 60),
                vec![
                    Duration::from_secs(1),
                    Duration::from_secs(5),
                    Duration::from_secs(30),
                ],
            ),
        },
        move || {
            let sender = sender.clone();
            let mut snapshots = system_services.subscribe();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("time-sync event runtime");
            runtime.block_on(async {
                let mut last_utc = None;
                let mut last_error = None;
                if !forward_time_sync_snapshot(
                    snapshots.borrow_and_update().clone(),
                    &sender,
                    &mut last_utc,
                    &mut last_error,
                ) {
                    return;
                }
                loop {
                    match next_time_sync_wakeup(&mut control_receiver, &mut snapshots).await {
                        TimeSyncWakeup::Control(TimeSyncControl::Stop) | TimeSyncWakeup::Closed => {
                            break;
                        }
                        TimeSyncWakeup::Control(TimeSyncControl::Refresh) => {
                            let _ = system_services.sync_time_now();
                        }
                        TimeSyncWakeup::SnapshotChanged => {
                            if !forward_time_sync_snapshot(
                                snapshots.borrow_and_update().clone(),
                                &sender,
                                &mut last_utc,
                                &mut last_error,
                            ) {
                                break;
                            }
                        }
                    }
                }
            });
        },
    )?;
    Ok(TimeSyncWorker {
        control_sender,
        handle: Some(handle),
    })
}

async fn next_time_sync_wakeup(
    control: &mut tokio::sync::mpsc::UnboundedReceiver<TimeSyncControl>,
    snapshots: &mut tokio::sync::watch::Receiver<system_services::SystemSnapshot>,
) -> TimeSyncWakeup {
    tokio::select! {
        control = control.recv() => control
            .map(TimeSyncWakeup::Control)
            .unwrap_or(TimeSyncWakeup::Closed),
        changed = snapshots.changed() => if changed.is_ok() {
            TimeSyncWakeup::SnapshotChanged
        } else {
            TimeSyncWakeup::Closed
        },
    }
}

fn forward_time_sync_snapshot(
    snapshot: system_services::SystemSnapshot,
    sender: &mpsc::Sender<TimedTimeSyncResult>,
    last_utc: &mut Option<DateTime<Utc>>,
    last_error: &mut Option<String>,
) -> bool {
    match snapshot.time {
        system_services::TimeState::Synced { utc, .. } if *last_utc != Some(utc) => {
            *last_utc = Some(utc);
            *last_error = None;
            sender
                .send(TimedTimeSyncResult {
                    result: Ok(utc),
                    received_at: Instant::now(),
                })
                .is_ok()
        }
        system_services::TimeState::Degraded { error, .. }
            if last_error.as_deref() != Some(error.as_str()) =>
        {
            *last_error = Some(error.clone());
            sender
                .send(TimedTimeSyncResult {
                    result: Err(time::TimeSyncError::new(vec![error])),
                    received_at: Instant::now(),
                })
                .is_ok()
        }
        _ => true,
    }
}

#[cfg(test)]
pub(super) async fn synchronize_configured_time(
    config: &storage::TimeSyncConfig,
    platform: &dyn Platform,
) -> TimeSyncResult {
    match config.source {
        storage::TimeSyncSource::NetworkServer => match config.server_url.as_deref() {
            Some(server_url) => time::fetch_time_from_server(server_url).await,
            None => time::fetch_standard_time().await,
        },
        storage::TimeSyncSource::OperatingSystem => platform
            .system_time()
            .map(DateTime::<Utc>::from)
            .map_err(|error| {
                time::TimeSyncError::new(vec![format!(
                    "could not read the operating system time: {error}"
                )])
            }),
    }
}

pub(super) fn system_services_config_for_startup(
    startup: &ShellStartupState,
) -> system_services::SystemServicesConfig {
    let mut services = system_services::SystemServicesConfig::default();
    let Some(config) = startup
        .storage_manager
        .as_ref()
        .and_then(|storage| storage.load_config().ok())
    else {
        return services;
    };
    services.weather_location = config.weather_location;
    services.storage_thresholds = system_status_thresholds_from_storage(&config.system_status);
    services.timezone_id = config.timezone.clone();
    services.time_sync_mode = match config.time_sync.source {
        storage::TimeSyncSource::NetworkServer => system_services::TimeSyncMode::Network,
        storage::TimeSyncSource::OperatingSystem => system_services::TimeSyncMode::OperatingSystem,
    };
    services.time_server_url = config.time_sync.server_url;
    services.timezone_location = app::setup_timezone_options()
        .into_iter()
        .find(|timezone| timezone.id == config.timezone)
        .map(|timezone| system_services::GeoLocation {
            latitude: timezone.latitude,
            longitude: timezone.longitude,
            city: Some(timezone.label),
        });
    services.cache_dir = startup
        .storage_manager
        .as_ref()
        .map(|storage| storage.layout().cache_path.join("system-services"));
    services
}

pub(super) fn spawn_terminal_graphics_probe_worker(
    sender: mpsc::SyncSender<ui::TerminalGraphicsProbe>,
    watchdog: &AppWatchdog,
) -> Result<ManagedThreadHandle<()>, watchdog::WatchdogError> {
    let group = watchdog
        .child_component(ComponentId::from_static("startup-probe"))
        .task_group("terminal-graphics");
    group.spawn_thread(
        TaskSpec::one_shot(TaskId::from_static("capabilities")),
        move || {
            let _ = sender.send(probe_terminal_graphics_protocol());
        },
    )
}

pub(super) fn shell_watchdog_descriptor() -> AppDescriptor {
    AppDescriptor::new(
        AppId::from_static("shell"),
        "Tundra Shell",
        env!("CARGO_PKG_VERSION"),
        AppCriticality::ProcessCritical,
    )
}

fn weathr_watchdog_descriptor() -> AppDescriptor {
    AppDescriptor::new(
        AppId::from_static("weathr"),
        "Weathr",
        env!("CARGO_PKG_VERSION"),
        AppCriticality::SessionCritical,
    )
}

pub(super) fn drain_watchdog_incidents(state: &mut ShellSession, watchdog: &ProcessWatchdog) {
    for incident in watchdog.drain_incidents() {
        show_watchdog_incident(state, incident);
    }
}

pub(super) fn show_watchdog_incident(state: &mut ShellSession, incident: IncidentReceipt) {
    let report_path = incident
        .text_report_path
        .as_ref()
        .or(incident.json_report_path.as_ref())
        .cloned();
    let report = report_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "report path unavailable".to_string());
    let full_summary = format!(
        "{}\n\nRecovery: {:?}\nIncident: {}\nReport: {}",
        incident.summary, incident.recovery, incident.incident_id, report
    );
    state.latest_watchdog_report = report_path;
    state.latest_watchdog_summary = Some(full_summary.clone());
    if state.app.diagnostics_snapshot().is_some() && !state.diagnostics_restart_is_required() {
        if state
            .diagnostics_task_runtime
            .as_ref()
            .is_some_and(ShellDiagnosticsTaskRuntime::is_busy)
        {
            state.diagnostics_rescan_pending = true;
        } else {
            state.request_diagnostics_scan();
        }
    }

    // Unclean-exit receipts describe a previous process, not a failure in the
    // current UI session. The watchdog has already persisted them for the
    // Diagnostics screen, so they must not interrupt the first shell frame
    // after the Weathr lockscreen.
    if incident.kind == IncidentKind::UncleanExit {
        return;
    }

    let can_view_details = state.diagnostics_can_view_details();
    let public_summary = format!(
        "A TundraUX component reported a critical error.\n\nRecovery: {}\nDetailed incident data is restricted to administrators.",
        diagnostics_recovery_label(&incident.recovery)
    );
    let mut actions = vec![ShellNotificationAction::new("continue", "Continue").cancel()];
    if can_view_details {
        actions.extend([
            ShellNotificationAction::new("open-report", "Open report")
                .with_follow_up(ShellCommand::OpenLatestCrashReport),
            ShellNotificationAction::new("copy-summary", "Copy summary")
                .with_follow_up(ShellCommand::CopyLatestCrashSummary),
        ]);
    }
    actions.push(
        ShellNotificationAction::new("exit", "Exit").with_follow_up(ShellCommand::RequestExit),
    );
    state.notify_critical_modal(
        if incident.recovery.is_recovered() {
            "Program recovered from a critical error"
        } else {
            "Program encountered a critical error"
        },
        if can_view_details {
            full_summary
        } else {
            public_summary
        },
        actions,
    );
}

pub(super) fn drain_time_sync_results(
    state: &mut ShellSession,
    receiver: &mpsc::Receiver<TimedTimeSyncResult>,
    cached: &mut Option<CachedTimeSyncResult>,
) {
    loop {
        match receiver.try_recv() {
            Ok(result) => apply_timed_time_sync_result_at(state, cached, result, Instant::now()),
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => break,
        }
    }
}

pub(super) fn apply_timed_time_sync_result_at(
    state: &mut ShellSession,
    cached: &mut Option<CachedTimeSyncResult>,
    timed: TimedTimeSyncResult,
    now: Instant,
) {
    match timed.result {
        Ok(utc) => {
            *cached = Some(CachedTimeSyncResult::Success {
                utc,
                received_at: timed.received_at,
            });
            let elapsed = now.saturating_duration_since(timed.received_at);
            state.apply_time_sync_result(Ok(utc + elapsed));
        }
        Err(error) => {
            *cached = Some(CachedTimeSyncResult::Failure);
            state.apply_time_sync_result(Err(error));
        }
    }
}

pub(super) fn with_fullscreen<W, T>(
    output: &mut W,
    body: impl FnOnce(&mut W) -> io::Result<T>,
) -> io::Result<T>
where
    W: Write,
{
    platform::with_terminal_fullscreen(output, body)
}

pub(super) fn write_smoke_loop_message(output: &mut impl Write) -> io::Result<()> {
    for line in startup_lines() {
        writeln!(output, "{line}")?;
    }
    writeln!(output, "Entering smoke loop")
}

#[cfg(test)]
mod runtime_preflight_tests {
    use super::*;
    use crossterm::event::{
        Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    #[test]
    fn idle_has_no_background_poll_deadline_but_active_work_does() {
        assert_eq!(
            background_poll_timeout(false, Duration::ZERO),
            Duration::MAX
        );
        assert_eq!(
            background_poll_timeout(true, Duration::ZERO),
            BACKGROUND_POLL_INTERVAL
        );
        assert_eq!(
            background_poll_timeout(true, Duration::from_millis(249)),
            Duration::from_millis(1)
        );
    }

    #[test]
    fn idle_time_sync_wait_has_no_periodic_wakeup() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let (control_sender, mut control_receiver) = tokio::sync::mpsc::unbounded_channel();
        let snapshot = || {
            let observed_at = Utc::now();
            system_services::SystemSnapshot {
                revision: 0,
                observed_at,
                weather: system_services::WeatherState::Loading,
                time: system_services::TimeState::Local {
                    local_time: observed_at.fixed_offset(),
                },
                storage: system_services::StorageState::Loading,
                network: system_services::NetworkState::Loading,
            }
        };
        let (snapshot_sender, mut snapshots) = tokio::sync::watch::channel(snapshot());

        runtime.block_on(async {
            assert!(
                tokio::time::timeout(
                    Duration::from_millis(25),
                    next_time_sync_wakeup(&mut control_receiver, &mut snapshots),
                )
                .await
                .is_err(),
                "an unchanged idle worker must remain asleep"
            );

            control_sender
                .send(TimeSyncControl::Refresh)
                .expect("refresh control");
            assert_eq!(
                next_time_sync_wakeup(&mut control_receiver, &mut snapshots).await,
                TimeSyncWakeup::Control(TimeSyncControl::Refresh)
            );

            snapshot_sender.send_replace(snapshot());
            assert_eq!(
                next_time_sync_wakeup(&mut control_receiver, &mut snapshots).await,
                TimeSyncWakeup::SnapshotChanged
            );
        });
    }

    #[test]
    fn batched_input_installs_settled_hit_map_before_the_next_event() {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        let origin = Instant::now();
        let mut redraw = RedrawScheduler::new(origin, RedrawIdentity::from_session(&state), false);
        redraw.did_draw(origin);
        let identity_before = RedrawIdentity::from_session(&state);

        state.apply_input(InputEvent::from_key_label("Esc"));
        assert!(
            state
                .hit_map()
                .regions()
                .iter()
                .any(|region| region.component == ShellComponent::ExitDialog),
            "the controller's immediate map is settled until motion is synchronized"
        );
        synchronize_motion_hit_map_after_input(&mut state, &mut redraw, identity_before, origin);
        assert!(
            state
                .hit_map()
                .regions()
                .iter()
                .any(|region| region.component == ShellComponent::ExitDialog),
            "the next batched event must see the immediately rendered dialog"
        );
        state.apply_input(InputEvent::from_key_label("Enter"));
        assert!(state.shutdown_requested());
        assert_eq!(state.last_command(), Some(&ShellCommand::ConfirmExit));
    }

    #[test]
    fn fullscreen_runtime_wires_motion_preferences_capabilities_and_contextual_renderers() {
        let source = include_str!("runtime.rs");
        let start = source
            .find("pub(super) fn run_fullscreen_shell_session")
            .expect("fullscreen runtime");
        let end = source[start..]
            .find("fn read_ready_terminal_event_batch")
            .map(|offset| start + offset)
            .expect("runtime helper boundary");
        let runtime = &source[start..end];

        assert!(runtime.contains("reduced_motion_enabled(&state)"));
        assert!(runtime.contains("shell_render_capabilities(terminal_graphics_probe)"));
        assert!(runtime.contains("ui::RenderContext::from_theme_with_transitions("));
        assert!(runtime.contains("state.refresh_hit_map_with_motion(motion_transitions)"));
        assert!(runtime.contains("render_context.page_area(area)"));
        assert!(runtime.contains("sync_shell_toast(&mut shell_toast"));
        for renderer in [
            "render_setup_with_context",
            "render_login_with_context",
            "render_bootstrap_admin_with_context",
            "render_user_management_with_context",
            "render_explorer_with_context",
            "render_launcher_with_context",
            "render_command_line_with_context",
            "render_editor_app_with_context",
            "render_settings_with_context",
            "render_diagnostics_with_context",
            "render_clock_with_context",
            "render_home_with_context",
            "render_exit_confirmation_with_context",
            "render_time_sync_failure_dialog_with_context",
            "render_notification_overlay_with_context",
        ] {
            assert!(runtime.contains(renderer), "missing {renderer}");
        }
        for legacy in [
            "ui::render_setup(",
            "ui::render_login(",
            "ui::render_launcher_with_icons(",
            "ui::render_home_with_icons(",
            "ui::render_notification_overlay(",
        ] {
            assert!(!runtime.contains(legacy), "legacy runtime call {legacy}");
        }
    }

    #[test]
    fn deferred_alert_toast_reentry_resumes_without_replaying_or_jumping() {
        let frame = |millis| ui::MotionFrame {
            now: Duration::from_millis(millis),
            delta: Duration::ZERO,
            reduced_motion: false,
        };
        let mut toast = None;
        sync_shell_toast(&mut toast, Some("Saved"), frame(0));
        sync_shell_toast(&mut toast, None, frame(200));
        let exiting = toast
            .as_ref()
            .expect("exiting toast")
            .visible_progress(frame(250));
        sync_shell_toast(&mut toast, Some("Saved"), frame(250));
        let resumed = toast.as_ref().expect("resumed toast");
        assert_eq!(resumed.visible_progress(frame(250)), exiting);

        let shown_at = resumed.shown_at;
        sync_shell_toast(&mut toast, Some("Saved"), frame(500));
        assert_eq!(toast.as_ref().expect("renewed toast").shown_at, shown_at);

        sync_shell_toast(&mut toast, Some("Different"), frame(600));
        let replacement = toast.as_ref().expect("replacement toast");
        assert_eq!(replacement.message, "Different");
        assert_eq!(replacement.visible_progress(frame(600)), 0);
    }

    fn recovery_asset_root(case: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tundra-startup-asset-recovery-{}-{}-{case}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ))
    }

    #[test]
    fn startup_auto_restore_recreates_the_complete_default_theme() {
        let root = recovery_asset_root("auto");
        let _ = std::fs::remove_dir_all(&root);
        let choices = Cell::new(0_usize);

        let outcome = resolve_startup_runtime_ascii_assets_at(
            &root,
            |report, last_error| {
                choices.set(choices.get() + 1);
                assert!(last_error.is_none());
                assert_eq!(report.checks.len(), ui::default_theme_files().len());
                assert!(
                    report
                        .warning_checks()
                        .iter()
                        .any(|check| check.key == "home_icons/explorer.png")
                );
                Ok(StartupAssetRecoveryChoice::AutoRestore)
            },
            || panic!("automatic recovery must not open the download page"),
        )
        .expect("startup recovery should succeed");
        let StartupAssetLoadOutcome::Loaded(assets) = outcome else {
            panic!("automatic recovery should continue startup");
        };

        assert_eq!(choices.get(), 1);
        assert!(ui::check_default_theme(&root).is_ok());
        assert!(
            assets
                .home_icon_image_bytes("explorer")
                .is_some_and(|bytes| bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
        );

        std::fs::remove_dir_all(root).expect("clean startup recovery fixture");
    }

    #[test]
    fn startup_download_choice_opens_the_release_page_and_closes_cleanly() {
        let root = recovery_asset_root("download");
        let _ = std::fs::remove_dir_all(&root);
        let opened = Cell::new(false);

        let outcome = resolve_startup_runtime_ascii_assets_at(
            &root,
            |report, _| {
                assert!(report.has_warnings());
                Ok(StartupAssetRecoveryChoice::Download)
            },
            || {
                opened.set(true);
                Ok(())
            },
        )
        .expect("download choice should close without an asset error");

        assert!(matches!(outcome, StartupAssetLoadOutcome::Exit));
        assert!(opened.get());
        assert!(!root.exists());
    }

    #[test]
    fn startup_restart_choice_requests_a_real_process_restart() {
        let root = recovery_asset_root("restart");
        let _ = std::fs::remove_dir_all(&root);

        let outcome = resolve_startup_runtime_ascii_assets_at(
            &root,
            |_, _| Ok(StartupAssetRecoveryChoice::Restart),
            || panic!("restart choice must not open the download page"),
        )
        .expect("restart choice should return a restart outcome");

        assert!(matches!(outcome, StartupAssetLoadOutcome::Restart));
        assert!(!root.exists());
    }

    #[test]
    fn startup_close_choice_exits_without_touching_the_asset_root() {
        let root = recovery_asset_root("close");
        let _ = std::fs::remove_dir_all(&root);

        let outcome = resolve_startup_runtime_ascii_assets_at(
            &root,
            |_, _| Ok(StartupAssetRecoveryChoice::Exit),
            || panic!("close choice must not open the download page"),
        )
        .expect("close choice should be a clean exit");

        assert!(matches!(outcome, StartupAssetLoadOutcome::Exit));
        assert!(!root.exists());
    }

    fn mouse_event(kind: MouseEventKind, column: u16, row: u16, modifiers: KeyModifiers) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers,
        })
    }

    fn collect_one_test_batch(source: &Rc<RefCell<VecDeque<Event>>>) -> Vec<Event> {
        let first = source
            .borrow_mut()
            .pop_front()
            .expect("test event source must not be empty");
        let poll_source = Rc::clone(source);
        let read_source = Rc::clone(source);
        collect_ready_terminal_event_batch(
            first,
            move || Ok(!poll_source.borrow().is_empty()),
            move || {
                read_source
                    .borrow_mut()
                    .pop_front()
                    .ok_or_else(|| io::Error::other("test event source was exhausted"))
            },
        )
        .expect("collect terminal event batch")
    }

    #[test]
    fn command_line_uses_a_low_latency_refresh_without_advancing_state_ticks() {
        assert_eq!(
            command_line_poll_timeout(true, Duration::from_millis(250)),
            (COMMAND_LINE_REFRESH_INTERVAL, false)
        );
        assert_eq!(
            command_line_poll_timeout(true, Duration::from_millis(5)),
            (Duration::from_millis(5), true)
        );
        assert_eq!(
            command_line_poll_timeout(false, Duration::from_millis(250)),
            (Duration::from_millis(250), true)
        );
    }

    #[test]
    fn command_line_runtime_leaves_shell_chrome_mouse_input_for_the_shell() {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
        state.screen_stack = vec![ShellScreen::Home, ShellScreen::CommandLine];
        state.refresh_hit_map();

        let clock_area = state
            .hit_map()
            .regions()
            .iter()
            .find(|region| region.component == ShellComponent::ClockButton)
            .expect("Command Line must expose the Shell clock button")
            .area;
        let clock_input =
            InputEvent::mouse_down(ui::MouseButton::Left, (clock_area.x, clock_area.y));
        assert!(!command_line_captures_input(&state, &clock_input));
        assert!(command_line_captures_input(
            &state,
            &InputEvent::mouse_up(ui::MouseButton::Left, (clock_area.x, clock_area.y))
        ));

        let terminal_area = ui::command_line_terminal_area(Rect::new(0, 0, 120, 40)).unwrap();
        let terminal_input = InputEvent::mouse_moved((terminal_area.x, terminal_area.y));
        assert!(command_line_captures_input(&state, &terminal_input));
        assert!(command_line_captures_input(
            &state,
            &InputEvent::key(ui::Key::Char('a'))
        ));
        assert!(command_line_captures_input(
            &state,
            &InputEvent::paste("command")
        ));
    }

    #[test]
    fn mouse_motion_flood_is_consumed_in_a_few_render_batches() {
        let source = Rc::new(RefCell::new(
            (0..10_000)
                .map(|index| {
                    mouse_event(
                        MouseEventKind::Moved,
                        (index % 200) as u16,
                        (index % 80) as u16,
                        KeyModifiers::NONE,
                    )
                })
                .collect::<VecDeque<_>>(),
        ));
        let mut rendered_events = Vec::new();
        let mut batch_count = 0;

        while !source.borrow().is_empty() {
            rendered_events.extend(collect_one_test_batch(&source));
            batch_count += 1;
        }

        assert_eq!(batch_count, 3);
        assert_eq!(rendered_events.len(), batch_count);
        assert_eq!(
            rendered_events.last(),
            Some(&mouse_event(
                MouseEventKind::Moved,
                (9_999 % 200) as u16,
                (9_999 % 80) as u16,
                KeyModifiers::NONE,
            ))
        );
    }

    #[test]
    fn mouse_coalescing_preserves_semantic_event_boundaries() {
        let source = Rc::new(RefCell::new(VecDeque::from([
            mouse_event(MouseEventKind::Moved, 1, 1, KeyModifiers::NONE),
            mouse_event(MouseEventKind::Moved, 2, 2, KeyModifiers::NONE),
            mouse_event(MouseEventKind::Moved, 3, 3, KeyModifiers::SHIFT),
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)),
            mouse_event(MouseEventKind::Moved, 4, 4, KeyModifiers::NONE),
            mouse_event(MouseEventKind::Moved, 5, 5, KeyModifiers::NONE),
            mouse_event(
                MouseEventKind::Down(MouseButton::Left),
                5,
                5,
                KeyModifiers::NONE,
            ),
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                6,
                6,
                KeyModifiers::NONE,
            ),
            mouse_event(
                MouseEventKind::Drag(MouseButton::Left),
                7,
                7,
                KeyModifiers::NONE,
            ),
            mouse_event(MouseEventKind::ScrollDown, 7, 7, KeyModifiers::NONE),
            mouse_event(MouseEventKind::ScrollDown, 7, 7, KeyModifiers::NONE),
            mouse_event(
                MouseEventKind::Up(MouseButton::Left),
                7,
                7,
                KeyModifiers::NONE,
            ),
            Event::Paste("preserve me".to_string()),
            Event::FocusGained,
            Event::Resize(100, 40),
            Event::Resize(120, 50),
        ])));

        let first_batch = collect_one_test_batch(&source);
        assert_eq!(
            first_batch,
            vec![
                mouse_event(MouseEventKind::Moved, 2, 2, KeyModifiers::NONE),
                mouse_event(MouseEventKind::Moved, 3, 3, KeyModifiers::SHIFT),
                Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL,)),
            ]
        );

        let second_batch = collect_one_test_batch(&source);
        assert_eq!(
            second_batch,
            vec![
                mouse_event(MouseEventKind::Moved, 5, 5, KeyModifiers::NONE),
                mouse_event(
                    MouseEventKind::Down(MouseButton::Left),
                    5,
                    5,
                    KeyModifiers::NONE,
                ),
                mouse_event(
                    MouseEventKind::Drag(MouseButton::Left),
                    7,
                    7,
                    KeyModifiers::NONE,
                ),
                mouse_event(MouseEventKind::ScrollDown, 7, 7, KeyModifiers::NONE,),
                mouse_event(MouseEventKind::ScrollDown, 7, 7, KeyModifiers::NONE,),
                mouse_event(
                    MouseEventKind::Up(MouseButton::Left),
                    7,
                    7,
                    KeyModifiers::NONE,
                ),
                Event::Paste("preserve me".to_string()),
            ]
        );

        let third_batch = collect_one_test_batch(&source);
        assert!(source.borrow().is_empty());
        assert_eq!(
            third_batch,
            vec![Event::FocusGained, Event::Resize(120, 50),]
        );
    }

    #[test]
    fn a_ready_key_is_dispatched_without_polling_for_more_events() {
        let polls = Cell::new(0_usize);
        let batch = collect_ready_terminal_event_batch(
            Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
            || {
                polls.set(polls.get() + 1);
                Ok(true)
            },
            || panic!("a key batch must not read a later event"),
        )
        .expect("key batch");

        assert_eq!(polls.get(), 0);
        assert_eq!(
            batch,
            vec![Event::Key(KeyEvent::new(
                KeyCode::Char('x'),
                KeyModifiers::NONE,
            ))]
        );
    }

    #[test]
    fn ready_event_drain_is_bounded_when_the_source_never_goes_idle() {
        let reads = Cell::new(0_usize);
        let batch = collect_ready_terminal_event_batch(
            mouse_event(MouseEventKind::Moved, 0, 0, KeyModifiers::NONE),
            || Ok(true),
            || {
                let next = reads.get() + 1;
                reads.set(next);
                Ok(mouse_event(
                    MouseEventKind::Moved,
                    (next % 200) as u16,
                    (next % 80) as u16,
                    KeyModifiers::NONE,
                ))
            },
        )
        .expect("bounded batch");

        assert_eq!(reads.get() + 1, MAX_READY_TERMINAL_EVENTS_PER_FRAME);
        assert_eq!(batch.len(), 1);
    }

    #[test]
    fn configured_operating_system_time_uses_platform_boundary() {
        let root = std::env::temp_dir().join(format!(
            "tundra-runtime-system-time-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let app_paths = platform::build_windows_app_paths(
            root.join("roaming"),
            root.join("local"),
            root.join("temp"),
        )
        .expect("test app paths");
        let user_dirs = platform::UserDirs::new(
            root.join("desktop"),
            root.join("documents"),
            root.join("downloads"),
            root.join("pictures"),
            root.join("videos"),
            root.join("music"),
            root.join("roaming"),
        )
        .expect("test user dirs");
        let platform = platform::mock::MockPlatform::new(user_dirs, app_paths);
        let system_time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        platform.set_system_time_result(Ok(system_time));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");

        let result = runtime
            .block_on(synchronize_configured_time(
                &storage::TimeSyncConfig {
                    source: storage::TimeSyncSource::OperatingSystem,
                    server_url: Some("https://ignored.example.test/".to_string()),
                },
                &platform,
            ))
            .expect("system time sync");

        assert_eq!(result, DateTime::<Utc>::from(system_time));
        assert!(
            platform
                .calls()
                .iter()
                .any(|call| { matches!(call, platform::mock::MockCall::SystemTime) })
        );
    }

    #[test]
    fn failed_terminal_preflight_writes_no_banner_or_fullscreen_sequence() {
        let fail = || Err(io::Error::other("terminal is too small"));

        let mut static_output = Vec::new();
        assert!(
            run_not_fullscreen_without_animation_with_loader(&mut static_output, fail).is_err()
        );
        assert!(static_output.is_empty());

        let mut animated_output = Vec::new();
        assert!(run_not_fullscreen_with_loader(&mut animated_output, fail).is_err());
        assert!(animated_output.is_empty());

        let mut fullscreen_output = Vec::new();
        assert!(
            run_fullscreen_once_without_animation_with_loader(&mut fullscreen_output, fail)
                .is_err()
        );
        assert!(fullscreen_output.is_empty());
    }

    #[test]
    fn theme_reloader_applies_active_user_changes_and_recovers_from_invalid_users() {
        let root = std::env::temp_dir().join(format!(
            "tundra-theme-reload-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let app_paths = platform::build_windows_app_paths(
            root.join("roaming"),
            root.join("local"),
            root.join("temp"),
        )
        .expect("test paths");
        let storage = StorageManager::open(app_paths)
            .expect("test storage")
            .manager;
        let started_at = Instant::now();
        let appearance = storage::AppearanceConfig {
            border_shape: storage::BorderShape::Square,
            border_color: storage::BorderColor::Rgb(0x38, 0xBD, 0xF8),
            accent_color: storage::BorderColor::LightMagenta,
            icon_display_mode: storage::IconDisplayMode::Image,
            ..storage::AppearanceConfig::default()
        };
        UserService::new(storage.clone())
            .bootstrap_admin_with_hint_and_appearance(
                "AdminUser",
                "StrongPass123",
                None,
                appearance.clone(),
            )
            .expect("bootstrap admin with appearance");
        let session = SessionService::new(storage.clone())
            .login("AdminUser", "StrongPass123")
            .expect("login");
        let mut reloader = UserThemeReloader::new(Some(storage.clone()), started_at);
        let mut theme = ui::TundraTheme::default_dark();
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        state.complete_login(session);

        reloader.last_observed = None;
        reloader.next_check = started_at;
        reloader.poll_at(started_at, &mut theme, &mut state);
        assert_eq!(theme.border_shape, ui::BorderShape::Square);
        assert_eq!(
            theme.border_color,
            ratatui::style::Color::Rgb(0x38, 0xBD, 0xF8)
        );
        assert_eq!(theme.accent_color, ratatui::style::Color::LightMagenta);
        assert_eq!(state.app.active_appearance(), Some(&appearance));

        std::fs::write(&storage.layout().users_path, "{ not valid json")
            .expect("corrupt users fixture");
        let failure_at = started_at + THEME_RELOAD_INTERVAL;
        reloader.last_observed = None;
        reloader.next_check = failure_at;
        reloader.poll_at(failure_at, &mut theme, &mut state);
        assert_eq!(
            theme.border_color,
            ratatui::style::Color::Rgb(0x38, 0xBD, 0xF8)
        );
        assert_eq!(theme.accent_color, ratatui::style::Color::LightMagenta);
        assert_eq!(
            state
                .to_notification_view_model()
                .expect("reload failure modal")
                .title,
            "Theme reload failed"
        );

        std::fs::remove_file(&storage.layout().users_path).expect("remove corrupt users");
        let mut users = storage::UsersDocument::default();
        let now = unix_millis();
        users.users.push(storage::UserRecord {
            id: state.auth_session().expect("session").user_id.clone(),
            username: "AdminUser".to_string(),
            display_name: "AdminUser".to_string(),
            role: "Admin".to_string(),
            password_hash: String::new(),
            password_hint: None,
            appearance,
            enabled: true,
            failed_login_attempts: 0,
            locked_until_epoch_ms: None,
            created_at_epoch_ms: now,
            updated_at_epoch_ms: now,
            last_login_at_epoch_ms: Some(now),
        });
        storage.save_users(&users).expect("repaired users");
        let recovery_at = failure_at + THEME_RELOAD_INTERVAL;
        reloader.last_observed = None;
        reloader.next_check = recovery_at;
        reloader.poll_at(recovery_at, &mut theme, &mut state);
        assert!(state.to_notification_view_model().is_none());

        platform::cleanup_temp_path(&root).expect("clean test root");
    }

    #[test]
    fn theme_reloader_switches_from_custom_admin_theme_to_managed_user_defaults() {
        let root = std::env::temp_dir().join(format!(
            "tundra-user-theme-switch-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let app_paths = platform::build_windows_app_paths(
            root.join("roaming"),
            root.join("local"),
            root.join("temp"),
        )
        .expect("test paths");
        let storage = StorageManager::open(app_paths)
            .expect("test storage")
            .manager;
        let custom = storage::AppearanceConfig {
            border_shape: storage::BorderShape::Square,
            border_color: storage::BorderColor::LightGreen,
            accent_color: storage::BorderColor::LightMagenta,
            icon_display_mode: storage::IconDisplayMode::Image,
            ..storage::AppearanceConfig::default()
        };
        let users = UserService::new(storage.clone());
        users
            .bootstrap_admin_with_hint_and_appearance("AdminUser", "StrongPass123", None, custom)
            .expect("bootstrap");
        let admin_session = SessionService::new(storage.clone())
            .login("AdminUser", "StrongPass123")
            .expect("admin login");
        users
            .create_user(
                &admin_session,
                "ManagedUser",
                "Managed User",
                UserRole::User,
                "ManagedPass123",
            )
            .expect("managed user");
        let managed_session = SessionService::new(storage.clone())
            .login("ManagedUser", "ManagedPass123")
            .expect("managed login");

        let started_at = Instant::now();
        let mut reloader = UserThemeReloader::new(Some(storage), started_at);
        let mut theme = ui::TundraTheme::default_dark();
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        state.complete_login(admin_session);
        reloader.poll_at(started_at, &mut theme, &mut state);
        assert_eq!(theme.border_shape, ui::BorderShape::Square);
        assert_eq!(theme.border_color, ratatui::style::Color::LightGreen);
        assert_eq!(theme.accent_color, ratatui::style::Color::LightMagenta);

        state.complete_login(managed_session);
        reloader.poll_at(started_at, &mut theme, &mut state);
        assert_eq!(theme.border_shape, ui::BorderShape::Rounded);
        assert_eq!(
            theme.border_color,
            ratatui::style::Color::Rgb(0x29, 0x43, 0x4E)
        );
        assert_eq!(
            theme.accent_color,
            ratatui::style::Color::Rgb(0x63, 0xD3, 0xE5)
        );

        platform::cleanup_temp_path(&root).expect("clean test root");
    }
}
