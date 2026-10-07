use super::*;
use ratatui::{Terminal, backend::TestBackend, style::Color};
use ui::{InputPhase, KeyEvent, KeyModifiers, MouseEvent, RenderCapabilities};

fn key(key: Key) -> InputEvent {
    InputEvent::Key(KeyEvent::new(key))
}

fn layout() -> ScreenKeyboardLayout {
    ui::screen_keyboard_layout(Rect::new(0, 0, 80, 24))
}

fn mouse(layout: &ScreenKeyboardLayout, letter: char, kind: MouseEventKind) -> InputEvent {
    let area = layout
        .buttons
        .iter()
        .find(|button| button.action == ScreenKeyboardAction::Letter(letter))
        .unwrap()
        .area;
    InputEvent::Mouse(MouseEvent::new(
        area.x + area.width / 2,
        area.y + area.height / 2,
        kind,
    ))
}

#[test]
fn screen_keyboard_types_only_english_letters_and_preserves_physical_case() {
    let layout = layout();
    let now = Instant::now();
    let mut state = ScreenKeyboardState::default();
    for input in [
        key(Key::Char('a')),
        key(Key::Char('Z')),
        InputEvent::Key(KeyEvent::new(Key::Char('b')).repeated()),
        InputEvent::Key(KeyEvent::with_phase(
            Key::Char('b'),
            KeyModifiers::NONE,
            InputPhase::Release,
        )),
        key(Key::Char('1')),
        key(Key::Char('中')),
        InputEvent::Paste("paste".to_owned()),
        InputEvent::Key(KeyEvent::with_modifiers(Key::Char('a'), KeyModifiers::ALT)),
    ] {
        assert!(!state.handle_input(input, &layout, now));
    }
    assert_eq!(state.text, "aZb");
    state.handle_input(key(Key::Backspace), &layout, now);
    assert_eq!(state.text, "aZ");
    state.focus = ScreenKeyboardAction::Clear;
    state.handle_input(key(Key::Enter), &layout, now);
    assert_eq!(state.text, "");
    state.focus = ScreenKeyboardAction::Exit;
    assert!(state.handle_input(key(Key::Space), &layout, now));
    assert!(state.handle_input(key(Key::Escape), &layout, now));
}

#[test]
fn screen_keyboard_navigation_uses_rows_and_supports_reverse_tab() {
    let layout = layout();
    let now = Instant::now();
    let mut state = ScreenKeyboardState::default();
    state.handle_input(key(Key::Down), &layout, now);
    assert_eq!(state.focus, ScreenKeyboardAction::Letter('a'));
    state.handle_input(key(Key::Down), &layout, now);
    assert_eq!(state.focus, ScreenKeyboardAction::Letter('z'));
    state.handle_input(key(Key::Up), &layout, now);
    assert_eq!(state.focus, ScreenKeyboardAction::Letter('s'));
    state.focus = ScreenKeyboardAction::Letter('q');
    state.handle_input(key(Key::BackTab), &layout, now);
    assert_eq!(state.focus, ScreenKeyboardAction::Exit);
    state.handle_input(key(Key::Tab), &layout, now);
    assert_eq!(state.focus, ScreenKeyboardAction::Letter('q'));
}

#[test]
fn screen_keyboard_mouse_click_requires_a_matching_release() {
    let layout = layout();
    let now = Instant::now();
    let mut state = ScreenKeyboardState::default();
    let left = MouseButton::Left;
    state.handle_input(mouse(&layout, 'q', MouseEventKind::Up(left)), &layout, now);
    state.handle_input(
        mouse(&layout, 'q', MouseEventKind::Click(left)),
        &layout,
        now,
    );
    assert_eq!(state.text, "");
    state.handle_input(
        mouse(&layout, 'q', MouseEventKind::Down(left)),
        &layout,
        now,
    );
    assert_eq!(state.text, "");
    state.handle_input(mouse(&layout, 'q', MouseEventKind::Up(left)), &layout, now);
    assert_eq!(state.text, "q");
    assert!(state.pressed.is_none());
    assert!(state.hovered.is_none());
    assert!(!state.keyboard_focus_visible);
    state.handle_input(mouse(&layout, 'q', MouseEventKind::Moved), &layout, now);
    assert!(
        state.hovered.is_none(),
        "stationary report must not restore click hover"
    );
    state.handle_input(mouse(&layout, 'w', MouseEventKind::Moved), &layout, now);
    assert!(state.hovered.is_some());
}

