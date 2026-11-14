use std::sync::Arc;

use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};
use ui::{
    RenderCapabilities, RenderContext, TundraTheme, WorkspaceAction, WorkspaceLayout,
    WorkspacePaneLayout, WorkspaceSplitMenu, WorkspaceSplitMenuViewModel, WorkspaceSplitMode,
    WorkspaceTaskbar, WorkspaceTaskbarAction, WorkspaceTaskbarItem, WorkspaceWindowAction,
    WorkspaceWindowButtonsState, automation::FrameCapture, render_workspace_pane_frame,
    render_workspace_split_menu, render_workspace_taskbar,
};
use workspace_protocol::{MIN_CONTENT_SIZE, SessionId};

const ID: SessionId = SessionId(8);

#[test]
fn complete_window_edges_and_actions_share_geometry_in_both_languages() {
    let available = Rect::new(0, 1, 108, 18);
    let terminal = Rect::new(0, 0, 108, 20);
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for (language, names, restore) in [
        (
            "en-US",
            ["Minimize", "Full screen", "Split", "Close window"],
            "Restore",
        ),
        ("zh-CN", ["最小化", "全屏", "分屏", "关闭窗口"], "还原"),
    ] {
        let language = i18n::LanguageSnapshot::load(&assets, language, 1)
            .unwrap()
            .snapshot;
        let _locale = i18n::enter_snapshot(Arc::new(language));
        let pane = WorkspaceLayout::new(
            terminal,
            available,
            &[(ID, MIN_CONTENT_SIZE)],
            &[ID],
            Some(ID),
            WorkspaceSplitMode::Grid,
        )
        .panes[0];
        let context = RenderContext::from_theme(
            &TundraTheme::default(),
            Default::default(),
            RenderCapabilities::ansi(),
        );
        let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
        let mut snapshot = None;
        terminal
            .draw(|frame| {
                let capture = FrameCapture::begin(frame.area(), "workspace");
                render_workspace_pane_frame(
                    frame,
                    &pane,
                    "日志 e\u{0301}",
                    true,
                    false,
                    WorkspaceWindowButtonsState::default(),
                    &context,
                );
                snapshot = Some(capture.finish(frame.buffer_mut()));
            })
            .unwrap();
        let snapshot = snapshot.unwrap();
        let buffer = terminal.backend().buffer();
        for point in [(0, 5), (107, 5), (1, 18), (0, 1), (107, 18)] {
            assert!(!buffer[point].symbol().trim().is_empty(), "{point:?}");
        }
        assert!(snapshot.lines[1].contains("日志 e\u{0301}"));
        let actions = [
            WorkspaceWindowAction::Minimize,
            WorkspaceWindowAction::ToggleMaximize,
            WorkspaceWindowAction::SplitMenu,
            WorkspaceWindowAction::Close,
        ];
        for (action, name) in actions.into_iter().zip(names) {
            let rect = pane.controls.rect(action);
            let node = snapshot
                .nodes
                .iter()
                .find(|node| node.id.ends_with(&action.id(ID)))
                .unwrap();
            assert_eq!(node.name, name);
            assert!(
                snapshot.lines[1].contains(name),
                "{name}: {}",
                snapshot.lines[1]
            );
            assert!(node.actionable);
            assert_eq!((node.rect.x, node.rect.width), (rect.x, rect.width));
            assert_eq!(pane.controls.hit_at((rect.x, rect.y)), Some(action));
            assert!(!matches!(buffer[(rect.x, rect.y)].bg, Color::Rgb(..)));
        }
        let mut restored = None;
        terminal
            .draw(|frame| {
                let capture = FrameCapture::begin(frame.area(), "workspace");
                render_workspace_pane_frame(
                    frame,
                    &pane,
                    "日志",
                    true,
                    true,
                    WorkspaceWindowButtonsState {
                        close_disabled: true,
                        ..Default::default()
                    },
                    &context,
                );
                restored = Some(capture.finish(frame.buffer_mut()));
            })
            .unwrap();
        let restored = restored.unwrap();
        assert!(restored.lines[1].contains(restore));
        assert!(restored.nodes.iter().any(|node| {
            node.id.ends_with(&WorkspaceWindowAction::Close.id(ID))
                && !node.enabled
                && !node.actionable
        }));

        let compact = WorkspacePaneLayout::for_blocked_window(ID, Rect::new(0, 1, 21, 3)).unwrap();
        let mut snapshot = None;
        terminal
            .draw(|frame| {
                let capture = FrameCapture::begin(frame.area(), "workspace");
                render_workspace_pane_frame(
                    frame,
                    &compact,
                    "日志",
                    true,
                    false,
                    WorkspaceWindowButtonsState::default(),
                    &context,
                );
                snapshot = Some(capture.finish(frame.buffer_mut()));
            })
            .unwrap();
        let snapshot = snapshot.unwrap();
        for ((action, name), glyph) in actions.into_iter().zip(names).zip(["−", "□", "◫", "×"])
        {
            let node = snapshot
                .nodes
                .iter()
                .find(|node| node.id.ends_with(&action.id(ID)))
                .unwrap();
            assert_eq!(node.name, name);
            assert_eq!(node.text.trim(), glyph);
            assert!(node.actionable);
        }
    }
}

fn windows(count: u64) -> Vec<WorkspaceTaskbarItem> {
    (1..=count)
        .map(|id| WorkspaceTaskbarItem {
            id: SessionId(id),
            title: format!("{id} 编辑器 e\u{0301}"),
            focused: id == 1,
            minimized: id > 1,
            closing: id == count,
            unresponsive: id == 2,
        })
        .collect()
}

