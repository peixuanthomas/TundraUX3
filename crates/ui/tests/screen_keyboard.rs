use std::collections::{BTreeSet, HashSet};

use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect, style::Color};
use ui::{
    RenderContext, SCREEN_KEYBOARD_LETTERS, ScreenKeyboardAction, ScreenKeyboardLayout,
    ScreenKeyboardModifiers, ScreenKeyboardViewModel, TundraTheme,
    components::{ButtonFrame, Surface},
    render_screen_keyboard, screen_keyboard_layout,
};
use unicode_width::UnicodeWidthStr;

fn view(text: &str) -> ScreenKeyboardViewModel<'_> {
    ScreenKeyboardViewModel {
        text,
        focus: ScreenKeyboardAction::Letter('q'),
        modifiers: ScreenKeyboardModifiers::default(),
        last_key: "",
        message: "",
    }
}

fn draw(
    bounds: Rect,
    collapsed: bool,
    model: &ScreenKeyboardViewModel<'_>,
    context: &RenderContext,
) -> (ScreenKeyboardLayout, Buffer) {
    let layout = screen_keyboard_layout(bounds, collapsed);
    let mut terminal = Terminal::new(TestBackend::new(bounds.right(), bounds.bottom())).unwrap();
    terminal
        .draw(|frame| render_screen_keyboard(frame, bounds, &layout, model, context))
        .unwrap();
    (layout, terminal.backend().buffer().clone())
}

fn row_text(buffer: &Buffer, area: Rect, y: u16) -> String {
    let mut text = String::new();
    let mut x = area.x;
    while x < area.right() {
        let symbol = buffer[(x, y)].symbol();
        text.push_str(symbol);
        x += UnicodeWidthStr::width(symbol).max(1) as u16;
    }
    text
}

fn key_area(layout: &ScreenKeyboardLayout, action: ScreenKeyboardAction) -> Rect {
    layout
        .buttons
        .iter()
        .find(|key| key.action == action)
        .unwrap()
        .area
}

#[test]
fn keyboard_uses_seven_full_width_rows_in_the_bottom_half() {
    use ScreenKeyboardAction::*;
    for bounds in [
        Rect::new(0, 0, 60, 14),
        Rect::new(0, 0, 80, 24),
        Rect::new(5, 8, 104, 42),
        Rect::new(3, 2, 160, 60),
    ] {
        let layout = screen_keyboard_layout(bounds, false);
        assert!(layout.usable);
        assert!(!layout.collapsed);
        assert_eq!(layout.buttons.len(), 74);
        assert_eq!(layout.keyboard.y, bounds.y + bounds.height / 2);
        assert_eq!(layout.text.bottom(), layout.status.y);
        assert_eq!(layout.hint.bottom(), layout.keyboard.y);
        let letters: String = layout
            .buttons
            .iter()
            .filter_map(|key| match key.action {
                Letter(letter) => Some(letter),
                _ => None,
            })
            .collect();
        assert_eq!(letters, SCREEN_KEYBOARD_LETTERS);
        let ids: HashSet<_> = layout
            .buttons
            .iter()
            .map(|key| key.action.button_id())
            .collect();
        assert_eq!(ids.len(), layout.buttons.len());
        let rows: BTreeSet<_> = layout.buttons.iter().map(|key| key.area.y).collect();
        assert_eq!(rows.len(), 7);
        for y in rows {
            let row: Vec<_> = layout
                .buttons
                .iter()
                .filter(|key| key.area.y == y)
                .collect();
            assert_eq!(row.first().unwrap().area.x, bounds.x);
            assert_eq!(row.last().unwrap().area.right(), bounds.right());
        }
        for (index, key) in layout.buttons.iter().enumerate() {
            assert!(!key.area.is_empty());
            assert_eq!(key.area.intersection(bounds), key.area);
            assert_eq!(key.area.intersection(layout.keyboard), key.area);
            assert_eq!(key.region().area, key.area);
            for other in layout.buttons.iter().skip(index + 1) {
                assert!(key.area.intersection(other.area).is_empty());
            }
        }
        assert_eq!(
            key_area(&layout, Backspace).y,
            key_area(&layout, Character('1')).y
        );
        assert_eq!(key_area(&layout, Backspace).right(), bounds.right());
        assert_eq!(key_area(&layout, Enter).y, key_area(&layout, Letter('a')).y);
        assert_eq!(key_area(&layout, Enter).right(), bounds.right());
        assert!(key_area(&layout, Tab).x < key_area(&layout, Letter('q')).x);
        assert!(key_area(&layout, CapsLock).x < key_area(&layout, Letter('a')).x);
        assert!(key_area(&layout, Shift).x < key_area(&layout, Letter('z')).x);
        assert!(key_area(&layout, Space).width >= key_area(&layout, LeftCtrl).width * 2);
        assert_eq!(key_area(&layout, RightCtrl).right(), bounds.right());
    }
    let compact = screen_keyboard_layout(Rect::new(0, 0, 80, 24), false);
    assert!(!compact.bordered_buttons);
    let bordered = screen_keyboard_layout(Rect::new(0, 0, 104, 42), false);
    assert!(bordered.bordered_buttons);
    assert!(bordered.buttons.iter().all(|key| key.area.height == 3));
    let wide = screen_keyboard_layout(Rect::new(0, 0, 160, 24), false);
    assert!(key_area(&wide, Letter('q')).width > key_area(&compact, Letter('q')).width);
}

