use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::{
    RenderContext, SCREEN_KEYBOARD_LETTERS, ScreenKeyboardAction, ScreenKeyboardViewModel,
    render_screen_keyboard, screen_keyboard_layout,
};

#[test]
fn screen_keyboard_has_standard_qwerty_rows_and_separate_actions() {
    for bounds in [Rect::new(0, 0, 80, 24), Rect::new(5, 8, 39, 10)] {
        let layout = screen_keyboard_layout(bounds);
        assert!(layout.usable);
        assert_eq!(layout.buttons.len(), 29);
        let letters = layout
            .buttons
            .iter()
            .filter_map(|button| match button.action {
                ScreenKeyboardAction::Letter(letter) => Some(letter),
                _ => None,
            })
            .collect::<String>();
        assert_eq!(letters, SCREEN_KEYBOARD_LETTERS);
        assert_eq!(layout.buttons[0].area.y, layout.buttons[9].area.y);
        assert_eq!(layout.buttons[10].area.y, layout.buttons[18].area.y);
        assert_eq!(layout.buttons[19].area.y, layout.buttons[25].area.y);
        assert!(layout.buttons[0].area.x < layout.buttons[10].area.x);
        assert!(layout.buttons[10].area.x < layout.buttons[19].area.x);
        for button in &layout.buttons {
            assert_eq!(button.area.intersection(bounds), button.area);
            assert_eq!(button.region().area, button.area);
            assert!(button.area.width > 0 && button.area.height > 0);
        }
        for (index, button) in layout.buttons.iter().enumerate() {
            for other in layout.buttons.iter().skip(index + 1) {
                assert!(button.area.intersection(other.area).is_empty());
            }
        }
    }
}

#[test]
fn screen_keyboard_small_windows_render_safely_and_keep_exit_visible() {
    for width in [1, 10, 38, 39, 58, 59, 80] {
        for height in [1, 2, 9, 10, 17, 18, 24] {
            let bounds = Rect::new(0, 0, width, height);
            let layout = screen_keyboard_layout(bounds);
            if height > 1 {
                assert!(
                    layout
                        .buttons
                        .iter()
                        .any(|button| button.action == ScreenKeyboardAction::Exit)
                );
            }
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    render_screen_keyboard(
                        frame,
                        bounds,
                        &layout,
                        &ScreenKeyboardViewModel {
                            text: "abcdefghijklmnopqrstuvwxyz",
                            focus: ScreenKeyboardAction::Exit,
                        },
                        &RenderContext::default(),
                    );
                })
                .unwrap();
        }
    }
}

#[test]
fn screen_keyboard_long_text_shows_the_latest_letters() {
    let bounds = Rect::new(0, 0, 39, 10);
    let layout = screen_keyboard_layout(bounds);
    let mut terminal = Terminal::new(TestBackend::new(39, 10)).unwrap();
    let text = format!("{}xyz", "a".repeat(100));
    terminal
        .draw(|frame| {
            render_screen_keyboard(
                frame,
                bounds,
                &layout,
                &ScreenKeyboardViewModel {
                    text: &text,
                    focus: ScreenKeyboardAction::Letter('q'),
                },
                &RenderContext::default(),
            );
        })
        .unwrap();
    let inner_x = layout.text.x + 1;
    let text_row = layout.text.y + 1;
    let visible = (inner_x..layout.text.right() - 1)
        .map(|x| terminal.backend().buffer()[(x, text_row)].symbol())
        .collect::<String>();
    assert_eq!(visible, format!("{}xyz", "a".repeat(34)));
}
