use super::*;

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
fn embedded_auth_pty_accepts_password_and_yn_and_resizes_without_echoing_secret() {
    let (tx, _rx) = mpsc::channel();
    let job = AutoAdminJob::new(
        "PTY fixture".into(),
        storage::AutoAdminPolicy::Automatic,
        tx,
    );
    let tty = job.open_terminal().unwrap();
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
            job.resize(Rect::new(0, 0, 82, 19));
            job.paste("test-secret");
            job.key(&KeyInput::new(InputKey::Enter));
            sent_password = true;
        }
        if output.contains("[y/n]") && !sent_yes {
            job.key(&KeyInput::new(InputKey::Char('y')));
            job.key(&KeyInput::new(InputKey::Enter));
            sent_yes = true;
        }
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "{output}");
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
}