#[test]
fn letter_rows_align_for_vertical_keyboard_navigation() {
    use ScreenKeyboardAction::*;
    for bounds in [
        Rect::new(0, 0, 60, 14),
        Rect::new(0, 0, 80, 24),
        Rect::new(5, 8, 104, 42),
        Rect::new(3, 2, 160, 60),
    ] {
        let layout = screen_keyboard_layout(bounds, false);
        for (upper, lower) in [('q', 'a'), ('w', 's'), ('e', 'd')] {
            if bounds.width < 80 && upper != 'q' {
                continue;
            }
            let current = key_area(&layout, Letter(upper));
            let center = current.x + current.width / 2;
            let next = layout
                .buttons
                .iter()
                .filter(|key| key.area.y > current.y)
                .min_by_key(|key| {
                    (
                        key.area.y.abs_diff(current.y),
                        (key.area.x + key.area.width / 2).abs_diff(center),
                    )
                })
                .unwrap();
            assert_eq!(next.action, Letter(lower), "{bounds:?}: {upper} down");
        }
        if bounds.width == 80 {
            assert!(key_area(&layout, CapsLock).width <= 10);
            assert!(key_area(&layout, Tab).width <= 10);
        }
    }
}

#[test]
fn keys_and_shift_cover_all_printable_ascii() {
    use ScreenKeyboardAction::*;
    let layout = screen_keyboard_layout(Rect::new(0, 0, 80, 24), false);
    for action in [
        Escape,
        Tab,
        CapsLock,
        Backspace,
        Enter,
        Shift,
        Space,
        LeftCtrl,
        RightCtrl,
        Alt,
        ToggleKeyboard,
        Copy,
        Paste,
        Clear,
        Exit,
    ] {
        assert!(layout.buttons.iter().any(|key| key.action == action));
    }
    for function in 1..=12 {
        assert!(
            layout
                .buttons
                .iter()
                .any(|key| key.action == Function(function))
        );
    }
    let mut printed = BTreeSet::new();
    for shift in [false, true] {
        let modifiers = ScreenKeyboardModifiers {
            shift,
            ..Default::default()
        };
        for key in &layout.buttons {
            if let Some(character) = key.action.character(modifiers) {
                if !character.is_control() {
                    printed.insert(character);
                }
            }
        }
    }
    assert_eq!(printed, (' '..='~').collect());
    for (shift, caps_lock, expected) in [
        (false, false, 'a'),
        (true, false, 'A'),
        (false, true, 'A'),
        (true, true, 'a'),
    ] {
        assert_eq!(
            Letter('a').character(ScreenKeyboardModifiers {
                shift,
                caps_lock,
                ..Default::default()
            }),
            Some(expected)
        );
    }
    assert_eq!(Tab.character(Default::default()), Some('\t'));
    assert_eq!(Enter.character(Default::default()), Some('\n'));
}

#[test]
fn collapsed_keyboard_keeps_toolbar_and_gives_text_more_room() {
    use ScreenKeyboardAction::*;
    let bounds = Rect::new(0, 0, 80, 24);
    let expanded = screen_keyboard_layout(bounds, false);
    let (collapsed, buffer) = draw(bounds, true, &view("kept text"), &RenderContext::default());
    assert!(collapsed.collapsed);
    assert_eq!(collapsed.buttons.len(), 5);
    assert!(collapsed.text.height > expanded.text.height);
    assert!(
        collapsed
            .buttons
            .iter()
            .all(|key| key.area.y == bounds.bottom() - 1)
    );
    assert!(
        collapsed
            .buttons
            .iter()
            .any(|key| key.action == ToggleKeyboard)
    );
    assert!(collapsed.buttons.iter().any(|key| key.action == Copy));
    assert!(collapsed.buttons.iter().any(|key| key.action == Paste));
    assert!(
        row_text(
            &buffer,
            key_area(&collapsed, ToggleKeyboard),
            bounds.bottom() - 1
        )
        .contains(&i18n::tr!("screen-keyboard-show"))
    );
}

