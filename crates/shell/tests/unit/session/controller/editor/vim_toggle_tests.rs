use super::*;

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
    assert!(state.app.storage_config().editor.vim_mode_enabled);
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

    assert!(!state.app.storage_config().editor.vim_mode_enabled);
    assert!(!state.editor_vim_toggle_requested);
    assert!(state.editor_vim_terminal.is_none());
    assert_eq!(
        state.pages.editor.editor_save_state.as_ref().unwrap().id,
        92
    );
}
