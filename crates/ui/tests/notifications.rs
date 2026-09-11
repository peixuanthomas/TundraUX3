use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::{
    NotificationActionViewModel, NotificationDialogLayout, NotificationLayout, NotificationLevel,
    NotificationTone, NotificationViewModel, TundraTheme, notification_layout,
    render_notification_overlay,
};

fn long_notification() -> NotificationViewModel {
    NotificationViewModel::new(
        "startup-repair",
        NotificationLevel::Modal,
        NotificationTone::Warning,
        "语言资源已修复",
        (0..45)
            .map(|index| format!("修复文件{index:02}：语言资源/默认翻译.ftl"))
            .chain(std::iter::once("最后一行：所有修复结果已列出".into()))
            .collect::<Vec<_>>()
            .join("\n"),
        vec![
            NotificationActionViewModel::new("continue", "继续启动").selected(true),
            NotificationActionViewModel::new("exit", "安全退出"),
        ],
    )
}
fn layout(area: Rect, model: &NotificationViewModel) -> NotificationDialogLayout {
    match notification_layout(area, model) {
        NotificationLayout::Dialog(layout) => layout,
        other => panic!("expected scrollable notification, got {other:?}"),
    }
}
fn draw(area: Rect, model: &NotificationViewModel, theme: &TundraTheme) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal
        .draw(|frame| render_notification_overlay(frame, area, model, theme))
        .unwrap();
    terminal
}
fn text(terminal: &Terminal<TestBackend>, area: Rect) -> String {
    let buffer = terminal.backend().buffer();
    (area.y..area.bottom())
        .flat_map(|y| (area.x..area.right()).map(move |x| buffer[(x, y)].symbol()))
        .collect::<String>()
        .replace(' ', "")
}
fn assert_actions_accessible(layout: &NotificationDialogLayout) {
    assert!(!layout.actions.is_empty());
    for (index, action) in layout.actions.iter().enumerate() {
        assert_eq!(action.index, index);
        assert!(action.area.width > 0 && action.area.height > 0);
        assert!(action.area.x >= layout.dialog.x + 1);
        assert!(action.area.right() < layout.dialog.right());
        assert!(action.area.y > layout.message.bottom());
        assert!(action.area.bottom() < layout.dialog.bottom());
        // The exact rectangles consumed by shell hit testing remain disjoint.
        for other in &layout.actions[index + 1..] {
            assert!(action.area.intersection(other.area).is_empty());
        }
    }
}

#[test]
fn long_chinese_repair_summary_scrolls_at_50_by_12_with_fixed_actions() {
    let area = Rect::new(0, 0, 50, 12);
    let mut model = long_notification();
    let first = layout(area, &model);
    assert_eq!(model.scroll_offset, 0);
    assert_eq!(first.scroll_offset, 0);
    assert!(first.max_scroll_offset > 30);
    assert!(first.scrollbar.is_some());
    assert_actions_accessible(&first);
    for theme in [
        TundraTheme::default_dark(),
        TundraTheme::default_dark().with_border_shape(ui::BorderShape::Square),
    ] {
        let terminal = draw(area, &model, &theme);
        assert!(text(&terminal, first.message).contains("修复文件00"));
        assert!(!text(&terminal, first.message).contains("最后一行"));
        assert!(text(&terminal, first.actions[0].area).contains("继续启动"));
        assert!(text(&terminal, first.actions[1].area).contains("安全退出"));
    }
    model.scroll_offset = usize::MAX;
    let last = layout(area, &model);
    assert_eq!(last.scroll_offset, last.max_scroll_offset);
    assert_eq!(last.actions, first.actions);
    assert_eq!(last.dialog, first.dialog);
    assert_actions_accessible(&last);
    let terminal = draw(area, &model, &TundraTheme::default_dark());
    assert!(text(&terminal, last.message).contains("最后一行：所有修复结果已列出"));
    assert!(!text(&terminal, last.message).contains("修复文件00"));
    assert!(text(&terminal, last.actions[0].area).contains("继续启动"));
    assert!(model.actions[0].selected);
}

#[test]
fn notification_scroll_clamps_on_resize_and_when_the_message_becomes_short() {
    let mut model = long_notification();
    model.scroll_offset = usize::MAX;
    let small = layout(Rect::new(0, 0, 50, 12), &model);
    let large = layout(Rect::new(0, 0, 80, 30), &model);
    assert!(large.message.height > small.message.height);
    assert!(large.max_scroll_offset < small.max_scroll_offset);
    assert_eq!(large.scroll_offset, large.max_scroll_offset);
    assert_actions_accessible(&large);
    model.message = "修复已完成".into();
    let short = layout(Rect::new(0, 0, 50, 12), &model);
    assert_eq!(short.scroll_offset, 0);
    assert_eq!(short.max_scroll_offset, 0);
    assert!(short.scrollbar.is_none());
    assert!(short.scroll_hint.is_none());
}

#[test]
fn wrapped_and_stacked_actions_keep_their_own_space_while_the_message_scrolls() {
    let area = Rect::new(0, 0, 50, 12);
    let mut model = long_notification();
    model.stacked_actions = true;
    model.actions[0].label = "继续启动并查看已经修复的全部语言资源文件".repeat(2);
    let first = layout(area, &model);
    assert!(first.actions[0].area.height >= 2);
    assert_actions_accessible(&first);
    model.scroll_offset = first.max_scroll_offset;
    let last = layout(area, &model);
    assert_eq!(first.actions, last.actions);
    let terminal = draw(area, &model, &TundraTheme::default_dark());
    assert!(text(&terminal, last.message).contains("最后一行"));
    assert!(text(&terminal, last.actions[1].area).contains("安全退出"));
}

#[test]
fn too_small_is_reserved_for_required_chrome_and_actions_not_message_length() {
    let model = long_notification();
    let tight = layout(Rect::new(0, 0, 50, 5), &model);
    assert_eq!(tight.message.height, 1);
    assert_actions_accessible(&tight);
    assert!(matches!(
        notification_layout(Rect::new(0, 0, 50, 4), &model),
        NotificationLayout::TooSmall { .. }
    ));
    assert!(matches!(
        notification_layout(Rect::new(0, 0, 2, 12), &model),
        NotificationLayout::TooSmall { .. }
    ));
    let mut no_actions = model;
    no_actions.actions.clear();
    let tiny = layout(Rect::new(0, 0, 50, 3), &no_actions);
    assert_eq!(tiny.message.height, 1);
    assert!(tiny.scrollbar.is_some());
    assert!(tiny.actions.is_empty());
}
