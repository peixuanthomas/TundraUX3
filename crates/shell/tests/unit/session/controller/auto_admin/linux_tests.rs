use super::*;
use platform::linux::authorization::Interaction;
use platform::service::ServiceError;
use std::os::unix::process::CommandExt;
use std::process::Command;
fn terminal_text(job: &AutoAdminJob) -> String {
    job.view_model(false, None, 0)
        .terminal
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect()
}

#[test]
fn aa_stop_and_confirmed_kill_reach_a_real_stubborn_pty_process() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (100, 35),
        ShellHomeMode::User,
    );
    let (tx, _rx) = mpsc::channel();
    let job = AutoAdminJob::new(
        "Stubborn PTY test".into(),
        storage::AutoAdminPolicy::Automatic,
        tx,
    );
    let tty = job.open_test_terminal().unwrap();
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "trap '' TERM; printf 'STOP-READY\\n'; exec sleep 30"])
        .stdin(tty.try_clone().unwrap())
        .stdout(tty.try_clone().unwrap())
        .stderr(tty);
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn().unwrap();
    job.track_test_child(child.id()).unwrap();
    state.auto_admin = AutoAdminState {
        job: Some(job.clone()),
        visible: true,
        ..Default::default()
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        job.poll_terminal();
        if terminal_text(&job).contains("STOP-READY") {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("PTY startup timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    state.activate_auto_admin_button(&job, 1);
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        child.try_wait().unwrap().is_none(),
        "SIGTERM is deliberately ignored"
    );
    state.poll_auto_admin_stop(Instant::now() + Duration::from_secs(11));
    assert!(job.stop_warning());
    state.handle_auto_admin_input(&InputEvent::key(InputKey::Right));
    state.handle_auto_admin_input(&InputEvent::key(InputKey::Enter));
    let deadline = Instant::now() + Duration::from_secs(3);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("AA did not kill the PTY process");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(libc::SIGKILL));
    job.finish(Err("Killed".into()));
    assert!(state.auto_admin_view().unwrap().finished);
    job.close_test_terminal();
}

#[test]
fn default_enter_approval_reaches_the_linux_authorization_adapter() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let (tx, _rx) = mpsc::channel();
    let job = state
        .begin_auto_admin("Enable fixture account".into(), true, tx)
        .unwrap();
    let authorization = AutoAdminAuthorization::new(job.clone());
    assert!(matches!(
        authorization.begin(),
        Err(ServiceError::AuthorizationCancelled)
    ));
    state.apply_input(InputEvent::key(InputKey::Enter));
    assert!(job.wait_for_approval().is_ok());
    assert!(authorization.begin().is_ok());
    assert!(!authorization.cancelled());
}

#[test]
fn foreground_auth_pty_accepts_password_and_yn_and_closes_only_after_completion() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let (tx, _rx) = mpsc::channel();
    let job = AutoAdminJob::new(
        "PTY fixture".into(),
        storage::AutoAdminPolicy::Automatic,
        tx,
    );
    let tty = job.open_test_terminal().unwrap();
    state.auto_admin = AutoAdminState {
        job: Some(job.clone()),
        visible: true,
        ..Default::default()
    };
    let mut command = Command::new("/usr/bin/timeout");
    command.args(["5", "/bin/sh", "-c", r#"printf 'Password: '; read password; test "$password" = 'test-secret' || exit 4; printf '\nContinue? [y/n] '; read answer; test "$answer" = y || exit 5; printf '\nSIZE:'; stty size; printf 'DONE\n'"#])
        .stdin(tty.try_clone().unwrap()).stdout(tty.try_clone().unwrap()).stderr(tty);
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY, 0) < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(6);
    let mut expected_size = String::new();
    let mut sent_password = false;
    let mut sent_yes = false;
    loop {
        job.poll_terminal();
        let output = terminal_text(&job);
        if output.contains("Password:") && !sent_password {
            state.apply_input(InputEvent::key(InputKey::F(12)));
            assert!(state.auto_admin_visible());
            assert_eq!(job.phase(), RUNNING);
            state.toggle_input_keyboard();
            state.advance_input_keyboard(ui::MotionFrame::reduced(std::time::Duration::ZERO));
            let area = ui::auto_admin_layout_in(
                state.auto_admin_available_area(),
                &state.auto_admin_view().unwrap(),
            )
            .terminal;
            expected_size = format!("{} {}", area.height, area.width);
            for ch in "test-secret".chars() {
                click_live_key(
                    &mut state,
                    if ch.is_ascii_alphabetic() {
                        ui::ScreenKeyboardAction::Letter(ch)
                    } else {
                        ui::ScreenKeyboardAction::Character(ch)
                    },
                );
            }
            click_live_key(&mut state, ui::ScreenKeyboardAction::Enter);
            sent_password = true;
        }
        if output.contains("[y/n]") && !sent_yes {
            state.apply_input(InputEvent::key(InputKey::F(12)));
            assert!(state.auto_admin_visible());
            assert_eq!(job.phase(), RUNNING);
            click_live_key(&mut state, ui::ScreenKeyboardAction::Letter('y'));
            click_live_key(&mut state, ui::ScreenKeyboardAction::Enter);
            sent_yes = true;
        }
        assert!(state.auto_admin_visible());
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{output}");
            job.finish(Ok("Done".into()));
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("PTY fixture timed out: {output}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    job.poll_terminal();
    let output = terminal_text(&job);
    assert!(output.contains("DONE"), "{output}");
    assert!(output.contains(&expected_size), "{output}");
    assert!(!output.contains("test-secret"));
    assert!(sent_password && sent_yes);
    assert!(state.auto_admin_visible());
    assert!(state.auto_admin_view().unwrap().finished);
    state.apply_input(InputEvent::key(InputKey::Escape));
    assert!(!state.auto_admin_visible());
}

fn click_live_key(state: &mut ShellSession, action: ui::ScreenKeyboardAction) {
    let layout = state.input_keyboard_layout();
    let area = layout
        .buttons
        .iter()
        .find(|button| button.action == action)
        .unwrap()
        .area;
    state.apply_input(InputEvent::mouse_down(
        PointerButton::Left,
        (area.x, area.y),
    ));
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, (area.x, area.y)));
}
