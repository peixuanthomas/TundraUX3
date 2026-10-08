use std::collections::{BTreeSet, HashSet};

use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect, style::Color};
use ui::{
    RenderContext, SCREEN_KEYBOARD_LETTERS, ScreenKeyboardAction, ScreenKeyboardLayout,
    ScreenKeyboardModifiers, ScreenKeyboardViewModel, TundraTheme,
    components::{ButtonFrame, Surface},
    render_screen_keyboard, screen_keyboard_layout, screen_keyboard_layout_with_visibility,
};
use unicode_width::UnicodeWidthStr;

fn view(text: &str) -> ScreenKeyboardViewModel<'_> {
    ScreenKeyboardViewModel {
        text,
        focus: ScreenKeyboardAction::Letter('q'),
        modifiers: ScreenKeyboardModifiers::default(),
        physical_pressed: &[],
        last_key: "",
        message: "",
    }
}

#[test]
fn aa_dialog_stays_inside_screen_and_above_every_animation_frame() {
    let context = RenderContext::default();
    for width in [0, 1, 12, 40, 59, 60, 80, 120, 160] {
        for height in [0, 1, 4, 12, 17, 18, 24, 42, 60] {
            let bounds = Rect::new(3, 4, width, height);
            let hidden = ui::screen_keyboard_aa_layout(bounds, true, 0, &context);
            for visibility in [0, 1, 100, 500, 999, 1_000] {
                for collapsed in [false, true] {
                    let layout =
                        ui::screen_keyboard_aa_layout(bounds, collapsed, visibility, &context);
                    if !layout.aa_dialog.is_empty() {
                        assert_eq!(layout.aa_dialog.intersection(bounds), layout.aa_dialog);
                    }
                    assert!(layout.aa_dialog.intersection(layout.keyboard).is_empty());
                    if visibility == 1_000 && layout.usable {
                        assert!(layout.aa_dialog.y <= hidden.aa_dialog.y);
                        assert_eq!(layout.keyboard.bottom(), bounds.bottom());
                        assert!(
                            layout
                                .buttons
                                .iter()
                                .any(|key| key.action == ScreenKeyboardAction::Letter('a'))
                        );
                    }
                    let mut ids = HashSet::new();
                    for button in &layout.buttons {
                        assert!(ids.insert(button.action.button_id()));
                        assert_eq!(button.area.intersection(bounds), button.area);
                        assert!(
                            button.area.intersection(layout.keyboard) == button.area
                                || button.area.intersection(layout.aa_dialog) == button.area
                        );
                    }
                    let mut terminal =
                        Terminal::new(TestBackend::new(bounds.right(), bounds.bottom())).unwrap();
                    terminal
                        .draw(|frame| {
                            ui::render_screen_keyboard_aa_test(
                                frame,
                                &layout,
                                &view("AA input"),
                                &context,
                            );
                            let before = frame.buffer_mut().clone();
                            ui::render_screen_keyboard_panel(
                                frame,
                                &layout,
                                &view("AA input"),
                                &context,
                            );
                            for y in layout.aa_dialog.y..layout.aa_dialog.bottom() {
                                for x in layout.aa_dialog.x..layout.aa_dialog.right() {
                                    assert_eq!(
                                        before[(x, y)],
                                        frame.buffer_mut()[(x, y)],
                                        "keyboard painted over AA"
                                    );
                                }
                            }
                        })
                        .unwrap();
                }
            }
        }
    }
}

