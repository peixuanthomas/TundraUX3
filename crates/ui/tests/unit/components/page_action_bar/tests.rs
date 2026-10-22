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
