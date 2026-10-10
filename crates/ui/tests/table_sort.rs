use std::cmp::Ordering;
use ui::{TableSort, compare_table_cells};

#[test]
fn numbers_sizes_rates_and_text_have_a_consistent_order() {
    for (a, b) in [
        ("2", "10"),
        ("9%", "80%"),
        ("900 MiB", "1 GiB"),
        ("900 KiB/s", "1 MiB/s"),
        ("800 MiB / 1 GiB", "2 GiB / 8 GiB"),
        ("20 °C", "100 °C"),
        ("alpha", "Zulu"),
        ("1.10.0", "2.0.0"),
    ] {
        assert_eq!(compare_table_cells(a, b), Ordering::Less, "{a} < {b}");
        assert_eq!(compare_table_cells(b, a), Ordering::Greater);
    }
    let values = ["2", "10", "10a", "--", "Alpha", "beta", "9%", "1 GiB"];
    for a in values {
        for b in values {
            for c in values {
                if compare_table_cells(a, b) != Ordering::Greater
                    && compare_table_cells(b, c) != Ordering::Greater
                {
                    assert_ne!(
                        compare_table_cells(a, c),
                        Ordering::Greater,
                        "{a} <= {b} <= {c}"
                    );
                }
            }
        }
    }
    let ascending = TableSort::toggle(None, 2);
    let descending = TableSort::toggle(Some(ascending), 2);
    assert!(!ascending.descending && descending.descending);
    assert!(!TableSort::toggle(Some(descending), 0).descending);
}

#[test]
fn management_actions_stay_below_details_and_headers_do_not_overlap_rows() {
    use ratatui::layout::Rect;
    let model = ui::ManagementViewModel {
        title: "Services".into(),
        columns: vec!["Service".into(), "Load".into()],
        rows: vec![vec!["alpha".into(), "loaded".into()]],
        actions: vec![
            ("[A] Start".into(), true),
            ("[S] Stop".into(), true),
            ("[T] Restart".into(), true),
        ],
        detail_action_start: Some(0),
        ..Default::default()
    };
    for (width, height) in [(120, 30), (80, 18), (50, 10)] {
        let main = Rect::new(0, 3, width, height);
        let layout = ui::management_layout(main, &model);
        assert!(layout.actions_panel.is_empty());
        for rect in &layout.actions {
            assert!(!rect.is_empty(), "{width}x{height}");
            assert_eq!(rect.intersection(layout.detail_actions_panel), *rect);
            assert!(rect.y >= layout.details.bottom());
            assert_eq!(rect.intersection(main), *rect);
        }
        for (_, rect) in &layout.headers {
            assert!(rect.bottom() <= layout.list_rows.y);
        }
        assert_eq!(
            layout.actions.last().unwrap().right(),
            if layout.compact_actions {
                layout.detail_actions_panel.right()
            } else {
                layout.detail_actions_panel.right() - 1
            }
        );
    }
}

#[test]
fn long_and_wide_cells_stay_under_their_own_header() {
    use ratatui::{buffer::Buffer, layout::Rect};
    let area = Rect::new(0, 0, 15, 2);
    let mut buffer = Buffer::empty(area);
    let context = ui::RenderContext::from_theme(
        &ui::TundraTheme::default_dark(),
        Default::default(),
        Default::default(),
    );
    let table = ui::components::DataTable::new(
        "test",
        ["Name", "State", "Count"],
        [["世界世界世界", "loadedloaded", "20"]],
    )
    .with_column_widths(vec![5, 5, 5])
    .bordered(false);
    table.render(area, &mut buffer, &context);
    assert_eq!(buffer[(0, 1)].symbol(), "世");
    assert_eq!(buffer[(2, 1)].symbol(), "界");
    assert_eq!(buffer[(4, 1)].symbol(), " ");
    assert_eq!(buffer[(5, 1)].symbol(), "l");
    assert_eq!(buffer[(10, 1)].symbol(), "2");
    assert_eq!(buffer[(11, 1)].symbol(), "0");
}
