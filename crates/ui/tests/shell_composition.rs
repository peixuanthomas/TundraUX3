use ratatui::{Terminal, backend::TestBackend, layout::Rect, widgets::Paragraph};
use ui::*;

fn chrome(width: u16, height: u16) -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".into(),
        build_mode: "debug".into(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (width, height),
        screen_stack: vec!["中文页面路径很长需要正确截断".repeat(8)],
        status: StatusViewModel {
            status: "系统状态消息很长但不应覆盖时钟".repeat(8),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: Some("09:30".into()),
            time_button_selected: false,
        },
    }
}
fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
    (0..terminal.backend().buffer().area.width)
        .map(|x| terminal.backend().buffer()[(x, y)].symbol())
        .collect()
}
fn render(
    model: &ShellChromeViewModel,
    context: &RenderContext,
) -> (Terminal<TestBackend>, ShellFrameLayout) {
    let (width, height) = model.terminal_size;
    let layout = ShellFrameLayout::new(
        Rect::new(0, 0, width, height),
        model.status.time_button_label.as_deref(),
        context,
    );
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(Paragraph::new("PAGE CONTENT"), layout.main);
            render_shell_chrome(frame, &layout, model, context);
        })
        .unwrap();
    (terminal, layout)
}
#[test]
fn chrome_title_and_information_share_one_row_without_spilling_into_content() {
    for width in [50, 80, 120] {
        let (terminal, layout) = render(&chrome(width, 24), &RenderContext::default());
        let ShellLayout::Full { top, .. } = layout.shell else {
            panic!("full layout")
        };
        assert_eq!(top.height, 3);
        let text = row(&terminal, top.y + 1);
        assert!(text.contains("TundraUX 3"));
        assert!(text.contains("debug"));
        assert!(text.replace(' ', "").contains("中文"));
        assert!(!row(&terminal, top.y).contains("debug"));
        assert!(!row(&terminal, top.bottom() - 1).contains("debug"));
        assert!(row(&terminal, layout.main.y).contains("PAGE CONTENT"));
        let message = layout.status_message.unwrap();
        let time = layout.time_button.unwrap();
        assert!(message.right() <= time.x);
        assert_eq!(message.intersection(time).area(), 0);
        assert!(row(&terminal, time.y + 1).contains("09:30"));
    }
}
#[test]
fn long_chinese_title_is_clipped_to_top_inner_row() {
    let mut model = chrome(50, 12);
    model.app_name = "终端交互环境".repeat(20);
    let (terminal, layout) = render(&model, &RenderContext::default());
    assert!(row(&terminal, 1).replace(' ', "").contains("终端交互环境"));
    assert!(row(&terminal, 1).contains("..."));
    assert!(!row(&terminal, 2).contains('终'));
    assert!(row(&terminal, layout.main.y).contains("PAGE CONTENT"));
}
#[test]
fn compact_threshold_exposes_no_shell_hit_regions() {
    for (width, height, compact) in [
        (49, 12, true),
        (50, 11, true),
        (50, 12, false),
        (80, 24, false),
    ] {
        let (terminal, layout) = render(&chrome(width, height), &RenderContext::default());
        assert_eq!(layout.is_compact(), compact);
        assert_eq!(layout.time_button.is_none(), compact);
        assert_eq!(layout.status_message.is_none(), compact);
        if compact {
            assert!(!row(&terminal, 1).contains("TundraUX"));
        }
    }
}
#[test]
fn page_transition_projects_only_main_and_leaves_chrome_pixels_and_hit_regions_fixed() {
    let idle = RenderContext::default();
    let mut moving = idle;
    moving.transitions.screen = Some(MotionTransition {
        kind: MotionTransitionKind::Page,
        direction: MotionDirection::Entering,
        progress: 100,
        phase_progress: 100,
        active: true,
        next_redraw_in: std::time::Duration::from_millis(16),
    });
    let model = chrome(80, 24);
    let (before, base) = render(&model, &idle);
    let (during, shifted) = render(&model, &moving);
    assert_eq!(shifted.main.y, base.main.y + 1);
    assert_eq!(shifted.main.height, base.main.height - 1);
    assert_eq!(shifted.time_button, base.time_button);
    assert_eq!(shifted.status_message, base.status_message);
    for y in [0, 1, 2, 21, 22, 23] {
        assert_eq!(row(&before, y), row(&during, y));
    }
    assert!(row(&during, shifted.main.y).contains("PAGE CONTENT"));
    assert!(!row(&during, base.main.y).contains("PAGE CONTENT"));
}

#[test]
fn editor_screen_content_uses_the_shared_main_without_erasing_shell_chrome() {
    let model = chrome(80, 24);
    let context = RenderContext::default();
    let editor = EditorViewModel::source("sample.txt", "editor content marker");
    let content = ScreenContent::Editor(&editor);
    let layout = ShellFrameLayout::new(Rect::new(0, 0, 80, 24), Some("09:30"), &context);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| {
            content.render_content(frame, &layout, &context, None, None);
            content.render_overlay(frame, &layout, &context);
            render_shell_chrome(frame, &layout, &model, &context);
        })
        .unwrap();
    assert!(row(&terminal, 1).contains("TundraUX 3"));
    assert!(row(&terminal, 22).contains("09:30"));
    assert!(
        (layout.main.y..layout.main.bottom())
            .any(|y| row(&terminal, y).contains("editor content marker"))
    );
}
