use super::*;
use ratatui::{Terminal, backend::TestBackend, style::Color};
use ui::{InputPhase, KeyEvent, KeyModifiers, MouseEvent, RenderCapabilities};

fn key(key: Key) -> InputEvent {
    InputEvent::Key(KeyEvent::new(key))
}

fn layout() -> ScreenKeyboardLayout {
    ui::screen_keyboard_layout(Rect::new(0, 0, 80, 24), false)
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
fn screen_keyboard_types_printable_keys_and_preserves_physical_case() {
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
    assert_eq!(state.text, "aZb1中paste");
    assert_eq!(state.last_key, "Alt+A");
    state.handle_input(key(Key::Backspace), &layout, now);
    assert_eq!(state.text, "aZb1中past");
    state.handle_input(key(Key::Enter), &layout, now);
    state.handle_input(key(Key::Space), &layout, now);
    assert_eq!(state.text, "aZb1中past\n ");
    state.focus = ScreenKeyboardAction::Clear;
    state.navigating_buttons = true;
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
    state.focus = ScreenKeyboardAction::Letter('q');
    state.handle_input(key(Key::BackTab), &layout, now);
    assert_eq!(state.focus, ScreenKeyboardAction::Tab);
    state.handle_input(key(Key::Tab), &layout, now);
    assert_eq!(state.focus, ScreenKeyboardAction::Letter('q'));
}

#[test]
fn screen_keyboard_modifiers_symbols_tabs_and_function_keys() {
    use ScreenKeyboardAction::*;
    let mut state = ScreenKeyboardState::default();
    for action in [
        Shift,
        Character('1'),
        Letter('a'),
        CapsLock,
        Letter('b'),
        Shift,
        Letter('c'),
        CapsLock,
        Tab,
        Space,
        Enter,
    ] {
        assert!(!state.activate(action));
    }
    assert_eq!(state.text, "!AbC\t \n");
    assert_eq!(state.modifiers, ScreenKeyboardModifiers::default());
    for (modifier, expected) in [(LeftCtrl, "Ctrl+A"), (RightCtrl, "RCtrl+A"), (Alt, "Alt+A")] {
        state.activate(modifier);
        state.activate(Letter('a'));
        assert_eq!(state.last_key, expected);
        assert_eq!(
            state.text, "!AbC\t \n",
            "combinations must not type letters"
        );
        assert!(modifier.is_latched(state.modifiers));
        state.activate(Letter('b'));
        assert_eq!(
            state.last_key,
            format!("{}B", expected.trim_end_matches('A'))
        );
        state.activate(modifier);
        assert_eq!(state.modifiers, ScreenKeyboardModifiers::default());
    }
    state.activate(Alt);
    state.activate(Function(12));
    assert_eq!(state.last_key, "Alt+F12");
    state.handle_input(key(Key::F(2)), &layout(), Instant::now());
    assert_eq!(state.last_key, "Alt+F2");
    state.activate(Alt);
    state.activate(Shift);
    state.activate(Alt);
    state.activate(LeftCtrl);
    state.handle_input(InputEvent::FocusLost, &layout(), Instant::now());
    assert_eq!(state.modifiers, ScreenKeyboardModifiers::default());
}

#[test]
fn screen_keyboard_latches_compose_and_survive_text_and_clipboard_actions() {
    use ScreenKeyboardAction::*;
    let mut state = ScreenKeyboardState::default();
    for action in [LeftCtrl, RightCtrl, Alt, Shift, Function(4), Function(5)] {
        state.activate(action);
    }
    assert_eq!(state.last_key, "Ctrl+RCtrl+Alt+Shift+F5");
    assert!(state.text.is_empty());
    for action in [Copy, Paste, Clear] {
        state.activate(action);
        assert!(
            state.modifiers.shift
                && state.modifiers.left_ctrl
                && state.modifiers.right_ctrl
                && state.modifiers.alt
        );
    }
    for action in [LeftCtrl, RightCtrl, Alt] {
        state.activate(action);
    }
    state.handle_input(key(Key::Char('a')), &layout(), Instant::now());
    state.handle_input(key(Key::Char('b')), &layout(), Instant::now());
    assert_eq!(state.text, "AB");
    state.activate(Shift);
    state.activate(Letter('c'));
    assert_eq!(state.text, "ABc");
}

#[test]
fn screen_keyboard_animation_reads_saved_speed_and_reduced_motion() {
    for speed in [50, 125, 200] {
        let appearance = storage::AppearanceConfig {
            animation_speed_percent: speed,
            ..Default::default()
        };
        let frame = keyboard_motion_frame(&appearance, Duration::ZERO);
        assert_eq!(frame.animation_speed_percent, speed);
        let mut motion = KeyboardMotion::default();
        motion.retarget(true, frame);
        let duration = MotionTimings::PAGE.mul_f64(100.0 / f64::from(speed));
        let halfway = keyboard_motion_frame(&appearance, duration / 2);
        assert!(motion.transition.requests_redraw(halfway));
        assert!(motion.visibility(halfway) > 0 && motion.visibility(halfway) < 1_000);
        let finished = keyboard_motion_frame(&appearance, duration);
        assert_eq!(motion.visibility(finished), 0);
        assert!(!motion.transition.requests_redraw(finished));
        motion.retarget(false, finished);
        assert_eq!(
            motion.visibility(keyboard_motion_frame(&appearance, duration * 2)),
            1_000
        );
    }
    let appearance = storage::AppearanceConfig {
        motion_preference: storage::MotionPreference::Reduced,
        animation_speed_percent: 50,
        ..Default::default()
    };
    let frame = keyboard_motion_frame(&appearance, Duration::ZERO);
    let mut motion = KeyboardMotion::default();
    motion.retarget(true, frame);
    assert_eq!(motion.visibility(frame), 0);
    assert!(!motion.transition.requests_redraw(frame));
    motion.retarget(false, frame);
    assert_eq!(motion.visibility(frame), 1_000);
}

#[test]
fn screen_keyboard_animation_reverses_without_jumping_and_keeps_toolbar_capture() {
    let frame = MotionFrame::default();
    let mut motion = KeyboardMotion::default();
    motion.retarget(true, frame);
    let halfway = MotionFrame {
        now: MotionTimings::PAGE / 2,
        ..frame
    };
    let visibility = motion.visibility(halfway);
    motion.retarget(false, halfway);
    assert_eq!(motion.visibility(halfway), visibility);
    assert_eq!(
        motion.visibility(MotionFrame {
            now: MotionTimings::PAGE * 2,
            ..frame
        }),
        1_000
    );

    let expanded = layout();
    let moving = ui::screen_keyboard_layout_with_visibility(Rect::new(0, 0, 80, 24), true, 500);
    let mut state = ScreenKeyboardState::default();
    state.handle_input(
        mouse(&expanded, 'q', MouseEventKind::Down(MouseButton::Left)),
        &expanded,
        Instant::now(),
    );
    state.sync_pointer_layout(&moving);
    assert!(state.pressed.is_none());
    let toggle = expanded
        .buttons
        .iter()
        .find(|key| key.action == ScreenKeyboardAction::ToggleKeyboard)
        .unwrap();
    state.handle_input(
        InputEvent::Mouse(MouseEvent::new(
            toggle.area.x,
            toggle.area.y,
            MouseEventKind::Down(MouseButton::Left),
        )),
        &expanded,
        Instant::now(),
    );
    state.sync_pointer_layout(&moving);
    assert!(state.pressed.is_some());
}

#[test]
fn screen_keyboard_physical_combinations_do_not_activate_focused_buttons() {
    for (key, name) in [
        (Key::Enter, "Enter"),
        (Key::Space, "Space"),
        (Key::Tab, "Tab"),
    ] {
        for (modifier, prefix) in [(KeyModifiers::CTRL, "Ctrl"), (KeyModifiers::ALT, "Alt")] {
            let mut state = ScreenKeyboardState::default();
            let input = InputEvent::Key(KeyEvent::with_modifiers(key.clone(), modifier));
            assert!(!state.handle_input(input, &layout(), Instant::now()));
            assert_eq!(state.text, "");
            assert_eq!(state.last_key, format!("{prefix}+{name}"));
        }
    }
    let mut state = ScreenKeyboardState::default();
    let input = InputEvent::Key(KeyEvent::with_modifiers(Key::BackTab, KeyModifiers::CTRL));
    state.handle_input(input, &layout(), Instant::now());
    assert_eq!(state.text, "");
    assert_eq!(state.last_key, "Ctrl+Shift+Tab");
}

#[test]
fn screen_keyboard_copy_paste_and_failures_preserve_text() {
    use platform::{
        AppPaths, Platform, UserDirs,
        mock::{MockPlatform, UnsupportedPlatform},
    };
    let root = std::env::temp_dir().join("tundra-screen-keyboard-clipboard");
    let dirs = UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("Data"),
    )
    .unwrap();
    let paths = AppPaths::from_parts(
        root.join("config.toml"),
        root.join("state"),
        root.join("cache"),
        root.join("logs"),
        root.join("temp"),
    )
    .unwrap();
    let platform = MockPlatform::new(dirs, paths);
    let mut state = ScreenKeyboardState::default();
    state.text = "原文".to_owned();
    platform.set_clipboard_text("\r\n中文\t🙂\0\u{7}");
    state.activate(ScreenKeyboardAction::Paste);
    state.apply_clipboard(&platform);
    assert_eq!(state.text, "原文\n中文\t🙂");
    let copy = InputEvent::Key(KeyEvent::with_modifiers(Key::Char('c'), KeyModifiers::CTRL));
    assert!(
        !state.handle_input(copy, &layout(), Instant::now()),
        "Ctrl+C copies instead of exiting"
    );
    state.apply_clipboard(&platform);
    assert_eq!(platform.read_clipboard_text().unwrap(), state.text);
    state.activate(ScreenKeyboardAction::Clear);
    state.activate(ScreenKeyboardAction::RightCtrl);
    state.activate(ScreenKeyboardAction::Letter('v'));
    assert_eq!(state.last_key, "RCtrl+V");
    state.apply_clipboard(&platform);
    assert_eq!(state.text, "原文\n中文\t🙂");
    for action in [ScreenKeyboardAction::Copy, ScreenKeyboardAction::Paste] {
        state.activate(action);
        state.apply_clipboard(&UnsupportedPlatform);
        assert_eq!(state.text, "原文\n中文\t🙂");
        assert!(!state.message.is_empty());
        assert!(state.pending_clipboard.is_none());
    }
}

