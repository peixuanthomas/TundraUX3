use std::panic::AssertUnwindSafe;
use tundra_watchdog::{
    AppCriticality, AppDescriptor, AppId, AppWatchdog, BoundaryKind, BoundarySpec, CaughtPanic,
    ComponentId, IncidentKind, IncidentReceipt, ManagedTaskGroup, ManagedThreadHandle, PanicAction,
    ProcessWatchdog, RecoveryOutcome, ReplaySafety, RestartPolicy, RuntimeSnapshot, TaskId,
    TaskKind, TaskSpec,
};

pub fn run_without_animation(output: &mut impl Write) -> io::Result<()> {
    run_not_fullscreen_without_animation(output)
}

pub fn run_not_fullscreen_without_animation(output: &mut impl Write) -> io::Result<()> {
    run_not_fullscreen_without_animation_with_loader(output, load_validated_runtime_ascii_assets)
}

fn run_not_fullscreen_without_animation_with_loader(
    output: &mut impl Write,
    load_assets: impl FnOnce() -> io::Result<tundra_ui::RuntimeAsciiAssets>,
) -> io::Result<()> {
    let ascii_assets = load_assets()?;
    render_static_banner_with_assets(output, &ascii_assets)?;
    write_smoke_loop_message(output)
}

pub fn run_with_banner_animation(output: &mut impl Write) -> io::Result<()> {
    run_not_fullscreen(
        output,
        ShellLaunchConfig {
            terminal_mode: ShellTerminalMode::NotFullscreen,
            ..ShellLaunchConfig::default()
        },
    )
}

pub fn run_not_fullscreen(output: &mut impl Write, _config: ShellLaunchConfig) -> io::Result<()> {
    run_not_fullscreen_with_loader(output, load_validated_runtime_ascii_assets)
}

fn run_not_fullscreen_with_loader(
    output: &mut impl Write,
    load_assets: impl FnOnce() -> io::Result<tundra_ui::RuntimeAsciiAssets>,
) -> io::Result<()> {
    let ascii_assets = load_assets()?;
    run_not_fullscreen_with_assets(output, &ascii_assets)
}

fn run_not_fullscreen_with_assets(
    output: &mut impl Write,
    ascii_assets: &tundra_ui::RuntimeAsciiAssets,
) -> io::Result<()> {
    display_animated_banner_with_assets(output, BANNER_DISPLAY_DURATION, ascii_assets)?;
    write_smoke_loop_message(output)
}

pub fn run_shell_blocking(output: &mut impl Write, config: ShellLaunchConfig) -> io::Result<()> {
    let process = ProcessWatchdog::global().ok_or_else(|| {
        io::Error::other("the process watchdog must be installed before starting tundra-shell")
    })?;
    run_shell_blocking_managed(output, config, process)
}

pub fn run_shell_blocking_managed(
    output: &mut impl Write,
    config: ShellLaunchConfig,
    process: ProcessWatchdog,
) -> io::Result<()> {
    match config.terminal_mode {
        ShellTerminalMode::Fullscreen => run_fullscreen_blocking_managed(output, config, process),
        ShellTerminalMode::NotFullscreen => {
            let shell = process
                .register_app(shell_watchdog_descriptor())
                .map_err(io::Error::other)?;
            match shell.run_boundary(
                BoundarySpec::new("shell.not-fullscreen", BoundaryKind::UiSession),
                AssertUnwindSafe(|| run_not_fullscreen(output, config)),
            ) {
                Ok(result) => result,
                Err(caught) => {
                    let reason = caught.payload().to_string();
                    let _ = caught.finalize(RecoveryOutcome::Unrecoverable(
                        "the non-fullscreen shell session cannot be reconstructed".to_string(),
                    ));
                    Err(io::Error::other(format!("shell panicked: {reason}")))
                }
            }
        }
    }
}

pub fn run_fullscreen_once_without_animation(output: &mut impl Write) -> io::Result<()> {
    run_fullscreen_once_without_animation_with_loader(output, load_validated_runtime_ascii_assets)
}

fn run_fullscreen_once_without_animation_with_loader(
    output: &mut impl Write,
    load_assets: impl FnOnce() -> io::Result<tundra_ui::RuntimeAsciiAssets>,
) -> io::Result<()> {
    let ascii_assets = load_assets()?;
    with_fullscreen(output, |output| {
        render_static_banner_with_assets(output, &ascii_assets)?;
        write_smoke_loop_message(output)
    })
}

pub fn run_fullscreen_blocking(
    output: &mut impl Write,
    config: ShellLaunchConfig,
) -> io::Result<()> {
    let process = ProcessWatchdog::global().ok_or_else(|| {
        io::Error::other("the process watchdog must be installed before starting tundra-shell")
    })?;
    run_fullscreen_blocking_managed(output, config, process)
}

