use super::*;

#[test]
fn centered_text_uses_display_cells_instead_of_bytes_or_scalars() {
    assert_eq!(centered_column(20, "按空格键开始".width()), 4);
    assert_eq!(centered_column(20, "e\u{301}".width()), 9);
    assert_eq!(centered_column(4, "按空格键开始".width()), 0);
}

#[test]
fn translated_lines_keep_graphemes_and_clip_whole_wide_characters() {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 7, 1));
    write_line(&mut buffer, 0, 0, "中e\u{301}文AB", Color::Cyan);
    assert_eq!(buffer[(0, 0)].symbol(), "中");
    assert_eq!(buffer[(2, 0)].symbol(), "e\u{301}");
    assert_eq!(buffer[(3, 0)].symbol(), "文");
    assert_eq!(buffer[(5, 0)].symbol(), "A");
    assert_eq!(buffer[(6, 0)].symbol(), "B");

    buffer.reset();
    write_line(&mut buffer, 0, 0, "中文中文", Color::Cyan);
    assert_eq!(buffer[(4, 0)].symbol(), "中");
    assert_eq!(buffer[(6, 0)].symbol(), " ");
    write_line(&mut buffer, u16::MAX, u16::MAX, "outside", Color::Red);
}

#[test]
fn differential_output_skips_wide_tails_and_clears_old_text() {
    let empty = Buffer::empty(Rect::new(0, 0, 8, 1));
    let mut chinese = empty.clone();
    write_line(&mut chinese, 0, 0, "中文A", Color::Cyan);
    let mut output = Vec::new();
    write_changed_cells(&mut output, &empty, &chinese).unwrap();
    let output = String::from_utf8(output).unwrap();
    assert!(output.contains("中文A"));
    assert_eq!(output.matches("\x1b[1;1H").count(), 1);

    let mut english = empty.clone();
    write_line(&mut english, 0, 0, "OK", Color::Cyan);
    let updates = chinese.diff(&english);
    assert!(
        updates
            .iter()
            .any(|(x, _, cell)| *x == 4 && cell.symbol() == " ")
    );
    let mut output = Vec::new();
    write_changed_cells(&mut output, &english, &english).unwrap();
    assert!(output.is_empty());
}

#[test]
fn terminal_size_validation_accepts_the_boundary_and_larger_sizes() {
    assert!(validate_terminal_size(MIN_TERMINAL_WIDTH, MIN_TERMINAL_HEIGHT).is_ok());
    assert!(validate_terminal_size(u16::MAX, u16::MAX).is_ok());
}

#[test]
fn terminal_size_validation_rejects_each_undersized_dimension() {
    for (width, height) in [
        (MIN_TERMINAL_WIDTH - 1, MIN_TERMINAL_HEIGHT),
        (MIN_TERMINAL_WIDTH, MIN_TERMINAL_HEIGHT - 1),
        (MIN_TERMINAL_WIDTH - 1, MIN_TERMINAL_HEIGHT - 1),
    ] {
        assert!(matches!(
            validate_terminal_size(width, height),
            Err(TerminalError::TooSmall {
                width: actual_width,
                height: actual_height,
                min_width: MIN_TERMINAL_WIDTH,
                min_height: MIN_TERMINAL_HEIGHT,
            }) if actual_width == width && actual_height == height
        ));
    }
}

#[test]
fn terminal_size_validation_honors_a_larger_embedded_shell_requirement() {
    assert!(matches!(
        validate_terminal_size_with_minimum(107, 20, 108, 20),
        Err(TerminalError::TooSmall {
            width: 107,
            height: 20,
            min_width: 108,
            min_height: 20,
        })
    ));
    assert!(validate_terminal_size_with_minimum(108, 20, 108, 20).is_ok());
}

#[test]
fn terminal_size_validation_rejects_requirements_above_renderer_capacity() {
    assert!(matches!(
        validate_terminal_size_with_minimum(1200, 20, 1200, 20),
        Err(TerminalError::RequirementTooLarge {
            min_width: 1200,
            min_height: 20,
            max_width: MAX_TERMINAL_WIDTH,
            max_height: MAX_TERMINAL_HEIGHT,
        })
    ));
}