#[test]
fn taskbar_keeps_minimized_windows_reachable_and_leaves_neighboring_status_cells() {
    let windows = windows(10);
    let area = Rect::new(0, 4, 40, 1);
    let first = WorkspaceTaskbar::new(area, &windows, 0);
    assert!(
        first
            .hits
            .iter()
            .any(|(action, _)| *action == WorkspaceTaskbarAction::ScrollRight)
    );
    assert!(
        !first
            .hits
            .iter()
            .any(|(action, _)| *action == WorkspaceTaskbarAction::ScrollLeft)
    );
    let last = WorkspaceTaskbar::new(area, &windows, usize::MAX);
    assert_eq!(last.scroll, last.max_scroll);
    assert!(
        last.hits
            .iter()
            .any(|(action, _)| *action == WorkspaceTaskbarAction::Activate(SessionId(10)))
    );
    assert!(
        !last
            .hits
            .iter()
            .any(|(action, _)| *action == WorkspaceTaskbarAction::ScrollRight)
    );
    for (action, rect) in &first.hits {
        assert_eq!(rect.intersection(area), *rect);
        assert_eq!(first.hit_at((rect.x, rect.y)), Some(*action));
    }
    let mut terminal = Terminal::new(TestBackend::new(42, 5)).unwrap();
    let mut snapshot = None;
    terminal
        .draw(|frame| {
            frame.buffer_mut()[(40, 4)].set_symbol("X");
            let capture = FrameCapture::begin(frame.area(), "workspace");
            render_workspace_taskbar(
                frame,
                &first,
                &windows,
                None,
                None,
                &RenderContext::default(),
            );
            snapshot = Some(capture.finish(frame.buffer_mut()));
        })
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(40, 4)].symbol(), "X");
    let snapshot = snapshot.unwrap();
    let active = snapshot
        .nodes
        .iter()
        .find(|node| node.id.ends_with("workspace.taskbar.1"))
        .unwrap();
    assert!(active.selected && active.actionable);
    let close = snapshot
        .nodes
        .iter()
        .find(|node| node.id.ends_with("workspace.taskbar.1.close"))
        .unwrap();
    assert!(close.actionable);
    let close_area = first
        .hits
        .iter()
        .find_map(|(action, area)| {
            (*action == WorkspaceTaskbarAction::Close(SessionId(1))).then_some(*area)
        })
        .unwrap();
    assert_eq!(
        terminal.backend().buffer()[(close_area.right() - 1, close_area.y)].symbol(),
        "−"
    );
    let minimized = snapshot
        .nodes
        .iter()
        .find(|node| node.id.ends_with("workspace.taskbar.2"))
        .unwrap();
    assert!(minimized.name.contains("Unresponsive"));
    assert!(minimized.actionable);
    assert!(
        last.hits
            .iter()
            .any(|(action, _)| *action == WorkspaceTaskbarAction::Close(SessionId(10)))
    );
}

#[test]
fn split_menu_is_anchored_and_limits_clicks_to_its_visible_items() {
    let bounds = Rect::new(1, 2, 106, 16);
    let anchor = Rect::new(80, 1, 14, 1);
    let mut model = WorkspaceSplitMenuViewModel {
        mode: WorkspaceSplitMode::TopBottom,
        windows: windows(12),
        ..Default::default()
    };
    let first = WorkspaceSplitMenu::new(bounds, anchor, &model);
    assert_eq!(first.area.intersection(bounds), first.area);
    assert_eq!(first.area.y, anchor.bottom());
    assert!(first.area.right() <= bounds.right());
    assert_eq!(first.hit_at((anchor.x, anchor.y)), None);
    assert!(first.max_scroll > 0);
    let bottom = Rect::new(anchor.x, bounds.bottom() - 1, anchor.width, 1);
    let above = WorkspaceSplitMenu::new(bounds, bottom, &model);
    assert_eq!(above.area.bottom(), bottom.y);
    model.scroll = first.max_scroll;
    let last = WorkspaceSplitMenu::new(bounds, anchor, &model);
    assert!(
        !last
            .hits
            .iter()
            .any(|(action, _)| *action == WorkspaceAction::Show(SessionId(12)))
    );
    assert!(
        last.hits
            .iter()
            .any(|(action, _)| *action == WorkspaceAction::Show(SessionId(11)))
    );
    for (action, rect) in &last.hits {
        assert_eq!(rect.intersection(last.area), *rect);
        assert_eq!(last.hit_at((rect.x, rect.y)), Some(*action));
    }
    let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
    let mut snapshot = None;
    terminal
        .draw(|frame| {
            let capture = FrameCapture::begin(frame.area(), "workspace");
            render_workspace_split_menu(frame, &last, &model, &RenderContext::default());
            snapshot = Some(capture.finish(frame.buffer_mut()));
        })
        .unwrap();
    let snapshot = snapshot.unwrap();
    assert!(snapshot.nodes.iter().any(|node| {
        node.id
            .ends_with(&WorkspaceAction::SetMode(WorkspaceSplitMode::TopBottom).id())
            && node.actionable
            && node.selected
    }));
    assert!(snapshot.nodes.iter().any(|node| {
        node.id
            .ends_with(&WorkspaceAction::Show(SessionId(12)).id())
            && !node.enabled
            && !node.actionable
    }));
}