#[test]
fn physical_presses_and_virtual_latches_share_themed_keycap_colors() {
    use ScreenKeyboardAction::*;
    for bounds in [Rect::new(0, 0, 80, 24), Rect::new(0, 0, 120, 48)] {
        for capabilities in [
            ui::RenderCapabilities::default(),
            ui::RenderCapabilities::ansi(),
        ] {
            let theme = TundraTheme::default_dark().with_accent_color(Color::Rgb(32, 64, 96));
            let mut context = RenderContext::from_theme(&theme, Default::default(), capabilities);
            let mut buttons = ButtonFrame::new(None, None, &context.compatibility_theme());
            buttons.keyboard_focus_visible = false;
            context.buttons = Some(buttons);
            let physical = [
                Letter('a'),
                Character('1'),
                RightCtrl,
                Tab,
                Enter,
                Space,
                Backspace,
                Function(12),
            ];
            let mut model = view("");
            model.physical_pressed = &physical;
            model.modifiers.shift = true;
            let (layout, buffer) = draw(bounds, false, &model, &context);
            let expected = context.compatibility_theme().button_pressed_color();
            for action in physical.into_iter().chain([Shift]) {
                let area = key_area(&layout, action);
                assert_eq!(buffer[(area.x, area.y)].fg, expected, "{action:?}");
                for y in area.y..area.bottom() {
                    for x in area.x..area.right() {
                        if !buffer[(x, y)].symbol().trim().is_empty() {
                            assert_eq!(buffer[(x, y)].fg, expected, "{action:?}");
                        }
                    }
                }
            }
            model.physical_pressed = &[];
            let (_, released) = draw(bounds, false, &model, &context);
            let shift = key_area(&layout, Shift);
            assert_eq!(released[(shift.x, shift.y)].fg, expected);
            let a = key_area(&layout, Letter('a'));
            assert_ne!(released[(a.x, a.y)].fg, expected);
        }
    }
}

#[test]
fn keyboard_panel_clears_page_and_chrome_text_between_keycaps() {
    let bounds = Rect::new(0, 0, 80, 24);
    let context = RenderContext::default();
    let layout = ui::screen_keyboard_aa_layout(bounds, false, 1_000, &context);
    let render = |underlying_text: bool| {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                if underlying_text {
                    for y in layout.keyboard.y..layout.keyboard.bottom() {
                        for x in layout.keyboard.x..layout.keyboard.right() {
                            frame.buffer_mut()[(x, y)].set_symbol("X");
                        }
                    }
                }
                ui::render_screen_keyboard_panel(frame, &layout, &view(""), &context);
            })
            .unwrap();
        terminal.backend().buffer().clone()
    };
    assert_eq!(render(true), render(false));
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
        assert_eq!(layout.buttons.len(), 75);
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
    assert_eq!(
        key_area(&compact, ScreenKeyboardAction::Letter('q')).height,
        2
    );
    let bordered = screen_keyboard_layout(Rect::new(0, 0, 104, 42), false);
    assert!(bordered.bordered_buttons);
    assert_eq!(
        key_area(&bordered, ScreenKeyboardAction::Letter('q')).height,
        3
    );
    assert_eq!(
        key_area(&bordered, ScreenKeyboardAction::ToggleKeyboard).height,
        1
    );
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
    assert_eq!(collapsed.buttons.len(), 6);
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
fn keycaps_are_literal_and_latches_stay_bright_without_pointer_or_keyboard_focus() {
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
            (Letter('q'), "q"),
            (Character('1'), "!"),
            (Character('['), "{"),
            (Character(']'), "}"),
            (Shift, "Shift"),
            (CapsLock, "CapsLock"),
            (LeftCtrl, "Ctrl"),
            (RightCtrl, "RCtrl"),
            (Alt, "Alt"),
            (Function(12), "F12"),
        ] {
            let area = key_area(&layout, action);
            let inner = if layout.bordered_buttons {
                Rect::new(area.x + 1, area.y + 1, area.width - 2, 1)
            } else {
                area
            };
            let row = (inner.y..inner.bottom())
                .find(|&y| row_text(&buffer, inner, y).trim() == expected)
                .unwrap_or_else(|| panic!("missing {expected} in {area:?}"));
            for x in inner.x..inner.right() {
                assert_eq!(
                    buffer[(x, row)].fg,
                    if action.is_latched(model.modifiers) {
                        context.compatibility_theme().button_pressed_color()
                    } else {
                        context.theme.text
                    }
                );
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
            let expected = action.character(Default::default()).unwrap().to_string();
            assert!(
                (inner.y..inner.bottom()).any(|y| row_text(&buffer, inner, y).trim() == expected)
            );
        }
    }
}

