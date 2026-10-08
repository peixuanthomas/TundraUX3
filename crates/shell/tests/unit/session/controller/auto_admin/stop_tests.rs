use super::*;
use ui::AutoAdminStopState;

fn session() -> (ShellSession, AutoAdminJob, mpsc::Receiver<OperationInput>) {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let (job, rx) = job(storage::AutoAdminPolicy::Automatic);
    job.enable_helper_control();
    job.emit(&OperationEvent::Connected {
        operation_id: "fixture".into(),
    });
    state.auto_admin = AutoAdminState {
        job: Some(job.clone()),
        visible: true,
        ..Default::default()
    };
    (state, job, rx)
}

#[test]
fn stop_waits_for_completion_and_never_automatically_kills() {
    let (mut state, job, rx) = session();
    job.request_stop(true);
    assert!(
        rx.try_recv().is_err(),
        "cannot kill before requesting termination"
    );
    job.request_stop(false);
    job.request_stop(false);
    assert!(matches!(rx.try_recv().unwrap(), OperationInput::Terminate));
    assert!(rx.try_recv().is_err(), "repeated requests are ignored");
    assert!(state.auto_admin_running());
    state.poll_auto_admin_stop(Instant::now() + Duration::from_secs(9));
    assert_eq!(
        state.auto_admin_view().unwrap().stop,
        AutoAdminStopState::Waiting
    );
    state.poll_auto_admin_stop(Instant::now() + Duration::from_secs(11));
    assert_eq!(
        state.auto_admin_view().unwrap().stop,
        AutoAdminStopState::Warning
    );
    assert_eq!(state.auto_admin.button_focus, Some(0));
    assert!(rx.try_recv().is_err(), "the timer never sends SIGKILL");
    state.handle_auto_admin_input(&InputEvent::key(InputKey::Enter));
    assert_eq!(
        state.auto_admin_view().unwrap().stop,
        AutoAdminStopState::Waiting
    );
    assert!(
        rx.try_recv().is_err(),
        "continue waiting does not resend termination"
    );
    job.finish(Err("Terminated".into()));
    state.poll_auto_admin_stop(Instant::now() + Duration::from_secs(30));
    assert!(state.auto_admin_view().unwrap().finished);
    assert_eq!(
        state.auto_admin_view().unwrap().stop,
        AutoAdminStopState::None
    );
}

#[test]
fn only_a_fresh_confirmation_can_force_stop_and_input_stays_blocked() {
    let (mut state, job, rx) = session();
    job.request_stop(false);
    rx.try_recv().unwrap();
    state.poll_auto_admin_stop(Instant::now() + Duration::from_secs(11));
    state.handle_auto_admin_input(&InputEvent::key(InputKey::Right));
    assert_eq!(state.auto_admin.button_focus, Some(1));
    state.handle_auto_admin_input(&InputEvent::key(InputKey::Enter));
    assert!(matches!(rx.try_recv().unwrap(), OperationInput::Kill));
    assert_eq!(
        state.auto_admin_view().unwrap().stop,
        AutoAdminStopState::Killing
    );
    for input in [
        InputEvent::key(InputKey::Enter),
        InputEvent::Paste("dangerous input".into()),
    ] {
        state.handle_auto_admin_input(&input);
    }
    assert!(rx.try_recv().is_err());
    assert!(
        job.running(),
        "sending SIGKILL does not falsely report completion"
    );
}

#[test]
fn timeout_cancels_mouse_capture_and_a_completed_job_cannot_be_killed() {
    let (mut state, job, rx) = session();
    job.request_stop(false);
    rx.try_recv().unwrap();
    let old = job.interaction_phase();
    state.auto_admin.pointer = Some((1, old, Instant::now()));
    state.poll_auto_admin_stop(Instant::now() + Duration::from_secs(11));
    assert!(state.auto_admin.pointer.is_none());
    let layout = ui::auto_admin_layout(Rect::new(0, 0, 120, 40), &state.auto_admin_view().unwrap());
    let kill = (layout.buttons[1].x, layout.buttons[1].y);
    state.handle_auto_admin_input(&InputEvent::mouse_up(PointerButton::Left, kill));
    assert!(rx.try_recv().is_err());
    state.handle_auto_admin_input(&InputEvent::mouse_down(PointerButton::Left, kill));
    job.finish(Ok("Finished during confirmation".into()));
    state.handle_auto_admin_input(&InputEvent::mouse_up(PointerButton::Left, kill));
    job.request_stop(true);
    assert!(rx.try_recv().is_err());
}

#[test]
fn no_owned_process_means_no_force_kill() {
    let (mut state, job, rx) = session();
    job.0.helper_control.store(false, Ordering::Release);
    job.request_stop(false);
    assert!(matches!(rx.try_recv().unwrap(), OperationInput::Cancel));
    state.poll_auto_admin_stop(Instant::now() + Duration::from_secs(11));
    assert!(!state.auto_admin_view().unwrap().can_kill);
    state.handle_auto_admin_input(&InputEvent::key(InputKey::Right));
    assert_eq!(state.auto_admin.button_focus, Some(0));
    job.request_stop(true);
    assert!(rx.try_recv().is_err());
}
