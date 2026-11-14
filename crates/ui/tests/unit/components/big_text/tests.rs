use super::*;

#[test]
fn widget_marks_the_escape_sequence_width_and_skips_its_covered_cells() {
    let area = Rect::new(0, 0, 12, 2);
    let mut buffer = Buffer::empty(area);

    BigText::new("Title", 1, Color::Gray).render(area, &mut buffer);

    assert!(
        buffer[(0, 0)]
            .symbol()
            .contains("]66;s=2:n=7:d=7:w=5;Title")
    );
    assert_eq!(
        buffer[(0, 0)].diff_option,
        CellDiffOption::ForcedWidth(NonZeroU16::new(5).unwrap())
    );
    assert_eq!(buffer[(1, 0)].diff_option, CellDiffOption::Skip);
    assert_eq!(buffer[(0, 1)].diff_option, CellDiffOption::Skip);
}

#[test]
fn wide_glyphs_reserve_their_full_terminal_width() {
    let area = Rect::new(0, 0, 4, 2);
    let mut buffer = Buffer::empty(area);

    BigText::new("好", 1, Color::Gray).render(area, &mut buffer);

    assert!(buffer[(0, 0)].symbol().contains("w=2;好"));
    assert_eq!(
        buffer[(0, 0)].diff_option,
        CellDiffOption::ForcedWidth(NonZeroU16::new(2).unwrap())
    );
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if (x, y) != (area.left(), area.top()) {
                assert_eq!(buffer[(x, y)].diff_option, CellDiffOption::Skip);
            }
        }
    }
}

#[test]
fn workspace_heading_uses_cells_in_the_worker_and_osc66_only_in_the_pane() {
    use ratatui::{Terminal, backend::TestBackend};

    let content = Rect::new(2, 1, 9, 4);
    let area = Rect::new(3, 2, 6, 2);
    let mut buffer = Buffer::empty(content);
    let capture = WorkspaceHeadingCapture::begin(content);
    BigText::new("Title", 1, Color::Gray).render(area, &mut buffer);
    let mut headings = capture.finish();
    assert_eq!(buffer[(3, 2)].symbol(), "T");
    assert_eq!(headings[0].area.x, 1);
    assert_eq!(headings[0].area.y, 1);

    let mut malformed = headings[0].clone();
    malformed.area.y = 3;
    malformed.text = "\x1b[31m".into();
    headings.push(malformed);
    let mut terminal = Terminal::new(TestBackend::new(16, 10)).unwrap();
    terminal
        .draw(|frame| {
            frame.buffer_mut()[(12, 5)].set_symbol("X");
            render_workspace_headings(
                frame,
                Rect::new(5, 4, 9, 4),
                &headings,
                &crate::RenderContext::default(),
            );
        })
        .unwrap();
    let output = terminal.backend().buffer();
    assert!(
        output[(6, 5)]
            .symbol()
            .contains("]66;s=2:n=7:d=7:w=5;Title")
    );
    assert_eq!(output[(12, 5)].symbol(), "X");
    assert!(!output[(6, 7)].symbol().contains('\x1b'));
}

#[test]
fn oversized_and_control_input_does_not_escape_the_heading_area() {
    let area = Rect::new(0, 0, 8, 2);
    let mut buffer = Buffer::empty(area);
    BigText::new("\x1bInvalidCharacterSequence", 1, Color::Gray).render(area, &mut buffer);
    let symbol = buffer[(0, 0)].symbol();
    assert!(symbol.contains(";Invalid"));
    assert!(!symbol.contains("CharacterSequence"));
    assert!(!symbol.contains("\x1bInvalid"));
    assert!(symbol.ends_with("\x1b[?7h\x1b[0m"));
}
