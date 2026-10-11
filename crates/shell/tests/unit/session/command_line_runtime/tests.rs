use super::*;
use crate::{InputModifiers, KeyInput};
use std::ffi::OsString;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
static NEXT_PTY_READER_TASK_ID: AtomicU64 = AtomicU64::new(1);
#[cfg(any(windows, unix))]
#[test]
fn embedded_panic_exit_enters_fullscreen_panic_without_login_or_dialog() {
    use watchdog::{
        AppCriticality, AppDescriptor, AppId, BoundaryKind, BoundarySpec, WatchdogConfig,
        WatchdogRuntime,
    };
    let root = std::env::temp_dir().join(format!(
        "tundra-command-line-panic-test-{}-{}",
        std::process::id(),
        NEXT_PTY_READER_TASK_ID.fetch_add(1, Ordering::Relaxed),
    ));
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("data"),
        "shell-panic-test",
        "test",
    ))
    .unwrap();
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("shell"),
            "Shell",
            "test",
            AppCriticality::ProcessCritical,
        ))
        .unwrap();
    let mut host = CommandLineHost::new(app.clone());
    #[cfg(windows)]
    let (program, args) = ("cmd.exe", vec!["/D", "/C", "exit", "76"]);
    #[cfg(unix)]
    let (program, args) = ("/bin/sh", vec!["-c", "exit 76"]);
    let pty = CommandLinePty::spawn(
        CommandLinePtyConfig {
            program: program.into(),
            args: args.into_iter().map(OsString::from).collect(),
            env: Vec::new(),
            cwd: Some(root.clone()),
            columns: DEFAULT_COLUMNS,
            rows: DEFAULT_ROWS,
            scrollback_lines: 100,
        },
        &host.reader_tasks,
    )
    .unwrap();
    host.state = CommandLineHostState::Running(pty);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        match host.poll() {
            CommandLineHostEvent::PanicRequested => break,
            CommandLineHostEvent::None => {
                assert!(
                    std::time::Instant::now() < deadline,
                    "panic request never reached Shell"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            event => panic!("unexpected host event: {event:?}"),
        }
    }
    assert!(matches!(host.state, CommandLineHostState::Inactive));
    let caught = app
        .run_boundary(
            BoundarySpec::new("shell.fullscreen-session", BoundaryKind::UiSession),
            crate::session::runtime::trigger_command_line_panic,
        )
        .expect_err("the Shell must really unwind");
    let id = caught.incident_id().to_string();
    let message = crate::session::runtime::finalize_session_panic(caught, "Shell UI");
    assert_eq!(
        message,
        "Shell UI: Intentional watchdog panic test requested from Command Line"
    );
    let mut state = crate::ShellSession::new(crate::ShellLaunchConfig::default(), (120, 40));
    let screen_before = state.active_screen();
    let message = crate::session::runtime::drain_watchdog_incidents(&mut state, &process)
        .expect("panic must request a standalone crash page even without login");
    assert!(message.contains("Intentional watchdog panic"));
    assert!(!message.contains(&id));
    assert!(!message.contains("Report:"));
    assert!(!message.contains("Recovery:"));
    assert!(state.to_notification_view_model().is_none());
    assert_eq!(state.active_screen(), screen_before);
    let catalog = process.list_incident_reports();
    let report = catalog
        .reports
        .iter()
        .find(|report| report.incident_id == id)
        .unwrap();
    assert_eq!(report.kind, watchdog::IncidentKind::Panic);
    assert_eq!(report.boundary, "shell.fullscreen-session");
    assert!(report.summary.contains("Intentional watchdog panic"));
    assert!(report.text_report_path.as_ref().unwrap().is_file());
    assert!(matches!(
        report.recovery,
        watchdog::RecoveryOutcome::Unrecoverable(_)
    ));
    // Background panic receipts must bypass account-gated notifications too.
    for role in [
        None,
        Some(identity::UserRole::User),
        Some(identity::UserRole::Admin),
    ] {
        let mut state = crate::ShellSession::new(crate::ShellLaunchConfig::default(), (120, 40));
        if let Some(role) = role {
            state.app.dispatch_at(
                app::AppCommand::SetAuthSession(Some(identity::AuthSession {
                    source: identity::IdentitySource::LocalAccount,
                    session_id: "panic-test-session".into(),
                    user_id: "panic-test-user".into(),
                    username: "panic-test".into(),
                    role,
                    started_at_epoch_ms: 1,
                })),
                std::time::Instant::now(),
            );
        }
        let screen_before = state.active_screen();
        let caught = app
            .run_boundary(
                BoundarySpec::new("background-task", BoundaryKind::Worker),
                || panic!("background worker failed to read a file"),
            )
            .expect_err("background task panic");
        caught
            .finalize(watchdog::RecoveryOutcome::Recovered(
                "worker restarted".into(),
            ))
            .unwrap();
        let message = crate::session::runtime::drain_watchdog_incidents(&mut state, &process)
            .expect("every account must see background panic details");
        assert!(message.contains("background worker failed to read a file"));
        assert!(!message.contains("Incident:"));
        assert!(!message.contains("Report:"));
        assert!(!message.contains("Recovery:"));
        assert!(state.to_notification_view_model().is_none());
        assert_eq!(state.active_screen(), screen_before);
    }
    drop(host);
    runtime.shutdown().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn scrollbar_drag_maps_both_track_ends_to_history_ends() {
    let scrollbar = ui::CommandLineScrollbarLayout {
        track: Rect::new(12, 5, 1, 10),
        thumb: Rect::new(12, 9, 1, 2),
    };
    assert_eq!(
        command_line_scrollback_offset_for_thumb(100, scrollbar, 5, 0),
        100
    );
    assert_eq!(
        command_line_scrollback_offset_for_thumb(100, scrollbar, 14, 1),
        0
    );
}

#[test]
fn failed_and_exited_terminals_ignore_escape_and_old_emergency_shortcut() {
    let fixture = PtyFixture::new("failed-input");
    let mut host = CommandLineHost::new(fixture.app.clone());
    for state in [
        CommandLineHostState::Failed {
            message: "test failure".into(),
        },
        CommandLineHostState::Exited { code: 3 },
    ] {
        host.state = state;
        for input in [
            InputEvent::key(InputKey::Escape),
            InputEvent::Key(KeyInput::with_modifiers(
                InputKey::Char('x'),
                InputModifiers::CTRL_SHIFT,
            )),
        ] {
            assert_eq!(host.handle_input(&input, None), CommandLineHostEvent::None);
            assert!(!matches!(host.state, CommandLineHostState::Inactive));
        }
        assert_eq!(
            host.handle_input(&InputEvent::key(InputKey::Enter), None),
            CommandLineHostEvent::None
        );
        assert!(matches!(host.state, CommandLineHostState::Inactive));
    }
}

/// Windows ConPTY is the production backend for the embedded CLI.  A
/// child merely staying alive does not prove that its pseudo console is
/// usable: a broken reader or writer leaves the Shell with an apparently
/// running, but completely blank, Command Line screen.  Keep this test
/// interactive and bounded so it verifies the two directions separately.
#[cfg(windows)]
#[test]
fn windows_conpty_receives_initial_and_typed_output() {
    use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

    let root = std::env::temp_dir().join(format!(
        "tundra-command-line-conpty-io-test-{}-{}",
        std::process::id(),
        NEXT_PTY_READER_TASK_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let config = WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("data"),
        "command-line-conpty-io-test",
        env!("CARGO_PKG_VERSION"),
    );
    let (runtime, process) = WatchdogRuntime::start_isolated(config).expect("isolated watchdog");
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("shell.command-line-conpty-io-test"),
            "Command Line ConPTY I/O Test",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::Optional,
        ))
        .expect("test app watchdog");
    let reader_tasks = app.task_group("pty-reader");
    let pty = CommandLinePty::spawn(
        CommandLinePtyConfig {
            program: OsString::from("cmd.exe"),
            args: vec![
                OsString::from("/D"),
                OsString::from("/Q"),
                OsString::from("/K"),
                OsString::from("echo TUNDRA_PTY_INITIAL_OUTPUT_OK"),
            ],
            env: Vec::new(),
            cwd: None,
            columns: 80,
            rows: 12,
            scrollback_lines: 100,
        },
        &reader_tasks,
    )
    .expect("interactive ConPTY child");
    let (snapshot, snapshot_revision) = pty.snapshot_with_revision();
    let ui_snapshot = Arc::new(to_ui_snapshot(&snapshot));
    let mut host = CommandLineHost {
        state: CommandLineHostState::Running(pty),
        snapshot,
        snapshot_revision,
        ui_snapshot,
        scrollbar_drag_offset: None,
        reader_tasks,
    };

    // The smallest valid outer Shell (108x22) leaves a 106x14 inner
    // terminal.  This guards against accidentally applying the outer
    // minimum to the already-inset PTY dimensions.
    host.resize_terminal(106, 14);
    assert_eq!((host.snapshot.columns, host.snapshot.rows), (106, 14));
    let pty = match &host.state {
        CommandLineHostState::Running(pty) => pty,
        state => panic!("expected running ConPTY host, got {state:?}"),
    };

    assert_snapshot_contains(
        pty,
        "TUNDRA_PTY_INITIAL_OUTPUT_OK",
        Duration::from_secs(5),
        "initial child output",
    );
    pty.write(b"echo TUNDRA_PTY_TYPED_INPUT_OK\r")
        .expect("write command to ConPTY");
    assert_snapshot_contains(
        pty,
        "TUNDRA_PTY_TYPED_INPUT_OK",
        Duration::from_secs(5),
        "output from typed command",
    );
    const UTF8_MARKER: &str = "TUNDRA_PTY_UTF8_保留所有权利_OK";
    pty.write(format!("echo {UTF8_MARKER}\r").as_bytes())
        .expect("write UTF-8 command to ConPTY");
    assert_snapshot_contains(
        pty,
        UTF8_MARKER,
        Duration::from_secs(5),
        "UTF-8 output from typed command",
    );

    host.terminate();
    drop(host);
    drop(app);
    drop(process);
    runtime.shutdown().expect("watchdog shutdown");
    let _ = std::fs::remove_dir_all(root);
}

