use super::{fit_cell, text_width, truncate_status_text};
use ratatui::text::Line;

#[test]
fn cell_fitting_and_status_truncation_use_terminal_display_width() {
    assert_eq!(text_width("界面"), 4);
    assert_eq!(fit_cell("界面", 5), "界面 ");
    assert_eq!(fit_cell("界面", 3), "界…");
    assert_eq!(Line::from(fit_cell("界面", 3)).width(), 3);
    assert_eq!(truncate_status_text("界面状态", 5), "界...");
    assert_eq!(Line::from(truncate_status_text("界面状态", 5)).width(), 5);
}
