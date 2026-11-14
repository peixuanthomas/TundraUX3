use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use ui::{
    RenderCapabilities, RenderContext, TundraTheme, automation, capture_workspace_controls,
    capture_workspace_frame, render_workspace_frame,
};
use workspace_protocol::{Color as WireColor, LayoutGeneration, Size};

#[test]
fn frame_cells_translate_and_clip_at_wide_character_boundaries() {
    let content = Rect::new(3, 4, 5, 1);
    let mut source = Buffer::empty(content);
    source.set_string(
        3,
        4,
        "中e\u{0301}文",
        Style::default()
            .fg(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
    );
    let mut frame = capture_workspace_frame(LayoutGeneration(8), &source, content, None);
    assert_eq!(frame.size, Size::new(5, 1));
    assert_eq!(frame.cells[0].foreground, WireColor::Indexed(14));
    assert!(frame.cells[1].skip);
    assert!(frame.validate().is_ok());
    frame.cells[2].foreground = WireColor::Rgb(100, 240, 240);
    let context = RenderContext::from_theme(
        &TundraTheme::default(),
        Default::default(),
        RenderCapabilities::ansi(),
    );
    let mut terminal = Terminal::new(TestBackend::new(9, 3)).unwrap();
    terminal
        .draw(|target| {
            target.buffer_mut()[(6, 1)].set_symbol("N");
            render_workspace_frame(target, Rect::new(2, 1, 4, 1), &frame, &context, true);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(2, 1)].symbol(), "中");
    assert_eq!(buffer[(4, 1)].symbol(), "e\u{0301}");
    assert_eq!(buffer[(5, 1)].symbol(), " ");
    assert_eq!(buffer[(6, 1)].symbol(), "N");
    assert_eq!(buffer[(2, 1)].fg, Color::LightCyan);
    assert!(!matches!(buffer[(4, 1)].fg, Color::Rgb(..)));
    assert!(buffer[(4, 1)].modifier.contains(Modifier::BOLD));
}

#[test]
fn frame_dto_keeps_pane_snapshot_ids_and_password_masks() {
    let area = Rect::new(0, 1, 8, 2);
    let mut source = Buffer::empty(area);
    source.set_string(0, 1, "secret", Style::default());
    source.set_string(0, 2, "Open", Style::default());
    let mut password = automation::node(
        "page/password",
        "textbox",
        "Password",
        Rect::new(0, 1, 6, 1),
    );
    password.sensitive = true;
    password.parent = Some("page".into());
    password.text = "secret".into();
    let mut button = automation::node("modal/open", "button", "Open", Rect::new(0, 2, 4, 1));
    button.focused = true;
    button.parent = Some("modal".into());
    button.actions.push("click".into());
    let mut frame = capture_workspace_frame(LayoutGeneration(9), &source, area, None);
    capture_workspace_controls(
        &mut frame,
        area,
        &[
            automation::node("page", "page", "", area),
            password,
            automation::node("modal", "dialog", "", area),
            button,
            automation::node("chrome/back", "button", "Back", Rect::new(0, 0, 4, 1)),
        ],
    );
    assert_eq!(frame.masks.len(), 1);
    assert_eq!(frame.cells[0].symbol, "s");
    assert!(
        !frame
            .controls
            .iter()
            .any(|node| node.text.contains("secret") || node.id.contains("chrome"))
    );

    let mut terminal = Terminal::new(TestBackend::new(30, 5)).unwrap();
    let mut snapshot = None;
    terminal
        .draw(|target| {
            let capture = automation::FrameCapture::begin(target.area(), "Editor");
            let active = Rect::new(1, 2, 8, 2);
            let other = Rect::new(10, 2, 8, 2);
            let context = RenderContext::default();
            capture.set_group("page", "page", active, false);
            render_workspace_frame(target, active, &frame, &context, true);
            capture.set_group("session/9", "page", other, false);
            render_workspace_frame(target, other, &frame, &context, false);
            snapshot = Some(capture.finish(target.buffer_mut()));
        })
        .unwrap();
    let snapshot = snapshot.unwrap();
    let password = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "page/password")
        .unwrap();
    assert_eq!(password.text, "••••");
    assert_eq!((password.rect.x, password.rect.y), (1, 2));
    let open = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "modal/open")
        .unwrap();
    assert_eq!(open.parent.as_deref(), Some("modal"));
    assert!(open.actionable && open.focused);
    let inactive = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "session/9/modal/open")
        .unwrap();
    assert!(!inactive.actionable && !inactive.focused);
    assert!(!snapshot.lines.join("\n").contains("secret"));
}
