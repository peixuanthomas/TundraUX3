use super::*;
use std::panic::AssertUnwindSafe;
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
        ShellRunOutcome::UpdatePrepared(_) => Err(update_requires_binary_entrypoint()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellRunOutcome {
    Exit,
    RestartRequested,
    ResetRequested,
    UpdatePrepared(std::path::PathBuf),
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
        ShellRunOutcome::UpdatePrepared(_) => Err(update_requires_binary_entrypoint()),
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

fn update_requires_binary_entrypoint() -> io::Error {
    io::Error::new(
        io::ErrorKind::Interrupted,
        "the binary entry point must launch the prepared update helper",
    )
}

pub fn run_fullscreen_blocking_managed_with_outcome(
    output: &mut impl Write,
    process: ProcessWatchdog,
) -> io::Result<ShellRunOutcome> {
    let config = ShellLaunchConfig::default();
    let platform: std::sync::Arc<dyn Platform> = std::sync::Arc::from(platform::native_platform());
    let (mut ascii_assets, recovery_report) = load_startup_runtime_ascii_assets()?;
    let mut startup_resource_report = Some(recovery_report);
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
    let configured_language = initial_startup
        .storage_manager
        .as_ref()
        .and_then(|storage| storage.load_config().ok())
        .map(|config| config.language)
        .unwrap_or_else(|| "en-US".into());
    let mut language_runtime =
        PreparedLanguage::load(ascii_assets.store().root(), &configured_language);
    let _startup_language = i18n::enter_snapshot(language_runtime.snapshot.clone());
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
    let mut explorer_task_runtime: Option<ShellExplorerTaskRuntime> = None;
    let mut diagnostics_task_runtime: Option<ShellDiagnosticsTaskRuntime> = None;
    // Linux installs its logind subscriptions lazily through this poll. Do it
    // before the first Weathr lockscreen so PrepareForShutdown can drive the
    // same process-wide shutdown flag used by the main Shell and lockscreen.
    let _ = platform.poll_lifecycle_event();

    loop {
        let _language = i18n::enter_snapshot(language_runtime.snapshot.clone());
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
            let language = language_runtime.snapshot.clone();
            let lockscreen_input = weathr::WeathrDisplayInput {
                localize: Arc::new(move |id, args| {
                    let mut message = i18n::LocalizedMessage::new(id);
                    for (name, value) in args {
                        message = message.with_arg(*name, value.clone());
                    }
                    language.render(&message)
                }),
                snapshots: system_services.subscribe(),
                clock_format: weathr::ClockFormat::TwentyFourHour,
                hide_hud: false,
                palette: weathr::theme::catalogue::DEFAULT_PALETTE,
                shutdown: terminal_control.shutdown_flag(),
                minimum_terminal_size: Some(terminal_size_requirement.as_terminal_size()),
                exit_semantic: weathr::ExitSemantic::Start,
                first_frame_callback: Some(Arc::new(|| {
                    app::update::mark_update_ready_from_env().map_err(io::Error::other)
                })),
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
                    let message = finalize_session_panic(caught, "Weathr lockscreen");
                    return run_panic_screen(
                        output,
                        &message,
                        &terminal_control,
                        platform.as_ref(),
                    );
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
                    language_runtime: &mut language_runtime,
                    startup_resource_report: startup_resource_report.take(),
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
                    FullscreenShellSessionOutcome::UpdatePrepared(manifest) => {
                        return Ok(ShellRunOutcome::UpdatePrepared(manifest));
                    }
                    FullscreenShellSessionOutcome::Panic(message) => {
                        let _language = i18n::enter_snapshot(language_runtime.snapshot.clone());
                        return run_panic_screen(
                            output,
                            &message,
                            &terminal_control,
                            platform.as_ref(),
                        );
                    }
                }
            }
            Ok(Err(error)) => return Err(error),
            Err(caught) => {
                let _language = i18n::enter_snapshot(language_runtime.snapshot.clone());
                let message = finalize_session_panic(caught, "Shell UI");
                return run_panic_screen(output, &message, &terminal_control, platform.as_ref());
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FullscreenShellSessionOutcome {
    Exit,
    RestartRequested,
    ReturnToLockscreen,
    ResetRequested,
    UpdatePrepared(std::path::PathBuf),
    Panic(String),
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

    pub(super) fn sync(&mut self, model: &ui::LauncherViewModel, main: Rect) {
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

    pub(super) fn sync_home(&mut self, model: &ui::HomeViewModel, main: Rect) {
        let labels = model
            .entries()
            .iter()
            .map(|entry| entry.icon_identity())
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
                .get(entry.icon_identity())
                .is_none_or(|cached| cached.area != icon_area);
            if !needs_prepare || self.home_unavailable.contains(entry.icon_identity()) {
                continue;
            }

            self.home_prepared.remove(entry.icon_identity());
            let prepared = model
                .home_icon_image_bytes_for_label(entry.icon_identity())
                .ok_or_else(|| "Home icon asset is not cached".to_string())
                .and_then(|bytes| {
                    self.picker
                        .prepare_bytes(bytes, icon_area)
                        .map_err(|error| error.to_string())
                });
            match prepared {
                Ok(image) => {
                    self.home_prepared.insert(
                        entry.icon_identity().to_string(),
                        CachedLauncherIcon {
                            area: icon_area,
                            image,
                        },
                    );
                }
                Err(_) => {
                    self.home_unavailable
                        .insert(entry.icon_identity().to_string());
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
        // A resize can land between preparation and Terminal::draw. A fixed-size
        // image protocol must never paint an obsolete allocation; use the page's
        // text fallback until the next preparation pass catches up.
        if icon.area != area {
            return false;
        }
        icon.image.render_centered(frame, area);
        true
    }
}

impl ui::HomeIconRenderer for LauncherIconRuntime {
    fn render_icon(&self, entry_label: &str, frame: &mut ratatui::Frame<'_>, area: Rect) -> bool {
        let Some(icon) = self.home_prepared.get(entry_label) else {
            return false;
        };
        // A resize can land between preparation and Terminal::draw. A fixed-size
        // image protocol must never paint an obsolete allocation; use the page's
        // text fallback until the next preparation pass catches up.
        if icon.area != area {
            return false;
        }
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
                    i18n::msg!("startup-theme-reload-title"),
                    i18n::msg!("startup-theme-reload-failed", reason = error.to_string()),
                    ui::NotificationTone::Error,
                    vec![
                        ShellNotificationAction::new("ok", i18n::msg!("resources-recovery-ok"))
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
    startup_resource_report: Option<ascii_assets::DefaultThemeRecoveryReport>,
    language_runtime: &'a mut PreparedLanguage,
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
        startup_resource_report,
        language_runtime,
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
            language: Some(language_runtime.clone()),
            explorer: explorer_task_runtime,
            diagnostics: diagnostics_task_runtime,
            editor: ShellEditorTaskRuntime::new_managed(shell_watchdog.clone()),
            settings: ShellSettingsTaskRuntime::new_managed_with_system_services(
                shell_watchdog.clone(),
                Some(system_services.clone()),
                settings_services_config,
                Some(std::sync::Arc::clone(&platform)),
            ),
        },
    );
    if let Some(report) = startup_resource_report {
        state.report_graphical_resource_recovery(&report);
    } else {
        // The startup report is process-scoped, not a login-session notification.
        state.app.dispatch_at(
            app::AppCommand::Notification(app::NotificationCommand::DismissModalByKey(
                "shell.resource-recovery".into(),
            )),
            Instant::now(),
        );
        state.repaired_resource_paths.clear();
        state.fallback_resource_paths.clear();
    }
    #[cfg(target_os = "linux")]
    let (authorization_host, authorization_interaction) =
        crate::authorization::AuthorizationHost::channel();
    #[cfg(target_os = "linux")]
    if let Ok(mut interaction) = state.settings_task_runtime.shared.authorization.lock() {
        *interaction = Some(authorization_interaction.clone());
    }
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
    let mut compositor = ScreenCompositor::default();
    let mut terminal_size_error = None;
    let mut terminal_suspended = false;
    let mut last_background_poll = runtime_origin;
    let mut update_ready_marked = false;

    loop {
        #[cfg(target_os = "linux")]
        if authorization_host
            .handle_pending(&mut guard, || terminal_control.shutdown_requested())?
        {
            redraw.request_redraw();
        }
        language_runtime.update_from(&state);
        let _language = i18n::enter_snapshot(state.language.clone());
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
                        i18n::msg!("startup-lifecycle-failed", reason = error.to_string()),
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
                    if let Some(routed) = compositor.cancel_for_suspend(&state) {
                        state.apply_routed_event(routed, platform.as_ref(), Instant::now());
                        redraw.request_redraw();
                    }
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
                CommandLineHostEvent::PanicRequested => trigger_command_line_panic(),
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
        if let Some(message) = drain_watchdog_incidents(&mut state, process_watchdog) {
            command_line_host.terminate();
            guard.restore()?;
            return Ok((
                FullscreenShellSessionOutcome::Panic(message),
                state.ascii_assets.clone(),
            ));
        }
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
            let _language = i18n::enter_snapshot(state.language.clone());
            let motion_frame = redraw.frame(frame_now, animation_speed_percent(&state));
            let motion_transitions = redraw.transitions(frame_now);
            let render_context = ui::RenderContext::from_theme_with_transitions(
                &theme,
                motion_frame,
                motion_transitions,
                shell_render_capabilities(terminal_graphics_probe),
            );
            let aspect = if state.content_screen() == ShellScreen::Clock {
                crossterm::terminal::window_size()
                    .map(|window| {
                        ui::TerminalCellAspectRatio::from_window_size(
                            window.columns,
                            window.rows,
                            window.width,
                            window.height,
                        )
                    })
                    .unwrap_or_default()
            } else {
                ui::TerminalCellAspectRatio::default()
            };
            let prepared = compositor.prepare(
                &state,
                &command_line_host,
                frame_now,
                aspect,
                render_context,
                launcher_icons.as_mut(),
            );
            let mut animation_running = false;
            guard.terminal_mut().draw(|frame| {
                animation_running =
                    compositor.render(frame, &mut state, &prepared, launcher_icons.as_ref());
            })?;
            if !update_ready_marked {
                app::update::mark_update_ready_from_env().map_err(io::Error::other)?;
                update_ready_marked = true;
            }
            redraw.did_draw(frame_now);
            if animation_running {
                redraw.request_animation_frame(frame_now);
            }
        }

        if terminal_control.shutdown_requested() {
            state.apply_input_with_platform(InputEvent::Shutdown, platform.as_ref());
        }
        if state.shutdown_requested() {
            break;
        }

        // An exit effect can finish on the frame just drawn. Apply its already-routed
        // action before deriving any blocking poll deadline, then immediately render the
        // resulting natural state on the next loop iteration.
        if let Some(routed) = compositor.take_deferred_close(&state) {
            state.apply_routed_event(routed, platform.as_ref(), Instant::now());
            redraw.request_redraw();
            continue;
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
                let boundary_changed = matches!(&terminal_event, event::Event::Resize(_, _));
                if boundary_changed {
                    compositor.cancel_for_bounds_change();
                }
                let input = crossterm_event_to_input(terminal_event);
                let command_line_captures = command_line_captures_input(&state, &input);
                if command_line_captures {
                    let (width, height) = state.terminal_size();
                    let terminal_area =
                        ui::command_line_terminal_area(Rect::new(0, 0, width, height));
                    match command_line_host.handle_input(&input, terminal_area) {
                        CommandLineHostEvent::None => {}
                        CommandLineHostEvent::PanicRequested => trigger_command_line_panic(),
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
                    let received_at = Instant::now();
                    let (input_action, motion_blocked) = compositor.dispatch_input(
                        &mut state,
                        input,
                        platform.as_ref(),
                        received_at,
                    );
                    action = Some(input_action);
                    if motion_blocked {
                        redraw.request_animation_frame(received_at);
                    }
                }
                synchronize_motion_hit_map_after_input(
                    &mut state,
                    &mut redraw,
                    identity_before_input,
                    Instant::now(),
                );
                let context = ui::RenderContext::from_theme(
                    &theme,
                    redraw.frame(Instant::now(), animation_speed_percent(&state)),
                    shell_render_capabilities(terminal_graphics_probe),
                );
                compositor.synchronize_after_input(&state, &context);
                if let Some(routed) = compositor.take_deferred_close(&state) {
                    action =
                        Some(state.apply_routed_event(routed, platform.as_ref(), Instant::now()));
                    redraw.request_redraw();
                    break;
                }
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
        if matches!(action, Some(ShellAction::PowerOff | ShellAction::Reboot)) {
            // Interactive authorization may temporarily take over the
            // terminal. Recovery has already been persisted by the command
            // handler, so restore the user's terminal before asking logind or
            // the native platform service to power off.
            guard.restore()?;
            let reboot = action == Some(ShellAction::Reboot);
            #[cfg(target_os = "linux")]
            let result = authorization_host.power(
                &mut guard,
                if reboot {
                    platform::linux::power::PowerAction::Reboot
                } else {
                    platform::linux::power::PowerAction::PowerOff
                },
                shell_watchdog,
                authorization_interaction.clone(),
                || terminal_control.shutdown_requested(),
            );
            #[cfg(not(target_os = "linux"))]
            let result = if reboot {
                platform.reboot()
            } else {
                platform.poweroff()
            };
            match result {
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
                        if reboot {
                            i18n::msg!("startup-reboot-failed", reason = error.to_string())
                        } else {
                            i18n::msg!("startup-shutdown-failed", reason = error.to_string())
                        },
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

    let outcome = if let Some(manifest) = state.update_apply_manifest().map(ToOwned::to_owned) {
        FullscreenShellSessionOutcome::UpdatePrepared(manifest)
    } else if reset_requested {
        FullscreenShellSessionOutcome::ResetRequested
    } else if state.restart_requested {
        FullscreenShellSessionOutcome::RestartRequested
    } else if state.return_to_lockscreen_requested() {
        FullscreenShellSessionOutcome::ReturnToLockscreen
    } else {
        FullscreenShellSessionOutcome::Exit
    };
    language_runtime.update_from(&state);
    Ok((outcome, state.ascii_assets.clone()))
}

pub(super) fn dispatch_motion_aware_input(
    state: &mut ShellSession,
    motion: &mut ShellMotionEffects,
    input: InputEvent,
    platform: &dyn Platform,
    received_at: Instant,
) -> (ShellAction, bool) {
    if let Some(action) = state.apply_input_preamble_at(&input, received_at) {
        return (action, false);
    }
    if motion.blocks_before_route(&input) {
        return (ShellAction::Redraw, true);
    }
    // Route once against live state. Deferred replay applies this exact semantic event
    // and deliberately skips the already-run preamble.
    let routed = state.route_input_at(input, received_at);
    match motion.intercept_input(&routed) {
        MotionInputDisposition::Apply => (
            state.apply_routed_event(routed, platform, received_at),
            false,
        ),
        MotionInputDisposition::Defer | MotionInputDisposition::Block => {
            (ShellAction::Redraw, true)
        }
    }
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

fn animation_speed_percent(state: &ShellSession) -> u16 {
    state
        .app
        .active_appearance()
        .map(storage::AppearanceConfig::normalized_animation_speed_percent)
        .unwrap_or(storage::DEFAULT_ANIMATION_SPEED_PERCENT)
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
        || state.settings_task_runtime.update_busy()
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
    let mut capabilities = crate::terminal_session::text_render_capabilities();
    capabilities.image_protocol = matches!(
        terminal_graphics_probe.status(),
        ui::TerminalGraphicsProbeStatus::Verified(_)
    );
    capabilities
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
            i18n::msg!("startup-session-refresh-failed", reason = error.to_string()),
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

pub(super) fn trigger_command_line_panic() -> ! {
    panic!("Intentional watchdog panic test requested from Command Line");
}

pub(super) fn finalize_session_panic(caught: CaughtPanic, session_name: &str) -> String {
    let reason = caught.payload().to_string();
    // Persist the full report, but keep the crash page focused on the error.
    let _ = caught.finalize(RecoveryOutcome::Unrecoverable(format!(
        "the {session_name} stopped; waiting for the user to restart or exit"
    )));
    format!("{session_name}: {reason}")
}

fn run_panic_screen(
    output: &mut impl Write,
    message: &str,
    terminal_control: &TerminalControlHandler,
    platform: &dyn Platform,
) -> io::Result<ShellRunOutcome> {
    // Use a fresh terminal and a fixed renderer, never the damaged Shell state.
    let mut guard = TerminalGuard::enter(output)?;
    let mut screen = ui::PanicScreen::new(message);
    let mut redraw = true;
    let outcome = loop {
        let _ = platform.poll_lifecycle_event();
        if terminal_control.shutdown_requested() {
            break ShellRunOutcome::Exit;
        }
        if redraw {
            guard.terminal_mut().draw(|frame| screen.render(frame))?;
            redraw = false;
        }
        if event::poll(Duration::from_millis(250))? {
            if let Some(outcome) = apply_panic_screen_event(&mut screen, event::read()?) {
                break outcome;
            }
            redraw = true;
        }
    };
    guard.restore()?;
    Ok(outcome)
}

fn apply_panic_screen_event(
    screen: &mut ui::PanicScreen,
    event: event::Event,
) -> Option<ShellRunOutcome> {
    use event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};
    match event {
        Event::Key(key) if key.kind != KeyEventKind::Release => {
            let control = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Char('c') if control => return Some(ShellRunOutcome::Exit),
                KeyCode::Char('q' | 'Q') | KeyCode::Esc if key.kind == KeyEventKind::Press => {
                    return Some(ShellRunOutcome::Exit);
                }
                KeyCode::Char('r' | 'R') if key.kind == KeyEventKind::Press => {
                    return Some(ShellRunOutcome::RestartRequested);
                }
                KeyCode::Up => screen.scroll_up(false),
                KeyCode::Down => screen.scroll_down(false),
                KeyCode::PageUp => screen.scroll_up(true),
                KeyCode::PageDown => screen.scroll_down(true),
                KeyCode::Home => screen.scroll_to_start(),
                KeyCode::End => screen.scroll_to_end(),
                _ => {}
            }
        }
        Event::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollUp => screen.scroll_up(false),
            MouseEventKind::ScrollDown => screen.scroll_down(false),
            _ => {}
        },
        _ => {}
    }
    None
}

pub(super) fn load_validated_runtime_ascii_assets() -> io::Result<ui::RuntimeAsciiAssets> {
    let ascii_assets = ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    checked_current_terminal_size(ShellTerminalSizeRequirement::from_assets(&ascii_assets))?;
    Ok(ascii_assets)
}

fn load_startup_runtime_ascii_assets() -> io::Result<(
    ui::RuntimeAsciiAssets,
    ascii_assets::DefaultThemeRecoveryReport,
)> {
    let (store, report) =
        ui::AsciiAssetStore::load_default_with_recovery().map_err(asset_io_error)?;
    Ok((ui::RuntimeAsciiAssets::from_store(store), report))
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

pub(super) fn drain_watchdog_incidents(
    state: &mut ShellSession,
    watchdog: &ProcessWatchdog,
) -> Option<String> {
    let mut panic_messages = Vec::new();
    for incident in watchdog.drain_incidents() {
        if incident.kind == IncidentKind::Panic {
            panic_messages.push(incident.summary);
        } else {
            show_watchdog_incident(state, incident);
        }
    }
    (!panic_messages.is_empty()).then(|| panic_messages.join("\n\n"))
}

fn watchdog_incident_summary(incident: &IncidentReceipt) -> String {
    let report = incident
        .text_report_path
        .as_ref()
        .or(incident.json_report_path.as_ref())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "report path unavailable".to_string());
    format!(
        "{}\n\nRecovery: {:?}\nIncident: {}\nReport: {}",
        incident.summary, incident.recovery, incident.incident_id, report
    )
}

pub(super) fn show_watchdog_incident(state: &mut ShellSession, incident: IncidentReceipt) {
    let full_summary = watchdog_incident_summary(&incident);
    state.latest_watchdog_report = incident.text_report_path.or(incident.json_report_path);
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
    let public_summary = i18n::msg!(
        "startup-critical-public",
        recovery = format!("{:?}", incident.recovery)
    );
    let display_summary = i18n::msg!(
        "startup-critical-detail",
        summary = incident.summary.clone(),
        recovery = format!("{:?}", incident.recovery),
        incident = incident.incident_id.to_string(),
        report = state
            .latest_watchdog_report
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default()
    );
    let mut actions =
        vec![ShellNotificationAction::new("continue", i18n::msg!("startup-continue")).cancel()];
    if can_view_details {
        actions.extend([
            ShellNotificationAction::new("open-report", i18n::msg!("startup-open-report"))
                .with_follow_up(ShellCommand::OpenLatestCrashReport),
            ShellNotificationAction::new("copy-summary", i18n::msg!("startup-copy-summary"))
                .with_follow_up(ShellCommand::CopyLatestCrashSummary),
        ]);
    }
    actions.push(
        ShellNotificationAction::new("exit", i18n::msg!("startup-exit"))
            .with_follow_up(ShellCommand::RequestExit),
    );
    state.notify_critical_modal(
        if incident.recovery.is_recovered() {
            i18n::msg!("startup-critical-recovered")
        } else {
            i18n::msg!("startup-critical-failed")
        },
        if can_view_details {
            display_summary
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
#[path = "../../tests/unit/session/runtime/runtime_preflight_tests.rs"]
mod runtime_preflight_tests;