pub fn run_fullscreen_blocking_managed(
    output: &mut impl Write,
    config: ShellLaunchConfig,
    process: ProcessWatchdog,
) -> io::Result<()> {
    let ascii_assets = load_validated_runtime_ascii_assets()?;
    let terminal_size_requirement = ShellTerminalSizeRequirement::from_assets(&ascii_assets);
    let platform = tundra_platform::native_platform();
    let shell_watchdog = process
        .register_app(shell_watchdog_descriptor())
        .map_err(io::Error::other)?;
    let weathr_watchdog = process
        .register_app(tundra_weathr::weathr_watchdog_descriptor())
        .map_err(io::Error::other)?;
    let diagnostics_watchdog = process
        .register_app(tundra_apps::diagnostics::diagnostics_watchdog_descriptor())
        .map_err(io::Error::other)?;
    let (time_sync_sender, time_sync_receiver) = mpsc::channel();
    let time_sync_watchdog = shell_watchdog.child_component(ComponentId::from_static("time-sync"));
    let _time_sync_worker =
        spawn_time_sync_worker(time_sync_sender, &time_sync_watchdog).map_err(io::Error::other)?;
    let mut cached_time_sync = None;
    let mut force_lockscreen = false;
    let mut session_recoveries = VecDeque::new();
    let mut explorer_task_runtime: Option<ShellExplorerTaskRuntime> = None;
    let mut diagnostics_task_runtime: Option<ShellDiagnosticsTaskRuntime> = None;

    loop {
        let mut startup =
            prepare_shell_startup(platform.as_ref(), config).map_err(io::Error::other)?;
        if explorer_task_runtime.is_none()
            && let Some(storage) = startup.storage_manager.as_ref()
        {
            let explorer_watchdog = process
                .register_app(tundra_apps::explorer_tasks::explorer_watchdog_descriptor())
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
                    tundra_weathr::run_shell_lockscreen_managed(
                        lockscreen_options,
                        lockscreen_watchdog,
                    )
                }),
            );
            match lockscreen_result {
                Ok(Ok(tundra_weathr::ShellLockscreenResult::Started)) => {}
                Ok(Ok(tundra_weathr::ShellLockscreenResult::Cancelled)) => return Ok(()),
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
            startup = prepare_shell_startup(platform.as_ref(), config).map_err(io::Error::other)?;
        }

        let session_result = shell_watchdog.run_boundary(
            BoundarySpec::new("shell.fullscreen-session", BoundaryKind::UiSession).terminal_owner(),
            AssertUnwindSafe(|| {
                run_fullscreen_shell_session(
                    output,
                    config,
                    startup,
                    ascii_assets.clone(),
                    platform.as_ref(),
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
enum FullscreenShellSessionOutcome {
    Exit,
    ReturnToLockscreen,
}

#[derive(Debug, Clone)]
enum CachedTimeSyncResult {
    Success {
        utc: DateTime<Utc>,
        received_at: Instant,
    },
    Failure,
}

#[derive(Debug)]
struct TimedTimeSyncResult {
    result: TimeSyncResult,
    received_at: Instant,
}

impl CachedTimeSyncResult {
    fn apply_to_state_at(&self, state: &mut ShellState, now: Instant) {
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

fn run_fullscreen_shell_session(
    output: &mut impl Write,
    config: ShellLaunchConfig,
    startup: ShellStartupState,
    ascii_assets: tundra_ui::RuntimeAsciiAssets,
    platform: &dyn Platform,
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
    let theme =
        tundra_ui::TundraTheme::default_dark().with_border_shape(startup.app_config.border_shape);
    let mut state = ShellState::new_with_runtime_services(
        config,
        initial_size,
        startup,
        ascii_assets,
        explorer_task_runtime,
        diagnostics_task_runtime,
        ShellEditorTaskRuntime::new_managed(shell_watchdog.clone()),
    );
    if let Some(cached) = cached_time_sync.as_ref() {
        cached.apply_to_state_at(&mut state, Instant::now());
    }
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
        let clock_snapshot = state.network_clock.snapshot();
        state.advance_clock_background_at(&clock_snapshot, frame_now);
        let terminal_cell_aspect_ratio = crossterm::terminal::window_size()
            .map(|window| {
                tundra_ui::TerminalCellAspectRatio::from_window_size(
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
        let editor = (content_screen == ShellScreen::Editor).then(|| state.to_editor_view_model());
        let diagnostics = state.to_diagnostics_view_model();
        let notification = state.to_notification_view_model();
        let exit_confirmation = tundra_ui::ExitConfirmViewModel::new();

        guard.terminal_mut().draw(|frame| {
            let area = frame.area();
            match content_screen {
                ShellScreen::FirstRunSetup => {
                    tundra_ui::render_setup(frame, area, &chrome, &setup, &theme);
                }
                ShellScreen::Login => {
                    tundra_ui::render_login(frame, area, &chrome, &login, &theme);
                }
                ShellScreen::BootstrapAdmin => {
                    tundra_ui::render_bootstrap_admin(
                        frame,
                        area,
                        &chrome,
                        &bootstrap_admin,
                        &theme,
                    );
                }
                ShellScreen::UserManagement => {
                    tundra_ui::render_user_management(
                        frame,
                        area,
                        &chrome,
                        &user_management,
                        &theme,
                    );
                }
                ShellScreen::Explorer => {
                    tundra_ui::render_explorer(frame, area, &chrome, &explorer, &theme);
                }
                ShellScreen::Editor => {
                    tundra_ui::render_editor_app(
                        frame,
                        area,
                        &chrome,
                        editor
                            .as_ref()
                            .expect("Editor content requires its view model"),
                        &theme,
                    );
                }
                ShellScreen::Diagnostics => {
                    tundra_ui::render_diagnostics(frame, area, &chrome, &diagnostics, &theme);
                }
                ShellScreen::Clock => {
                    tundra_ui::render_clock(frame, area, &chrome, &clock, &theme);
                }
                ShellScreen::Home | ShellScreen::ExitConfirm => {
                    tundra_ui::render_home(frame, area, &chrome, &home, &theme);
                }
            }

            if notification.is_none() && active_screen == ShellScreen::ExitConfirm {
                tundra_ui::render_exit_confirmation(frame, area, &exit_confirmation, &theme);
            }
            if notification.is_none()
                && let Some(dialog) = time_sync_dialog.as_ref()
            {
                tundra_ui::render_time_sync_failure_dialog(frame, area, dialog, &theme);
            }
            if let Some(notification) = notification.as_ref() {
                tundra_ui::render_notification_overlay(frame, area, notification, &theme);
            }
        })?;

        if terminal_control.shutdown_requested() {
            state.apply_input_with_platform(InputEvent::Shutdown, platform);
        }
        if state.shutdown_requested() {
            break;
        }

        let poll_now = Instant::now();
        let poll_timeout = state.auth_poll_timeout(
            poll_now,
            state.notifications.poll_timeout(poll_now, tick_rate),
        );
        let action = if event::poll(poll_timeout)? {
            let terminal_event = event::read()?;
            if let event::Event::Resize(width, height) = terminal_event
                && let Err(error) = terminal_size_requirement.validate((width, height))
            {
                terminal_size_error = Some(io::Error::other(error));
                break;
            }
            state.apply_input_with_platform(crossterm_event_to_input(terminal_event), platform)
        } else {
            state.apply_input_with_platform(InputEvent::Tick, platform)
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

const SESSION_RECOVERY_WINDOW: Duration = Duration::from_secs(60);
const MAX_SESSION_RECOVERIES: usize = 2;

fn reserve_session_recovery(recoveries: &mut VecDeque<Instant>, now: Instant) -> bool {
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

fn recover_session_panic(
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

fn load_validated_runtime_ascii_assets() -> io::Result<tundra_ui::RuntimeAsciiAssets> {
    let ascii_assets = tundra_ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    checked_current_terminal_size(ShellTerminalSizeRequirement::from_assets(&ascii_assets))?;
    Ok(ascii_assets)
}

#[cfg(test)]
mod runtime_preflight_tests {
    use super::*;

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
}

fn spawn_time_sync_worker(
    sender: mpsc::Sender<TimedTimeSyncResult>,
    watchdog: &AppWatchdog,
) -> Result<ManagedThreadHandle<()>, tundra_watchdog::WatchdogError> {
    let group = watchdog.task_group("network-clock");
    group.spawn_thread(
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

            runtime.block_on(async move {
                loop {
                    let result = tundra_weathr::network_clock::fetch_standard_time().await;
                    if sender
                        .send(TimedTimeSyncResult {
                            result,
                            received_at: Instant::now(),
                        })
                        .is_err()
                    {
                        break;
                    }
                    tokio::time::sleep(TIME_SYNC_INTERVAL).await;
                }
            });
        },
    )
}

fn shell_watchdog_descriptor() -> AppDescriptor {
    AppDescriptor::new(
        AppId::from_static("shell"),
        "Tundra Shell",
        env!("CARGO_PKG_VERSION"),
        AppCriticality::ProcessCritical,
    )
}

fn drain_watchdog_incidents(state: &mut ShellState, watchdog: &ProcessWatchdog) {
    for incident in watchdog.drain_incidents() {
        show_watchdog_incident(state, incident);
    }
}

fn show_watchdog_incident(state: &mut ShellState, incident: IncidentReceipt) {
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
    if state.diagnostics_snapshot.is_some() && !state.diagnostics_restart_is_required() {
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

fn drain_time_sync_results(
    state: &mut ShellState,
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

fn apply_timed_time_sync_result_at(
    state: &mut ShellState,
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

fn with_fullscreen<W, T>(
    output: &mut W,
    body: impl FnOnce(&mut W) -> io::Result<T>,
) -> io::Result<T>
where
    W: Write,
{
    tundra_platform::with_terminal_fullscreen(output, body)
}

fn write_smoke_loop_message(output: &mut impl Write) -> io::Result<()> {
    for line in startup_lines() {
        writeln!(output, "{line}")?;
    }
    writeln!(output, "Entering smoke loop")
}
