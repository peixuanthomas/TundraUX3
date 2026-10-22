use super::*;

#[test]
fn vim_write_copy_from_configuration_uses_the_requested_raw_file_target() {
    let root = std::env::temp_dir().join(format!(
        "tundra-config-vim-copy-{}-{}",
        std::process::id(),
        next_editor_task_id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let original = root.join("original.conf");
    let target = root.join("copy.bin");
    std::fs::write(&original, "original\n").unwrap();
    let document = ConfigDocument {
        path: original.clone(),
        content: "original\n".into(),
        version: "version-1".into(),
        existed: true,
        uid: 0,
        gid: 0,
        mode: 0o640,
        validator: "none".into(),
        check: ConfigCheck::NotChecked,
        backup_id: None,
    };
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.open_editor();
    state.pages.editor.editor_config = ConfigEditorState {
        path: Some(original.clone()),
        document: Some(document.clone()),
        ..Default::default()
    };
    state.replace_config_text(&document, "unsaved candidate\n".into());
    state.pages.editor.editor_vim_write_pending = true;
    let raw = vec![0xff, 0xfe, b'V', 0, b'I', 0, b'M', 0, 0, 0];

    assert!(state.save_editor_document(
        target.clone(),
        app::editor::SaveSnapshot::external_bytes(83, raw.clone()),
        &platform::mock::UnsupportedPlatform,
    ));
    assert!(!state.config_editor_form_visible());
    assert!(state.pages.editor.editor_config.pending_action.is_none());
    let save = state.pages.editor.editor_save_state.as_ref().unwrap();
    assert_eq!(save.path, target);
    assert_eq!(
        state.tasks.owner(TaskDomain::Editor, save.id),
        Some(TaskOwner::Page(state.active_page_id()))
    );

    let deadline = Instant::now() + Duration::from_secs(10);
    while state.pages.editor.editor_save_state.is_some() {
        state.poll_editor_background_tasks(&platform::mock::UnsupportedPlatform);
        assert!(Instant::now() < deadline, "raw write-copy did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(std::fs::read(&target).unwrap(), raw);
    assert_eq!(std::fs::read_to_string(&original).unwrap(), "original\n");
    assert_eq!(state.pages.editor.editor_vim_save_result, Some(Ok(target)));
    let editor = state.app.editor_state().unwrap();
    assert_eq!(editor.document.path, Some(original.clone()));
    assert_eq!(editor.source(), "unsaved candidate\n");
    assert!(editor.is_dirty());
    assert_eq!(state.pages.editor.editor_config.path, Some(original));
    assert!(!state.config_editor_form_visible());
    assert!(state.pages.editor.editor_config.pending_action.is_none());
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}