#[test]
fn small_windows_render_safely_and_keep_controls_reachable() {
    for width in [0, 1, 2, 3, 4, 5, 10, 59, 60, 80, 104] {
        for height in [0, 1, 2, 3, 8, 13, 14, 24, 42] {
            for collapsed in [false, true] {
                let bounds = Rect::new(2, 3, width, height);
                let (layout, _) = draw(
                    bounds,
                    collapsed,
                    &view("界🙂\nnew\tline"),
                    &RenderContext::default(),
                );
                if width > 0 && height > 0 {
                    assert!(
                        layout
                            .buttons
                            .iter()
                            .any(|key| key.action == ScreenKeyboardAction::Exit)
                    );
                    if width > 1 {
                        assert!(
                            layout
                                .buttons
                                .iter()
                                .any(|key| key.action == ScreenKeyboardAction::ToggleKeyboard)
                        );
                    }
                }
                for key in &layout.buttons {
                    assert!(!key.area.is_empty());
                    assert_eq!(key.area.intersection(bounds), key.area);
                }
            }
        }
    }
    // Layout arithmetic must also remain safe near the terminal coordinate limit.
    let largest = screen_keyboard_layout(Rect::new(0, 0, u16::MAX, u16::MAX), false);
    assert!(
        largest
            .buttons
            .iter()
            .all(|key| key.area.intersection(largest.keyboard) == key.area)
    );
}

#[test]
fn keycaps_are_literal_and_latches_do_not_reuse_hover_colors() {
    use ScreenKeyboardAction::*;
    for bounds in [Rect::new(0, 0, 80, 24), Rect::new(0, 0, 104, 42)] {
        let mut model = view("");
        model.modifiers.shift = true;
        model.modifiers.left_ctrl = true;
        model.modifiers.right_ctrl = true;
        model.modifiers.alt = true;
        model.modifiers.caps_lock = true;
        let base = TundraTheme::default().with_accent_color(Color::Rgb(100, 140, 180));
        let mut context = RenderContext::from_theme(&base, Default::default(), Default::default());
        let mut interactions = ButtonFrame::new(None, None, &base);
        interactions.keyboard_focus_visible = false;
        context.buttons = Some(interactions);
        let (layout, buffer) = draw(bounds, false, &model, &context);
        for (action, expected) in [
            (Letter('q'), "Q"),
            (Character('1'), "!"),
            (Character('['), "{"),
            (Character(']'), "}"),
            (Shift, "Shift*"),
            (CapsLock, "CapsLock*"),
            (LeftCtrl, "Ctrl*"),
            (RightCtrl, "RCtrl*"),
            (Alt, "Alt*"),
            (Function(12), "F12"),
        ] {
            let area = key_area(&layout, action);
            let inner = if layout.bordered_buttons {
                Rect::new(area.x + 1, area.y + 1, area.width - 2, 1)
            } else {
                area
            };
            assert_eq!(row_text(&buffer, inner, inner.y).trim(), expected);
            for x in inner.x..inner.right() {
                assert_eq!(buffer[(x, inner.y)].fg, context.theme.text);
            }
        }
        model.modifiers = Default::default();
        let (layout, buffer) = draw(bounds, false, &model, &context);
        for action in [Character('['), Character(']')] {
            let area = key_area(&layout, action);
            let inner = if layout.bordered_buttons {
                Rect::new(area.x + 1, area.y + 1, area.width - 2, 1)
            } else {
                area
            };
            assert_eq!(
                row_text(&buffer, inner, inner.y).trim(),
                action.character(Default::default()).unwrap().to_string()
            );
        }
    }
}

#[test]
fn typed_text_wraps_wide_graphemes_and_keeps_newest_lines() {
    let bounds = Rect::new(0, 0, 60, 14);
    let text = format!("old line\n{}\nlatest🙂e\u{301}\tend", "界".repeat(80));
    let (layout, buffer) = draw(bounds, false, &view(&text), &RenderContext::default());
    let inner = Surface::new().bordered(true).inner(layout.text);
    assert_eq!(inner.height, 2);
    let top = row_text(&buffer, inner, inner.y);
    let latest = row_text(&buffer, inner, inner.y + 1);
    assert!(top.contains('界'));
    assert!(!top.contains("old"));
    assert!(latest.starts_with("latest🙂e\u{301}   end"));
    assert!(!latest.contains('\u{fffd}'));
    assert_eq!(buffer[(inner.x + 6, inner.y + 1)].symbol(), "🙂");
    assert_eq!(buffer[(inner.x + 8, inner.y + 1)].symbol(), "e\u{301}");

    let (layout, buffer) = draw(
        bounds,
        false,
        &view("first\nsecond\n"),
        &RenderContext::default(),
    );
    let inner = Surface::new().bordered(true).inner(layout.text);
    assert!(row_text(&buffer, inner, inner.y).starts_with("second"));
    assert!(row_text(&buffer, inner, inner.y + 1).trim().is_empty());
}

#[test]
fn recent_key_and_clipboard_message_have_a_separate_status_line() {
    let mut model = view("typed text");
    model.last_key = "RCtrl+F12";
    model.message = "copied";
    let (layout, buffer) = draw(
        Rect::new(0, 0, 80, 24),
        false,
        &model,
        &RenderContext::default(),
    );
    let status = row_text(&buffer, layout.status, layout.status.y);
    assert!(status.contains("RCtrl+F12"));
    assert!(status.contains("copied"));
}
