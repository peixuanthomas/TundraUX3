use ratatui::layout::Rect;
use ui::{WorkspaceLayout, WorkspacePaneLayout, WorkspaceSplitMode, WorkspaceWindowAction};
use workspace_protocol::{SessionId, Size};

const MINIMUM: Size = Size {
    width: 106,
    height: 16,
};

fn areas(width: u16, height: u16) -> (Rect, Rect) {
    (
        Rect::new(0, 0, width, height),
        Rect::new(0, 1, width, height - 2),
    )
}

#[test]
fn grid_capacity_follows_terminal_size() {
    for (width, height, capacity) in [
        (108, 20, 1),
        (120, 36, 1),
        (180, 50, 2),
        (217, 41, 4),
        (240, 50, 4),
        (360, 76, 9),
        (360, 77, 12),
        (360, 80, 12),
    ] {
        let (terminal, available) = areas(width, height);
        assert_eq!(
            WorkspaceLayout::capacity(
                terminal,
                available,
                &[(SessionId(1), MINIMUM)],
                WorkspaceSplitMode::Grid,
            ),
            capacity,
        );
    }
}

#[test]
fn single_pane_reserves_all_four_window_edges_at_the_terminal_minimum() {
    let (terminal, available) = areas(108, 20);
    let id = SessionId(1);
    let layout = WorkspaceLayout::new(
        terminal,
        available,
        &[(id, MINIMUM)],
        &[id],
        Some(id),
        WorkspaceSplitMode::Grid,
    );
    assert!(!layout.blocked);
    assert_eq!(layout.panes.len(), 1);
    assert_eq!(layout.panes[0].id, id);
    let pane = layout.panes[0];
    assert_eq!(pane.area, available);
    assert_eq!(pane.content, Rect::new(1, 2, 106, 16));
    assert_eq!(pane.title.unwrap().y, pane.area.y);
    assert!(pane.controls.close.right() < pane.area.right());
    for area in [
        pane.controls.minimize,
        pane.controls.maximize,
        pane.controls.split,
        pane.controls.close,
    ] {
        assert_eq!(area.height, 1);
        assert_eq!(area.y, pane.area.y);
        assert!(area.intersection(pane.content).is_empty());
    }
}

#[test]
fn axis_modes_have_two_slots_or_keep_one_full_pane() {
    let windows = [(SessionId(1), MINIMUM), (SessionId(2), MINIMUM)];
    let visible = [SessionId(1), SessionId(2)];
    let (terminal, available) = areas(180, 50);
    let horizontal = WorkspaceLayout::new(
        terminal,
        available,
        &windows,
        &visible,
        Some(SessionId(2)),
        WorkspaceSplitMode::LeftRight,
    );
    assert_eq!(horizontal.capacity, 1);
    assert_eq!(horizontal.panes[0].id, SessionId(2));
    assert_eq!(horizontal.panes[0].area, available);
    assert_eq!(horizontal.panes[0].content, Rect::new(1, 2, 178, 46));

    let vertical = WorkspaceLayout::new(
        terminal,
        available,
        &windows,
        &visible,
        Some(SessionId(2)),
        WorkspaceSplitMode::TopBottom,
    );
    assert_eq!(vertical.capacity, 2);
    assert_eq!(vertical.panes[0].area, Rect::new(0, 1, 180, 24));
    assert_eq!(vertical.panes[0].content, Rect::new(1, 2, 178, 22));
    assert_eq!(vertical.panes[1].area, Rect::new(0, 26, 180, 23));
    assert_eq!(vertical.panes[1].content, Rect::new(1, 27, 178, 21));

    let (terminal, available) = areas(360, 80);
    for mode in [WorkspaceSplitMode::LeftRight, WorkspaceSplitMode::TopBottom] {
        assert_eq!(
            WorkspaceLayout::capacity(terminal, available, &windows, mode),
            2
        );
    }
}

#[test]
fn grid_picks_the_more_square_shape_and_spreads_the_last_row() {
    let (terminal, available) = areas(327, 64);
    let visible = [SessionId(1), SessionId(2), SessionId(3)];
    let windows = visible.map(|id| (id, MINIMUM));
    let layout = WorkspaceLayout::new(
        terminal,
        available,
        &windows,
        &visible,
        None,
        WorkspaceSplitMode::Grid,
    );
    assert_eq!(layout.panes[0].area, Rect::new(0, 1, 163, 31));
    assert_eq!(layout.panes[1].area, Rect::new(164, 1, 163, 31));
    assert_eq!(layout.panes[2].area, Rect::new(0, 33, 327, 30));
    assert_eq!(layout.panes[2].content, Rect::new(1, 34, 325, 28));
}

