use crate::TundraTheme;
use crate::components::{Button, ButtonEmphasis};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn render(button: &Button, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, 3);
    let mut buffer = Buffer::empty(area);
    button.render(area, &mut buffer, &TundraTheme::default_dark());
    buffer
}

#[test]
fn primary_normal_uses_accent_label_and_border() {
    let theme = TundraTheme::default_dark();
    let button = Button::new("save", "Save").with_emphasis(ButtonEmphasis::Primary);
    let buffer = render(&button, 12);
    assert_eq!(buffer[(1, 1)].fg, theme.accent_color);
    assert_eq!(buffer[(0, 1)].fg, theme.accent_color);
    assert!(
        buffer[(1, 1)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
}

#[test]
fn danger_normal_uses_error_label_and_border() {
    let theme = TundraTheme::default_dark();
    let button = Button::new("delete", "Delete").with_emphasis(ButtonEmphasis::Danger);
    let buffer = render(&button, 12);
    assert_eq!(buffer[(1, 1)].fg, theme.error);
    assert_eq!(buffer[(0, 1)].fg, theme.error);
}

#[test]
fn focus_fill_changes_background_but_keeps_danger_text() {
    let theme = TundraTheme::default_dark();
    let mut button = Button::new("delete", "Delete")
        .with_emphasis(ButtonEmphasis::Danger)
        .with_focus_fill(Some(ratatui::style::Color::Blue));
    button.state.focused = true;
    let buffer = render(&button, 12);
    assert_eq!(buffer[(1, 1)].fg, theme.error);
    assert_eq!(buffer[(1, 1)].bg, ratatui::style::Color::Blue);
}

#[test]
fn disabled_wins_over_emphasis_and_focus() {
    let mut button = Button::new("delete", "Delete")
        .with_emphasis(ButtonEmphasis::Danger)
        .with_focus_fill(Some(ratatui::style::Color::Blue));
    button.state.focused = true;
    button.set_disabled(true);
    let buffer = render(&button, 12);
    assert_eq!(buffer[(1, 1)].fg, ratatui::style::Color::DarkGray);
    assert_ne!(buffer[(1, 1)].bg, ratatui::style::Color::Blue);
}
