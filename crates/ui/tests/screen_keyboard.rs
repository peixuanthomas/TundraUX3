use std::collections::{BTreeSet, HashSet};

use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect, style::Color};
use ui::{
    RenderContext, ScreenKeyboardAction, ScreenKeyboardLayout, ScreenKeyboardModifiers,
    ScreenKeyboardViewModel, TundraTheme,
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
fn ordinary_key_legends_meet_contrast_without_losing_custom_colors_or_feedback() {
    use ScreenKeyboardAction::*;
    for (theme, capabilities, ordinary) in [
        (
            TundraTheme::default_dark(),
            ui::RenderCapabilities::default(),
            Color::Rgb(109, 127, 134),
        ),
        (
            TundraTheme::default_dark().with_border_color(Color::Rgb(140, 160, 180)),
            ui::RenderCapabilities::default(),
            Color::Rgb(140, 160, 180),
        ),
        (
            TundraTheme {
                background: Color::White,
                border_color: Color::White,
                ..Default::default()
            },
            ui::RenderCapabilities::default(),
            Color::Rgb(117, 117, 117),
        ),
        (
            TundraTheme::default_dark(),
            ui::RenderCapabilities::ansi(),
            Color::DarkGray,
        ),
        (
            TundraTheme::default_dark().with_border_color(Color::Black),
            ui::RenderCapabilities::ansi(),
            Color::White,
        ),
    ] {
        for bounds in [Rect::new(0, 0, 80, 24), Rect::new(0, 0, 120, 48)] {
            let mut context = RenderContext::from_theme(&theme, Default::default(), capabilities);
            let mut model = view("");
            model.focus = Letter('w');
            let (layout, buffer) = draw(bounds, false, &model, &context);
            let q = key_area(&layout, Letter('q'));
            let w = key_area(&layout, Letter('w'));
            let center = |area: Rect| {
                (
                    area.x + (area.width - 1) / 2,
                    area.y + (area.height - 1) / 2,
                )
            };
            assert_eq!(buffer[center(q)].fg, ordinary);
            assert_eq!(buffer[center(q)].bg, context.theme.canvas);
            assert_eq!(buffer[center(w)].fg, context.theme.accent);
            let region = layout
                .buttons
                .iter()
                .find(|key| key.action == Letter('q'))
                .unwrap()
                .region();
            let mut buttons = ButtonFrame::new(
                Some(region.clone()),
                Some(region),
                &context.compatibility_theme(),
            );
            buttons.keyboard_focus_visible = false;
            context.buttons = Some(buttons);
            let (_, pressed) = draw(bounds, false, &model, &context);
            assert_eq!(pressed[center(q)].bg, context.theme.accent);
            assert_eq!(
                pressed[center(q)].fg,
                context.theme.filled_style(context.theme.accent).fg.unwrap()
            );
            context.buttons.as_mut().unwrap().pressed = None;
            context.buttons.as_mut().unwrap().hovered = None;
            let (_, released) = draw(bounds, false, &model, &context);
            assert_eq!(released[center(q)].fg, ordinary);
            assert_eq!(released[center(q)].bg, context.theme.canvas);
            if layout.bordered_buttons {
                assert_eq!(released[(q.x, q.y)].fg, Color::DarkGray);
            }
        }
    }
}

#[test]
fn live_paste_and_hide_are_regular_keys_at_the_bottom_corners() {
    use ScreenKeyboardAction::*;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for language in ["en-US", "zh-CN"] {
        let snapshot = i18n::LanguageSnapshot::load(&root, language, 1)
            .unwrap()
            .snapshot;
        let _language = i18n::enter_snapshot(std::sync::Arc::new(snapshot));
        for bounds in [
            Rect::new(0, 0, 108, 20),
            Rect::new(5, 3, 140, 40),
            Rect::new(0, 0, 120, 48),
        ] {
            for auto_admin in [false, true] {
                let layout = ui::screen_keyboard_input_layout(bounds, true, auto_admin);
                if !layout.usable {
                    continue;
                }
                let paste = key_area(&layout, Paste);
                let hide = key_area(&layout, ToggleKeyboard);
                let character = key_area(&layout, Letter('q'));
                assert_eq!(paste.x, bounds.x);
                assert_eq!(hide.right(), bounds.right());
                assert_eq!(paste.y, hide.y);
                assert_eq!(paste.bottom(), bounds.bottom());
                assert_eq!(paste.height, character.height);
                assert_eq!(hide.height, character.height);
                let mut terminal =
                    Terminal::new(TestBackend::new(bounds.right(), bounds.bottom())).unwrap();
                terminal
                    .draw(|frame| {
                        ui::render_screen_keyboard_panel(
                            frame,
                            &layout,
                            &view(""),
                            &RenderContext::default(),
                        )
                    })
                    .unwrap();
                if layout.bordered_buttons {
                    for area in [paste, hide] {
                        assert_eq!(
                            terminal.backend().buffer()[(area.x, area.y)].symbol(),
                            terminal.backend().buffer()[(character.x, character.y)].symbol()
                        );
                    }
                }
                for visibility in [1_000, 750, 500, 1, 0] {
                    let sliding = ui::screen_keyboard_input_layout_with_visibility(
                        bounds, true, visibility, auto_admin,
                    );
                    for (index, key) in sliding.buttons.iter().enumerate() {
                        assert_eq!(key.area.intersection(sliding.keyboard), key.area);
                        for other in sliding.buttons.iter().skip(index + 1) {
                            assert!(key.area.intersection(other.area).is_empty());
                        }
                    }
                }
            }
        }
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
                        assert_eq!(
                            buffer[(x, center_y)].fg,
                            context
                                .compatibility_theme()
                                .readable_text_color(context.theme.border)
                        );
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
