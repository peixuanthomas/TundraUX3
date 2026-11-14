use super::*;

#[test]
fn closing_window_keeps_reported_minimum_until_guardian_removal() {
    let id = SessionId(1);
    let minimum = Size::new(130, 25);
    let mut window = Window {
        launch: WindowLaunch::Clock,
        worker: None,
        stream: None,
        init: None,
        frame: None,
        image_assets: BTreeMap::new(),
        status: WindowStatus {
            minimum,
            ..WindowStatus::default()
        },
        state: ProcessState::Closing,
        queued: VecDeque::new(),
        viewport: None,
        close: CloseProgress::Cleaning(Instant::now()),
        force_when_started: false,
        view_requested: None,
        launch_error: false,
    };
    let bounds = Rect::new(0, 0, 120, 25);
    let content = Rect::new(0, 1, 120, 23);
    window.finish_cleanup();
    window.update_status(WindowStatus::default());
    assert_eq!(window.close, CloseProgress::Exiting);
    assert_eq!(window.status.minimum, minimum);
    assert_eq!(
        WorkspaceLayout::capacity(
            bounds,
            content,
            &[(id, window.status.minimum)],
            WorkspaceSplitMode::Grid
        ),
        0,
    );
    assert_eq!(
        WorkspaceLayout::capacity(bounds, content, &[], WorkspaceSplitMode::Grid),
        1,
    );
}
