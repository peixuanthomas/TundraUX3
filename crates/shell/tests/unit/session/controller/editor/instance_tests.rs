use super::*;

fn session() -> ShellSession {
    ShellSession::new_for_home_mode(ShellLaunchConfig::default(), (120, 40), ShellHomeMode::User)
}

fn insert(state: &mut ShellSession, text: &str) {
    state.app.dispatch_at(
        app::AppCommand::Editor(app::editor::EditorCommand::InsertText(text.into())),
        Instant::now(),
    );
}

#[test]
fn a_busy_save_picker_registers_the_save_to_its_original_editor() {
    let root = std::env::temp_dir().join(format!(
        "tundra-picker-save-owner-{}-{}",
        std::process::id(),
        next_editor_task_id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let path = root.join("original.txt");
    let mut state = session();
    state.open_editor();
    insert(&mut state, "original document");
    let original = state.active_page_id();
    state.app.dispatch_at(
        app::AppCommand::Editor(app::editor::EditorCommand::RequestSave),
        Instant::now(),
    );
    let snapshot = match state.app.take_editor_effects().remove(0) {
        app::editor::EditorEffect::SaveFilePicker { snapshot, .. } => snapshot,
        _ => panic!("an untitled document should request a save picker"),
    };
    state.pages.editor.editor_discard_for_open = true;
    state.enter_new_page(ShellScreen::Explorer);
    let picker = state.active_page_id();
    state.pages.explorer.editor_picker_owner = Some(original);
    state.pages.explorer.explorer_purpose = ExplorerPurpose::EditorSaveAs {
        snapshot: snapshot.clone(),
    };
    state.register_page_task(
        TaskDomain::Explorer,
        704,
        TaskClass::Operation,
        TaskControl::new(|| {}, || false),
    );
    state.open_editor();
    insert(&mut state, "covering document");
    let covering = state.active_page_id();
    state.pages.editor.editor_discard_for_open = true;
    let covering_before = state.pages.editor.get(covering).unwrap().clone();

    assert_eq!(
        state.with_page_context(picker, |session| session.save_editor_document(
            path.clone(),
            snapshot,
            &platform::mock::UnsupportedPlatform,
        )),
        Some(true)
    );

    assert_eq!(state.active_page_id(), covering);
    assert!(state.page_screen(picker).is_some());
    assert!(
        state
            .pages
            .explorer
            .get(picker)
            .unwrap()
            .editor_picker_save_submitted
    );
    let save = state
        .pages
        .editor
        .get(original)
        .unwrap()
        .editor_save_state
        .as_ref()
        .unwrap()
        .id;
    assert_eq!(
        state.tasks.owner(TaskDomain::Editor, save),
        Some(TaskOwner::Page(original))
    );
    assert_eq!(state.pages.editor.get(covering), Some(&covering_before));
    state.continue_page_tasks_in_background(picker);
    assert_eq!(state.active_page_id(), covering);
    assert!(state.page_screen(picker).is_none());
    assert!(
        !state
            .pages
            .editor
            .get(original)
            .unwrap()
            .editor_discard_for_open
    );
    assert_eq!(state.pages.editor.get(covering), Some(&covering_before));

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        state.poll_editor_background_tasks(&platform::mock::UnsupportedPlatform);
        if !state.tasks.page_running(original)
            && state
                .pages
                .editor
                .get(original)
                .unwrap()
                .editor_save_state
                .is_none()
        {
            break;
        }
        assert!(Instant::now() < deadline, "editor save did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "original document");
    assert_eq!(state.active_page_id(), covering);
    assert_eq!(
        state.app.editor_state().unwrap().export_text(),
        "covering document"
    );
    assert!(state.app.editor_state().unwrap().is_dirty());
    assert_eq!(state.pages.editor.get(covering), Some(&covering_before));
    state.with_page_context(original, |session| {
        assert!(!session.app.editor_state().unwrap().is_dirty());
    });
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_new_editor_keeps_the_hidden_editors_draft_and_pending_save() {
    let mut state = session();
    state.open_editor();
    let first_page = state.active_page_id();
    insert(&mut state, "first draft");
    let first_recovery = state.pages.editor.editor_recovery_id.clone();
    state.pages.editor.editor_save_state = Some(EditorSaveState {
        id: 91,
        path: PathBuf::from("first.txt"),
        document_generation: 1,
        revision: 1,
        stage: EditorTaskStage::Writing,
    });
    state.enter_screen(ShellScreen::Launcher);
    state.open_editor();
    assert_ne!(state.active_page_id(), first_page);
    assert!(state.app.editor_state().is_some());
    assert_eq!(state.app.editor_state().unwrap().export_text(), "");
    assert_ne!(state.pages.editor.editor_recovery_id, first_recovery);
    assert!(state.pages.editor.editor_save_state.is_none());
    state.finish_editor_close(false);
    state.return_from_screen(ShellScreen::Launcher);
    assert_eq!(state.active_page_id(), first_page);
    assert_eq!(
        state.app.editor_state().unwrap().export_text(),
        "first draft"
    );
    assert_eq!(state.pages.editor.editor_recovery_id, first_recovery);
    assert_eq!(
        state.pages.editor.editor_save_state.as_ref().unwrap().id,
        91
    );
}

#[test]
fn failed_load_submission_restores_explorer_without_leaving_a_new_editor() {
    let mut state = session();
    state.enter_screen(ShellScreen::Explorer);
    let pages = state.live_page_ids();
    state.editor_task_runtime = ShellEditorTaskRuntime::unavailable();
    assert!(
        state
            .begin_editor_open_task(
                PathBuf::from("unavailable.txt"),
                EditorTaskAccess::Editable,
                None,
                false
            )
            .is_err()
    );
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert_eq!(state.live_page_ids(), pages);
    assert!(state.app.editor_state().is_none());
    assert!(state.pages.editor.ids().is_empty());
}

#[test]
fn failed_picker_submission_preserves_the_existing_draft_and_recovery_id() {
    let mut state = session();
    state.open_editor();
    insert(&mut state, "original draft");
    let recovery_id = state.pages.editor.editor_recovery_id.clone();
    state.enter_screen(ShellScreen::Explorer);
    state.pages.explorer.explorer_purpose = ExplorerPurpose::EditorOpen;
    let pages = state.live_page_ids();
    state.editor_task_runtime = ShellEditorTaskRuntime::unavailable();
    assert!(
        state
            .begin_editor_open_task(
                PathBuf::from("unavailable.txt"),
                EditorTaskAccess::Editable,
                None,
                true
            )
            .is_err()
    );
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert_eq!(state.live_page_ids(), pages);
    assert_eq!(
        state.app.editor_state().unwrap().export_text(),
        "original draft"
    );
    assert_eq!(state.pages.editor.editor_recovery_id, recovery_id);
}

#[test]
fn failed_external_open_does_not_change_a_hidden_editors_message() {
    let mut state = session();
    state.open_editor();
    insert(&mut state, "hidden draft");
    let first = state.active_page_id();
    state.pages.editor.editor_message = Some("keep this message".into());
    state.enter_screen(ShellScreen::Explorer);
    let explorer = state.active_page_id();
    state.editor_task_runtime = ShellEditorTaskRuntime::unavailable();
    assert!(!state.open_editor_path(PathBuf::from("unavailable.txt")));
    assert_eq!(state.active_page_id(), explorer);
    assert_eq!(
        state.pages.editor.get(first).unwrap().editor_message,
        Some("keep this message".into())
    );
    assert!(state.pages.common.error_message.is_some());
}

fn pending_save(state: &mut ShellSession, id: u64, path: PathBuf) {
    state.app.dispatch_at(
        app::AppCommand::Editor(app::editor::EditorCommand::RequestSave),
        Instant::now(),
    );
    let _ = state.app.take_editor_effects();
    state.pages.editor.editor_save_state = Some(EditorSaveState {
        id,
        path,
        document_generation: state.pages.editor.editor_document_generation,
        revision: state.app.editor_state().unwrap().revision(),
        stage: EditorTaskStage::Inspecting,
    });
    state.register_page_task(
        TaskDomain::Editor,
        id,
        TaskClass::Operation,
        TaskControl::new(|| {}, || true),
    );
}

fn fingerprint() -> DocumentFingerprint {
    DocumentFingerprint {
        len: 11,
        modified: None,
        content_hash: 42,
    }
}

#[test]
fn hidden_load_failure_belongs_to_its_original_picker_not_the_covering_page() {
    for from_picker in [false, true] {
        for covering_screen in [ShellScreen::Clock, ShellScreen::Editor] {
            let mut state = session();
            if from_picker {
                state.open_editor();
                insert(&mut state, "original draft");
            }
            state.enter_new_page(ShellScreen::Explorer);
            let picker = state.active_page_id();
            if from_picker {
                state.pages.explorer.explorer_purpose = ExplorerPurpose::EditorOpen;
            }
            let rollback = state.begin_editor_navigation(from_picker);
            let loading = state.active_page_id();
            let id = next_editor_task_id();
            state.pages.editor.editor_load_state = Some(EditorLoadState {
                id,
                path: PathBuf::from("missing.txt"),
                stage: EditorTaskStage::Inspecting,
                completed_bytes: 0,
                total_bytes: None,
                operation: EditorLoadOperation::Open {
                    navigation: if from_picker {
                        EditorLoadNavigation::EditorPicker
                    } else {
                        EditorLoadNavigation::Explorer
                    },
                    rollback,
                    reload: None,
                    replacing_dirty: from_picker,
                },
            });
            state.register_page_task(
                TaskDomain::Editor,
                id,
                TaskClass::Query,
                TaskControl::new(|| {}, || true),
            );
            state.enter_new_page(covering_screen);
            let covering = state.active_page_id();
            let focus = state.pages.common.focused_component;
            state
                .editor_task_runtime
                .shared
                .event_tx
                .send(EditorTaskEvent::LoadFinished {
                    id,
                    result: Box::new(Err("missing source".into())),
                })
                .unwrap();

            state.poll_editor_background_tasks(&platform::mock::UnsupportedPlatform);

            assert_eq!(state.active_page_id(), covering);
            assert_eq!(state.pages.common.focused_component, focus);
            assert!(state.pages.common.error_message.is_none());
            assert!(
                state
                    .pages
                    .common
                    .get(picker)
                    .unwrap()
                    .error_message
                    .is_some()
            );
            assert!(
                state
                    .app
                    .notification_center()
                    .page_alert_message_for_key(picker, EDITOR_ALERT_KEY)
                    .is_some()
            );
            assert!(
                state
                    .app
                    .notification_center()
                    .page_alert_message_for_key(covering, EDITOR_ALERT_KEY)
                    .is_none()
            );
            if from_picker {
                state.with_page_context(loading, |session| {
                    assert_eq!(
                        session.app.editor_state().unwrap().export_text(),
                        "original draft"
                    );
                    let message = session
                        .to_editor_view_model()
                        .status_message
                        .expect("the original editor must have a terminal load status");
                    assert!(!message.starts_with("Loading"));
                    assert!(message.contains("missing source"));
                    assert!(session.pages.editor.editor_load_state.is_none());
                });
            } else {
                assert!(state.page_screen(loading).is_none());
            }
            state.close_page_instance(covering);
            assert_eq!(state.active_page_id(), picker);
            assert!(state.page_alert_message_for_key(EDITOR_ALERT_KEY).is_some());
        }
    }
}

#[test]
fn hidden_editor_save_events_only_update_their_owner_and_duplicates_are_ignored() {
    let mut state = session();
    state.open_editor();
    insert(&mut state, "first draft");
    let first = state.active_page_id();
    let id = next_editor_task_id();
    pending_save(&mut state, id, PathBuf::from("first.txt"));
    state.enter_screen(ShellScreen::Launcher);
    state.open_editor();
    insert(&mut state, "second draft");
    let second = state.active_page_id();
    state
        .editor_task_runtime
        .shared
        .event_tx
        .send(EditorTaskEvent::Progress {
            id,
            stage: EditorTaskStage::Writing,
            completed_bytes: 7,
            total_bytes: Some(11),
        })
        .unwrap();
    state.poll_editor_background_tasks(&platform::mock::UnsupportedPlatform);
    assert_eq!(
        state
            .pages
            .editor
            .get(first)
            .unwrap()
            .editor_save_state
            .as_ref()
            .unwrap()
            .stage,
        EditorTaskStage::Writing
    );
    assert!(state.pages.editor.editor_save_state.is_none());
    state
        .editor_task_runtime
        .shared
        .event_tx
        .send(EditorTaskEvent::SaveFinished {
            id,
            result: Ok(fingerprint()),
        })
        .unwrap();
    state.poll_editor_background_tasks(&platform::mock::UnsupportedPlatform);
    assert_eq!(state.active_page_id(), second);
    assert_eq!(
        state.app.editor_state().unwrap().export_text(),
        "second draft"
    );
    assert!(state.app.editor_state().unwrap().is_dirty());
    state.with_page_context(first, |session| {
        assert!(!session.app.editor_state().unwrap().is_dirty());
        assert_eq!(
            session.app.editor_state().unwrap().document.path,
            Some(PathBuf::from("first.txt"))
        );
    });
    let first_message = state
        .pages
        .editor
        .get(first)
        .unwrap()
        .editor_message
        .clone();
    let second_message = state.pages.editor.editor_message.clone();
    state
        .editor_task_runtime
        .shared
        .event_tx
        .send(EditorTaskEvent::SaveFinished {
            id,
            result: Err(EditorSaveTaskError::Write("stale duplicate".into())),
        })
        .unwrap();
    state.poll_editor_background_tasks(&platform::mock::UnsupportedPlatform);
    assert_eq!(
        state.pages.editor.get(first).unwrap().editor_message,
        first_message
    );
    assert_eq!(state.pages.editor.editor_message, second_message);
}

#[test]
fn a_closed_editors_late_load_cannot_replace_a_new_editor() {
    let mut state = session();
    state.open_editor();
    let first = state.active_page_id();
    let id = next_editor_task_id();
    state.register_page_task(
        TaskDomain::Editor,
        id,
        TaskClass::Query,
        TaskControl::new(|| {}, || true),
    );
    state.tasks.close_queries(first);
    state.close_page_instance(first);
    state.open_editor();
    insert(&mut state, "keep new draft");
    let second = state.active_page_id();
    state
        .editor_task_runtime
        .shared
        .event_tx
        .send(EditorTaskEvent::LoadFinished {
            id,
            result: Box::new(Ok(EditorLoadedTaskDocument {
                state: EditorState::new(),
                fingerprint: fingerprint(),
                total_bytes: 0,
                rich_blocks: None,
            })),
        })
        .unwrap();
    state.poll_editor_background_tasks(&platform::mock::UnsupportedPlatform);
    assert_eq!(state.active_page_id(), second);
    assert_eq!(
        state.app.editor_state().unwrap().export_text(),
        "keep new draft"
    );
}

#[test]
fn detached_save_completion_keeps_the_new_editor_and_preserves_failed_recovery() {
    for (success, cleanup_fails) in [(false, false), (true, false), (true, true)] {
        let root = std::env::temp_dir().join(format!(
            "tundra-detached-editor-save-{}-{}",
            std::process::id(),
            next_editor_task_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let paths = platform::AppPaths::from_parts(
            root.join("config"),
            root.join("state"),
            root.join("cache"),
            root.join("logs"),
            root.join("temp"),
        )
        .unwrap();
        let lease =
            app::editor_recovery::acquire_editor_recovery(&paths, "alice", &Default::default())
                .unwrap();
        app::editor_recovery::write_editor_recovery_v2(
            &lease,
            &app::editor_recovery::EditorRecoveryRecordV2::source(
                "first draft",
                app::editor_recovery::RecoveryDocumentKind::PlainText,
            ),
        )
        .unwrap();
        let recovery_path = lease.path().to_owned();
        let recovery_bytes = std::fs::read(&recovery_path).unwrap();
        let mut state = session();
        state.open_editor();
        insert(&mut state, "first draft");
        let first = state.active_page_id();
        state.pages.editor.editor_recovery_id = lease.id().clone();
        state.pages.editor.editor_recovery_lease = Some(lease);
        let id = next_editor_task_id();
        pending_save(&mut state, id, root.join("first.txt"));
        assert!(state.prepare_editor_background_close());
        state.tasks.transfer_operations(first);
        state.close_page_instance(first);
        state.open_editor();
        insert(&mut state, "second draft");
        let second = state.active_page_id();
        assert!(
            app::editor_recovery::claim_next_editor_recovery(&paths, "alice")
                .unwrap()
                .is_none()
        );
        if cleanup_fails {
            std::fs::remove_file(&recovery_path).unwrap();
            std::fs::create_dir(&recovery_path).unwrap();
        }
        state
            .editor_task_runtime
            .shared
            .event_tx
            .send(EditorTaskEvent::SaveFinished {
                id,
                result: if success {
                    Ok(fingerprint())
                } else {
                    Err(EditorSaveTaskError::Write("write failed".into()))
                },
            })
            .unwrap();
        state.poll_editor_background_tasks(&platform::mock::UnsupportedPlatform);
        assert_eq!(state.active_page_id(), second);
        assert_eq!(
            state.app.editor_state().unwrap().export_text(),
            "second draft"
        );
        assert!(state.app.editor_state().unwrap().is_dirty());
        if cleanup_fails {
            assert!(recovery_path.is_dir());
            std::fs::remove_dir(&recovery_path).unwrap();
            std::fs::write(&recovery_path, recovery_bytes).unwrap();
            assert!(
                app::editor_recovery::claim_next_editor_recovery(&paths, "alice")
                    .unwrap()
                    .is_none(),
                "cleanup retries must retain only the closed editor's recovery lease"
            );
            state
                .editor_task_runtime
                .retry_recovery_cleanup(Instant::now() + Duration::from_secs(11));
        }
        assert_eq!(recovery_path.exists(), !success);
        assert_eq!(
            app::editor_recovery::claim_next_editor_recovery(&paths, "alice")
                .unwrap()
                .is_some(),
            !success
        );
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }
}
