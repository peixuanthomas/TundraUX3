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

fn shell_state() -> ShellSession {
    ShellSession::new_for_home_mode(ShellLaunchConfig::default(), (108, 20), ShellHomeMode::User)
}

fn pointer(
    workspace: &mut Workspace,
    state: &mut ShellSession,
    (x, y): CellPosition,
    kind: ui::MouseEventKind,
) -> bool {
    workspace
        .input(&InputEvent::Mouse(MouseInput::new(x, y, kind)), state)
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
    let mut state = shell_state();
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

    assert!(pointer(
        &mut workspace,
        &mut state,
        (close.x, close.y),
        down
    ));
    assert_eq!(workspace.windows[&first].close, CloseProgress::Idle);
    workspace.remove(first);
    assert!(pointer(&mut workspace, &mut state, (close.x, close.y), up));
    for area in [close, first_tab] {
        pointer(&mut workspace, &mut state, (area.x, area.y), down);
        pointer(&mut workspace, &mut state, (area.x, area.y), up);
    }
    assert!(!workspace.windows.contains_key(&first));
    assert!(!workspace.selection.includes(first));
    assert_eq!(workspace.selection.focused(), Some(second));
    assert_eq!(workspace.windows[&second].close, CloseProgress::Idle);
}

#[test]
fn taskbar_minus_closes_its_window_on_release() {
    let mut workspace = Workspace::new(None, false);
    let mut state = shell_state();
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
    pointer(&mut workspace, &mut state, (1, 19), down);
    pointer(&mut workspace, &mut state, (4, 19), up);
    assert_eq!(workspace.windows[&target].close, CloseProgress::Idle);
    assert_eq!(workspace.selection.focused(), Some(foreground));

    pointer(&mut workspace, &mut state, (1, 19), down);
    assert_eq!(workspace.windows[&target].close, CloseProgress::Idle);
    pointer(&mut workspace, &mut state, (1, 19), up);
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
fn ordinary_open_reuses_the_exact_application_and_preserves_split_add() {
    let mut workspace = Workspace::new(None, false);
    let editor = SessionId(1);
    let script = SessionId(2);
    for (id, launch) in [
        (editor, WindowLaunch::EditorPath("/draft.txt".into())),
        (script, WindowLaunch::Script),
    ] {
        let mut app = window(Size::new(106, 16));
        app.launch = launch;
        workspace.windows.insert(id, app);
        workspace.selection.open(id);
    }
    workspace.selection.add_from_home();
    workspace
        .open_requested(WindowLaunch::Editor, false, None)
        .unwrap();
    assert_eq!(workspace.selection.visible(2), vec![script, editor]);
    assert_eq!(workspace.selection.focused(), Some(editor));
    assert_eq!(workspace.windows.len(), 2);

    let original = &workspace.windows[&editor].launch;
    assert!(!WindowLaunch::EditorPath("/other.txt".into()).can_reuse(original));
    assert!(!WindowLaunch::Editor.can_reuse(&WindowLaunch::Script));
    assert_eq!(original.for_new_session(), WindowLaunch::Editor);
}

#[test]
fn taskbar_context_moves_only_the_taskbar_order() {
    let mut workspace = Workspace::new(None, false);
    let mut state = shell_state();
    let (first, second) = (SessionId(1), SessionId(2));
    for id in [first, second] {
        workspace.windows.insert(id, window(Size::new(106, 16)));
        workspace.taskbar_order.push(id);
        workspace.selection.show(id);
    }
    workspace.relayout(Rect::new(0, 0, 220, 20), Rect::new(0, 1, 220, 18), false);
    let area = Rect::new(0, 19, 110, 1);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(220, 20)).unwrap();
    terminal
        .draw(|frame| {
            workspace.render_taskbar(
                frame,
                Some(area),
                &ui::RenderContext::default(),
                None,
                false,
            )
        })
        .unwrap();
    let taskbar = workspace.taskbar.as_ref().unwrap();
    let (_, title) = taskbar
        .hits
        .iter()
        .find(|(action, _)| *action == WorkspaceTaskbarAction::Activate(first))
        .unwrap();
    let point = (title.x, title.y);
    pointer(
        &mut workspace,
        &mut state,
        point,
        ui::MouseEventKind::Down(PointerButton::Right),
    );
    pointer(
        &mut workspace,
        &mut state,
        point,
        ui::MouseEventKind::Up(PointerButton::Right),
    );
    let menu = workspace.taskbar_menu.as_ref().unwrap();
    assert_eq!(
        menu.menu
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["open", "new-session", "move"]
    );
    for key in [
        InputKey::Down,
        InputKey::Down,
        InputKey::Enter,
        InputKey::Enter,
    ] {
        workspace.input(&InputEvent::key(key), &mut state).unwrap();
    }
    assert!(workspace.taskbar_menu.is_none());
    assert_eq!(workspace.taskbar_order, vec![second, first]);
    assert_eq!(workspace.selection.visible(2), vec![first, second]);
    assert_eq!(workspace.selection.focused(), Some(second));
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
        .input(
            &InputEvent::Key(KeyInput::new(InputKey::Enter)),
            &mut shell_state(),
        )
        .unwrap();
    assert_eq!(workspace.selection.focused(), None);
    assert!(!workspace.selection.includes(id));
}

#[test]
fn a_blocked_window_keeps_close_controls_and_unfreezes_its_confirmation_on_resize() {
    let mut state = shell_state();
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
    assert!(pointer(
        &mut workspace,
        &mut state,
        (close.x, close.y),
        down
    ));
    assert!(pointer(&mut workspace, &mut state, (close.x, close.y), up));
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
