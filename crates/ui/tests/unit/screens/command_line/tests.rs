use super::*;
use crate::command_line_terminal_area;

#[test]
fn minimum_outer_size_uses_the_standard_shell_main_panel() {
    assert_eq!(
        command_line_terminal_area(Rect::new(0, 0, 108, 22)),
        Some(Rect::new(1, 4, 106, 14))
    );
    assert!(command_line_terminal_area(Rect::new(0, 0, 107, 22)).is_none());
    assert!(command_line_terminal_area(Rect::new(0, 0, 108, 21)).is_none());
}

#[test]
fn snapshot_handles_partial_cell_vectors_without_panicking() {
    let snapshot = CommandLineTerminalSnapshot {
        columns: 2,
        rows: 1,
        cursor_style: CommandLineCursorStyle::Default,
        scrollback_rows: 0,
        scrollback_offset: 0,
        cells: vec![CommandLineCell {
            symbol: "A".to_string(),
            ..CommandLineCell::default()
        }],
        images: Vec::new(),
    };
    assert_eq!(snapshot.cell(0, 0).unwrap().symbol, "A");
    assert!(snapshot.cell(1, 0).is_none());
}

#[test]
fn visible_symbols_preserve_wide_and_combining_graphemes() {
    assert_eq!(visible_symbol("界"), "界");
    assert_eq!(visible_symbol("e\u{301}"), "e\u{301}");
    assert_eq!(visible_symbol("\u{1b}"), " ");
}

#[test]
fn scrollbar_reserves_the_right_column_and_tracks_history_position() {
    let area = Rect::new(3, 5, 10, 4);
    let mut snapshot = CommandLineTerminalSnapshot::blank(9, 4);
    snapshot.scrollback_rows = 4;

    let bottom = command_line_scrollbar_layout(area, &snapshot).expect("scrollbar");
    assert_eq!(bottom.track, Rect::new(12, 5, 1, 4));
    assert_eq!(bottom.thumb, Rect::new(12, 7, 1, 2));
    assert_eq!(
        command_line_content_area(area, &snapshot),
        Rect::new(3, 5, 9, 4)
    );

    snapshot.scrollback_offset = snapshot.scrollback_rows;
    let top = command_line_scrollbar_layout(area, &snapshot).expect("scrollbar");
    assert_eq!(top.thumb, Rect::new(12, 5, 1, 2));
}

#[test]
fn live_terminal_without_history_uses_the_full_width() {
    let area = Rect::new(3, 5, 10, 4);
    let snapshot = CommandLineTerminalSnapshot::blank(10, 4);
    assert!(command_line_scrollbar_layout(area, &snapshot).is_none());
    assert_eq!(command_line_content_area(area, &snapshot), area);
}

#[test]
fn terminal_lines_preserve_wide_graphemes_styles_and_partial_rows() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(9, 1);
    for (column, symbol) in ["界", " ", "e\u{301}", "👩‍💻", " ", "\u{1b}", "X"]
        .into_iter()
        .enumerate()
    {
        snapshot.cells[column].symbol = symbol.to_string();
    }
    snapshot.cells[0].style.bold = true;
    snapshot.cells[6].cursor = true;
    snapshot.cells.truncate(7);

    let theme = TundraTheme::default_dark();
    let line = terminal_snapshot_line(&snapshot, 0, &theme, true);
    assert_eq!(line.to_string(), "界e\u{301}👩‍💻 X");
    assert_eq!(line.width(), 7);
    assert!(line.spans[0].style.add_modifier.contains(Modifier::BOLD));
    assert!(
        line.spans
            .last()
            .unwrap()
            .style
            .add_modifier
            .contains(Modifier::REVERSED)
    );
    let native_line = terminal_snapshot_line(&snapshot, 0, &theme, false);
    assert!(
        !native_line
            .spans
            .last()
            .unwrap()
            .style
            .add_modifier
            .contains(Modifier::REVERSED)
    );
}
