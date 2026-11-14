use super::*;

#[test]
fn vim_install_blocks_editing_and_escape_cancels_without_closing_the_document() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.open_editor();
    let page = state.content_page_id();
    let before = state.app.editor_state().unwrap().clone();
    state.editor_vim_install = Some((
        page,
        ui::EditorVimInstallViewModel {
            progress: Default::default(),
            cancelling: false,
        },
    ));
    let platform = platform::mock::UnsupportedPlatform;
    state.apply_input_with_platform(InputEvent::Paste("blocked text".into()), &platform);
    state.apply_input_with_platform(
        InputEvent::Key(KeyInput::new(InputKey::Char('x'))),
        &platform,
    );
    assert_eq!(state.app.editor_state(), Some(&before));
    state.apply_input_with_platform(InputEvent::Key(KeyInput::new(InputKey::Escape)), &platform);
    assert!(state.current_vim_install().unwrap().cancelling);
    assert_eq!(state.content_page_id(), page);
    assert_eq!(state.app.editor_state(), Some(&before));
}

#[test]
fn native_vim_f8_queues_export_without_interfering_with_a_pending_save() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.open_editor();
    assert!(state.set_editor_vim_enabled(true));
    state.editor_vim_terminal = Some(Arc::new(ui::CommandLineTerminalSnapshot::blank(80, 20)));
    state.pages.editor.editor_save_state = Some(EditorSaveState {
        id: 91,
        path: PathBuf::from("native.txt"),
        document_generation: state.pages.editor.editor_document_generation,
        revision: 7,
        stage: EditorTaskStage::Writing,
    });
    let before = state.app.editor_state().unwrap().clone();

    state.handle_editor_key(
        KeyInput::new(InputKey::F(8)),
        &platform::mock::UnsupportedPlatform,
    );

    assert!(state.editor_vim_toggle_requested);
    assert!(state.editor_vim_enabled());
    assert!(state.editor_vim_terminal.is_some());
    assert_eq!(
        state.pages.editor.editor_save_state.as_ref().unwrap().id,
        91
    );
    assert_eq!(state.app.editor_state(), Some(&before));
}

#[test]
fn ordinary_editor_f8_still_waits_for_a_pending_save() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.open_editor();
    assert!(state.set_editor_vim_enabled(false));
    state.pages.editor.editor_save_state = Some(EditorSaveState {
        id: 92,
        path: PathBuf::from("ordinary.txt"),
        document_generation: state.pages.editor.editor_document_generation,
        revision: 1,
        stage: EditorTaskStage::Writing,
    });

    state.handle_editor_key(
        KeyInput::new(InputKey::F(8)),
        &platform::mock::UnsupportedPlatform,
    );

    assert!(!state.editor_vim_enabled());
    assert!(!state.editor_vim_toggle_requested);
    assert!(state.editor_vim_terminal.is_none());
    assert_eq!(
        state.pages.editor.editor_save_state.as_ref().unwrap().id,
        92
    );
}

#[test]
fn vim_toggle_changes_only_the_current_editor_session() {
    let mut first = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let mut second = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    first.open_editor();
    second.open_editor();
    let default = first.app.storage_config().editor.vim_mode_enabled;

    first.set_editor_vim_enabled(!default);

    assert_eq!(first.editor_vim_enabled(), !default);
    assert_eq!(second.editor_vim_enabled(), default);
    assert_eq!(first.app.storage_config().editor.vim_mode_enabled, default);

    let mut updated = first.app.storage_config().clone();
    updated.editor.vim_mode_enabled = default;
    first.replace_storage_config(updated);
    assert_eq!(first.editor_vim_enabled(), !default);
}
