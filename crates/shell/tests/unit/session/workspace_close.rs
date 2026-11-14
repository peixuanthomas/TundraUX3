use super::*;

fn window(minimum: Size) -> Window {
    Window {
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
        state: ProcessState::Running,
        queued: VecDeque::new(),
        viewport: None,
        close: CloseProgress::Idle,
        force_when_started: false,
        view_requested: None,
        launch_error: false,
    }
}

fn pointer(workspace: &mut Workspace, (x, y): CellPosition, kind: ui::MouseEventKind) -> bool {
    workspace
        .input(&InputEvent::Mouse(MouseInput::new(x, y, kind)), false)
        .unwrap()
}

#[test]
fn closing_window_keeps_reported_minimum_until_guardian_removal() {
    let id = SessionId(1);
    let minimum = Size::new(130, 25);
    let mut window = window(minimum);
    window.close = CloseProgress::Cleaning(Instant::now());
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

#[test]
fn exited_window_frame_and_taskbar_hits_do_not_survive_until_redraw() {
    let (first, second) = (SessionId(1), SessionId(2));
    let mut workspace = Workspace::new(None, false);
    for id in [first, second] {
        workspace
            .windows
            .insert(id, window(WindowStatus::default().minimum));
        workspace.selection.show(id);
    }
    let bounds = Rect::new(0, 0, 240, 20);
    workspace.relayout(bounds, Rect::new(0, 1, 240, 18), false);
    let close = workspace.layout.panes[0].controls.close;
    let taskbar = WorkspaceTaskbar::new(
        Rect::new(0, 19, 200, 1),
        &[WorkspaceTaskbarItem {
            id: first,
            title: "Clock".into(),
            focused: false,
            minimized: false,
            closing: false,
            unresponsive: false,
        }],
        0,
    );
    let first_tab = taskbar.hits[0].1;
    workspace.taskbar = Some(taskbar);
    let down = ui::MouseEventKind::Down(PointerButton::Left);
    let up = ui::MouseEventKind::Up(PointerButton::Left);

    assert!(pointer(&mut workspace, (close.x, close.y), down));
    assert_eq!(workspace.windows[&first].close, CloseProgress::Idle);
    workspace.remove(first);
    assert!(pointer(&mut workspace, (close.x, close.y), up));
    for area in [close, first_tab] {
        pointer(&mut workspace, (area.x, area.y), down);
        pointer(&mut workspace, (area.x, area.y), up);
    }
    assert!(!workspace.windows.contains_key(&first));
    assert!(!workspace.selection.includes(first));
    assert_eq!(workspace.selection.focused(), Some(second));
    assert_eq!(workspace.windows[&second].close, CloseProgress::Idle);
}

#[test]
fn taskbar_minus_closes_its_window_on_release() {
    let mut workspace = Workspace::new(None, false);
    let foreground = SessionId(1);
    let target = SessionId(2);
    for id in [target, foreground] {
        workspace.windows.insert(id, window(Size::new(106, 16)));
        workspace.selection.open(id);
    }
    let taskbar = WorkspaceTaskbar::new(
        Rect::new(0, 19, 108, 1),
        &[WorkspaceTaskbarItem {
            id: target,
            title: "Clock".into(),
            focused: false,
            minimized: true,
            closing: false,
            unresponsive: false,
        }],
        0,
    );
    workspace.taskbar = Some(taskbar);
    let down = ui::MouseEventKind::Down(PointerButton::Left);
    let up = ui::MouseEventKind::Up(PointerButton::Left);
    pointer(&mut workspace, (1, 19), down);
    pointer(&mut workspace, (4, 19), up);
    assert_eq!(workspace.windows[&target].close, CloseProgress::Idle);
    assert_eq!(workspace.selection.focused(), Some(foreground));

    pointer(&mut workspace, (1, 19), down);
    assert_eq!(workspace.windows[&target].close, CloseProgress::Idle);
    pointer(&mut workspace, (1, 19), up);
    assert!(matches!(
        workspace.windows[&target].close,
        CloseProgress::Requested(_)
    ));
    assert_eq!(workspace.windows[&foreground].close, CloseProgress::Idle);
    assert_eq!(
        workspace.windows[&target].queued.back(),
        Some(&ShellToApp::RequestClose)
    );
}

#[test]
fn stale_window_list_keyboard_focus_does_not_restore_an_exited_window() {
    let id = SessionId(1);
    let mut workspace = Workspace::new(None, false);
    workspace
        .windows
        .insert(id, window(WindowStatus::default().minimum));
    workspace.selection.show(id);
    workspace.panel = Some(WorkspacePanelViewModel {
        selected: Some(id),
        action_focus: Some(Action::Switch(id)),
        ..Default::default()
    });
    workspace
        .panel_hits
        .push((Action::Switch(id), Rect::new(0, 0, 10, 1)));

    workspace.remove(id);
    workspace
        .input(&InputEvent::Key(KeyInput::new(InputKey::Enter)), false)
        .unwrap();
    assert_eq!(workspace.selection.focused(), None);
    assert!(!workspace.selection.includes(id));
}

#[test]
fn a_blocked_window_keeps_close_controls_and_unfreezes_its_confirmation_on_resize() {
    let id = SessionId(1);
    let mut workspace = Workspace::new(None, false);
    workspace.windows.insert(id, window(Size::new(130, 25)));
    workspace.selection.show(id);
    let small = Rect::new(0, 0, 108, 20);
    let small_content = Rect::new(0, 1, 108, 18);
    workspace.relayout(small, small_content, false);
    let close = workspace.blocked_pane().unwrap().controls.close;
    let down = ui::MouseEventKind::Down(PointerButton::Left);
    let up = ui::MouseEventKind::Up(PointerButton::Left);
    assert!(pointer(&mut workspace, (close.x, close.y), down));
    assert!(pointer(&mut workspace, (close.x, close.y), up));
    let window = workspace.windows.get_mut(&id).unwrap();
    assert!(matches!(window.close, CloseProgress::Requested(_)));
    assert!(matches!(
        window.viewport,
        Some(ShellToApp::Viewport {
            size: None,
            visible: false,
            ..
        })
    ));
    window.close = CloseProgress::Confirmation {
        size_notice_dismissed: false,
    };

    workspace.relayout(small, small_content, false);
    assert_eq!(workspace.wait_for, Some(id));
    assert_eq!(workspace.input_focus, None);
    workspace
        .panel_hits
        .push((Action::Dismiss, Rect::new(0, 0, 10, 1)));

    workspace.relayout(Rect::new(0, 0, 132, 29), Rect::new(0, 1, 132, 27), false);
    assert_eq!(workspace.wait_for, None);
    assert!(workspace.panel_hits.is_empty());
    assert_eq!(workspace.input_focus, Some(id));
    assert!(matches!(
        workspace.windows[&id].viewport,
        Some(ShellToApp::Viewport {
            size: Some(Size {
                width: 130,
                height: 25
            }),
            visible: true,
            focused: true,
            ..
        })
    ));
    assert!(matches!(
        workspace.windows[&id].close,
        CloseProgress::Confirmation { .. }
    ));
}
