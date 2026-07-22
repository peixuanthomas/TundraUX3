use super::*;
use std::panic::AssertUnwindSafe;
use watchdog::{
    AppCriticality, AppDescriptor, AppId, AppWatchdog, BoundaryKind, BoundarySpec, CaughtPanic,
    ComponentId, IncidentKind, IncidentReceipt, ManagedThreadHandle, PanicAction, ProcessWatchdog,
    RecoveryOutcome, ReplaySafety, RestartPolicy, RuntimeSnapshot, TaskId, TaskKind, TaskSpec,
};

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
    run_fullscreen_blocking_managed(output, process)
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
    let config = ShellLaunchConfig::default();
    let ascii_assets = load_validated_runtime_ascii_assets()?;
    let terminal_size_requirement = ShellTerminalSizeRequirement::from_assets(&ascii_assets);
    checked_current_terminal_size(terminal_size_requirement)?;
    let platform: std::sync::Arc<dyn Platform> = std::sync::Arc::from(platform::native_platform());
    let shell_watchdog = process
        .register_app(shell_watchdog_descriptor())
        .map_err(io::Error::other)?;
    let weathr_watchdog = process
        .register_app(weathr::weathr_watchdog_descriptor())
        .map_err(io::Error::other)?;
    let diagnostics_watchdog = process
        .register_app(app::diagnostics::diagnostics_watchdog_descriptor())
        .map_err(io::Error::other)?;
    let initial_startup = prepare_shell_startup(platform.as_ref()).map_err(io::Error::other)?;
    let (time_sync_sender, time_sync_receiver) = mpsc::channel();
    let time_sync_watchdog = shell_watchdog.child_component(ComponentId::from_static("time-sync"));
    // Both background jobs must be live before the blocking frost animation so
    // normal login can consume time calibration and prefetched weather data.
    let _time_sync_worker = spawn_time_sync_worker(
        time_sync_sender,
        &time_sync_watchdog,
        initial_startup.storage_manager.clone(),
        std::sync::Arc::clone(&platform),
    )
    .map_err(io::Error::other)?;
    let weather_prefetch_options =
        startup_lockscreen_launch_options(&initial_startup, terminal_size_requirement);
    let _weather_prefetch_worker =
        spawn_weather_prefetch_worker(weather_prefetch_options, &weathr_watchdog)
            .map_err(io::Error::other)?;
    with_fullscreen(output, |output| {
        display_startup_banner_with_assets_colored(
            output,
            &ascii_assets,
            initial_startup.app_config.border_color,
        )
    })?;
    let mut initial_startup = Some(initial_startup);
    let mut cached_time_sync = None;
    let mut force_lockscreen = false;
    let mut session_recoveries = VecDeque::new();
    let mut explorer_task_runtime: Option<ShellExplorerTaskRuntime> = None;
    let mut diagnostics_task_runtime: Option<ShellDiagnosticsTaskRuntime> = None;

    loop {
        let mut startup = match initial_startup.take() {
            Some(startup) => startup,
            None => prepare_shell_startup(platform.as_ref()).map_err(io::Error::other)?,
        };
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
            let lockscreen_options =
                startup_lockscreen_launch_options(&startup, terminal_size_requirement);
            let lockscreen_watchdog = weathr_watchdog.clone();
            let lockscreen_result = weathr_watchdog.run_boundary(
                BoundarySpec::new("shell-lockscreen-ui-session", BoundaryKind::UiSession)
                    .terminal_owner(),
                AssertUnwindSafe(|| {
                    weathr::run_shell_lockscreen_managed(lockscreen_options, lockscreen_watchdog)
                }),
            );
            match lockscreen_result {
                Ok(Ok(weathr::ShellLockscreenResult::Started)) => {}
                Ok(Ok(weathr::ShellLockscreenResult::Cancelled)) => return Ok(()),
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
                run_fullscreen_shell_session(
                    output,
                    config,
                    startup,
                    ascii_assets.clone(),
                    std::sync::Arc::clone(&platform),
                    &time_sync_receiver,
                    &mut cached_time_sync,
                    &shell_watchdog,
                    &process,
                    explorer_task_runtime.clone(),
                    diagnostics_task_runtime.clone(),
                )
            }),
        );
        match session_result {
            Ok(Ok(FullscreenShellSessionOutcome::Exit)) => return Ok(()),
            Ok(Ok(FullscreenShellSessionOutcome::ReturnToLockscreen)) => {
                force_lockscreen = true;
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
            return Ok(());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FullscreenShellSessionOutcome {
    Exit,
    ReturnToLockscreen,
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
    pub(super) _worker: ManagedThreadHandle<()>,
}

impl LauncherIconRuntime {
    fn detect_and_spawn(
        platform: std::sync::Arc<dyn Platform>,
        watchdog: &AppWatchdog,
    ) -> Result<Option<Self>, String> {
        let picker = ui::EditorImagePicker::detect_stdio().map_err(|error| error.to_string())?;
        let Some(picker) = picker else {
            return Ok(None);
        };
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
        Ok(Some(Self {
            picker,
            requests: request_sender,
            results: result_receiver,
            pending: HashSet::new(),
            unavailable: HashSet::new(),
            source_icons: HashMap::new(),
            prepared: HashMap::new(),
            _worker: worker,
        }))
    }

    fn protocol(&self) -> ui::EditorGraphicsProtocol {
        self.picker.protocol()
    }

    fn sync(&mut self, model: &ui::LauncherViewModel, main: Rect) {
        while let Ok(result) = self.results.try_recv() {
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

#[derive(Debug)]
pub(super) struct TimedTimeSyncResult {
    pub(super) result: TimeSyncResult,
    pub(super) received_at: Instant,
}

pub(super) struct TimeSyncWorker {
    pub(super) stop_sender: mpsc::Sender<()>,
    pub(super) handle: Option<ManagedThreadHandle<()>>,
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
    fn stop_and_join(&mut self) {
        let _ = self.stop_sender.send(());
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

pub(super) fn run_fullscreen_shell_session(
    output: &mut impl Write,
    config: ShellLaunchConfig,
    startup: ShellStartupState,
    ascii_assets: ui::RuntimeAsciiAssets,
    platform: std::sync::Arc<dyn Platform>,
    time_sync_receiver: &mpsc::Receiver<TimedTimeSyncResult>,
    cached_time_sync: &mut Option<CachedTimeSyncResult>,
    shell_watchdog: &AppWatchdog,
    process_watchdog: &ProcessWatchdog,
    explorer_task_runtime: Option<ShellExplorerTaskRuntime>,
    diagnostics_task_runtime: Option<ShellDiagnosticsTaskRuntime>,
) -> io::Result<FullscreenShellSessionOutcome> {
    let terminal_size_requirement = ShellTerminalSizeRequirement::from_assets(&ascii_assets);
    let initial_size = checked_current_terminal_size(terminal_size_requirement)?;
    let terminal_control = TerminalControlHandler::install();
    let mut guard = TerminalGuard::enter(output)?;
    let launcher_icon_result =
        LauncherIconRuntime::detect_and_spawn(std::sync::Arc::clone(&platform), shell_watchdog);
    let terminal_graphics_protocol = launcher_icon_result
        .as_ref()
        .ok()
        .and_then(|runtime| runtime.as_ref())
        .map(LauncherIconRuntime::protocol);
    if let Some(diagnostics) = diagnostics_task_runtime.as_ref() {
        diagnostics.set_terminal_graphics_protocol(platform.kind(), terminal_graphics_protocol);
    }
    let mut launcher_icons = launcher_icon_result.unwrap_or(None);
    let theme_storage = startup.storage_manager.clone();
    if startup.auth_bootstrap_required {
        display_first_run_banner_with_assets_colored(
            guard.terminal_mut().backend_mut(),
            &ascii_assets,
            startup.app_config.border_color,
        )?;
    }
    let mut theme = ui::TundraTheme::default_dark();
    let mut state = ShellSession::new_with_runtime_services(
        config,
        initial_size,
        startup,
        ascii_assets,
        explorer_task_runtime,
        diagnostics_task_runtime,
        ShellEditorTaskRuntime::new_managed(shell_watchdog.clone()),
        ShellSettingsTaskRuntime::new_managed(shell_watchdog.clone()),
    );
    state.launcher_task_runtime = Some(ShellLauncherTaskRuntime::new_managed(
        std::sync::Arc::clone(&platform),
        shell_watchdog.clone(),
    ));
    if let Some(cached) = cached_time_sync.as_ref() {
        cached.apply_to_state_at(&mut state, Instant::now());
    }
    let mut theme_reloader = UserThemeReloader::new(theme_storage, Instant::now());
    let tick_rate = Duration::from_millis(250);
    let mut terminal_size_error = None;
    let mut restore_terminal_on_exit = true;

    loop {
        if let Err(error) = terminal_size_requirement.validate(crossterm::terminal::size()?) {
            terminal_size_error = Some(io::Error::other(error));
            break;
        }

        drain_time_sync_results(&mut state, time_sync_receiver, cached_time_sync);
        drain_watchdog_incidents(&mut state, process_watchdog);
        shell_watchdog.heartbeat(RuntimeSnapshot {
            screen: Some(format!("{:?}", state.active_screen())),
            terminal_size: Some(state.terminal_size()),
            ..RuntimeSnapshot::default()
        });
        let frame_now = Instant::now();
        theme_reloader.poll_at(frame_now, &mut theme, &mut state);
        let clock_snapshot = state.app.snapshot().clock;
        state.advance_clock_background_at(&clock_snapshot, frame_now);
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
        let active_screen = state.active_screen();
        let content_screen = state.content_screen();
        let chrome = state.to_shell_chrome_view_model();
        let home = state.to_home_view_model();
        let clock = state
            .to_clock_view_model_at(&clock_snapshot, frame_now)
            .with_terminal_cell_aspect_ratio(terminal_cell_aspect_ratio);
        let time_sync_dialog = state.to_time_sync_dialog_view_model();
        let setup = state.to_setup_view_model();
        let login = state.to_login_view_model_at(frame_now);
        let bootstrap_admin = state.to_bootstrap_admin_view_model();
        let user_management = state.to_user_management_view_model();
        let explorer = state.to_explorer_view_model();
        let launcher = state.to_launcher_view_model();
        if content_screen == ShellScreen::Launcher
            && let Some(icon_runtime) = launcher_icons.as_mut()
            && let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(
                0,
                0,
                state.terminal_size().0,
                state.terminal_size().1,
            ))
        {
            icon_runtime.sync(&launcher, main);
        }
        let editor = (content_screen == ShellScreen::Editor).then(|| state.to_editor_view_model());
        let settings = (content_screen == ShellScreen::Settings)
            .then(|| state.to_settings_view_model())
            .flatten();
        let diagnostics = state.to_diagnostics_view_model();
        let notification = state.to_notification_view_model();
        let exit_confirmation = ui::ExitConfirmViewModel::new();

        guard.terminal_mut().draw(|frame| {
            let area = frame.area();
            match content_screen {
                ShellScreen::FirstRunSetup => {
                    ui::render_setup(frame, area, &chrome, &setup, &theme);
                }
                ShellScreen::Login => {
                    ui::render_login(frame, area, &chrome, &login, &theme);
                }
                ShellScreen::BootstrapAdmin => {
                    ui::render_bootstrap_admin(frame, area, &chrome, &bootstrap_admin, &theme);
                }
                ShellScreen::UserManagement => {
                    ui::render_user_management(frame, area, &chrome, &user_management, &theme);
                }
                ShellScreen::Explorer => {
                    ui::render_explorer(frame, area, &chrome, &explorer, &theme);
                }
                ShellScreen::Launcher => {
                    ui::render_launcher_with_icons(
                        frame,
                        area,
                        &chrome,
                        &launcher,
                        &theme,
                        launcher_icons
                            .as_ref()
                            .map(|runtime| runtime as &dyn ui::LauncherIconRenderer),
                    );
                }
                ShellScreen::Editor => {
                    ui::render_editor_app(
                        frame,
                        area,
                        &chrome,
                        editor
                            .as_ref()
                            .expect("Editor content requires its view model"),
                        &theme,
                    );
                }
                ShellScreen::Settings => {
                    ui::render_settings(
                        frame,
                        area,
                        &chrome,
                        settings
                            .as_ref()
                            .expect("Settings content requires its view model"),
                        &theme,
                    );
                }
                ShellScreen::Diagnostics => {
                    ui::render_diagnostics(frame, area, &chrome, &diagnostics, &theme);
                }
                ShellScreen::Clock => {
                    ui::render_clock(frame, area, &chrome, &clock, &theme);
                }
                ShellScreen::Home | ShellScreen::ExitConfirm => {
                    ui::render_home(frame, area, &chrome, &home, &theme);
                }
            }

            if notification.is_none() && active_screen == ShellScreen::ExitConfirm {
                ui::render_exit_confirmation(frame, area, &exit_confirmation, &theme);
            }
            if notification.is_none()
                && let Some(dialog) = time_sync_dialog.as_ref()
            {
                ui::render_time_sync_failure_dialog(frame, area, dialog, &theme);
            }
            if let Some(notification) = notification.as_ref() {
                ui::render_notification_overlay(frame, area, notification, &theme);
            }
        })?;

        if terminal_control.shutdown_requested() {
            state.apply_input_with_platform(InputEvent::Shutdown, platform.as_ref());
        }
        if state.shutdown_requested() {
            break;
        }

        let poll_now = Instant::now();
        let poll_timeout = state.auth_poll_timeout(
            poll_now,
            state.notification_poll_timeout(poll_now, tick_rate),
        );
        let action = if event::poll(poll_timeout)? {
            let terminal_event = event::read()?;
            if let event::Event::Resize(width, height) = terminal_event
                && let Err(error) = terminal_size_requirement.validate((width, height))
            {
                terminal_size_error = Some(io::Error::other(error));
                break;
            }
            state.apply_input_with_platform(
                crossterm_event_to_input(terminal_event),
                platform.as_ref(),
            )
        } else {
            state.apply_input_with_platform(InputEvent::Tick, platform.as_ref())
        };

        if action == ShellAction::Exit {
            break;
        }
        if action == ShellAction::PowerOff {
            restore_terminal_on_exit = false;
            break;
        }
    }

    if restore_terminal_on_exit {
        guard.restore()?;
    } else {
        guard.skip_restore();
    }
    drop(guard);

    if let Some(error) = terminal_size_error {
        return Err(error);
    }

    let outcome = if state.return_to_lockscreen_requested() {
        FullscreenShellSessionOutcome::ReturnToLockscreen
    } else {
        FullscreenShellSessionOutcome::Exit
    };
    Ok(outcome)
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

#[cfg(test)]
mod runtime_preflight_tests {
    use super::*;

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
        assert_eq!(theme.border_color, ratatui::style::Color::White);
        assert_eq!(theme.accent_color, ratatui::style::Color::Cyan);

        platform::cleanup_temp_path(&root).expect("clean test root");
    }
}

pub(super) fn spawn_time_sync_worker(
    sender: mpsc::Sender<TimedTimeSyncResult>,
    watchdog: &AppWatchdog,
    storage: Option<StorageManager>,
    platform: std::sync::Arc<dyn Platform>,
) -> Result<TimeSyncWorker, watchdog::WatchdogError> {
    let (stop_sender, stop_receiver) = mpsc::channel();
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
            let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            else {
                return;
            };

            runtime.block_on(async {
                loop {
                    match stop_receiver.try_recv() {
                        Ok(()) | Err(mpsc::TryRecvError::Disconnected) => break,
                        Err(mpsc::TryRecvError::Empty) => {}
                    }
                    let time_sync = match storage.as_ref() {
                        Some(storage) => match storage.load_config() {
                            Ok(config) => Ok(config.time_sync),
                            Err(error) => Err(time::TimeSyncError::new(vec![format!(
                                "could not load time sync settings: {error}"
                            )])),
                        },
                        None => Ok(storage::TimeSyncConfig::default()),
                    };
                    let result = match time_sync {
                        Ok(time_sync) => {
                            synchronize_configured_time(&time_sync, platform.as_ref()).await
                        }
                        Err(error) => Err(error),
                    };
                    if sender
                        .send(TimedTimeSyncResult {
                            result,
                            received_at: Instant::now(),
                        })
                        .is_err()
                    {
                        break;
                    }
                    match stop_receiver.recv_timeout(TIME_SYNC_INTERVAL) {
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            });
        },
    )?;
    Ok(TimeSyncWorker {
        stop_sender,
        handle: Some(handle),
    })
}

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

pub(super) fn spawn_weather_prefetch_worker(
    options: weathr::LaunchOptions,
    watchdog: &AppWatchdog,
) -> Result<ManagedThreadHandle<()>, watchdog::WatchdogError> {
    let group = watchdog
        .child_component(ComponentId::from_static("startup-prefetch"))
        .task_group("weather");
    group.spawn_thread(
        TaskSpec::one_shot(TaskId::from_static("refresh")),
        move || {
            let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            let _ = runtime.block_on(weathr::prefetch_weather(options.clone()));
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