#[test]
fn grid_breaks_shape_ties_with_fewer_unused_slots_and_then_fewer_rows() {
    let (terminal, available) = areas(440, 90);
    let visible = (0..10).map(SessionId).collect::<Vec<_>>();
    let windows = visible
        .iter()
        .copied()
        .map(|id| (id, MINIMUM))
        .collect::<Vec<_>>();
    let layout = WorkspaceLayout::new(
        terminal,
        available,
        &windows,
        &visible[..6],
        None,
        WorkspaceSplitMode::Grid,
    );
    assert_eq!(
        layout.panes[0].title.unwrap().y,
        layout.panes[2].title.unwrap().y
    );
    assert!(layout.panes[2].title.unwrap().y < layout.panes[3].title.unwrap().y);

    let square = WorkspaceLayout::new(
        terminal,
        available,
        &windows,
        &visible[..9],
        None,
        WorkspaceSplitMode::Grid,
    );
    assert_eq!(
        square.panes[0].title.unwrap().y,
        square.panes[2].title.unwrap().y
    );
    assert_eq!(square.panes[0].area.width, 146);
}

#[test]
fn resize_keeps_focus_and_preserves_the_order_of_other_survivors() {
    let (terminal, available) = areas(217, 41);
    let visible = [
        SessionId(8),
        SessionId(3),
        SessionId(7),
        SessionId(2),
        SessionId(1),
    ];
    let windows = visible.map(|id| (id, MINIMUM));
    let layout = WorkspaceLayout::new(
        terminal,
        available,
        &windows,
        &visible,
        Some(SessionId(1)),
        WorkspaceSplitMode::Grid,
    );
    assert_eq!(
        layout.panes.iter().map(|pane| pane.id).collect::<Vec<_>>(),
        [SessionId(8), SessionId(3), SessionId(7), SessionId(1)],
    );
    assert!(layout.panes.iter().all(|pane| {
        pane.content.width >= MINIMUM.width && pane.content.height >= MINIMUM.height
    }));
}

#[test]
fn hidden_windows_contribute_to_both_global_minimum_dimensions() {
    let (terminal, available) = areas(180, 50);
    let windows = [
        (SessionId(1), MINIMUM),
        (
            SessionId(2),
            Size {
                width: 240,
                height: 20,
            },
        ),
        (
            SessionId(3),
            Size {
                width: 108,
                height: 54,
            },
        ),
    ];
    let layout = WorkspaceLayout::new(
        terminal,
        available,
        &windows,
        &[SessionId(1)],
        Some(SessionId(1)),
        WorkspaceSplitMode::Grid,
    );
    assert!(layout.blocked);
    assert_eq!(layout.capacity, 0);
    assert_eq!(
        layout.size_needed,
        Size {
            width: 242,
            height: 58
        }
    );
    assert_eq!(layout.available, available);
    assert!(layout.panes.is_empty());
}

#[test]
fn size_blocked_workspace_can_still_show_window_controls() {
    let id = SessionId(4);
    let (terminal, available) = areas(108, 20);
    let strict = Size {
        width: 108,
        height: 18,
    };
    let layout = WorkspaceLayout::new(
        terminal,
        available,
        &[(id, strict)],
        &[id],
        Some(id),
        WorkspaceSplitMode::Grid,
    );
    assert!(layout.blocked && layout.panes.is_empty());
    assert_eq!(
        layout.size_needed,
        Size {
            width: 110,
            height: 22,
        }
    );
    for area in [available, Rect::new(3, 4, 21, 3)] {
        let pane = WorkspacePaneLayout::for_blocked_window(id, area).unwrap();
        let close = pane.controls.close;
        assert_eq!(pane.content.width, area.width - 2);
        assert_eq!(close.intersection(area), close);
        assert_eq!(
            pane.controls.hit_at((close.x, close.y)),
            Some(WorkspaceWindowAction::Close)
        );
    }
    assert!(WorkspacePaneLayout::for_blocked_window(id, Rect::new(0, 1, 20, 18)).is_none());
    assert!(WorkspacePaneLayout::for_blocked_window(id, Rect::new(0, 1, 108, 2)).is_none());
}

#[test]
fn split_capacity_requires_space_for_all_window_edges_and_the_gap() {
    let windows = [(SessionId(1), MINIMUM), (SessionId(2), MINIMUM)];
    for (width, height, mode, capacity) in [
        (216, 20, WorkspaceSplitMode::LeftRight, 1),
        (217, 20, WorkspaceSplitMode::LeftRight, 2),
        (108, 38, WorkspaceSplitMode::TopBottom, 1),
        (108, 39, WorkspaceSplitMode::TopBottom, 2),
    ] {
        let (terminal, available) = areas(width, height);
        assert_eq!(
            WorkspaceLayout::capacity(terminal, available, &windows, mode),
            capacity
        );
    }
}
