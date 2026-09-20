use super::{terminal_width, truncate_to_terminal_width};

#[test]
fn terminal_width_uses_ratatui_cell_width_for_cjk_and_emoji() {
    assert_eq!(terminal_width("中文"), 4);
    assert_eq!(terminal_width("日本"), 4);
    assert_eq!(terminal_width("🙂"), 2);
}

#[test]
fn truncation_keeps_wide_graphemes_intact() {
    assert_eq!(truncate_to_terminal_width("A中文B", 3), "A中");
    assert_eq!(truncate_to_terminal_width("A🙂B", 3), "A🙂");
}
