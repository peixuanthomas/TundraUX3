use super::*;

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
    let tty = job.open_terminal().unwrap();
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
    *job.0.process.lock().unwrap() =
        Some(platform::management::termination::ProcessTree::child(child.id()).unwrap());
    state.auto_admin = AutoAdminState {
        job: Some(job.clone()),
        visible: true,
        ..Default::default()
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        job.poll_terminal();
        if job
            .0
            .display
            .lock()
            .unwrap()
            .parser
            .screen()
            .contents()
            .contains("STOP-READY")
        {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("PTY startup timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    state.activate_auto_admin_button(&job, 3);
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
    job.close_terminal();
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
    let tty = job.open_terminal().unwrap();
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
    let mut sent_password = false;
    let mut sent_yes = false;
    loop {
        job.poll_terminal();
        let output = job.0.display.lock().unwrap().parser.screen().contents();
        if output.contains("Password:") && !sent_password {
            state.apply_input(InputEvent::key(InputKey::F(12)));
            assert!(state.auto_admin_visible());
            assert_eq!(job.phase(), RUNNING);
            job.resize(Rect::new(0, 0, 82, 19));
            state.apply_input(InputEvent::Paste("test-secret".into()));
            state.apply_input(InputEvent::key(InputKey::Enter));
            sent_password = true;
        }
        if output.contains("[y/n]") && !sent_yes {
            state.apply_input(InputEvent::key(InputKey::F(12)));
            assert!(state.auto_admin_visible());
            assert_eq!(job.phase(), RUNNING);
            state.apply_input(InputEvent::key(InputKey::Char('y')));
            state.apply_input(InputEvent::key(InputKey::Enter));
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
    let output = job.0.display.lock().unwrap().parser.screen().contents();
    assert!(output.contains("DONE"), "{output}");
    assert!(output.contains("19 82"), "{output}");
    assert!(!output.contains("test-secret"));
    assert!(sent_password && sent_yes);
    assert!(state.auto_admin_visible());
    assert!(state.auto_admin_view().unwrap().finished);
    state.apply_input(InputEvent::key(InputKey::Escape));
    assert!(!state.auto_admin_visible());
}
