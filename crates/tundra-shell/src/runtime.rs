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

pub fn run_shell_blocking(
    output: &mut impl Write,
    config: ShellLaunchConfig,
) -> io::Result<()> {
    match config.terminal_mode {
        ShellTerminalMode::Fullscreen => run_fullscreen_blocking(output, config),
        ShellTerminalMode::NotFullscreen => run_not_fullscreen(output, config),
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
    let ascii_assets = load_validated_runtime_ascii_assets()?;
    let terminal_size_requirement = ShellTerminalSizeRequirement::from_assets(&ascii_assets);
    let platform = tundra_platform::native_platform();
    install_panic_restore_hook();
    let (time_sync_sender, time_sync_receiver) = mpsc::channel();
    let _time_sync_worker = spawn_time_sync_worker(time_sync_sender);
    let mut cached_time_sync = None;
    let mut force_lockscreen = false;

    loop {
        let mut startup =
            prepare_shell_startup(platform.as_ref(), config).map_err(io::Error::other)?;
        if force_lockscreen || should_show_startup_lockscreen(&startup) {
            let lockscreen_options =
                startup_lockscreen_launch_options(&startup, terminal_size_requirement);
            match tundra_weathr::run_shell_lockscreen_blocking_with_options(lockscreen_options)
                .map_err(io::Error::other)?
            {
                tundra_weathr::ShellLockscreenResult::Started => {}
                tundra_weathr::ShellLockscreenResult::Cancelled => return Ok(()),
            }
            startup = prepare_shell_startup(platform.as_ref(), config).map_err(io::Error::other)?;
        }

        match run_fullscreen_shell_session(
            output,
            config,
            startup,
            ascii_assets.clone(),
            platform.as_ref(),
            &time_sync_receiver,
            &mut cached_time_sync,
        )? {
            FullscreenShellSessionOutcome::Exit => return Ok(()),
            FullscreenShellSessionOutcome::ReturnToLockscreen => {
                force_lockscreen = true;
            }
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
) -> io::Result<FullscreenShellSessionOutcome> {
    let terminal_size_requirement = ShellTerminalSizeRequirement::from_assets(&ascii_assets);
    let initial_size = checked_current_terminal_size(terminal_size_requirement)?;
    let terminal_control = TerminalControlHandler::install();
    let mut guard = TerminalGuard::enter(output)?;
    let mut state =
        ShellState::new_with_startup_and_assets(config, initial_size, startup, ascii_assets);
    if let Some(cached) = cached_time_sync.as_ref() {
        cached.apply_to_state_at(&mut state, Instant::now());
    }
    let tick_rate = Duration::from_millis(250);
    let theme = tundra_ui::TundraTheme::default_dark();
    let mut terminal_size_error = None;

    loop {
        if let Err(error) = terminal_size_requirement.validate(crossterm::terminal::size()?) {
            terminal_size_error = Some(io::Error::other(error));
            break;
        }

        drain_time_sync_results(&mut state, time_sync_receiver, cached_time_sync);
        let frame_now = Instant::now();
        let clock_snapshot = state.network_clock.snapshot();
        state.advance_clock_background_at(&clock_snapshot, frame_now);
        let chrome = state.to_shell_chrome_view_model();
        let home = state.to_home_view_model();
        let clock = state.to_clock_view_model_at(&clock_snapshot, frame_now);
        let time_sync_dialog = state.to_time_sync_dialog_view_model();
        let setup = state.to_setup_view_model();
        let login = state.to_login_view_model_at(frame_now);
        let bootstrap_admin = state.to_bootstrap_admin_view_model();
        let user_management = state.to_user_management_view_model();
        let explorer = state.to_explorer_view_model();
        let notification = state.to_notification_view_model();
        let active_screen = state.active_screen();
        let content_screen = state.content_screen();
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
    }

    guard.restore()?;
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
            run_fullscreen_once_without_animation_with_loader(&mut fullscreen_output, fail).is_err()
        );
        assert!(fullscreen_output.is_empty());
    }
}

fn spawn_time_sync_worker(sender: mpsc::Sender<TimedTimeSyncResult>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
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
    })
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
