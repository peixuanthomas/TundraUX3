use crate::components::{
    ButtonEmphasis, ComponentTone, PageAction, PageActionBar, PageActionStatus,
};
use ratatui::layout::Rect;

fn bar(actions: Vec<PageAction>) -> PageActionBar {
    PageActionBar::new("explorer")
        .with_status(vec![PageActionStatus {
            text: "2 selected".into(),
            tone: ComponentTone::Muted,
        }])
        .with_actions(actions)
}

fn five_actions() -> Vec<PageAction> {
    vec![
        PageAction::new("open", "Enter Open").primary(),
        PageAction::new("new", "N New"),
        PageAction::new("rename", "F2 Rename"),
        PageAction::new("delete", "Del Delete").danger(),
        PageAction::new("menu", "Shift+F10 Menu"),
    ]
}

#[test]
fn actions_are_right_aligned_and_primary_is_rightmost() {
    let bar = bar(vec![
        PageAction::new("open", "Enter Open").primary(),
        PageAction::new("new", "N New"),
        PageAction::new("delete", "Del Delete").danger(),
    ]);
    let layout = bar.layout(Rect::new(0, 0, 80, 5));
    let open = layout
        .actions
        .iter()
        .find(|a| a.id.as_str() == "open")
        .unwrap();
    let new = layout
        .actions
        .iter()
        .find(|a| a.id.as_str() == "new")
        .unwrap();
    assert_eq!(open.area.right(), 80);
    assert!(new.area.right() < open.area.x);
    assert_eq!(open.emphasis, ButtonEmphasis::Primary);
}

#[test]
fn narrow_width_wraps_actions_without_dropping_them() {
    let bar = bar(vec![
        PageAction::new("open", "Enter Open").primary(),
        PageAction::new("new", "N New"),
        PageAction::new("rename", "F2 Rename"),
        PageAction::new("delete", "Del Delete").danger(),
        PageAction::new("menu", "Shift+F10 Menu"),
    ]);
    let layout = bar.layout(Rect::new(0, 0, 50, 5));
    assert_eq!(layout.actions.len(), 5);
    assert!(
        layout
            .actions
            .iter()
            .any(|a| a.area.y != layout.actions[0].area.y)
    );
}

#[test]
fn hit_returns_action_id_and_disabled_state() {
    let bar = bar(vec![
        PageAction::new("open", "Enter Open").primary(),
        PageAction::new("rename", "F2 Rename").disabled(true),
    ]);
    let layout = bar.layout(Rect::new(0, 0, 80, 5));
    let rename = layout
        .actions
        .iter()
        .find(|a| a.id.as_str() == "rename")
        .unwrap();
    let hit = bar
        .hit(&layout, rename.area.x + 1, rename.area.y)
        .expect("rename hit");
    assert_eq!(hit.id.as_str(), "rename");
    assert!(!hit.enabled);
}

#[test]
fn height_matches_layout_area_for_status_and_actions() {
    let bar = bar(vec![PageAction::new("open", "Enter Open").primary()]);
    assert_eq!(bar.height(80), 2);
    let layout = bar.layout(Rect::new(0, 0, 80, 5));
    assert_eq!(layout.area.height, 2);
    assert_eq!(layout.status.len(), 1);
    assert_eq!(layout.actions.len(), 1);
}

#[test]
fn total_height_is_capped_and_keeps_leading_status_lines() {
    let status = ["error", "progress", "metadata"]
        .into_iter()
        .map(|text| PageActionStatus {
            text: text.into(),
            tone: ComponentTone::Muted,
        })
        .collect();
    let bar = PageActionBar::new("explorer")
        .with_status(status)
        .with_actions(five_actions());
    assert!(bar.height(50) <= 4);
    let layout = bar.layout(Rect::new(0, 0, 50, 5));
    assert_eq!(layout.area.height, 4);
    assert_eq!(layout.status.len(), 2);
    assert_eq!(layout.status[0].y, layout.area.y);
    assert_eq!(layout.status[1].y, layout.area.y + 1);
    assert_eq!(layout.actions.len(), 5);
}

#[test]
fn five_actions_fit_three_rows_at_minimum_width() {
    let bar = bar(five_actions());
    let layout = bar.layout(Rect::new(0, 0, 50, 5));
    assert_eq!(layout.actions.len(), 5);
    let mut rows: Vec<u16> = layout.actions.iter().map(|action| action.area.y).collect();
    rows.sort_unstable();
    rows.dedup();
    assert!(rows.len() <= 3);
}

#[test]
fn short_area_skips_action_rows_that_do_not_fit() {
    let bar = bar(five_actions());
    let layout = bar.layout(Rect::new(0, 0, 50, 1));
    assert_eq!(layout.area.height, 1);
    assert!(layout.status.is_empty());
    assert!(layout.actions.iter().all(|action| action.area.height == 1));
    assert!(
        layout
            .actions
            .iter()
            .all(|action| action.id.as_str() != "menu")
    );
    for action in &layout.actions {
        let hit = bar
            .hit(&layout, action.area.x, action.area.y)
            .expect("visible action hit");
        assert_eq!(hit.id, action.id);
    }
}

#[test]
fn long_literal_labels_wrap_without_changing_the_existing_button_id() {
    use ratatui::{Terminal, backend::TestBackend};
    let label = "[U] 更新所选软件包并升级整个系统";
    let bar = PageActionBar::new("").with_actions(vec![
        PageAction::new("package-upgrade", label)
            .with_bracketed_label(false)
            .primary(),
    ]);
    let theme = crate::TundraTheme::default_dark();
    let mut context =
        crate::RenderContext::from_theme(&theme, Default::default(), Default::default());
    let buttons = crate::components::ButtonFrame::new(None, None, &theme);
    context.buttons = Some(buttons.clone());
    let bounds = Rect::new(0, 0, 20, 4);
    let mut terminal = Terminal::new(TestBackend::new(20, 4)).unwrap();
    let mut layout = bar.layout(bounds);
    terminal
        .draw(|frame| {
            layout = bar.render(frame, bounds, &context);
        })
        .unwrap();
    assert!(layout.actions[0].area.height > 1);
    assert_eq!(buttons.regions()[0].id.as_str(), "package-upgrade");
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert_eq!(
        text.split_whitespace().collect::<String>(),
        label.split_whitespace().collect::<String>()
    );
}
