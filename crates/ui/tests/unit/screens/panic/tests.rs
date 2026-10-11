use super::*;
use ratatui::{Terminal, backend::TestBackend};

fn draw(screen: &mut PanicScreen, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| screen.render(frame)).unwrap();
    // Wide characters own their trailing cell; TestBackend resets that
    // placeholder instead of storing a separately painted background.
    let mut cells = terminal.backend().buffer().content.iter();
    while let Some(cell) = cells.next() {
        assert_eq!(cell.bg, Color::Black);
        for _ in 1..cell.symbol().width() {
            cells.next();
        }
    }
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn crash_page_shows_hardcoded_art_and_error_without_assets() {
    let mut screen = PanicScreen::new("Intentional panic: 文件读取失败");
    let rendered = draw(&mut screen, 100, 30);
    assert!(rendered.contains("X               X"));
    assert!(rendered.contains(".'      '."));
    assert!(rendered.contains(r"/_______/________________\________\"));
    assert!(rendered.contains("Intentional panic:"));
    // The test backend includes the blank trailing cell of wide characters.
    assert!(rendered.replace(' ', "").contains("文件读取失败"));
    assert!(rendered.contains("R: Restart | Q: Exit"));
    assert!(!rendered.contains("has stopped"));
    assert!(!rendered.contains("Scroll"));
    assert!(rendered.find("Intentional panic:") < rendered.find("X               X"));
}

#[test]
fn small_crash_page_prioritizes_details_and_filters_terminal_controls() {
    let mut screen = PanicScreen::new(
        "Reason: index out of bounds\nTask: pty-reader-1\nLocation: row.rs:99:28\nFull report: /tmp/crash.txt\x07",
    );
    let rendered = draw(&mut screen, 60, 8);
    for detail in [
        "index out of bounds",
        "pty-reader-1",
        "row.rs:99:28",
        "/tmp/crash.txt",
    ] {
        assert!(rendered.contains(detail));
    }
    assert!(!rendered.contains("X               X"));
    assert!(!screen.text.contains('\x07'));
    assert_eq!(screen.max_scroll, 0);
}

#[test]
fn long_unicode_error_can_be_scrolled_and_resize_clamps_position() {
    let message = format!("{}\nLAST ERROR", "报错文本".repeat(100));
    let mut screen = PanicScreen::new(&message);
    draw(&mut screen, 24, 8);
    screen.scroll_to_end();
    assert!(draw(&mut screen, 24, 8).contains("LAST ERROR"));
    screen.scroll_up(true);
    assert!(screen.scroll < screen.max_scroll);
    draw(&mut screen, 120, 50);
    assert_eq!(screen.scroll, 0);
    for (width, height) in [(0, 0), (1, 1), (2, 3), (10, 5)] {
        draw(&mut screen, width, height);
    }
}
