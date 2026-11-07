use super::*;

fn fixture(kind: PlatformKind) -> (ShellSession, platform::mock::MockPlatform) {
    let root = std::env::temp_dir();
    let platform = platform::mock::MockPlatform::new(
        platform::UserDirs::new(&root, &root, &root, &root, &root, &root, &root).unwrap(),
        platform::AppPaths::from_parts(root.join("config.toml"), &root, &root, &root, &root)
            .unwrap(),
    )
    .with_kind(kind);
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.open_editor();
    state.apply_editor_command(
        app::editor::EditorCommand::InsertText("first line\nsecond word\nlast line".into()),
        &platform,
    );
    state.apply_editor_command(
        app::editor::EditorCommand::MoveTo {
            position: app::editor::EditorPosition::Source(22),
            extend_selection: false,
        },
        &platform,
    );
    (state, platform)
}

fn command() -> InputModifiers {
    InputModifiers {
        super_key: true,
        ..InputModifiers::none()
    }
}

#[test]
fn mac_command_navigation_uses_line_and_document_boundaries() {
    for (key, expected) in [
        (InputKey::Left, 11),
        (InputKey::Right, 22),
        (InputKey::Up, 0),
        (InputKey::Down, 32),
    ] {
        let (mut state, platform) = fixture(PlatformKind::Macos);
        state.handle_editor_key(KeyInput::with_modifiers(key, command()), &platform);
        assert_eq!(
            state.app.editor_state().unwrap().cursor.byte_offset,
            expected
        );
    }
    let (mut state, platform) = fixture(PlatformKind::Macos);
    state.handle_editor_key(
        KeyInput::with_modifiers(
            InputKey::Left,
            InputModifiers {
                shift: true,
                ..command()
            },
        ),
        &platform,
    );
    let editor = state.app.editor_state().unwrap();
    assert_eq!(
        editor.selected_range().unwrap(),
        app::editor::SourceRange::new(11, 22)
    );
}

#[test]
fn mac_command_backspace_deletes_to_line_start_and_undo_restores_once() {
    let (mut state, platform) = fixture(PlatformKind::Macos);
    state.handle_editor_key(
        KeyInput::with_modifiers(InputKey::Backspace, command()),
        &platform,
    );
    assert_eq!(
        state.app.editor_state().unwrap().source(),
        "first line\n\nlast line"
    );
    state.handle_editor_key(
        KeyInput::with_modifiers(InputKey::Char('z'), command()),
        &platform,
    );
    assert_eq!(
        state.app.editor_state().unwrap().source(),
        "first line\nsecond word\nlast line"
    );
    state.handle_editor_key(
        KeyInput::with_modifiers(InputKey::Left, command()),
        &platform,
    );
    state.handle_editor_key(
        KeyInput::with_modifiers(InputKey::Backspace, command()),
        &platform,
    );
    assert_eq!(
        state.app.editor_state().unwrap().source(),
        "first line\nsecond word\nlast line"
    );
}

#[test]
fn mac_option_and_control_navigation_keep_word_boundaries() {
    for (kind, modifiers) in [
        (PlatformKind::Macos, InputModifiers::alt()),
        (PlatformKind::Macos, InputModifiers::ctrl()),
        (PlatformKind::Windows, InputModifiers::ctrl()),
    ] {
        let (mut state, platform) = fixture(kind);
        state.handle_editor_key(
            KeyInput::with_modifiers(InputKey::Left, modifiers),
            &platform,
        );
        assert_eq!(state.app.editor_state().unwrap().cursor.byte_offset, 18);
        state.handle_editor_key(
            KeyInput::with_modifiers(InputKey::Right, modifiers),
            &platform,
        );
        assert_eq!(state.app.editor_state().unwrap().cursor.byte_offset, 22);
        state.handle_editor_key(
            KeyInput::with_modifiers(InputKey::Backspace, modifiers),
            &platform,
        );
        assert_eq!(
            state.app.editor_state().unwrap().source(),
            "first line\nsecond \nlast line"
        );
    }
}

#[test]
fn vim_startup_does_not_apply_host_mac_navigation_or_deletion() {
    let (mut state, platform) = fixture(PlatformKind::Macos);
    assert!(state.set_editor_vim_enabled(true));
    let before = state.app.editor_state().unwrap().clone();
    for (key, modifiers) in [
        (InputKey::Left, command()),
        (InputKey::Backspace, command()),
        (InputKey::Backspace, InputModifiers::alt()),
    ] {
        state.handle_editor_key(KeyInput::with_modifiers(key, modifiers), &platform);
    }
    assert_eq!(state.app.editor_state(), Some(&before));
}

#[test]
fn mac_command_backspace_preserves_existing_selection_scope_and_read_only_state() {
    let (mut state, platform) = fixture(PlatformKind::Macos);
    state.handle_editor_key(
        KeyInput::with_modifiers(
            InputKey::Left,
            InputModifiers {
                shift: true,
                ..InputModifiers::alt()
            },
        ),
        &platform,
    );
    state.handle_editor_key(
        KeyInput::with_modifiers(InputKey::Backspace, command()),
        &platform,
    );
    assert_eq!(
        state.app.editor_state().unwrap().source(),
        "first line\nsecond \nlast line"
    );
    state.apply_editor_command(
        app::editor::EditorCommand::SetExecutionReadOnly(true),
        &platform,
    );
    let before = state.app.editor_state().unwrap().clone();
    state.handle_editor_key(
        KeyInput::with_modifiers(InputKey::Backspace, command()),
        &platform,
    );
    assert_eq!(state.app.editor_state(), Some(&before));
}
