use ratatui::layout::Rect;
use std::time::Duration;
use ui::{MotionFrame, MotionTimings, PanelMotion, RevealOrigin, reveal_rect};

#[test]
fn panel_reverses_from_visible_position_and_reduced_motion_finishes_immediately() {
    let frame = |now, reduced_motion| MotionFrame {
        now: Duration::from_millis(now),
        reduced_motion,
        ..Default::default()
    };
    let mut motion = PanelMotion::default();
    assert!(motion.update(1_000, frame(0, false), MotionTimings::DIALOG));
    motion.update(1_000, frame(60, false), MotionTimings::DIALOG);
    let middle = motion.value();
    assert!((1..1_000).contains(&middle));
    assert!(motion.update(0, frame(60, false), MotionTimings::DIALOG));
    assert_eq!(motion.value(), middle);
    motion.update(0, frame(120, false), MotionTimings::DIALOG);
    assert!(motion.value() < middle);
    assert!(!motion.update(1_000, frame(121, true), MotionTimings::DIALOG));
    assert_eq!(motion.value(), 1_000);
}

#[test]
fn details_fold_the_list_into_the_shared_edge_on_wide_and_narrow_pages() {
    for width in [60, 120] {
        let area = Rect::new(2, 3, width, 25);
        let mut model = ui::ManagementViewModel::default();
        model.columns = vec!["Name".into()];
        model.rows = vec![vec!["Item".into()]];
        let start = ui::management_layout(area, &model);
        model.details_only = true;
        let end = ui::management_layout(area, &model);
        model.details_expansion = Some(500);
        let mid = ui::management_layout(area, &model);
        if width >= 90 {
            assert!(start.list.width > mid.list.width && mid.list.width > end.list.width);
            assert_eq!(mid.list.right(), mid.details.x);
            assert!(start.details.x > mid.details.x && mid.details.x > end.details.x);
            assert_eq!(start.details.right(), mid.details.right());
        } else {
            assert!(start.list.height > mid.list.height && mid.list.height > end.list.height);
            assert_eq!(mid.list.bottom(), mid.details.y);
            assert!(start.details.y > mid.details.y && mid.details.y > end.details.y);
        }
        model.details_expansion = Some(1_000);
        assert_eq!(ui::management_layout(area, &model).details, end.details);
    }
}

#[test]
fn reveal_keeps_its_origin_and_stays_inside_the_panel() {
    let area = Rect::new(3, 4, 20, 10);
    for origin in [
        RevealOrigin::Right,
        RevealOrigin::Center,
        RevealOrigin::TopLeft,
        RevealOrigin::TopRight,
        RevealOrigin::BottomLeft,
        RevealOrigin::BottomRight,
    ] {
        assert!(reveal_rect(area, 0, origin).is_empty());
        assert_eq!(reveal_rect(area, 1_000, origin), area);
        let middle = reveal_rect(area, 500, origin);
        assert_eq!(middle.intersection(area), middle);
        assert_eq!(middle.width, 10);
        if origin == RevealOrigin::Right {
            assert_eq!(middle.right(), area.right());
            assert_eq!(middle.height, area.height);
        }
    }
}
