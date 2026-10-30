use ratatui::layout::Rect;
use std::time::Duration;
use ui::{MotionFrame, MotionTimings, PanelMotion, RevealOrigin, reveal_rect};

#[test]
fn phase_times_change_the_curve_and_duration_while_speed_scales_all_stages() {
    let long_start = ui::MotionPhases::from_millis([80, 40, 80]);
    let short_start = ui::MotionPhases::from_millis([20, 160, 20]);
    assert_eq!(long_start.duration(), short_start.duration());
    assert!(long_start.progress(250) < short_start.progress(250));
    for phases in [
        long_start,
        short_start,
        ui::MotionPhases::from_millis([100, 30, 70]),
    ] {
        assert_eq!((phases.progress(0), phases.progress(1_000)), (0, 1_000));
        assert!((0..1_000).all(|time| phases.progress(time) <= phases.progress(time + 1)));
        for speed in [50, 100, 200] {
            let frame = |now| MotionFrame {
                now: Duration::from_millis(now),
                animation_speed_percent: speed,
                ..Default::default()
            };
            let mut panel = PanelMotion::default();
            panel.update_with_phases(1_000, frame(0), phases);
            let total = 200 * 100 / u64::from(speed);
            assert!(panel.update_with_phases(1_000, frame(total / 4), phases));
            assert_eq!(panel.value(), phases.progress(250));
            assert!(!panel.update_with_phases(1_000, frame(total), phases));
            assert_eq!(panel.value(), 1_000);
        }
    }
}

#[test]
fn motion_accelerates_cruises_and_decelerates_without_overshoot() {
    let curve = ui::ease_in_out_cruise;
    assert_eq!((curve(0), curve(500), curve(1_000)), (0, 500, 1_000));
    assert_eq!(curve(u16::MAX), 1_000);
    let speeds: Vec<_> = (0..1_000)
        .step_by(50)
        .map(|time| curve(time + 50) - curve(time))
        .collect();
    assert!(speeds[..5].windows(2).all(|pair| pair[0] < pair[1]));
    assert!(speeds[5..15].iter().all(|speed| (66..=67).contains(speed)));
    assert!(speeds[15..].windows(2).all(|pair| pair[0] > pair[1]));
    for time in 0..1_000 {
        assert!((0..=2).contains(&(curve(time + 1) - curve(time))));
        assert_eq!(curve(time), 1_000 - curve(1_000 - time));
    }
}

#[test]
fn shared_panel_and_scheduled_motion_use_the_same_curve_in_both_directions() {
    let frame = |now| MotionFrame {
        now: Duration::from_millis(now),
        ..Default::default()
    };
    for (start, target) in [(0, 1_000), (1_000, 0)] {
        let mut panel = PanelMotion::new(start);
        panel.update(target, frame(0), MotionTimings::DIALOG);
        for time in [18, 45, 90, 135, 162, 180] {
            panel.update(target, frame(time), MotionTimings::DIALOG);
            let scheduled = ui::schedule_motion_range(
                ui::MotionTransitionKind::Dialog,
                if target == 0 {
                    ui::MotionDirection::Exiting
                } else {
                    ui::MotionDirection::Entering
                },
                start,
                target,
                Duration::ZERO,
                frame(time),
            );
            assert_eq!(panel.value(), scheduled.progress);
        }
    }
}

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

#[test]
fn button_border_moves_and_grows_to_the_surface_bounds() {
    let button = Rect::new(80, 28, 12, 3);
    let panel = Rect::new(4, 5, 100, 20);
    let origin = RevealOrigin::Button(button);
    assert_eq!(reveal_rect(panel, 0, origin), button);
    assert_eq!(reveal_rect(panel, 500, origin), Rect::new(42, 17, 56, 12));
    assert_eq!(reveal_rect(panel, 1_000, origin), panel);
    for amount in 0..=1_000 {
        let current = reveal_rect(panel, amount, origin);
        assert_eq!(current.intersection(button.union(panel)), current);
    }
}

#[test]
fn moving_surface_keeps_only_its_border_until_the_transition_finishes() {
    use ratatui::{buffer::Buffer, style::Color, widgets::Paragraph};
    use ui::components::Surface;
    let bounds = Rect::new(0, 0, 12, 5);
    let mut source = Buffer::empty(bounds);
    let mut context = ui::RenderContext::default();
    context.theme.border = Color::Red;
    Surface::new()
        .bordered(true)
        .render(bounds, &mut source, &context);
    use ratatui::widgets::Widget;
    Paragraph::new("中中文文本字").render(Rect::new(1, 2, 10, 1), &mut source);
    let moving = Rect::new(20, 10, 7, 5);
    let result = ui::morph_surface(&source, moving);
    assert_eq!(result[(20, 10)].symbol(), source[(0, 0)].symbol());
    assert_eq!(result[(26, 14)].symbol(), source[(11, 4)].symbol());
    assert_eq!(result[(21, 10)].symbol(), "─");
    assert_eq!(result[(20, 12)].symbol(), "│");
    assert_eq!(result[(20, 12)].fg, Color::Red);
    for area in [moving, bounds] {
        let result = ui::morph_surface(&source, area);
        assert!(
            area.inner(ratatui::layout::Margin::new(1, 1))
                .positions()
                .all(|position| result[position].symbol() == " ")
        );
    }
    assert_eq!(source[(1, 2)].symbol(), "中");
}
