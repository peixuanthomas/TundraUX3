#![cfg(unix)]

use super::*;

fn external_shell(script: &str, args: &[&str]) -> ExternalTerminalLaunch {
    ExternalTerminalLaunch {
        program: PathBuf::from("/bin/sh"),
        args: ["-c", script, "tundra-host-test"]
            .into_iter()
            .chain(args.iter().copied())
            .map(str::to_string)
            .collect(),
    }
}

fn start_external(
    fixture: &PtyFixture,
    launch: &ExternalTerminalLaunch,
    directory: &Path,
) -> CommandLineHost {
    let mut host = CommandLineHost::new(fixture.app.clone());
    host.ensure_started_recorded(
        platform::native_platform().as_ref(),
        "terminal-test",
        ratatui::style::Color::Cyan,
        Some(directory),
        Some(launch),
        None,
    );
    assert!(
        host.is_running(),
        "external launch failed: {:?}",
        host.state
    );
    host
}

fn wait_for_exit(host: &mut CommandLineHost) -> CommandLineHostEvent {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let event = host.poll();
        if event != CommandLineHostEvent::None || !host.is_running() {
            return event;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "external process did not exit; output: {}",
            snapshot_text(&host.ui_snapshot)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn external_exit_codes_75_and_76_keep_output_without_shell_control_effects() {
    let fixture = PtyFixture::new("external-exit-codes");
    for code in ["75", "76"] {
        let launch = external_shell("printf 'FINAL-OUTPUT-%s\\n' \"$1\"; exit \"$1\"", &[code]);
        let mut host = start_external(&fixture, &launch, &fixture.root);
        assert_eq!(wait_for_exit(&mut host), CommandLineHostEvent::None);
        assert!(matches!(
            host.state,
            CommandLineHostState::Exited { code: actual } if actual.to_string() == code
        ));
        assert!(host.output.is_some());
        assert!(snapshot_text(&host.ui_snapshot).contains(&format!("FINAL-OUTPUT-{code}")));
        assert_eq!(host.view_model().title.as_deref(), Some("sh"));
        assert_eq!(
            host.handle_input(
                &InputEvent::Key(KeyInput::with_modifiers(
                    InputKey::F(12),
                    InputModifiers::SHIFT
                )),
                None,
            ),
            CommandLineHostEvent::None
        );
        assert_eq!(
            host.handle_input(&InputEvent::key(InputKey::F(12)), None),
            CommandLineHostEvent::ExitToCaller
        );
    }
}

#[test]
fn external_spawn_failure_can_return_with_the_keyboard() {
    let fixture = PtyFixture::new("external-spawn-failure");
    let launch = ExternalTerminalLaunch {
        program: fixture.root.join("missing-terminal-program"),
        args: Vec::new(),
    };
    let mut host = CommandLineHost::new(fixture.app.clone());
    host.ensure_started_recorded(
        platform::native_platform().as_ref(),
        "terminal-test",
        ratatui::style::Color::Cyan,
        Some(&fixture.root),
        Some(&launch),
        None,
    );
    assert!(matches!(host.state, CommandLineHostState::Failed { .. }));
    assert!(!host.is_running());
    assert_eq!(
        host.handle_input(&InputEvent::key(InputKey::Escape), None),
        CommandLineHostEvent::None
    );
    assert_eq!(
        host.handle_input(&InputEvent::key(InputKey::F(12)), None),
        CommandLineHostEvent::ExitToCaller
    );
}

#[test]
fn external_arguments_and_unicode_directory_are_passed_without_shell_interpretation() {
    let fixture = PtyFixture::new("external-arguments");
    let directory = fixture.root.join("中文 project");
    std::fs::create_dir_all(&directory).unwrap();
    let launch = external_shell(
        "printf '%s\\000' \"$PWD\" \"$1\" \"$2\" \"$3\" > arguments.bin; printf 'ARGUMENTS-RECEIVED'; exit 9",
        &["two words", "$(touch SHOULD_NOT_EXIST)", ""],
    );
    let mut host = start_external(&fixture, &launch, &directory);
    assert_eq!(wait_for_exit(&mut host), CommandLineHostEvent::None);
    let mut expected = Vec::new();
    let actual_directory = std::fs::canonicalize(&directory).unwrap();
    for argument in [
        actual_directory.to_str().unwrap(),
        "two words",
        "$(touch SHOULD_NOT_EXIST)",
        "",
    ] {
        expected.extend_from_slice(argument.as_bytes());
        expected.push(0);
    }
    assert_eq!(
        std::fs::read(directory.join("arguments.bin")).unwrap(),
        expected
    );
    assert!(snapshot_text(&host.ui_snapshot).contains("ARGUMENTS-RECEIVED"));
    assert!(!directory.join("SHOULD_NOT_EXIST").exists());
}

#[test]
fn failed_external_output_retains_history_and_can_be_resized() {
    let fixture = PtyFixture::new("external-retained-output");
    let launch = external_shell(
        "i=0; while [ \"$i\" -lt 70 ]; do printf 'LINE-%03d\\r\\n' \"$i\"; i=$((i+1)); done; exit 9",
        &[],
    );
    let mut host = start_external(&fixture, &launch, &fixture.root);
    assert_eq!(wait_for_exit(&mut host), CommandLineHostEvent::None);
    assert!(host.ui_snapshot.scrollback_rows > 0);
    assert!(snapshot_text(&host.ui_snapshot).contains("LINE-069"));
    host.set_scrollback(usize::MAX);
    assert!(snapshot_text(&host.ui_snapshot).contains("LINE-000"));
    host.resize_terminal(87, 11);
    assert_eq!((host.ui_snapshot.columns, host.ui_snapshot.rows), (87, 11));
    host.set_scrollback(0);
    assert!(snapshot_text(&host.ui_snapshot).contains("LINE-069"));
    assert!(matches!(
        host.state,
        CommandLineHostState::Exited { code: 9 }
    ));
}

#[test]
fn retry_uses_the_same_external_program_arguments_and_directory() {
    let fixture = PtyFixture::new("external-retry");
    let launch = external_shell(
        "printf 'run\\n' >> \"$1\"; printf 'EXTERNAL-RETRY'; exit 9",
        &["runs.txt"],
    );
    let platform = platform::native_platform();
    let mut hosts = CommandLineHosts::new(fixture.app.clone());
    let mut state = crate::ShellSession::new(crate::ShellLaunchConfig::default(), (120, 40));
    state.set_navigation_path(vec![
        crate::ShellScreen::Home,
        crate::ShellScreen::CommandLine,
    ]);
    let id = state.active_page_id();
    state.pages.command_line.command_line_external = Some(launch.clone());
    state.pages.command_line.command_line_start_directory = Some(fixture.root.clone());
    for expected_runs in [1, 2] {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let (_, event) = hosts.poll(&mut state, platform.as_ref(), ratatui::style::Color::Cyan);
            assert_eq!(event, CommandLineHostEvent::None);
            if matches!(
                hosts.hosts[&id].state,
                CommandLineHostState::Exited { code: 9 }
            ) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "external retry did not finish"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(hosts.hosts[&id].launch.as_ref(), Some(&launch));
        assert_eq!(
            std::fs::read_to_string(fixture.root.join("runs.txt"))
                .unwrap()
                .lines()
                .count(),
            expected_runs
        );
        assert!(snapshot_text(&hosts.hosts[&id].ui_snapshot).contains("EXTERNAL-RETRY"));
        if expected_runs == 1 {
            assert_eq!(
                hosts.handle_input(&state, &InputEvent::key(InputKey::Enter), None),
                CommandLineHostEvent::None
            );
        }
    }
}

#[test]
fn successful_external_process_returns_to_its_original_launcher_instance() {
    let fixture = PtyFixture::new("external-return");
    let platform = platform::native_platform();
    let mut hosts = CommandLineHosts::new(fixture.app.clone());
    let mut state = crate::ShellSession::new(crate::ShellLaunchConfig::default(), (120, 40));
    state.set_navigation_path(vec![crate::ShellScreen::Home, crate::ShellScreen::Launcher]);
    let caller_id = state.active_page_id();
    state.enter_new_page(crate::ShellScreen::CommandLine);
    let terminal_id = state.active_page_id();
    state.pages.command_line.command_line_external =
        Some(external_shell("printf 'COMPLETE'; exit 0", &[]));
    state.pages.command_line.command_line_start_directory = Some(fixture.root.clone());
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while state.active_page_id() == terminal_id {
        let (_, event) = hosts.poll(&mut state, platform.as_ref(), ratatui::style::Color::Cyan);
        assert_eq!(event, CommandLineHostEvent::None);
        assert!(
            std::time::Instant::now() < deadline,
            "successful external process did not return"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(state.active_page_id(), caller_id);
    assert_eq!(state.active_screen(), crate::ShellScreen::Launcher);
    assert!(!hosts.hosts.contains_key(&terminal_id));
}

#[test]
fn failed_external_exit_releases_unfinished_synchronized_output() {
    let fixture = PtyFixture::new("external-synchronized-exit");
    let launch = external_shell(
        "printf '\\033[?2026hFINAL_SYNCHRONIZED_OUTPUT'; exit 9",
        &[],
    );
    let mut host = start_external(&fixture, &launch, &fixture.root);
    assert_eq!(wait_for_exit(&mut host), CommandLineHostEvent::None);
    assert!(matches!(
        host.state,
        CommandLineHostState::Exited { code: 9 }
    ));
    assert!(snapshot_text(&host.ui_snapshot).contains("FINAL_SYNCHRONIZED_OUTPUT"));
}

#[test]
fn host_preserves_kitty_events_and_mouse_capture_across_resize() {
    let fixture = PtyFixture::new("external-input-events");
    let expected =
        b"\x1b[97;1u\x1b[97;1:2u\x1b[97;1:3u\x1b[<0;3;4M\x1b[<32;34;12M\x1b[<0;34;12m\x1b[122;1u";
    let script = format!(
        "stty raw -echo; printf '\\033[>11u\\033[?1002h\\033[?1006hINPUT_READY'; dd bs=1 count={} of=\"$1\" 2>/dev/null; printf '\\033[<u\\033[?1002l\\033[?1006lINPUT_COMPLETE'; exit 9",
        expected.len()
    );
    let launch = external_shell(&script, &["input.bin"]);
    let mut host = start_external(&fixture, &launch, &fixture.root);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        assert_eq!(host.poll(), CommandLineHostEvent::None);
        if snapshot_text(&host.ui_snapshot).contains("INPUT_READY") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "child did not enable input protocols"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    host.resize_terminal(30, 10);
    let area = Some(Rect::new(10, 5, 30, 10));
    host.handle_input(&InputEvent::mouse_up(ui::MouseButton::Left, (99, 99)), area);
    for phase in [
        ui::InputPhase::Press,
        ui::InputPhase::Repeat,
        ui::InputPhase::Release,
    ] {
        host.handle_input(
            &InputEvent::Key(KeyInput::with_phase(
                InputKey::Char('a'),
                InputModifiers::NONE,
                phase,
            )),
            area,
        );
    }
    host.handle_input(
        &InputEvent::mouse_down(ui::MouseButton::Left, (12, 8)),
        area,
    );
    host.resize_terminal(34, 12);
    assert_eq!((host.ui_snapshot.columns, host.ui_snapshot.rows), (34, 12));
    let area = Some(Rect::new(10, 5, 34, 12));
    host.handle_input(
        &InputEvent::mouse_drag(ui::MouseButton::Left, (99, 99)),
        area,
    );
    host.handle_input(&InputEvent::mouse_up(ui::MouseButton::Left, (99, 99)), area);
    // A trailing key makes duplicate release bytes observable in the fixed read.
    host.handle_input(&InputEvent::key(InputKey::Char('z')), area);
    assert_eq!(wait_for_exit(&mut host), CommandLineHostEvent::None);
    assert_eq!(
        std::fs::read(fixture.root.join("input.bin")).unwrap(),
        expected
    );
    assert!(snapshot_text(&host.ui_snapshot).contains("INPUT_COMPLETE"));
}