fn assert_snapshot_contains(
    pty: &CommandLinePty,
    expected: &str,
    timeout: Duration,
    description: &str,
) {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let snapshot = pty.snapshot();
        let screen = snapshot
            .cells
            .iter()
            .flat_map(|row| row.iter())
            .map(|cell| cell.text.as_str())
            .collect::<String>();
        if screen.contains(expected) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "PTY did not render {description}; last screen: {screen:?}",
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(any(windows, unix))]
#[test]
fn physical_escape_and_ctrl_shift_x_reach_the_real_child_terminal() {
    let fixture = PtyFixture::new("physical-key-bytes");
    #[cfg(windows)]
    let (program, args) = {
        let script = fixture.root.join("read-keys.ps1");
        std::fs::write(
            &script,
            "[Console]::WriteLine('READY_FOR_KEYS')\n$first = [Console]::ReadKey($true)\n$second = [Console]::ReadKey($true)\n[Console]::WriteLine(('KEY_BYTES:{0:x2} {1:x2}' -f [int]$first.KeyChar, [int]$second.KeyChar))\nStart-Sleep -Seconds 60\n",
        ).unwrap();
        (
            OsString::from("powershell.exe"),
            vec![
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                script.into_os_string(),
            ],
        )
    };
    #[cfg(unix)]
    let (program, args) = (
        OsString::from("/bin/sh"),
        vec!["-c".into(), "stty raw -echo; printf 'READY_FOR_KEYS\\r\\n'; bytes=$(dd bs=1 count=2 2>/dev/null | od -An -tx1); printf 'KEY_BYTES:%s\\r\\n' \"$bytes\"; sleep 60".into()],
    );
    let mut host = CommandLineHost::new(fixture.app.clone());
    host.state = CommandLineHostState::Running(
        CommandLinePty::spawn(
            CommandLinePtyConfig {
                program,
                args,
                env: Vec::new(),
                cwd: Some(fixture.root.clone()),
                columns: 160,
                rows: 24,
                scrollback_lines: 100,
            },
            &host.reader_tasks,
        )
        .unwrap(),
    );
    assert_snapshot_contains(
        running_pty(&host),
        "READY_FOR_KEYS",
        Duration::from_secs(8),
        "raw terminal input reader",
    );
    for input in [
        InputEvent::key(InputKey::Escape),
        InputEvent::Key(KeyInput::with_modifiers(
            InputKey::Char('x'),
            InputModifiers::CTRL_SHIFT,
        )),
    ] {
        assert_eq!(
            host.handle_input(&input, Some(Rect::new(0, 0, 160, 24))),
            CommandLineHostEvent::None
        );
    }
    assert_snapshot_contains(
        running_pty(&host),
        "1b 18",
        Duration::from_secs(5),
        "Escape and Ctrl+Shift+X bytes echoed by the child",
    );
    assert!(host.is_running());
}