#[test]
fn screen_keyboard_collapse_keeps_text_and_cancels_old_key_capture() {
    let bounds = Rect::new(0, 0, 80, 24);
    let expanded = ui::screen_keyboard_layout(bounds, false);
    let now = Instant::now();
    let mut state = ScreenKeyboardState::default();
    state.text = "keep me".to_owned();
    state.handle_input(
        mouse(&expanded, 'q', MouseEventKind::Down(MouseButton::Left)),
        &expanded,
        now,
    );
    state.activate(ScreenKeyboardAction::ToggleKeyboard);
    assert!(state.collapsed);
    assert!(state.pressed.is_none());
    let collapsed = ui::screen_keyboard_layout(bounds, true);
    state.ensure_visible_focus(&collapsed);
    assert_eq!(state.focus, ScreenKeyboardAction::ToggleKeyboard);
    state.handle_input(
        mouse(&expanded, 'q', MouseEventKind::Up(MouseButton::Left)),
        &collapsed,
        now,
    );
    assert_eq!(state.text, "keep me");
    state.handle_input(key(Key::Char('!')), &collapsed, now);
    assert_eq!(state.text, "keep me!");
    state.activate(ScreenKeyboardAction::ToggleKeyboard);
    assert!(!state.collapsed);
    assert_eq!(state.text, "keep me!");
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
                    modifiers: state.modifiers,
                    last_key: &state.last_key,
                    message: &state.message,
                },
                &context,
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let label_color = (area.y..area.bottom())
        .find_map(|y| {
            (area.x..area.right()).find_map(|x| {
                (buffer[(x, y)].symbol()
                    == ScreenKeyboardAction::Letter(letter)
                        .character(state.modifiers)
                        .unwrap()
                        .to_string())
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
