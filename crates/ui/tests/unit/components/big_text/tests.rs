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
