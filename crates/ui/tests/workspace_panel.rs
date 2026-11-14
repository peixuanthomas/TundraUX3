use std::sync::Arc;

use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::{
    ColorCapability, RenderCapabilities, RenderContext, TundraTheme, WorkspaceAction,
    WorkspaceCloseWait, WorkspacePanel, WorkspacePanelViewModel, WorkspaceSplitMode,
    WorkspaceWindowViewModel, automation::FrameCapture, render_workspace_close_wait,
    render_workspace_panel,
};
use workspace_protocol::{SessionId, Size};

const ACTIVE: SessionId = SessionId(1);
const BACKGROUND: SessionId = SessionId(2);

fn model() -> WorkspacePanelViewModel {
    WorkspacePanelViewModel {
        windows: vec![
            WorkspaceWindowViewModel {
                id: ACTIVE,
                title: "Diagnostics".into(),
                minimum_size: Size::new(108, 18),
                visible: true,
                unresponsive: false,
                closing: false,
            },
            WorkspaceWindowViewModel {
                id: BACKGROUND,
                title: "文本 e\u{0301} 窗口".repeat(12),
                minimum_size: Size::new(160, 24),
                visible: false,
                unresponsive: true,
                closing: false,
            },
        ],
        focused: Some(ACTIVE),
        selected: Some(BACKGROUND),
        mode: WorkspaceSplitMode::Grid,
        size_needed: Size::new(160, 26),
        capacity: 0,
        blocked: true,
        scroll: 0,
        action_focus: Some(WorkspaceAction::Select(BACKGROUND)),
        hovered: None,
        pressed: None,
    }
}

#[test]
fn size_blocked_panel_keeps_background_windows_and_danger_action_accessible() {
    let model = model();
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for (language, required, unresponsive, background) in [
        ("en-US", "160×26", "Unresponsive", "In background"),
        ("zh-CN", "160×26", "无响应", "后台"),
    ] {
        let snapshot = i18n::LanguageSnapshot::load(&assets, language, 1)
            .unwrap()
            .snapshot;
        let _locale = i18n::enter_snapshot(Arc::new(snapshot));
        for color in [ColorCapability::TrueColor, ColorCapability::Ansi] {
            let context = RenderContext::from_theme(
                &TundraTheme::default(),
                Default::default(),
                RenderCapabilities {
                    color,
                    ..Default::default()
                },
            );
            let area = Rect::new(0, 1, 108, 16);
            let panel = WorkspacePanel::new(area, &model);
            let mut terminal = Terminal::new(TestBackend::new(108, 18)).unwrap();
            let mut snapshot = None;
            terminal
                .draw(|frame| {
                    let capture = FrameCapture::begin(frame.area(), "workspace");
                    render_workspace_panel(frame, &panel, &model, &context);
                    snapshot = Some(capture.finish(frame.buffer_mut()));
                })
                .unwrap();
            let snapshot = snapshot.unwrap();
            let text = snapshot.lines.join("\n");
            assert!(text.contains(required), "{text}");
            assert!(text.contains(unresponsive), "{text}");
            assert!(text.contains(background), "{text}");

            for action in [
                WorkspaceAction::Select(BACKGROUND),
                WorkspaceAction::Close(BACKGROUND),
                WorkspaceAction::ForceKill(BACKGROUND),
            ] {
                let (.., region) = panel.hits.iter().find(|(hit, _)| *hit == action).unwrap();
                let coords = (region.x + region.width / 2, region.y);
                assert_eq!(panel.hit_at(coords), Some(action));
                let node = snapshot
                    .nodes
                    .iter()
                    .find(|node| node.id.ends_with(&action.id()))
                    .unwrap();
                assert!(node.actionable);
                assert_eq!((node.rect.x, node.rect.y), (region.x, region.y));
                if action == WorkspaceAction::ForceKill(BACKGROUND) {
                    let cell = &terminal.backend().buffer()[coords];
                    assert_eq!(cell.fg, context.theme.danger);
                }
            }
        }
    }
}

#[test]
fn close_wait_renders_wait_and_force_kill_with_shared_hit_regions() {
    let area = Rect::new(0, 1, 108, 18);
    let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
    for (size_blocked, phrase) in [
        (false, "Cleanup is still running."),
        (true, "This window needs a larger terminal"),
    ] {
        let wait = WorkspaceCloseWait::new(area, "编辑器 e\u{0301}", BACKGROUND, size_blocked);
        let mut snapshot = None;
        terminal
            .draw(|frame| {
                let capture = FrameCapture::begin(frame.area(), "workspace");
                render_workspace_close_wait(
                    frame,
                    &wait,
                    None,
                    None,
                    None,
                    &RenderContext::default(),
                );
                snapshot = Some(capture.finish(frame.buffer_mut()));
            })
            .unwrap();
        let snapshot = snapshot.unwrap();
        assert!(snapshot.lines.join("\n").contains(phrase));
        for action in [
            WorkspaceAction::Dismiss,
            WorkspaceAction::ForceKill(BACKGROUND),
        ] {
            let (_, area) = wait.hits.iter().find(|(hit, _)| *hit == action).unwrap();
            assert_eq!(wait.hit_at((area.x, area.y)), Some(action));
            let node = snapshot
                .nodes
                .iter()
                .find(|node| node.id.ends_with(&action.id()))
                .unwrap();
            assert_eq!(node.focused, action == WorkspaceAction::Dismiss);
        }
    }
}