#[cfg(target_os = "linux")]
#[test]
fn terminal_close_also_stops_descendants_that_create_their_own_session() {
    let fixture = PtyFixture::new("descendant-session");
    let mut host = fixture.shell();
    let root_pid = running_pty(&host).process_id().unwrap().unwrap();
    let script = fixture.root.join("new-session.sh");
    let pid_file = fixture.root.join("new-session-child.pid");
    std::fs::write(
        &script,
        format!("sleep 60 &\necho $! > '{}'\nwait\n", pid_file.display()),
    )
    .unwrap();
    running_pty(&host)
        .write(format!("setsid --wait /bin/sh '{}' & wait\r", script.display()).as_bytes())
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let descendant_pid = loop {
        if let Ok(contents) = std::fs::read_to_string(&pid_file)
            && let Ok(pid) = contents.trim().parse::<u32>()
        {
            break pid;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "new-session child did not start"
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(process_is_running(descendant_pid));
    host.try_terminate().unwrap();
    wait_until_stopped(root_pid);
    wait_until_stopped(descendant_pid);
}

#[cfg(any(windows, unix))]
#[test]
fn shell_back_button_requires_confirmation_and_keeps_other_terminal_alive() {
    let fixture = PtyFixture::new("close-confirmation");
    let platform = platform::native_platform();
    let mut hosts = CommandLineHosts::new(fixture.app.clone());
    let mut state = crate::ShellSession::new(crate::ShellLaunchConfig::default(), (120, 40));
    state.set_navigation_path(vec![
        crate::ShellScreen::Home,
        crate::ShellScreen::CommandLine,
    ]);
    let first_id = state.active_page_id();
    let first = fixture.shell();
    let first_pid = running_pty(&first).process_id().unwrap().unwrap();
    hosts.hosts.insert(first_id, first);
    state.enter_screen(crate::ShellScreen::Clock);
    let caller_id = state.active_page_id();
    state.enter_screen(crate::ShellScreen::CommandLine);
    let second_id = state.active_page_id();
    let second = fixture.shell();
    let second_pid = running_pty(&second).process_id().unwrap().unwrap();
    let descendant_pid = fixture.start_descendant(&second);
    hosts.hosts.insert(second_id, second);

    hosts.sync_tasks(&mut state);
    for id in [first_id, second_id] {
        assert_eq!(
            state.tasks.owner(
                crate::session::TaskDomain::CommandLine,
                hosts.tasks[&id].request
            ),
            Some(crate::session::TaskOwner::Page(id))
        );
    }

    // Physical terminal keys never act as Shell navigation, including the
    // formerly reserved Ctrl+Shift+X chord.
    for input in [
        InputEvent::key(InputKey::Escape),
        InputEvent::Key(KeyInput::with_modifiers(
            InputKey::Char('x'),
            InputModifiers::CTRL_SHIFT,
        )),
    ] {
        assert_eq!(
            hosts.handle_input(&state, &input, Some(Rect::new(0, 0, 120, 40))),
            CommandLineHostEvent::None
        );
    }
    assert!(process_is_running(first_pid));
    assert!(process_is_running(second_pid));
    assert!(process_is_running(descendant_pid));

    state.refresh_hit_map();
    let back = state
        .hit_map()
        .regions()
        .iter()
        .find(|region| region.component == crate::ShellComponent::BackButton)
        .unwrap()
        .area;
    state.button_regions.push(ui::components::ButtonRegion {
        id: "shell.back".into(),
        area: back,
        disabled: false,
    });
    let now = std::time::Instant::now();
    state.apply_input_with_platform_at(
        InputEvent::mouse_down(ui::MouseButton::Left, (back.x, back.y)),
        platform.as_ref(),
        now,
    );
    state.apply_input_with_platform_at(
        InputEvent::mouse_up(ui::MouseButton::Left, (back.x, back.y)),
        platform.as_ref(),
        now + Duration::from_millis(20),
    );
    assert_eq!(state.command_line_close_request, Some((second_id, false)));
    hosts.process_close_request(&mut state);
    let modal = state
        .to_notification_view_model()
        .expect("close confirmation");
    assert_eq!(modal.title, "Close Command Line?");
    assert_eq!(modal.actions.len(), 2);
    assert_eq!(modal.actions[0].id, "continue-command-line");
    assert_eq!(modal.actions[0].label, "Continue using");
    assert_eq!(modal.actions[1].label, "Terminate and return");
    assert!(process_is_running(second_pid));
    assert!(process_is_running(descendant_pid));

    state.apply_input_with_platform(InputEvent::key(InputKey::Escape), platform.as_ref());
    assert!(state.to_notification_view_model().is_none());
    assert_eq!(state.active_page_id(), second_id);
    assert!(process_is_running(second_pid));
    state.request_command_line_close();
    hosts.process_close_request(&mut state);
    state.apply_input_with_platform(InputEvent::key(InputKey::Right), platform.as_ref());
    state.apply_input_with_platform(InputEvent::key(InputKey::Enter), platform.as_ref());
    assert_eq!(state.command_line_close_request, Some((second_id, true)));
    hosts.process_close_request(&mut state);
    assert_eq!(state.active_page_id(), caller_id);
    hosts.sync_tasks(&mut state);
    assert!(!hosts.tasks.contains_key(&second_id));
    assert!(hosts.tasks.contains_key(&first_id));
    assert!(!hosts.hosts.contains_key(&second_id));
    wait_until_stopped(second_pid);
    wait_until_stopped(descendant_pid);
    assert!(
        process_is_running(first_pid),
        "closing the top terminal must preserve the covered terminal"
    );
    hosts.terminate_all().unwrap();
    wait_until_stopped(first_pid);
}

#[cfg(any(windows, unix))]
#[test]
fn covered_terminal_output_input_and_completion_are_bound_to_page_ids() {
    let fixture = PtyFixture::new("page-instances");
    let platform = platform::native_platform();
    let mut hosts = CommandLineHosts::new(fixture.app.clone());
    let mut state = crate::ShellSession::new(crate::ShellLaunchConfig::default(), (120, 40));
    state.set_navigation_path(vec![
        crate::ShellScreen::Home,
        crate::ShellScreen::CommandLine,
    ]);
    let first_id = state.active_page_id();
    hosts.hosts.insert(first_id, fixture.shell());
    state.enter_screen(crate::ShellScreen::Clock);
    state.enter_screen(crate::ShellScreen::CommandLine);
    let second_id = state.active_page_id();
    assert_ne!(first_id, second_id);
    hosts.hosts.insert(second_id, fixture.shell());
    let second_pid = running_pty(&hosts.hosts[&second_id])
        .process_id()
        .unwrap()
        .unwrap();
    running_pty(&hosts.hosts[&first_id])
        .write(b"echo FIRST_BACKGROUND_OUTPUT\r")
        .unwrap();
    hosts.handle_input(
        &state,
        &InputEvent::paste("echo SECOND_ONLY_INPUT\r"),
        Some(Rect::new(0, 0, 120, 40)),
    );

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let (_, event) = hosts.poll(&mut state, platform.as_ref(), ratatui::style::Color::Cyan);
        assert_eq!(event, CommandLineHostEvent::None);
        let first_output = snapshot_text(&hosts.hosts[&first_id].snapshot);
        let second_output = snapshot_text(&hosts.hosts[&second_id].snapshot);
        if first_output.contains("FIRST_BACKGROUND_OUTPUT")
            && second_output.contains("SECOND_ONLY_INPUT")
        {
            assert!(!first_output.contains("SECOND_ONLY_INPUT"));
            assert!(!second_output.contains("FIRST_BACKGROUND_OUTPUT"));
            assert_eq!(
                hosts.visible_host(&state).view_model(),
                hosts.hosts[&second_id].view_model()
            );
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "independent terminal output never arrived"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    state.enter_screen(crate::ShellScreen::Clock);
    let top_id = state.active_page_id();
    running_pty(&hosts.hosts[&first_id])
        .write(b"exit 0\r")
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while hosts.hosts.contains_key(&first_id) {
        hosts.poll(&mut state, platform.as_ref(), ratatui::style::Color::Cyan);
        assert!(
            std::time::Instant::now() < deadline,
            "covered successful exit was not reaped"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!state.live_page_ids().contains(&first_id));
    assert_eq!(
        state.active_page_id(),
        top_id,
        "a covered exit must not close the visible page"
    );
    assert!(process_is_running(second_pid));

    state.close_page_instance(second_id);
    hosts.poll(&mut state, platform.as_ref(), ratatui::style::Color::Cyan);
    assert!(hosts.hosts.is_empty());
    wait_until_stopped(second_pid);
    assert_eq!(state.active_page_id(), top_id);
}

#[test]
fn nonrunning_terminal_returns_without_confirmation_and_stale_confirmation_is_ignored() {
    let fixture = PtyFixture::new("nonrunning-close");
    let mut hosts = CommandLineHosts::new(fixture.app.clone());
    let mut state = crate::ShellSession::new(crate::ShellLaunchConfig::default(), (120, 40));
    state.set_navigation_path(vec![
        crate::ShellScreen::Home,
        crate::ShellScreen::CommandLine,
    ]);
    let first_id = state.active_page_id();
    let mut failed = CommandLineHost::new(fixture.app.clone());
    failed.state = CommandLineHostState::Failed {
        message: "could not start test child".into(),
    };
    hosts.hosts.insert(first_id, failed);
    state.request_command_line_close();
    hosts.process_close_request(&mut state);
    assert_eq!(state.active_screen(), crate::ShellScreen::Home);
    assert!(state.to_notification_view_model().is_none());
    assert!(hosts.hosts.is_empty());

    state.enter_screen(crate::ShellScreen::CommandLine);
    let replacement_id = state.active_page_id();
    assert_ne!(first_id, replacement_id);
    state.confirm_command_line_close(first_id);
    assert!(state.command_line_close_request.is_none());
    state.notify_critical_modal("Existing modal", "Must close first", vec![]);
    state.request_command_line_close();
    assert!(state.command_line_close_request.is_none());
    assert_eq!(state.active_page_id(), replacement_id);
}

fn snapshot_text(snapshot: &TerminalSnapshot) -> String {
    snapshot
        .cells
        .iter()
        .flat_map(|row| row.iter())
        .map(|cell| cell.text.as_str())
        .collect()
}

#[test]
fn live_keyboard_types_into_child_pty_and_resize_reaches_the_child() {
    let fixture = PtyFixture::new("live-keyboard");
    let mut host = fixture.shell();
    let mut state = crate::ShellSession::new_for_home_mode(
        crate::ShellLaunchConfig::default(),
        (120, 40),
        crate::ShellHomeMode::User,
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.enter_screen(crate::ShellScreen::CommandLine);
    let original = state.command_line_input_area().unwrap();
    state.toggle_input_keyboard();
    state.advance_input_keyboard(ui::MotionFrame::reduced(std::time::Duration::ZERO));
    let area = state.command_line_input_area().unwrap();
    assert!(area.height < original.height);
    host.resize_to_area(area);
    let platform = platform::native_platform();
    let command = if cfg!(unix) {
        "echo KEYBOARDOK; stty size"
    } else {
        "echo KEYBOARDOK"
    };
    for ch in command.chars().chain(std::iter::once('\n')) {
        use ui::ScreenKeyboardAction as A;
        let action = match ch {
            '\n' => A::Enter,
            ' ' => A::Space,
            ch if ch.is_ascii_alphabetic() => A::Letter(ch.to_ascii_lowercase()),
            ch => A::Character(ch),
        };
        state.screen_keyboard.modifiers.shift = ch.is_ascii_uppercase();
        let layout = state.input_keyboard_layout();
        let key = layout
            .buttons
            .iter()
            .find(|button| button.action == action)
            .unwrap()
            .area;
        let point = (key.x, key.y);
        assert!(
            state
                .route_screen_keyboard_input(
                    InputEvent::mouse_down(ui::MouseButton::Left, point),
                    platform.as_ref(),
                    std::time::Instant::now()
                )
                .is_none()
        );
        let event = state
            .route_screen_keyboard_input(
                InputEvent::mouse_up(ui::MouseButton::Left, point),
                platform.as_ref(),
                std::time::Instant::now(),
            )
            .unwrap();
        host.handle_input(&event, Some(area));
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        host.poll();
        let output = host
            .view_model()
            .terminal
            .cells
            .iter()
            .map(|c| c.symbol.as_str())
            .collect::<String>();
        let marker_count = output.matches("KEYBOARDOK").count();
        let sized = !cfg!(unix) || output.contains(&format!("{} {}", area.height, area.width));
        if marker_count >= 2 && sized {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "live keyboard output/resize missing: {output}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    state.toggle_input_keyboard();
    state.advance_input_keyboard(ui::MotionFrame::reduced(std::time::Duration::ZERO));
    host.resize_to_area(state.command_line_input_area().unwrap());
    assert_eq!(state.command_line_input_area(), Some(original));
    host.try_terminate().unwrap();
}

struct PtyFixture {
    root: PathBuf,
    runtime: Option<watchdog::WatchdogRuntime>,
    app: watchdog::AppWatchdog,
}

impl PtyFixture {
    fn new(name: &str) -> Self {
        use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};
        let root = std::env::temp_dir().join(format!(
            "tundra-command-line-{name}-{}-{}",
            std::process::id(),
            NEXT_PTY_READER_TASK_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("data"),
            "command-line-test",
            "test",
        ))
        .unwrap();
        let app = process
            .register_app(AppDescriptor::new(
                AppId::from_static("command-line-test"),
                "Command Line Test",
                "test",
                AppCriticality::Optional,
            ))
            .unwrap();
        Self {
            root,
            runtime: Some(runtime),
            app,
        }
    }

    fn shell(&self) -> CommandLineHost {
        #[cfg(windows)]
        let (program, args) = ("cmd.exe", vec!["/D", "/Q"]);
        #[cfg(unix)]
        let (program, args) = ("/bin/sh", Vec::<&str>::new());
        let mut host = CommandLineHost::new(self.app.clone());
        let pty = CommandLinePty::spawn(
            CommandLinePtyConfig {
                program: program.into(),
                args: args.into_iter().map(OsString::from).collect(),
                env: Vec::new(),
                cwd: Some(self.root.clone()),
                columns: 160,
                rows: 24,
                scrollback_lines: 200,
            },
            &host.reader_tasks,
        )
        .unwrap();
        host.state = CommandLineHostState::Running(pty);
        host
    }

    fn start_descendant(&self, host: &CommandLineHost) -> u32 {
        let pid_file = self.root.join("descendant.pid");
        #[cfg(windows)]
        let command = {
            let script = self.root.join("descendant.ps1");
            std::fs::write(&script, format!("$child = Start-Process ping.exe -ArgumentList '-t','127.0.0.1' -NoNewWindow -PassThru\n$child.Id | Set-Content -LiteralPath '{}'\nWait-Process -Id $child.Id\n", pid_file.display())).unwrap();
            format!(
                "powershell.exe -NoProfile -ExecutionPolicy Bypass -File \"{}\"\r",
                script.display()
            )
        };
        #[cfg(unix)]
        let command = format!("sleep 60 & echo $! > '{}'; wait\r", pid_file.display());
        running_pty(host).write(command.as_bytes()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            if let Ok(contents) = std::fs::read_to_string(&pid_file)
                && let Ok(pid) = contents.trim().parse()
            {
                return pid;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "descendant did not publish its pid"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for PtyFixture {
    fn drop(&mut self) {
        self.runtime.take().unwrap().shutdown().unwrap();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn running_pty(host: &CommandLineHost) -> &CommandLinePty {
    match &host.state {
        CommandLineHostState::Running(pty) => pty,
        other => panic!("expected running PTY, got {other:?}"),
    }
}

fn process_is_running(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        && stat
            .rsplit_once(") ")
            .is_some_and(|(_, state)| state.starts_with('Z'))
    {
        return false;
    }
    platform::native_platform().is_process_alive(pid).unwrap()
}

fn wait_until_stopped(pid: u32) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while process_is_running(pid) {
        assert!(
            std::time::Instant::now() < deadline,
            "process {pid} survived terminal close"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