#[test]
fn screen_keyboard_drag_move_out_focus_loss_resize_and_long_press_cancel() {
    let layout = layout();
    let now = Instant::now();
    let left = MouseButton::Left;
    for cancel in [
        mouse(&layout, 'w', MouseEventKind::Drag(left)),
        mouse(&layout, 'w', MouseEventKind::Moved),
        InputEvent::FocusLost,
        InputEvent::Resize {
            width: 79,
            height: 24,
        },
        key(Key::F(1)),
    ] {
        let mut state = ScreenKeyboardState::default();
        state.handle_input(
            mouse(&layout, 'q', MouseEventKind::Down(left)),
            &layout,
            now,
        );
        state.handle_input(cancel, &layout, now);
        state.handle_input(mouse(&layout, 'q', MouseEventKind::Up(left)), &layout, now);
        assert_eq!(state.text, "");
        assert!(state.pressed.is_none());
    }
    let mut state = ScreenKeyboardState::default();
    state.handle_input(
        mouse(&layout, 'q', MouseEventKind::Down(left)),
        &layout,
        now,
    );
    assert!(state.expire_press(now + BUTTON_MAX_PRESS + Duration::from_millis(1)));
    state.handle_input(mouse(&layout, 'q', MouseEventKind::Up(left)), &layout, now);
    assert_eq!(state.text, "");
}

fn key_colors(
    state: &ScreenKeyboardState,
    layout: &ScreenKeyboardLayout,
    mut context: RenderContext,
    letter: char,
) -> (Color, Color) {
    context.buttons = Some(state.button_frame(&context));
    let area = layout
        .buttons
        .iter()
        .find(|button| button.action == ScreenKeyboardAction::Letter(letter))
        .unwrap()
        .area;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| {
            ui::render_screen_keyboard(
                frame,
                frame.area(),
                layout,
                &ScreenKeyboardViewModel {
                    text: &state.text,
                    focus: state.focus,
                },
                &context,
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let label_color = (area.y..area.bottom())
        .find_map(|y| {
            (area.x..area.right()).find_map(|x| {
                (buffer[(x, y)].symbol() == letter.to_ascii_uppercase().to_string())
                    .then_some(buffer[(x, y)].fg)
            })
        })
        .unwrap();
    (label_color, buffer[(area.x, area.y)].fg)
}

#[test]
fn screen_keyboard_actual_colors_follow_input_and_theme_in_rgb_and_ansi() {
    let layout = layout();
    let now = Instant::now();
    let left = MouseButton::Left;
    for (capabilities, accent, pressed, normal) in [
        (
            RenderCapabilities::default(),
            Color::Rgb(32, 64, 96),
            Color::Rgb(110, 130, 151),
            Color::Rgb(230, 241, 244),
        ),
        (
            RenderCapabilities::ansi(),
            Color::Cyan,
            Color::LightCyan,
            Color::White,
        ),
    ] {
        let theme = TundraTheme::default_dark().with_accent_color(accent);
        let context = RenderContext::from_theme(&theme, Default::default(), capabilities);
        let mut state = ScreenKeyboardState::default();
        assert_eq!(
            key_colors(&state, &layout, context.clone(), 'q'),
            (accent, accent)
        );
        state.handle_input(mouse(&layout, 'w', MouseEventKind::Moved), &layout, now);
        assert_eq!(
            key_colors(&state, &layout, context.clone(), 'w'),
            (accent, accent)
        );
        assert_eq!(key_colors(&state, &layout, context.clone(), 'q').0, normal);
        state.handle_input(
            mouse(&layout, 'w', MouseEventKind::Down(left)),
            &layout,
            now,
        );
        assert_eq!(
            key_colors(&state, &layout, context.clone(), 'w'),
            (pressed, pressed)
        );
        state.handle_input(mouse(&layout, 'w', MouseEventKind::Up(left)), &layout, now);
        assert_eq!(key_colors(&state, &layout, context.clone(), 'w').0, normal);
        state.handle_input(key(Key::Right), &layout, now);
        assert_eq!(
            key_colors(&state, &layout, context.clone(), 'e'),
            (accent, accent)
        );
        state.handle_input(mouse(&layout, 'w', MouseEventKind::Moved), &layout, now);
        assert_eq!(
            key_colors(&state, &layout, context.clone(), 'e'),
            (accent, accent)
        );
        assert_eq!(key_colors(&state, &layout, context, 'w').0, normal);
    }
}