#[test]
fn keys_show_only_the_current_character_with_shift_and_caps_lock() {
    use ScreenKeyboardAction::*;
    for (bounds, bordered) in [
        (Rect::new(0, 0, 60, 14), false),
        (Rect::new(0, 0, 80, 24), false),
        (Rect::new(0, 0, 104, 42), true),
        (Rect::new(0, 0, 80, 32), false),
        (Rect::new(0, 0, 104, 56), true),
    ] {
        for capabilities in [
            ui::RenderCapabilities::default(),
            ui::RenderCapabilities::ansi(),
        ] {
            let theme = TundraTheme::default_dark();
            let mut context = RenderContext::from_theme(&theme, Default::default(), capabilities);
            let mut interactions = ButtonFrame::new(None, None, &context.compatibility_theme());
            interactions.keyboard_focus_visible = false;
            context.buttons = Some(interactions);
            for (shift, caps_lock) in [(false, false), (true, false), (false, true), (true, true)] {
                let mut model = view("");
                model.modifiers.shift = shift;
                model.modifiers.caps_lock = caps_lock;
                let (layout, buffer) = draw(bounds, false, &model, &context);
                assert_eq!(layout.bordered_buttons, bordered);
                for (action, expected) in [
                    (Character('1'), if shift { "!" } else { "1" }),
                    (Character('['), if shift { "{" } else { "[" }),
                    (Letter('q'), if shift ^ caps_lock { "Q" } else { "q" }),
                ] {
                    let area = key_area(&layout, action);
                    let inner = if bordered {
                        Rect::new(area.x + 1, area.y + 1, area.width - 2, area.height - 2)
                    } else {
                        area
                    };
                    let labels: Vec<_> = (inner.y..inner.bottom())
                        .map(|y| row_text(&buffer, inner, y).trim().to_owned())
                        .filter(|label| !label.is_empty())
                        .collect();
                    assert_eq!(labels, [expected], "{bounds:?}, {action:?}");
                    let center_y = inner.y + inner.height.saturating_sub(1) / 2;
                    assert_eq!(row_text(&buffer, inner, center_y).trim(), expected);
                    for x in inner.x..inner.right() {
                        assert_eq!(buffer[(x, center_y)].fg, context.theme.text);
                    }
                    assert_eq!(
                        action.character(model.modifiers).unwrap().to_string(),
                        expected
                    );
                }
            }
        }
    }
}

#[test]
fn sliding_keyboard_keeps_toolbar_fixed_and_matches_visible_hit_regions() {
    use ScreenKeyboardAction::*;
    for bounds in [
        Rect::new(0, 0, 80, 24),
        Rect::new(4, 3, 104, 56),
        Rect::new(0, 0, u16::MAX, u16::MAX),
    ] {
        let expanded = screen_keyboard_layout(bounds, false);
        let toolbar = key_area(&expanded, ToggleKeyboard);
        let mut previous_y = expanded.keyboard.y;
        for visibility in [1_000, 875, 500, 250, 1, 0] {
            let layout = screen_keyboard_layout_with_visibility(bounds, true, visibility);
            assert_eq!(key_area(&layout, ToggleKeyboard), toolbar);
            assert!(layout.keyboard.y >= previous_y);
            previous_y = layout.keyboard.y;
            assert_eq!(layout.hint.bottom(), layout.keyboard.y);
            for (index, button) in layout.buttons.iter().enumerate() {
                assert_eq!(button.area.intersection(bounds), button.area);
                assert_eq!(button.region().area, button.area);
                for other in layout.buttons.iter().skip(index + 1) {
                    assert!(button.area.intersection(other.area).is_empty());
                }
            }
            if visibility == 0 {
                assert_eq!(layout.buttons.len(), 6);
            }
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
