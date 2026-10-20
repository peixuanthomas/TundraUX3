use super::*;

#[path = "package_workspace.rs"]
mod package_workspace;

#[test]
fn management_details_expand_animates_without_requery_and_respects_reduced_motion() {
    for kind in [
        ManagementKind::Services,
        ManagementKind::Packages,
        ManagementKind::Disks,
    ] {
        for reduced in [false, true] {
            let mut session = shortcut_action_state(kind, "inspect");
            session
                .pages
                .management
                .management_state
                .snapshot
                .details_complete = true;
            session.pages.management.management_state.snapshot.details =
                vec![("Name".into(), "Selected".into())];
            let context = ui::RenderContext::default();
            let bounds = Rect::new(0, 0, 120, 40);
            let layout = ui::ShellFrameLayout::new(bounds, None, &context);
            let mut motion = crate::session::motion_effects::ShellMotionEffects::default();
            motion.update_layout(&session, &layout, context.theme, reduced);
            assert!(!motion.is_running());
            session.handle_management_key(&KeyInput::new(InputKey::F(4)));
            assert!(session.to_management_view_model().details_only);
            assert!(
                session
                    .pages
                    .management
                    .management_state
                    .query_job
                    .is_none()
            );
            motion.update_layout(&session, &layout, context.theme, reduced);
            assert_eq!(motion.is_running(), !reduced);
            let mut cell = ratatui::buffer::Cell::new("X");
            cell.set_fg(context.theme.text)
                .set_bg(context.theme.surface);
            let final_frame = ratatui::buffer::Buffer::filled(bounds, cell);
            let mut buffer = final_frame.clone();
            motion.process(Duration::ZERO, &mut buffer, &session);
            assert_eq!(buffer == final_frame, reduced);
            buffer = final_frame.clone();
            motion.process(Duration::from_secs(1), &mut buffer, &session);
            assert!(!motion.is_running());
            assert_eq!(buffer, final_frame);
        }
    }
}

#[test]
fn failed_empty_management_queries_expose_complete_details_without_retry_or_authorization() {
    for kind in [
        ManagementKind::Services,
        ManagementKind::Network,
        ManagementKind::Disks,
        ManagementKind::Ssh,
        ManagementKind::SystemSettings,
    ] {
        for pointer in [false, true] {
            let mut session = shortcut_action_state(kind, "inspect");
            session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
            session.pages.management.management_state.snapshot = ManagementSnapshot::default();
            let (job, _) = session.management_job();
            let native = "native query failed: environment unavailable\nfull diagnostic line";
            *job.0.snapshot.lock().unwrap() =
                Some(Err(ManagementError::UnsupportedEnvironment(native.into())));
            session.register_management_job(&job, TaskDomain::ManagementQuery, TaskClass::Query);
            session.pages.management.management_state.query_job = Some(job);
            session.poll_management();
            let model = session.to_management_view_model();
            assert!(model.rows.is_empty());
            assert_ne!(model.empty_message, i18n::tr!("management-empty"));
            assert!(model.details.contains(native));
            assert!(
                model
                    .actions
                    .iter()
                    .any(|(label, enabled)| label.contains("F4") && *enabled)
            );
            if pointer {
                let index = session
                    .management_actions()
                    .iter()
                    .position(|(action, _)| action.id == "query_details")
                    .unwrap();
                let area = ui::management_layout(session.management_main(), &model).actions[index];
                click_management_point(&mut session, (area.x, area.y));
            } else {
                session.handle_management_key(&KeyInput::new(InputKey::F(4)));
            }
            let state = &session.pages.management.management_state;
            assert!(state.details_only);
            assert!(state.query_job.is_none() && state.operation_job.is_none());
            assert!(state.form.is_none());
            assert!(session.to_management_view_model().details.contains(native));
            session.handle_management_key(&KeyInput::new(InputKey::Escape));
            assert!(!session.pages.management.management_state.details_only);
        }
    }
}

#[test]
fn empty_management_information_keeps_full_requirements_and_backup_guidance_reachable() {
    for kind in [
        ManagementKind::Firewall,
        ManagementKind::Backups,
        ManagementKind::ScheduledTasks,
    ] {
        let mut session = shortcut_action_state(kind, "inspect");
        session.pages.management.management_state.snapshot = ManagementSnapshot {
            notices: vec!["Short explanation".into()],
            details: vec![(
                "Requirements and next step".into(),
                "Full requirements, recovery scope and diagnostic output".into(),
            )],
            details_complete: true,
            ..Default::default()
        };
        let model = session.to_management_view_model();
        assert_eq!(model.empty_message, "Short explanation");
        assert!(model.details.contains("Full requirements"));
        let index = session
            .management_actions()
            .iter()
            .position(|(action, _)| action.id == "query_details")
            .unwrap();
        session.activate_management_action(index);
        assert!(session.pages.management.management_state.details_only);
        assert!(
            session
                .pages
                .management
                .management_state
                .query_job
                .is_none()
        );
    }
}

#[test]
fn process_summary_headers_sort_whole_rows_and_retain_selected_identity() {
    let mut session = shortcut_action_state(ManagementKind::Processes, "term");
    let state = &mut session.pages.management.management_state;
    state.snapshot.columns = [
        "Name",
        "PID",
        "CPU %",
        "Memory",
        "PPID",
        "UID",
        "State",
        "Nice (main thread)",
    ]
    .map(String::from)
    .to_vec();
    state.snapshot.rows[0].cells = ["Zulu", "9", "2.0", "90.0 MiB", "1", "1000", "S", "0"]
        .map(String::from)
        .to_vec();
    let mut other = state.snapshot.rows[0].clone();
    other.id = "process-10".into();
    other.cells = ["alpha", "10", "10.0", "100.0 MiB", "1", "1000", "S", "0"]
        .map(String::from)
        .to_vec();
    state.snapshot.rows.push(other);
    let selected_id = state.snapshot.rows[state.selected].id.clone();
    for (column, ascending_id) in [
        (0, "process-10"),
        (1, selected_id.as_str()),
        (2, selected_id.as_str()),
        (3, selected_id.as_str()),
    ] {
        let layout = ui::management_layout(
            session.management_main(),
            &session.to_management_view_model(),
        );
        let (_, header) = layout
            .headers
            .iter()
            .find(|(index, _)| *index == column)
            .unwrap();
        click_management_point(&mut session, (header.x, header.y));
        let state = &session.pages.management.management_state;
        assert_eq!(state.sort.unwrap().column, column);
        assert_eq!(state.snapshot.rows[0].id, ascending_id);
        assert_eq!(state.snapshot.rows[state.selected].id, selected_id);
        assert_eq!(
            state.snapshot.rows[0].cells[1],
            if ascending_id == "process-10" {
                "10"
            } else {
                "9"
            }
        );
    }
}

fn translated_management_language(generation: u64) -> i18n::LanguageSnapshot {
    i18n::LanguageSnapshotData {
        code: "zh-CN".into(),
        generation,
        current: vec!["test-management-name = 名称\ntest-management-count = 数量\n".into()],
        english: vec![],
    }
    .restore()
    .unwrap()
}

#[test]
fn management_form_f10_answers_once_and_tab_applies_pending_search() {
    let mut session = state();
    let (job, responses) = session.management_job();
    session.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    session.pages.management.management_state.operation_job = Some(job);
    session.pages.management.management_state.form = Some(ManagementEditor {
        instance: next_management_form_instance(),
        title: "Scheduled task answer".into(),
        message: String::new(),
        message_scroll: 0,
        selected: 0,
        purpose: FormPurpose::Answer("schedule-question".into()),
        fields: vec![ManagementField {
            id: "answer".into(),
            value: "Yes".into(),
            required: true,
            ..Default::default()
        }],
    });
    session.handle_management_key(&KeyInput::new(InputKey::F(10)).repeated());
    assert!(responses.try_recv().is_err());
    session.handle_management_key(&KeyInput::new(InputKey::F(10)));
    assert!(
        matches!(responses.try_recv().unwrap(), OperationInput::Answer { id, value } if id == "schedule-question" && value == "Yes")
    );
    assert!(session.pages.management.management_state.form.is_none());

    let mut session = shortcut_action_state(ManagementKind::Users, "inspect");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    session.management_touch_control(ui::ManagementControl::Search);
    session.handle_management_paste("alice");
    assert!(session.pages.management.management_state.filtering);
    session.handle_management_key(&KeyInput::new(InputKey::Tab));
    assert!(!session.pages.management.management_state.filtering);
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .filter,
        "alice"
    );
    assert!(!session.pages.management.management_state.actions_focused);
}

#[test]
fn cached_management_language_refreshes_once_and_keeps_authorized_query_and_sort() {
    let mut session = shortcut_action_state(ManagementKind::Backups, "inspect");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    let old_language = (
        session.language.code().to_owned(),
        session.language.generation(),
    );
    let page = session.active_page_id();
    let state = &mut session.pages.management.management_state;
    state.query.as_mut().unwrap().filter = "selected filter".into();
    state.snapshot.refresh_action = Some(ManagementAction {
        id: "inspect_fixture".into(),
        privileged: true,
        ..Default::default()
    });
    state.snapshot.columns = vec!["Name".into(), "Count".into()];
    state.snapshot.rows[0].cells = vec!["Zulu".into(), "2".into()];
    let mut other = state.snapshot.rows[0].clone();
    other.id = "other".into();
    other.cells = vec!["alpha".into(), "10".into()];
    state.snapshot.rows.push(other);
    state.snapshot_language = Some(old_language);
    state.snapshot_query = state.query.clone();
    session.sort_management(0);
    let expected_query = session.pages.management.management_state.query.clone();
    let expected_sort = session.pages.management.management_state.sort;
    session.enter_new_page(ShellScreen::Settings);
    let generation = session.language.generation() + 1;
    session.publish_language(
        i18n::LanguageCatalog::built_in(),
        i18n::LanguageLoad {
            snapshot: translated_management_language(generation),
            diagnostics: vec![],
        },
    );
    session.poll_management();
    assert!(
        session
            .pages
            .management
            .get(page)
            .unwrap()
            .management_state
            .last_language_attempt
            .is_none()
    );
    session.return_from_screen(ShellScreen::Settings);
    assert_eq!(session.active_page_id(), page);
    session.poll_management();
    let language = ("zh-CN".into(), generation);
    let state = &session.pages.management.management_state;
    assert_eq!(state.last_language_attempt.as_ref(), Some(&language));
    assert_eq!(state.query, expected_query);
    assert_eq!(state.refresh_command().unwrap().action, "inspect_fixture");
    assert_eq!(
        state.refresh_command().unwrap().values["filter"],
        "selected filter"
    );
    session.pages.management.management_state.status = "do not retry failed refresh".into();
    session.poll_management();
    assert_eq!(
        session.pages.management.management_state.status,
        "do not retry failed refresh"
    );
    let mut snapshot = session.pages.management.management_state.snapshot.clone();
    snapshot.columns = vec![
        session.language.render(&i18n::msg!("test-management-name")),
        session
            .language
            .render(&i18n::msg!("test-management-count")),
    ];
    snapshot.rows.reverse();
    let (job, _) = session.management_job();
    *job.0.snapshot.lock().unwrap() = Some(Ok(snapshot));
    session.register_management_job(&job, TaskDomain::ManagementQuery, TaskClass::Query);
    session.pages.management.management_state.query_job = Some(job);
    session.poll_management();
    let state = &session.pages.management.management_state;
    assert_eq!(state.snapshot_language.as_ref(), Some(&language));
    assert_eq!(state.snapshot.columns, ["名称", "数量"]);
    assert_eq!(state.sort, expected_sort);
    assert_eq!(state.snapshot.rows[0].id, "other");
    assert_eq!(state.snapshot.rows[state.selected].id, "mock-selected-item");
    assert_eq!(state.query, expected_query);
}

#[test]
fn language_refresh_waits_for_writes_and_rejects_old_language_results_as_current() {
    let mut session = shortcut_action_state(ManagementKind::ScheduledTasks, "inspect");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    let (job, _) = session.management_job();
    let old_language = job.0.language.clone();
    session.pages.management.management_state.snapshot_language = Some(old_language.clone());
    session.pages.management.management_state.operation_job = Some(job.clone());
    session.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    let generation = session.language.generation() + 1;
    session.publish_language(
        i18n::LanguageCatalog::built_in(),
        i18n::LanguageLoad {
            snapshot: translated_management_language(generation),
            diagnostics: vec![],
        },
    );
    session.poll_management();
    assert!(
        session
            .pages
            .management
            .management_state
            .last_language_attempt
            .is_none()
    );
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .operation_job
            .as_ref(),
        Some(&job)
    );
    assert!(!job.0.cancelled.load(Ordering::Relaxed));
    let snapshot = session.pages.management.management_state.snapshot.clone();
    job.0.events.lock().unwrap().extend([
        OperationEvent::Snapshot { snapshot },
        OperationEvent::Completed {
            message: "write finished".into(),
        },
    ]);
    session.poll_management();
    assert_eq!(
        session.pages.management.management_state.snapshot_language,
        Some(old_language)
    );
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .last_language_attempt,
        Some(("zh-CN".into(), generation))
    );
    assert!(
        session
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    );
}

#[test]
fn language_refresh_sort_retention_requires_the_same_view() {
    let mut state = ManagementState::default();
    let mut old_query = ManagementQuery::new(ManagementKind::Firewall);
    old_query.scope = "listeners".into();
    state.snapshot_language = Some(("en-US".into(), 1));
    state.snapshot_query = Some(old_query);
    state.sort_columns = vec!["Name".into(), "Count".into()];
    state.sort = Some(ui::TableSort {
        column: 1,
        descending: true,
    });
    let new_query = ManagementQuery::new(ManagementKind::Firewall);
    let mut snapshot = ManagementSnapshot {
        columns: vec!["规则".into(), "来源".into()],
        ..Default::default()
    };
    state.sort_snapshot(&mut snapshot, &("zh-CN".into(), 2), Some(&new_query));
    assert!(state.sort.is_none());
    state.sort = Some(ui::TableSort {
        column: 0,
        descending: false,
    });
    let mut snapshot = ManagementSnapshot {
        columns: vec!["Rule".into(), "Source".into()],
        ..Default::default()
    };
    state.sort_snapshot(&mut snapshot, &("en-US".into(), 3), Some(&new_query));
    assert!(state.sort.is_some());
}

#[test]
fn new_management_choices_translate_only_defined_option_fields() {
    for (action, field_id, raw, key) in [
        (
            "add_rule",
            "duration",
            "runtime",
            "management-value-runtime",
        ),
        (
            "add_rule",
            "duration",
            "permanent",
            "management-value-permanent",
        ),
        ("add_rule", "duration", "both", "management-value-both"),
        (
            "ssh_configure",
            "password_authentication",
            "yes",
            "management-value-yes",
        ),
        (
            "ssh_configure",
            "pubkey_authentication",
            "no",
            "management-value-no",
        ),
        (
            "ssh_configure",
            "permit_root_login",
            "yes",
            "management-value-root-login-yes",
        ),
        (
            "ssh_configure",
            "permit_root_login",
            "no",
            "management-value-root-login-no",
        ),
        (
            "ssh_configure",
            "permit_root_login",
            "prohibit-password",
            "management-value-root-login-prohibit-password",
        ),
        (
            "ssh_configure",
            "permit_root_login",
            "forced-commands-only",
            "management-value-root-login-forced-commands-only",
        ),
    ] {
        let purpose = FormPurpose::Action(
            ManagementAction {
                id: action.into(),
                ..Default::default()
            },
            None,
        );
        let field = ManagementField {
            id: field_id.into(),
            value: raw.into(),
            choices: vec![raw.into()],
            ..Default::default()
        };
        assert_eq!(form_value_text(&purpose, &field, raw), i18n::tr!(key));
        assert_eq!(field.value, raw);
        assert_eq!(field.choices, vec![raw]);
        let account_field = ManagementField {
            id: "username".into(),
            ..field.clone()
        };
        assert_eq!(form_value_text(&purpose, &account_field, raw), raw);
        let unrelated = FormPurpose::Action(
            ManagementAction {
                id: "user_info".into(),
                ..Default::default()
            },
            None,
        );
        assert_eq!(form_value_text(&unrelated, &field, raw), raw);
    }
}

#[test]
fn editor_dialogs_use_specific_translated_titles_and_actions() {
    let mut session = state();
    session.enter_screen(ShellScreen::Editor);
    for action in [
        "edit-goto",
        "edit-file",
        "edit-language",
        "edit-newline",
        "edit-encoding",
        "edit-indent",
        "edit-about",
    ] {
        session.config_form(action, "Content".into(), vec![]);
        let form = session.to_management_view_model().form.unwrap();
        assert!(!form.title.contains("config-editor-"));
        assert_ne!(form.title, i18n::tr!("shell-editor"));
        assert!(!form.submit_label.unwrap().contains("config-editor-"));
        assert_eq!(form.dismiss_only, action == "edit-about");
    }
}

#[test]
fn system_configuration_opens_the_path_form_without_a_single_item_menu() {
    let mut session = state();
    session.enter_screen(ShellScreen::Editor);
    session.config_editor_action(ui::EditorConfigAction::Menu);
    let form = session.to_management_view_model().form.unwrap();
    assert!(form.identity.ends_with(".config-open"));
    assert!(
        form.identity
            .split('.')
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 0
    );
    assert!(form.choice.is_none());
    assert!(!form.message.is_empty());
    assert_eq!(form.fields.len(), 1);
    assert_eq!(form.fields[0].label, i18n::tr!("config-editor-path"));
    assert!(form.fields[0].value.is_empty());
}

#[test]
fn editor_choice_dialogs_apply_the_original_value_and_close_in_one_step() {
    let mut session = state();
    session.enter_screen(ShellScreen::Editor);
    session.config_form(
        "edit-language",
        String::new(),
        vec![ManagementField {
            id: "value".into(),
            value: "Auto".into(),
            choices: vec!["Auto".into(), "Text".into()],
            ..Default::default()
        }],
    );
    let form = session.to_management_view_model().form.unwrap();
    assert_eq!(
        form.choice.unwrap().values,
        vec![
            i18n::tr!("ui-editor-language-auto"),
            i18n::tr!("ui-editor-language-text")
        ]
    );
    session.pages.management.management_state.choice_selected = 1;
    session.handle_management_choice_key(&KeyInput::new(InputKey::Enter));
    assert!(session.pages.management.management_state.form.is_none());
    assert_eq!(
        session.pages.editor.editor_language.as_deref(),
        Some("Text")
    );
    session.config_form(
        "edit-indent",
        String::new(),
        vec![ManagementField {
            id: "value".into(),
            value: "Space:4".into(),
            choices: vec!["Space:4".into(), "Tab:4".into()],
            ..Default::default()
        }],
    );
    let values = session
        .to_management_view_model()
        .form
        .unwrap()
        .choice
        .unwrap()
        .values;
    assert!(values[0].contains('4') && !values[0].contains("Space:"));
    assert!(values[1].contains('4') && !values[1].contains("Tab:"));
    session.handle_management_choice_key(&KeyInput::new(InputKey::Escape));
    assert!(session.pages.management.management_state.form.is_none());
}

#[test]
fn same_kind_operations_finish_without_replacing_each_others_snapshot() {
    let mut session = state();
    let first = session.active_page_id();
    let (first_job, _) = session.management_job();
    session.register_management_job(
        &first_job,
        TaskDomain::ManagementOperation,
        TaskClass::Operation,
    );
    session.pages.management.management_state.operation_job = Some(first_job.clone());
    session.enter_new_page(ShellScreen::Management);
    let second = session.active_page_id();
    session.pages.management.management_state.kind = Some(ManagementKind::Services);
    let (second_job, _) = session.management_job();
    session.register_management_job(
        &second_job,
        TaskDomain::ManagementOperation,
        TaskClass::Operation,
    );
    session.pages.management.management_state.operation_job = Some(second_job.clone());
    for (job, label) in [
        (&first_job, "first operation"),
        (&second_job, "second operation"),
    ] {
        job.0.events.lock().unwrap().extend([
            OperationEvent::Snapshot {
                snapshot: ManagementSnapshot {
                    backend: label.into(),
                    ..Default::default()
                },
            },
            OperationEvent::Completed {
                message: label.into(),
            },
        ]);
    }
    session.poll_management();
    assert_eq!(session.active_page_id(), second);
    assert_eq!(
        session.pages.management.management_state.snapshot.backend,
        "second operation"
    );
    assert_eq!(
        session.pages.management.management_state.outcome.as_deref(),
        Some("second operation")
    );
    let hidden = &session
        .pages
        .management
        .get(first)
        .unwrap()
        .management_state;
    assert_eq!(hidden.snapshot.backend, "first operation");
    assert_eq!(hidden.outcome.as_deref(), Some("first operation"));
    assert!(hidden.operation_job.is_none());
    assert!(
        session
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    );
}

#[test]
fn same_kind_queries_deliver_to_their_own_covered_page() {
    let mut session = state();
    session.pages.management.management_state.kind = Some(ManagementKind::Packages);
    let first = session.active_page_id();
    let (first_job, _) = session.management_job();
    session.register_management_job(&first_job, TaskDomain::ManagementQuery, TaskClass::Query);
    session.pages.management.management_state.query_job = Some(first_job.clone());
    session.enter_new_page(ShellScreen::Management);
    let second = session.active_page_id();
    session.pages.management.management_state.kind = Some(ManagementKind::Packages);
    let (second_job, _) = session.management_job();
    session.register_management_job(&second_job, TaskDomain::ManagementQuery, TaskClass::Query);
    session.pages.management.management_state.query_job = Some(second_job.clone());
    *first_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        backend: "first query".into(),
        ..Default::default()
    }));
    *second_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        backend: "second query".into(),
        ..Default::default()
    }));
    session.poll_management();
    assert_eq!(session.active_page_id(), second);
    assert_eq!(
        session.pages.management.management_state.snapshot.backend,
        "second query"
    );
    assert_eq!(
        session
            .pages
            .management
            .get(first)
            .unwrap()
            .management_state
            .snapshot
            .backend,
        "first query"
    );
    session.close_page_instance(second);
    assert_eq!(session.active_page_id(), first);
    assert_eq!(
        session.pages.management.management_state.snapshot.backend,
        "first query"
    );
}

#[test]
fn closed_management_query_is_cancelled_and_cannot_write_into_a_new_page() {
    let mut session = state();
    let first = session.active_page_id();
    let (job, _) = session.management_job();
    session.register_management_job(&job, TaskDomain::ManagementQuery, TaskClass::Query);
    session.pages.management.management_state.query_job = Some(job.clone());
    session.close_page_instance(first);
    assert!(job.0.cancelled.load(Ordering::Relaxed));
    session.enter_new_page(ShellScreen::Management);
    *job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        backend: "late old result".into(),
        ..Default::default()
    }));
    session.poll_management();
    assert!(
        session
            .pages
            .management
            .management_state
            .snapshot
            .backend
            .is_empty()
    );
    assert_eq!(
        session.tasks.owner(TaskDomain::ManagementQuery, job.0.id),
        None
    );
}

#[test]
fn detached_management_operation_does_not_keep_or_update_its_page() {
    let mut session = state();
    let first = session.active_page_id();
    let (job, _) = session.management_job();
    session.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    session.pages.management.management_state.operation_job = Some(job.clone());
    session.tasks.transfer_operations(first);
    session.detach_management_page(first);
    session.close_page_instance(first);
    assert!(session.pages.management.get(first).is_none());
    assert!(!job.0.cancelled.load(Ordering::Relaxed));
    session.enter_new_page(ShellScreen::Management);
    session.pages.management.management_state.status = "new page".into();
    job.0.events.lock().unwrap().extend([
        OperationEvent::Snapshot {
            snapshot: ManagementSnapshot {
                backend: "old operation".into(),
                ..Default::default()
            },
        },
        OperationEvent::Completed {
            message: "finished old operation".into(),
        },
    ]);
    session.poll_management();
    assert_eq!(session.pages.management.management_state.status, "new page");
    assert!(
        session
            .pages
            .management
            .management_state
            .snapshot
            .backend
            .is_empty()
    );
    assert!(session.detached_management_operations.is_empty());
    assert_eq!(
        session
            .tasks
            .owner(TaskDomain::ManagementOperation, job.0.id),
        None
    );
}

#[test]
fn account_state_translations_preserve_names_and_native_output() {
    let users = Some(ManagementKind::Users);
    assert_eq!(
        management_detail_value(users, "Source", "remote"),
        i18n::tr!("management-account-source-remote")
    );
    assert_eq!(
        management_detail_value(users, "Password locked", "true"),
        i18n::tr!("management-value-true")
    );
    assert_eq!(
        management_detail_value(
            users,
            "Groups",
            "unknown (select the account to query full membership)"
        ),
        i18n::tr!("management-value-unknown-select-the-account-to-query-full-membership")
    );
    for (key, value) in [
        ("Groups", "remote"),
        ("Home", "/home/local"),
        ("Account expiry", "2030-01-01"),
    ] {
        assert_eq!(management_detail_value(users, key, value), value);
    }
    assert_eq!(
        management_detail_value(Some(ManagementKind::Services), "Source", "remote"),
        "remote"
    );
}

fn shortcut_action_state(kind: ManagementKind, id: &str) -> ShellSession {
    let mut session = state();
    session.pages.management.management_state.kind = Some(kind);
    session.pages.management.management_state.query = Some(ManagementQuery::new(kind));
    session
        .pages
        .management
        .management_state
        .snapshot
        .rows
        .push(ManagementRow {
            id: "mock-selected-item".into(),
            identity: BTreeMap::from([("identity".into(), "mock".into())]),
            actions: vec![ManagementAction {
                id: id.into(),
                label: id.into(),
                primary: true,
                confirm: true,
                privileged: true,
                fields: vec![ManagementField {
                    id: "option".into(),
                    label: "Option".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        });
    session
}

#[test]
fn package_navigation_stays_separate_and_disabled_actions_do_not_execute() {
    for id in [
        "remove",
        "install",
        "upgrade",
        "pacman_install",
        "pacman_upgrade",
    ] {
        let mut session = shortcut_action_state(ManagementKind::Packages, id);
        let actions = session.management_actions();
        assert_eq!(
            actions
                .iter()
                .take(3)
                .map(|(action, _)| action.id.as_str())
                .collect::<Vec<_>>(),
            ["scope_installed", "scope_updates", "scope_search"]
        );
        let index = actions
            .iter()
            .position(|(action, _)| action.id == id)
            .unwrap();
        let model = session.to_management_view_model();
        assert!(model.workspace.unwrap().primary_actions.contains(&index));
        assert!(model.actions[index].1);
        session.pages.management.management_state.snapshot.rows[0].actions[0].disabled_reason =
            Some("Unavailable".into());
        assert!(!session.to_management_view_model().actions[index].1);
        session.activate_management_action(index);
        assert!(session.pages.management.management_state.form.is_none());
        assert!(
            session
                .pages
                .management
                .management_state
                .draft_job
                .is_none()
        );
        assert!(
            session
                .pages
                .management
                .management_state
                .operation_job
                .is_none()
        );
    }
}

#[test]
fn package_detail_buttons_use_the_same_target_for_pointer_and_keyboard() {
    for by_touch in [false, true] {
        let mut session = shortcut_action_state(ManagementKind::Packages, "install");
        session.terminal_size = (120, 30);
        let model = session.to_management_view_model();
        let layout = ui::management_layout(session.management_main(), &model);
        let index = session
            .management_actions()
            .iter()
            .position(|(action, _)| action.id == "install")
            .unwrap();
        let install = layout.actions[index];
        assert_eq!(install.intersection(layout.detail_actions_panel), install);
        if by_touch {
            click_management_point(&mut session, (install.x, install.y));
        } else {
            session.handle_management_key(&KeyInput::new(InputKey::Char('i')));
        }
        assert!(
            matches!(session.pages.management.management_state.form.as_ref().map(|form| &form.purpose), Some(FormPurpose::Action(action, Some(row))) if action.id == "install" && row.id == "mock-selected-item")
        );
    }
}

#[test]
fn common_account_actions_are_visible_and_dangerous_changes_stay_in_more() {
    let mut session = shortcut_action_state(ManagementKind::Users, "user_info");
    session.pages.management.management_state.snapshot.rows[0]
        .actions
        .extend(
            [
                "user_password",
                "user_groups",
                "user_delete",
                "user_lock",
                "user_expiry",
            ]
            .map(|id| ManagementAction {
                id: id.into(),
                ..Default::default()
            }),
        );
    session
        .pages
        .management
        .management_state
        .snapshot
        .actions
        .extend(["user_create", "set_view"].map(|id| ManagementAction {
            id: id.into(),
            ..Default::default()
        }));
    let actions = session.management_actions();
    assert_eq!(
        actions
            .iter()
            .map(|(action, _)| action.id.as_str())
            .collect::<Vec<_>>(),
        [
            "user_create",
            "set_view",
            "more_actions",
            "user_info",
            "user_password",
            "user_groups"
        ]
    );
    assert_eq!(
        session.to_management_view_model().detail_action_start,
        Some(0)
    );
    session.activate_management_action(2);
    assert!(
        matches!(&session.pages.management.management_state.form.as_ref().unwrap().purpose, FormPurpose::Menu(items) if items.iter().any(|(action, _)| action.id == "user_delete"))
    );
}

#[test]
fn services_expose_restart_and_logs_but_keep_startup_changes_in_more() {
    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.pages.management.management_state.snapshot.rows[0]
        .actions
        .extend(
            ["stop", "restart", "view_logs", "enable", "disable"].map(|id| ManagementAction {
                id: id.into(),
                ..Default::default()
            }),
        );
    let model = session.to_management_view_model();
    assert_eq!(model.actions.len(), 5);
    let layout = ui::management_layout(Rect::new(0, 0, 100, 24), &model);
    assert_eq!(layout.actions.len(), 5);
    assert!(layout.action_next.is_empty());
    assert!(model.actions[2].0.starts_with("[T]"));
    assert!(model.actions[3].0.starts_with("[L]"));
}

fn click_management_point(session: &mut ShellSession, point: (u16, u16)) {
    let now = Instant::now();
    assert!(
        session
            .prepare_button_input(InputEvent::mouse_down(PointerButton::Left, point), now)
            .is_none()
    );
    let released = session.prepare_button_input(
        InputEvent::mouse_up(PointerButton::Left, point),
        now + Duration::from_millis(30),
    );
    let Some(InputEvent::Mouse(mouse)) = released else {
        panic!("button must capture matching release")
    };
    session.handle_management_pointer(mouse);
}

#[test]
fn clearing_management_search_and_refreshing_restores_the_full_list() {
    for refresh_with_key in [false, true] {
        let mut session = shortcut_action_state(ManagementKind::Packages, "install");
        session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
        session.pages.management.management_state.filter_input = "bash".into();
        session.apply_management_filter();
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .query
                .as_ref()
                .unwrap()
                .filter,
            "bash"
        );

        let (search_job, _) = session.management_job();
        *search_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
            rows: vec![ManagementRow {
                id: "bash".into(),
                ..Default::default()
            }],
            ..Default::default()
        }));
        session.register_management_job(&search_job, TaskDomain::ManagementQuery, TaskClass::Query);
        session.pages.management.management_state.query_job = Some(search_job);
        session.poll_management();
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .snapshot
                .rows
                .len(),
            1
        );

        session
            .pages
            .management
            .management_state
            .query
            .as_mut()
            .unwrap()
            .target = Some("bash".into());
        session
            .pages
            .management
            .management_state
            .list_scroll_explicit = true;
        session.management_touch_control(ui::ManagementControl::Search);
        for _ in 0..4 {
            session.handle_management_key(&KeyInput::new(InputKey::Backspace));
        }
        assert!(
            session
                .pages
                .management
                .management_state
                .filter_input
                .is_empty()
        );
        if refresh_with_key {
            session.handle_management_key(&KeyInput::new(InputKey::F(5)));
        } else {
            let layout = ui::management_layout(
                session.management_main(),
                &session.to_management_view_model(),
            );
            let area = layout
                .controls
                .iter()
                .find(|(control, _)| *control == ui::ManagementControl::Refresh)
                .unwrap()
                .1;
            click_management_point(&mut session, (area.x, area.y));
        }
        let query = session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap();
        assert!(query.filter.is_empty());
        assert!(query.target.is_none());
        assert!(
            !session
                .pages
                .management
                .management_state
                .list_scroll_explicit
        );

        let (full_job, _) = session.management_job();
        *full_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
            rows: ["bash", "coreutils", "curl"]
                .into_iter()
                .map(|id| ManagementRow {
                    id: id.into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }));
        session.register_management_job(&full_job, TaskDomain::ManagementQuery, TaskClass::Query);
        session.pages.management.management_state.query_job = Some(full_job);
        session.poll_management();
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .snapshot
                .rows
                .len(),
            3
        );
    }
}

#[test]
fn refreshing_management_discards_pending_results_even_without_a_worker() {
    let mut session = shortcut_action_state(ManagementKind::Packages, "install");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    session.pages.management.management_state.filter_input = "bash".into();
    session.apply_management_filter();
    let (old_job, _) = session.management_job();
    session.register_management_job(&old_job, TaskDomain::ManagementQuery, TaskClass::Query);
    session.pages.management.management_state.query_job = Some(old_job.clone());
    session
        .pages
        .management
        .management_state
        .filter_input
        .clear();
    session.management_touch_control(ui::ManagementControl::Refresh);
    assert!(
        session
            .pages
            .management
            .management_state
            .query_job
            .is_none()
    );

    *old_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        backend: "old-search-result".into(),
        ..Default::default()
    }));
    session.poll_management();
    assert_ne!(
        session.pages.management.management_state.snapshot.backend,
        "old-search-result"
    );

    let (current_job, _) = session.management_job();
    *current_job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        backend: "full-list".into(),
        ..Default::default()
    }));
    session.register_management_job(&current_job, TaskDomain::ManagementQuery, TaskClass::Query);
    session.pages.management.management_state.query_job = Some(current_job);
    session.poll_management();
    assert_eq!(
        session.pages.management.management_state.snapshot.backend,
        "full-list"
    );
    session.poll_management();
    assert_eq!(
        session.pages.management.management_state.snapshot.backend,
        "full-list"
    );
}

#[test]
fn management_refresh_updates_drafts_but_retains_an_unchanged_detail_target() {
    let mut session = shortcut_action_state(ManagementKind::Packages, "install");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    session.pages.management.management_state.filter_input = "bash".into();
    session.apply_management_filter();
    session
        .pages
        .management
        .management_state
        .query
        .as_mut()
        .unwrap()
        .target = Some("bash".into());
    session.management_touch_control(ui::ManagementControl::Refresh);
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .target
            .as_deref(),
        Some("bash")
    );
    session.pages.management.management_state.filter_input = "curl".into();
    session.management_touch_control(ui::ManagementControl::Refresh);
    let query = session
        .pages
        .management
        .management_state
        .query
        .as_ref()
        .unwrap();
    assert_eq!(query.filter, "curl");
    assert!(query.target.is_none());
}

#[test]
fn management_action_shortcuts_match_clicks_and_preserve_fields_and_identity() {
    for (kind, ids) in [
        (
            ManagementKind::Services,
            vec![
                "start",
                "stop",
                "restart",
                "enable",
                "disable",
                "view_logs",
                "set_view",
            ],
        ),
        (
            ManagementKind::Processes,
            vec!["term", "kill", "stop", "cont", "nice", "set_view"],
        ),
        (
            ManagementKind::Packages,
            vec![
                "install",
                "remove",
                "upgrade",
                "upgrade_all",
                "pacman_install",
                "pacman_upgrade",
                "pacman_upgrade_all",
                "refresh",
                "scope_search",
                "scope_installed",
                "scope_updates",
            ],
        ),
        (
            ManagementKind::Network,
            vec![
                "inspect_network",
                "configure",
                "wifi-connect",
                "wifi-disconnect",
                "wifi-forget",
                "forget_saved_wifi",
                "check",
            ],
        ),
        (
            ManagementKind::Disks,
            vec!["mount", "unmount", "scan", "open_directory"],
        ),
    ] {
        for id in ids {
            let mut keyed = shortcut_action_state(kind, id);
            let mut clicked = shortcut_action_state(kind, id);
            if kind == ManagementKind::Packages {
                for session in [&mut keyed, &mut clicked] {
                    let scope = match id {
                        "upgrade_all" | "pacman_upgrade_all" => Some("updates"),
                        "refresh" => Some("search"),
                        _ => None,
                    };
                    if let Some(scope) = scope {
                        session
                            .pages
                            .management
                            .management_state
                            .query
                            .as_mut()
                            .unwrap()
                            .scope = scope.into();
                    }
                    session
                        .pages
                        .management
                        .management_state
                        .package_advanced_open = id == "refresh";
                }
            }
            let (shortcut, key) = management_action_shortcut(Some(kind), id)
                .expect("each current action has a shortcut");
            keyed.handle_management_key(&KeyInput::new(key));
            let visible = clicked
                .management_actions()
                .iter()
                .position(|(action, _)| action.id == id && action.disabled_reason.is_none());
            let area = if let Some(index) = visible {
                let model = clicked.to_management_view_model();
                assert_eq!(
                    model.actions[index].0,
                    format!(
                        "[{shortcut}] {}",
                        if kind == ManagementKind::Packages
                            && matches!(id, "scope_search" | "scope_installed" | "scope_updates")
                        {
                            i18n::tr!(format!(
                                "management-package-nav-{}",
                                id.strip_prefix("scope_").unwrap()
                            ))
                        } else {
                            action_label(
                                &clicked.pages.management.management_state.snapshot.rows[0].actions
                                    [0],
                            )
                        }
                    )
                );
                ui::management_layout(clicked.management_main(), &model).actions[index]
            } else {
                let more = clicked
                    .management_actions()
                    .iter()
                    .position(|(action, _)| action.id == "more_actions")
                    .unwrap();
                clicked.activate_management_action(more);
                let FormPurpose::Menu(items) = &clicked
                    .pages
                    .management
                    .management_state
                    .form
                    .as_ref()
                    .unwrap()
                    .purpose
                else {
                    panic!("more menu");
                };
                let index = items
                    .iter()
                    .position(|(action, _)| action.id == id)
                    .unwrap();
                clicked.pages.management.management_state.choice_scroll = index;
                ui::management_layout(
                    clicked.management_main(),
                    &clicked.to_management_view_model(),
                )
                .choice_rows
                .into_iter()
                .find(|(item, _)| *item == index)
                .unwrap()
                .1
            };
            click_management_point(&mut clicked, (area.x, area.y));
            let keyed_form = keyed
                .pages
                .management
                .management_state
                .form
                .as_ref()
                .unwrap();
            let clicked_form = clicked
                .pages
                .management
                .management_state
                .form
                .as_ref()
                .unwrap();
            assert_ne!(keyed_form.instance, clicked_form.instance);
            assert_eq!(
                (
                    &keyed_form.title,
                    &keyed_form.message,
                    keyed_form.message_scroll,
                    &keyed_form.fields,
                    keyed_form.selected,
                    &keyed_form.purpose
                ),
                (
                    &clicked_form.title,
                    &clicked_form.message,
                    clicked_form.message_scroll,
                    &clicked_form.fields,
                    clicked_form.selected,
                    &clicked_form.purpose
                ),
                "{kind:?}/{id}"
            );
            assert!(
                keyed.pages.management.management_state.form.is_some(),
                "{id} still collects operation fields before AutoAdmin approval"
            );
            assert!(
                keyed
                    .pages
                    .management
                    .management_state
                    .operation_job
                    .is_none(),
                "no real Linux operation in shortcut test"
            );
        }
    }
}

#[test]
fn management_toolbar_shortcuts_match_clicks() {
    for (control, key) in [
        (
            ui::ManagementControl::Search,
            KeyInput::new(InputKey::Char('s')),
        ),
        (
            ui::ManagementControl::Search,
            KeyInput::new(InputKey::Char('/')),
        ),
        (
            ui::ManagementControl::Refresh,
            KeyInput::new(InputKey::Char('r')),
        ),
        (
            ui::ManagementControl::Refresh,
            KeyInput::new(InputKey::F(5)),
        ),
        (
            ui::ManagementControl::ApplySearch,
            KeyInput::with_modifiers(InputKey::Enter, ui::KeyModifiers::CTRL),
        ),
        (
            ui::ManagementControl::ClearSearch,
            KeyInput::with_modifiers(InputKey::Char('u'), ui::KeyModifiers::CTRL),
        ),
        (
            ui::ManagementControl::Details,
            KeyInput::new(InputKey::F(4)),
        ),
        (
            ui::ManagementControl::Terminal,
            KeyInput::with_modifiers(InputKey::Char('t'), ui::KeyModifiers::CTRL),
        ),
    ] {
        let mut keyed = shortcut_action_state(ManagementKind::Services, "start");
        let mut clicked = shortcut_action_state(ManagementKind::Services, "start");
        keyed.pages.management.management_state.filter_input = "needle".into();
        clicked.pages.management.management_state.filter_input = "needle".into();
        keyed.handle_management_key(&key);
        let layout = ui::management_layout(
            clicked.management_main(),
            &clicked.to_management_view_model(),
        );
        match control {
            ui::ManagementControl::Search => {
                click_management_point(&mut clicked, (layout.filter.x, layout.filter.y))
            }
            ui::ManagementControl::Refresh | ui::ManagementControl::ClearSearch => {
                let area = layout
                    .controls
                    .iter()
                    .find(|(id, _)| *id == control)
                    .unwrap()
                    .1;
                click_management_point(&mut clicked, (area.x, area.y));
            }
            ui::ManagementControl::ApplySearch => {
                click_management_point(&mut clicked, (layout.filter.x, layout.filter.y));
                clicked.handle_management_key(&KeyInput::new(InputKey::Enter));
            }
            // Retain the existing keyboard commands even though these controls
            // now live in menus or the current-task entry.
            _ => clicked.management_touch_control(control),
        }
        assert_eq!(
            keyed.pages.management.management_state.query,
            clicked.pages.management.management_state.query,
            "{control:?}"
        );
        assert_eq!(
            keyed.pages.management.management_state.filter_input,
            clicked.pages.management.management_state.filter_input
        );
        assert_eq!(
            keyed.pages.management.management_state.filtering,
            clicked.pages.management.management_state.filtering
        );
        assert_eq!(
            keyed.pages.management.management_state.details_only,
            clicked.pages.management.management_state.details_only
        );
        assert_eq!(
            keyed.pages.management.management_state.terminal_mode,
            clicked.pages.management.management_state.terminal_mode
        );
    }
}

#[test]
fn management_shortcuts_cannot_bypass_disabled_actions_or_unrelated_modifiers() {
    let mut session = shortcut_action_state(ManagementKind::Processes, "kill");
    session.pages.management.management_state.snapshot.rows[0].actions[0].disabled_reason =
        Some("Protected process".into());
    for key in [InputKey::Char('k'), InputKey::Char('1')] {
        session.handle_management_key(&KeyInput::new(key));
        assert!(session.pages.management.management_state.form.is_none());
        assert_eq!(
            session.pages.management.management_state.status,
            i18n::tr!("management-problem-unavailable")
        );
        assert_eq!(
            session.pages.management.management_state.output,
            "Protected process"
        );
    }
    session.pages.management.management_state.snapshot.rows[0].actions[0].disabled_reason = None;
    for modifiers in [
        ui::KeyModifiers::CTRL,
        ui::KeyModifiers::ALT,
        ui::KeyModifiers {
            super_key: true,
            ..Default::default()
        },
        ui::KeyModifiers {
            meta: true,
            ..Default::default()
        },
    ] {
        for key in [
            InputKey::Char('k'),
            InputKey::Char('s'),
            InputKey::Char('1'),
            InputKey::F(5),
        ] {
            session.handle_management_key(&KeyInput::with_modifiers(key, modifiers));
            assert!(session.pages.management.management_state.form.is_none());
            assert!(!session.pages.management.management_state.filtering);
            assert!(
                session
                    .pages
                    .management
                    .management_state
                    .query_job
                    .is_none()
            );
        }
    }
    let (job, _) = session.management_job();
    session.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    session.pages.management.management_state.operation_job = Some(job);
    assert!(!session.to_management_view_model().actions[0].1);
    session.handle_management_key(&KeyInput::new(InputKey::Char('k')));
    assert!(session.pages.management.management_state.form.is_none());
    session.handle_management_key(&KeyInput::new(InputKey::F(10)));
    assert!(
        matches!(session.pages.management.management_state.form.as_ref().unwrap().purpose, FormPurpose::Action(ref action, _) if action.id == "cancel_operation")
    );
}

#[test]
fn management_text_input_and_shortcut_repeats_do_not_trigger_actions() {
    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.handle_management_key(&KeyInput::new(InputKey::Char('s')));
    session.handle_management_key(&KeyInput::new(InputKey::Char('s')).repeated());
    assert!(
        session
            .pages
            .management
            .management_state
            .filter_input
            .is_empty(),
        "held Search does not type into its new field"
    );
    for c in "srak1".chars() {
        session.handle_management_key(&KeyInput::new(InputKey::Char(c)));
    }
    assert_eq!(
        session.pages.management.management_state.filter_input,
        "srak1"
    );
    assert!(session.pages.management.management_state.form.is_none());
    session.handle_management_key(&KeyInput::new(InputKey::Escape));
    session.pages.management.management_state.snapshot.rows[0].actions[0]
        .fields
        .push(ManagementField {
            id: "text".into(),
            ..Default::default()
        });
    session.handle_management_key(&KeyInput::new(InputKey::Char('a')));
    session.handle_management_key(&KeyInput::new(InputKey::Char('a')).repeated());
    assert!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value
            .is_empty()
    );
    for c in "srak1".chars() {
        session.handle_management_key(&KeyInput::new(InputKey::Char(c)));
    }
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "srak1"
    );
    assert!(
        session
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    );
    session.handle_management_key(&KeyInput::with_modifiers(
        InputKey::Char('t'),
        ui::KeyModifiers::CTRL,
    ));
    assert!(session.pages.management.management_state.terminal_mode);
    session.handle_management_key(
        &KeyInput::with_modifiers(InputKey::Char('t'), ui::KeyModifiers::CTRL).repeated(),
    );
    assert!(
        session.pages.management.management_state.terminal_mode,
        "held Output does not toggle again"
    );
}

#[test]
fn management_choice_enter_and_form_control_enter_preserve_input_and_required_checks() {
    let mut session = state();
    let (job, responses) = session.management_job();
    session.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    session.pages.management.management_state.operation_job = Some(job);
    session.pages.management.management_state.form = Some(ManagementEditor {
        instance: next_management_form_instance(),
        title: "Mock question".into(),
        message: String::new(),
        message_scroll: 0,
        selected: 0,
        purpose: FormPurpose::Answer("mock-question".into()),
        fields: vec![ManagementField {
            id: "answer".into(),
            value: "No".into(),
            choices: vec!["No".into(), "Yes".into()],
            required: true,
            ..Default::default()
        }],
    });
    session.handle_management_key(&KeyInput::new(InputKey::Enter));
    assert_eq!(
        session.pages.management.management_state.choice_field,
        Some(0)
    );
    session.handle_management_key(&KeyInput::new(InputKey::End));
    session.handle_management_key(&KeyInput::new(InputKey::Enter));
    session.handle_management_key(&KeyInput::new(InputKey::Enter).repeated());
    assert!(session.pages.management.management_state.form.is_some());
    assert!(responses.try_recv().is_err());
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "Yes"
    );
    session.handle_management_key(&KeyInput::with_modifiers(
        InputKey::Enter,
        ui::KeyModifiers::CTRL,
    ));
    assert!(
        matches!(responses.try_recv().unwrap(), OperationInput::Answer { id, value } if id == "mock-question" && value == "Yes")
    );
    assert!(session.pages.management.management_state.form.is_none());

    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.pages.management.management_state.snapshot.rows[0].actions[0]
        .fields
        .push(ManagementField {
            id: "required".into(),
            required: true,
            ..Default::default()
        });
    session.handle_management_key(&KeyInput::new(InputKey::Char('a')));
    session.handle_management_key(&KeyInput::with_modifiers(
        InputKey::Enter,
        ui::KeyModifiers::CTRL,
    ));
    assert!(session.pages.management.management_state.form.is_some());
    assert!(
        session
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    );
    assert_eq!(
        session.pages.management.management_state.status,
        i18n::tr!("management-required")
    );
}

#[test]
fn duplicate_action_mnemonics_use_first_target_and_menu_keeps_each_target() {
    let mut session = shortcut_action_state(ManagementKind::Disks, "scan");
    session
        .pages
        .management
        .management_state
        .snapshot
        .actions
        .push(ManagementAction {
            id: "scan".into(),
            confirm: true,
            ..Default::default()
        });
    let model = session.to_management_view_model();
    assert_eq!(
        model.actions[0].0,
        format!(
            "[A] {}",
            action_label(&session.pages.management.management_state.snapshot.rows[0].actions[0])
        )
    );
    assert!(model.actions[0].0.starts_with("[A]"));
    session.handle_management_key(&KeyInput::new(InputKey::Char('a')));
    assert!(matches!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .purpose,
        FormPurpose::Action(_, Some(_))
    ));
    session.handle_management_key(&KeyInput::new(InputKey::Escape));
    let more = session
        .management_actions()
        .iter()
        .position(|(action, _)| action.id == "more_actions")
        .unwrap();
    session.activate_management_action(more);
    session.handle_management_choice_key(&KeyInput::new(InputKey::End));
    session.handle_management_choice_key(&KeyInput::new(InputKey::Enter));
    assert!(matches!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .purpose,
        FormPurpose::Action(_, None)
    ));
}

#[test]
fn management_menu_paging_keys_keep_secondary_actions_reachable() {
    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.terminal_size = (60, 18);
    session.pages.management.management_state.snapshot.rows[0].actions = (0..8)
        .map(|index| ManagementAction {
            id: format!("mock-{index}"),
            label: format!("Mock {index}"),
            confirm: true,
            ..Default::default()
        })
        .collect();
    let layout = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    );
    assert_eq!(
        layout.actions.len(),
        1,
        "only the more-actions entry stays on the page"
    );
    click_management_point(&mut session, (layout.actions[0].x, layout.actions[0].y));
    let visible_rows = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    )
    .choice_rows
    .len()
    .max(1);
    session.handle_management_key(&KeyInput::new(InputKey::PageDown));
    let selected = visible_rows.min(7);
    assert_eq!(
        session.pages.management.management_state.choice_selected,
        selected
    );
    assert!(matches!(
        &session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .purpose,
        FormPurpose::Menu(_)
    ));
    let layout = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    );
    assert!(
        layout
            .choice_rows
            .iter()
            .any(|(index, _)| *index == selected)
    );
    session.handle_management_key(&KeyInput::new(InputKey::Home));
    assert_eq!(session.pages.management.management_state.choice_selected, 0);
    session.handle_management_key(&KeyInput::new(InputKey::End));
    session.handle_management_key(&KeyInput::new(InputKey::Enter));
    assert!(
        matches!(&session.pages.management.management_state.form.as_ref().unwrap().purpose, FormPurpose::Action(action,_) if action.id=="mock-7")
    );
}

#[test]
fn compact_management_routes_mouse_to_the_visible_page() {
    let mut state = state();
    state.terminal_size = (60, 18);
    state.refresh_hit_map();
    assert_eq!(
        state.hit_map.target_at((10, 8)),
        Some(ShellComponent::Management)
    );
}

#[test]
fn held_enter_does_not_confirm_an_action() {
    let mut state = state();
    state
        .pages
        .management
        .management_state
        .snapshot
        .actions
        .push(ManagementAction {
            id: "recover".into(),
            label: "Reconnect".into(),
            confirm: true,
            ..Default::default()
        });
    state.activate_management_action(0);
    state.handle_management_key(&KeyInput::new(InputKey::Enter).repeated());
    assert!(state.pages.management.management_state.form.is_some());
    assert!(
        state
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn switching_applications_keeps_background_questions_and_operation() {
    let mut state = state();
    let original = state.active_page_id();
    let (job, _) = state.management_job();
    state.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    state.pages.management.management_state.operation_job = Some(job.clone());
    state.open_management(ManagementKind::Packages);
    assert_eq!(
        state.pages.management.management_state.kind,
        Some(ManagementKind::Packages)
    );
    job.0
        .events
        .lock()
        .unwrap()
        .push_back(OperationEvent::Question {
            id: "confirm".into(),
            prompt: "Continue?".into(),
            choices: vec!["Continue".into(), "Cancel".into()],
            secret: false,
        });
    state.poll_management();
    assert!(state.pages.management.management_state.form.is_none());
    assert!(
        state
            .pages
            .management
            .get(original)
            .unwrap()
            .management_state
            .form
            .is_some()
    );
    state.close_page_instance(state.active_page_id());
    assert_eq!(state.active_page_id(), original);
    assert!(
        state
            .pages
            .management
            .management_state
            .operation_job
            .is_some()
    );
    assert_eq!(
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "Cancel"
    );
}

#[test]
fn terminal_ctrl_c_is_routed_to_task_and_configuration_cannot_be_dismissed() {
    let mut state = state();
    state.pages.management.management_state.terminal_mode = true;
    let key = KeyInput::from_label("Ctrl+C");
    assert!(matches!(
        state.route_key_input(&key).1,
        ShellCommand::ManagementKey(_)
    ));
    state.pages.management.management_state.form = Some(ManagementEditor {
        instance: next_management_form_instance(),
        message_scroll: 0,
        title: "Configuration".into(),
        message: String::new(),
        fields: Vec::new(),
        selected: 0,
        purpose: FormPurpose::Answer("package-config-1".into()),
    });
    state.pages.management.management_state.terminal_mode = false;
    state.handle_management_key(&KeyInput::new(InputKey::Escape));
    assert!(state.pages.management.management_state.form.is_some());
}

#[test]
fn changed_process_identity_invalidates_pressed_action() {
    let mut state = state();
    state
        .pages
        .management
        .management_state
        .snapshot
        .rows
        .push(ManagementRow {
            id: "42".into(),
            identity: BTreeMap::from([("start_time_ticks".into(), "100".into())]),
            actions: vec![ManagementAction {
                id: "kill".into(),
                label: "Kill".into(),
                primary: true,
                confirm: true,
                ..Default::default()
            }],
            ..Default::default()
        });
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    let point = (layout.actions[0].x, layout.actions[0].y);
    let before = state.management_button_at(point);
    state.pages.management.management_state.snapshot.rows[0]
        .identity
        .insert("start_time_ticks".into(), "200".into());
    assert_ne!(before, state.management_button_at(point));
}

fn state() -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::LocalAccount,
            session_id: "management-test".into(),
            user_id: "alice".into(),
            username: "alice".into(),
            role: UserRole::User,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    state.enter_screen(ShellScreen::Management);
    state.pages.management.management_state.kind = Some(ManagementKind::Services);
    state.pages.management.management_state.query =
        Some(ManagementQuery::new(ManagementKind::Services));
    state
}

#[test]
fn terminal_button_reopens_finished_auto_admin_instead_of_a_second_input_surface() {
    let mut state = state();
    let (responses, _inputs) = mpsc::channel();
    let job = state
        .begin_auto_admin("Operation".into(), false, responses)
        .unwrap();
    state.pages.management.management_state.auto_admin_job = Some(job.clone());
    job.finish(Ok("Done".into()));
    state.close_auto_admin();
    assert!(!state.auto_admin_visible());
    state.management_touch_control(ui::ManagementControl::Terminal);
    assert!(state.auto_admin_visible());
    assert!(!state.pages.management.management_state.terminal_mode);
    assert!(!job.running());
}

#[test]
fn text_fields_take_priority_over_actions_and_keep_secrets_out_of_view_model() {
    let mut state = state();
    state.pages.management.management_state.form = Some(ManagementEditor {
        instance: next_management_form_instance(),
        message_scroll: 0,
        title: "Authorization".into(),
        message: String::new(),
        fields: vec![ManagementField {
            id: "password".into(),
            secret: true,
            ..Default::default()
        }],
        selected: 0,
        purpose: FormPurpose::Answer("sudo-password".into()),
    });
    for key in [InputKey::Char('r'), InputKey::Space, InputKey::Char('1')] {
        state.handle_management_key(&KeyInput::new(key));
    }
    assert_eq!(
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "r 1"
    );
    assert_eq!(
        state.to_management_view_model().form.unwrap().fields[0].value,
        "•••"
    );
    assert!(state.pages.management.management_state.query_job.is_none());
}

#[test]
fn choice_fields_reject_free_text_and_paste() {
    let mut state = state();
    state.pages.management.management_state.form = Some(ManagementEditor {
        instance: next_management_form_instance(),
        message_scroll: 0,
        title: "Choice".into(),
        message: String::new(),
        fields: vec![ManagementField {
            id: "answer".into(),
            value: "No".into(),
            choices: vec!["No".into(), "Yes".into()],
            ..Default::default()
        }],
        selected: 0,
        purpose: FormPurpose::Answer("confirmation".into()),
    });
    state.handle_management_paste("anything");
    state.handle_management_key(&KeyInput::new(InputKey::Char('x')));
    assert_eq!(
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "No"
    );
    state.handle_management_key(&KeyInput::new(InputKey::Right));
    assert_eq!(
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "Yes"
    );
}

#[test]
fn result_snapshot_and_outcome_survive_completion() {
    let mut state = state();
    let (job, _) = state.management_job();
    job.0.events.lock().unwrap().extend([
        OperationEvent::Snapshot {
            snapshot: ManagementSnapshot {
                backend: "authorized-inspection".into(),
                ..Default::default()
            },
        },
        OperationEvent::Completed {
            message: "Inspection complete".into(),
        },
    ]);
    state.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    state.pages.management.management_state.operation_job = Some(job);
    state.poll_management();
    assert_eq!(
        state.pages.management.management_state.snapshot.backend,
        "authorized-inspection"
    );
    assert_eq!(
        state.pages.management.management_state.status,
        "Inspection complete"
    );
    assert!(state.pages.management.management_state.query_job.is_none());
}

#[test]
fn action_capture_requires_matching_release() {
    let mut state = state();
    state
        .pages
        .management
        .management_state
        .snapshot
        .actions
        .push(ManagementAction {
            id: "start".into(),
            label: "Start".into(),
            confirm: true,
            ..Default::default()
        });
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    let point = ui::Point::new(layout.actions[0].x, layout.actions[0].y);
    let down = InputEvent::Mouse(MouseInput {
        position: point,
        kind: ui::MouseEventKind::Down(PointerButton::Left),
        modifiers: ui::KeyModifiers::NONE,
    });
    let now = Instant::now();
    assert!(state.prepare_button_input(down, now).is_none());
    assert!(state.pages.management.management_state.form.is_none());
    let released = state.prepare_button_input(
        InputEvent::Mouse(MouseInput {
            position: point,
            kind: ui::MouseEventKind::Up(PointerButton::Left),
            modifiers: ui::KeyModifiers::NONE,
        }),
        now + Duration::from_millis(50),
    );
    let Some(InputEvent::Mouse(mouse)) = released else {
        panic!("expected captured release");
    };
    state.handle_management_pointer(mouse);
    assert!(state.pages.management.management_state.form.is_some());
}

#[test]
fn launcher_management_entries_are_linux_only_and_guest_is_blocked() {
    let mut state = state();
    let entries = state.built_in_launcher_applications();
    for kind in [
        ManagementKind::Services,
        ManagementKind::Processes,
        ManagementKind::Packages,
        ManagementKind::Network,
        ManagementKind::Disks,
    ] {
        assert_eq!(
            entries
                .iter()
                .any(|entry| entry.id == format!("builtin.{}", kind.id())),
            cfg!(target_os = "linux")
        );
    }
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::LocalAccount,
            session_id: "guest".into(),
            user_id: "guest".into(),
            username: "guest".into(),
            role: UserRole::Guest,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    state.set_navigation_path(vec![ShellScreen::Home]);
    state.open_management(ManagementKind::Packages);
    assert_eq!(state.active_screen(), ShellScreen::Home);
}

#[test]
fn management_scrollbar_drag_preserves_viewport_and_cancel_stops_capture() {
    let mut state = state();
    state.pages.management.management_state.snapshot.columns = vec!["Name".into()];
    state.pages.management.management_state.snapshot.rows = (0..100)
        .map(|index| ManagementRow {
            id: index.to_string(),
            cells: vec![format!("row-{index}")],
            ..Default::default()
        })
        .collect();
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    let bar = *layout
        .scrollbars
        .iter()
        .find(|bar| bar.target == ui::ManagementScrollTarget::Rows)
        .unwrap();
    let pointer = |position: (u16, u16), kind| MouseInput {
        position: ui::Point::new(position.0, position.1),
        kind,
        modifiers: ui::KeyModifiers::NONE,
    };
    state.handle_management_pointer(pointer(
        (bar.thumb.x, bar.thumb.y),
        ui::MouseEventKind::Down(PointerButton::Left),
    ));
    state.handle_management_pointer(pointer(
        (bar.track.x, bar.track.bottom()),
        ui::MouseEventKind::Drag(PointerButton::Left),
    ));
    assert_eq!(
        state.pages.management.management_state.scroll,
        100 - layout.list_capacity
    );
    state.poll_management();
    assert_eq!(
        state.pages.management.management_state.scroll,
        100 - layout.list_capacity
    );
    state.cancel_management_pointer_gesture();
    assert!(!state.management_pointer_drag_active());
}

#[test]
fn touch_choice_list_selects_without_cycling_or_submitting() {
    let mut state = state();
    state.pages.management.management_state.form = Some(ManagementEditor {
        instance: next_management_form_instance(),
        title: "Choice".into(),
        message: String::new(),
        message_scroll: 0,
        fields: vec![ManagementField {
            id: "answer".into(),
            label: "Action".into(),
            value: "No".into(),
            choices: vec!["No".into(), "Yes".into()],
            ..Default::default()
        }],
        selected: 0,
        purpose: FormPurpose::Answer("question".into()),
    });
    let pointer = |area: Rect| MouseInput {
        position: ui::Point::new(area.x, area.y),
        kind: ui::MouseEventKind::Click(PointerButton::Left),
        modifiers: ui::KeyModifiers::NONE,
    };
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    state.handle_management_pointer(pointer(layout.fields[0].1));
    assert_eq!(
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "No"
    );
    assert_eq!(
        state.pages.management.management_state.choice_field,
        Some(0)
    );
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    let choice = layout
        .choice_rows
        .iter()
        .find(|(index, _)| *index == 1)
        .unwrap()
        .1;
    state.handle_management_pointer(pointer(choice));
    assert_eq!(
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value,
        "Yes"
    );
    assert!(
        state
            .pages
            .management
            .management_state
            .choice_field
            .is_none()
    );
    assert!(state.pages.management.management_state.form.is_some());
}

#[test]
fn escape_asks_before_transferring_management_operation_to_the_session() {
    let mut state = state();
    state.set_navigation_path(vec![
        ShellScreen::Home,
        ShellScreen::Launcher,
        ShellScreen::Management,
    ]);
    let (job, _) = state.management_job();
    state.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    state.pages.management.management_state.operation_job = Some(job);
    let page = state.active_page_id();
    state.handle_management_key(&KeyInput::new(InputKey::Escape));
    assert_eq!(state.active_screen(), ShellScreen::Management);
    assert!(state.to_notification_view_model().is_some());
    state.continue_page_tasks_in_background(page);
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::Launcher
    );
    assert!(state.pages.management.get(page).is_none());
    assert_eq!(state.detached_management_operations.len(), 1);
}

#[test]
fn compact_management_uses_the_reserved_shell_content_area() {
    let mut state = state();
    state.terminal_size = (49, 11);
    assert_eq!(state.management_main(), Rect::new(0, 1, 49, 9));
    assert_eq!(state.keyboard_launcher_area().unwrap().y, 10);
    assert!(state.management_button_at((48, 0)).is_none());
}

#[test]
fn management_touch_buttons_are_hidden_by_global_overlays() {
    let mut state = state();
    let layout = ui::management_layout(state.management_main(), &state.to_management_view_model());
    let point = (layout.controls[0].1.x, layout.controls[0].1.y);
    assert!(state.management_button_at(point).is_some());
    state.time_sync_dialog_visible = true;
    assert!(state.management_button_at(point).is_none());
    state.time_sync_dialog_visible = false;
    state.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Management),
        anchor: point,
    });
    assert!(state.management_button_at(point).is_none());
}

#[test]
fn management_shortcuts_route_to_the_foreground_popup_and_escape_closes_it() {
    let mut session = shortcut_action_state(ManagementKind::Services, "start");
    session.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Management),
        anchor: (5, 5),
    });
    for key in [
        KeyInput::new(InputKey::Char('s')),
        KeyInput::new(InputKey::Char('a')),
        KeyInput::new(InputKey::F(5)),
        KeyInput::with_modifiers(InputKey::Enter, ui::KeyModifiers::CTRL),
    ] {
        assert_eq!(
            session.route_key_input(&key).0,
            RoutedTarget::Popup(ShellComponent::ContextMenu)
        );
        session.apply_input(InputEvent::Key(key));
        assert!(session.pages.management.management_state.form.is_none());
        assert!(!session.pages.management.management_state.filtering);
        assert!(
            session
                .pages
                .management
                .management_state
                .query_job
                .is_none()
        );
    }
    session.apply_input(InputEvent::from_key_label("Esc"));
    assert!(session.pages.common.active_popup.is_none());
    assert_eq!(session.active_screen(), ShellScreen::Management);
}

#[test]
fn global_notification_releases_management_scroll_capture() {
    let mut state = state();
    state.pages.management.management_state.scrollbar_grab =
        Some((ui::ManagementScrollTarget::Rows, 0));
    state.notify_modal(
        "Notice",
        "Stop dragging",
        ui::NotificationTone::Info,
        vec![],
    );
    assert!(
        state
            .pages
            .management
            .management_state
            .scrollbar_grab
            .is_none()
    );
}

#[test]
fn terminal_events_close_the_choice_list_and_release_form_drag() {
    for event in [
        OperationEvent::Completed {
            message: "Completed".into(),
        },
        OperationEvent::Failed {
            message: "Failed".into(),
        },
        OperationEvent::Disconnected {
            message: "Disconnected".into(),
        },
    ] {
        let mut state = state();
        state.pages.management.management_state.form = Some(ManagementEditor {
            instance: next_management_form_instance(),
            title: "Question".into(),
            message: String::new(),
            message_scroll: 0,
            fields: vec![ManagementField {
                id: "answer".into(),
                choices: vec!["No".into(), "Yes".into()],
                ..Default::default()
            }],
            selected: 0,
            purpose: FormPurpose::Answer("question".into()),
        });
        state.pages.management.management_state.choice_field = Some(0);
        state.pages.management.management_state.choice_scroll = 3;
        state.pages.management.management_state.choice_columns = 8;
        state.pages.management.management_state.form_field_scroll = Some(4);
        state.pages.management.management_state.scrollbar_grab =
            Some((ui::ManagementScrollTarget::Choices, 0));
        state.pages.management.management_state.received_snapshot = true;
        let (job, _) = state.management_job();
        job.0.events.lock().unwrap().push_back(event);
        state.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
        state.pages.management.management_state.operation_job = Some(job);
        state.poll_management();
        assert!(state.pages.management.management_state.form.is_none());
        assert!(
            state
                .pages
                .management
                .management_state
                .choice_field
                .is_none()
        );
        assert!(
            state
                .pages
                .management
                .management_state
                .form_field_scroll
                .is_none()
        );
        assert_eq!(state.pages.management.management_state.choice_scroll, 0);
        assert_eq!(state.pages.management.management_state.choice_columns, 0);
        assert!(!state.management_pointer_drag_active());
        assert!(!state.handle_management_choice_key(&KeyInput::new(InputKey::Right)));
    }
}

#[test]
fn compact_more_actions_makes_every_secondary_action_touch_reachable() {
    let mut state = state();
    state.terminal_size = (60, 18);
    state.pages.management.management_state.snapshot.actions = (0..6)
        .map(|index| ManagementAction {
            id: format!("action-{index}"),
            label: format!("Action {index}"),
            confirm: true,
            ..Default::default()
        })
        .collect();
    let tap = |area: Rect| MouseInput {
        position: ui::Point::new(area.x, area.y),
        kind: ui::MouseEventKind::Click(PointerButton::Left),
        modifiers: ui::KeyModifiers::NONE,
    };
    for index in 0..6 {
        let layout =
            ui::management_layout(state.management_main(), &state.to_management_view_model());
        assert_eq!(layout.actions.len(), 1);
        assert!(layout.action_next.is_empty());
        state.handle_management_pointer(tap(layout.actions[0]));
        let mut layout =
            ui::management_layout(state.management_main(), &state.to_management_view_model());
        if !layout.choice_rows.iter().any(|(i, _)| *i == index) {
            let bar = *layout
                .scrollbars
                .iter()
                .find(|bar| bar.target == ui::ManagementScrollTarget::Choices)
                .unwrap();
            state.handle_management_pointer(MouseInput {
                position: ui::Point::new(bar.thumb.x, bar.thumb.y),
                kind: ui::MouseEventKind::Down(PointerButton::Left),
                modifiers: ui::KeyModifiers::NONE,
            });
            let end = ui::Point::new(bar.track.x, bar.track.bottom());
            state.handle_management_pointer(MouseInput {
                position: end,
                kind: ui::MouseEventKind::Drag(PointerButton::Left),
                modifiers: ui::KeyModifiers::NONE,
            });
            state.handle_management_pointer(MouseInput {
                position: end,
                kind: ui::MouseEventKind::Up(PointerButton::Left),
                modifiers: ui::KeyModifiers::NONE,
            });
            layout =
                ui::management_layout(state.management_main(), &state.to_management_view_model());
        }
        let row = layout
            .choice_rows
            .iter()
            .find(|(i, _)| *i == index)
            .unwrap()
            .1;
        state.handle_management_pointer(tap(row));
        assert!(
            matches!(&state.pages.management.management_state.form.as_ref().unwrap().purpose,FormPurpose::Action(action,_) if action.id==format!("action-{index}"))
        );
        state.cancel_management_form();
    }
}

#[test]
fn more_menu_keyboard_and_touch_cannot_execute_a_disabled_item() {
    for by_touch in [false, true] {
        let mut state = state();
        state.pages.management.management_state.snapshot.actions = vec![ManagementAction {
            id: "remove".into(),
            label: "Remove".into(),
            confirm: true,
            disabled_reason: Some("Protected object".into()),
            ..Default::default()
        }];
        state.activate_management_action(0);
        assert!(matches!(
            &state
                .pages
                .management
                .management_state
                .form
                .as_ref()
                .unwrap()
                .purpose,
            FormPurpose::Menu(_)
        ));
        if by_touch {
            let layout =
                ui::management_layout(state.management_main(), &state.to_management_view_model());
            let region = layout.choice_rows[0].1;
            state.handle_management_pointer(MouseInput {
                position: ui::Point::new(region.x, region.y),
                kind: ui::MouseEventKind::Click(PointerButton::Left),
                modifiers: ui::KeyModifiers::NONE,
            });
        } else {
            state.handle_management_key(&KeyInput::new(InputKey::Space));
        }
        assert!(matches!(
            &state
                .pages
                .management
                .management_state
                .form
                .as_ref()
                .unwrap()
                .purpose,
            FormPurpose::Menu(_)
        ));
        assert!(
            state
                .pages
                .management
                .management_state
                .operation_job
                .is_none()
        );
        assert_eq!(
            state.pages.management.management_state.choice_field,
            Some(0)
        );
        state.handle_management_key(&KeyInput::new(InputKey::Escape));
        assert!(state.pages.management.management_state.form.is_none());
        assert_eq!(state.active_screen(), ShellScreen::Management);
    }
}

#[test]
fn main_actions_are_limited_to_two_and_tab_enter_reaches_the_more_menu() {
    let mut state = state();
    state.pages.management.management_state.snapshot.actions = (0..5)
        .map(|index| ManagementAction {
            id: format!("action-{index}"),
            label: format!("Action {index}"),
            primary: true,
            confirm: true,
            ..Default::default()
        })
        .collect();
    let actions = state.management_actions();
    assert_eq!(actions.len(), 3);
    assert_eq!(actions[2].0.id, "more_actions");
    state.handle_management_key(&KeyInput::new(InputKey::Tab));
    state.handle_management_key(&KeyInput::new(InputKey::End));
    assert_eq!(state.pages.management.management_state.selected_action, 2);
    state.handle_management_key(&KeyInput::new(InputKey::Enter));
    assert!(
        matches!(&state.pages.management.management_state.form.as_ref().unwrap().purpose,FormPurpose::Menu(items) if items.len()==5)
    );
    state.handle_management_key(&KeyInput::new(InputKey::End));
    state.handle_management_key(&KeyInput::new(InputKey::Space));
    assert!(
        matches!(&state.pages.management.management_state.form.as_ref().unwrap().purpose,FormPurpose::Action(action,_) if action.id=="action-4")
    );
}

#[test]
fn more_menu_shortcuts_open_the_named_action_and_held_keys_do_not_type_into_its_form() {
    let mut state = shortcut_action_state(ManagementKind::Disks, "scan");
    state.pages.management.management_state.snapshot.rows[0].actions[0].primary = false;
    let more = state
        .management_actions()
        .iter()
        .position(|(action, _)| action.id == "more_actions")
        .unwrap();
    state.activate_management_action(more);
    assert!(
        state
            .to_management_view_model()
            .form
            .unwrap()
            .choice
            .unwrap()
            .values[0]
            .ends_with("    A")
    );
    state.handle_management_key(&KeyInput::new(InputKey::Char('a')));
    assert!(
        matches!(&state.pages.management.management_state.form.as_ref().unwrap().purpose,FormPurpose::Action(action,_) if action.id=="scan")
    );
    state.handle_management_key(&KeyInput::new(InputKey::Char('a')).repeated());
    assert!(
        state
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .fields[0]
            .value
            .is_empty()
    );
    assert!(
        state
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    );
}

fn package_list_state() -> ShellSession {
    let mut session = state();
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    session.pages.management.management_state.kind = Some(ManagementKind::Packages);
    session.pages.management.management_state.query =
        Some(ManagementQuery::new(ManagementKind::Packages));
    session.pages.management.management_state.snapshot = ManagementSnapshot {
        backend: "apt/dpkg".into(),
        columns: vec![
            "Package".into(),
            "Installed".into(),
            "Available".into(),
            "Description".into(),
        ],
        rows: ["alpha:amd64", "bash:amd64", "omega:amd64"]
            .into_iter()
            .map(|id| ManagementRow {
                id: id.into(),
                cells: vec![id.into(), "1".into(), String::new(), "Summary".into()],
                actions: vec![ManagementAction {
                    id: "remove".into(),
                    label: "Remove".into(),
                    privileged: true,
                    confirm: true,
                    primary: true,
                    ..Default::default()
                }],
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    session.pages.management.management_state.selected = 1;
    session
}

#[test]
fn all_package_subviews_keep_navigation_after_empty_or_failed_queries() {
    for scope in [
        "installed",
        "search",
        "updates",
        "sources",
        "conflicts",
        "status",
    ] {
        let mut session = package_list_state();
        session.pages.management.management_state.snapshot = ManagementSnapshot::default();
        session
            .pages
            .management
            .management_state
            .query
            .as_mut()
            .unwrap()
            .scope = scope.into();
        let actions = session.all_management_actions();
        for destination in [
            "search",
            "installed",
            "updates",
            "sources",
            "conflicts",
            "status",
        ] {
            assert_eq!(
                actions
                    .iter()
                    .filter(|(action, _)| action.id == format!("scope_{destination}"))
                    .count(),
                1
            );
        }
        assert!(
            session
                .to_management_view_model()
                .title
                .contains(&management_label(&format!("scope-{scope}"), scope))
        );
        session.handle_management_key(&KeyInput::new(InputKey::F(7)));
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .query
                .as_ref()
                .unwrap()
                .scope,
            "installed"
        );
        assert!(
            session
                .pages
                .management
                .management_state
                .operation_job
                .is_none()
        );
    }
}

#[test]
fn switching_package_subviews_clears_old_file_target_filter_and_focus() {
    let mut session = package_list_state();
    let query = session
        .pages
        .management
        .management_state
        .query
        .as_mut()
        .unwrap();
    query.scope = "conflicts".into();
    query.target = Some("/etc/demo.conf.dpkg-dist".into());
    query.filter = "demo.conf".into();
    query.options.insert("scope-only".into(), "old".into());
    session.pages.management.management_state.filter_input = "demo.conf".into();
    session.pages.management.management_state.filtering = true;
    session.pages.management.management_state.details_only = true;
    session.pages.management.management_state.terminal_mode = true;
    session.pages.management.management_state.selected_action = 3;
    session.pages.management.management_state.actions_focused = true;
    session.perform_management_action(
        ManagementAction {
            id: "scope_installed".into(),
            ..Default::default()
        },
        None,
    );
    let query = session
        .pages
        .management
        .management_state
        .query
        .as_ref()
        .unwrap();
    assert_eq!(query.scope, "installed");
    assert!(query.target.is_none() && query.filter.is_empty() && query.options.is_empty());
    assert!(
        session
            .pages
            .management
            .management_state
            .filter_input
            .is_empty()
    );
    assert!(
        !session.pages.management.management_state.filtering
            && !session.pages.management.management_state.details_only
            && !session.pages.management.management_state.terminal_mode
    );
    assert_eq!(session.pages.management.management_state.selected_action, 0);
    assert!(!session.pages.management.management_state.actions_focused);
    assert!(
        session
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    );
}

#[test]
fn package_details_keep_all_rows_and_escape_returns_to_the_list() {
    for open_key in [InputKey::Enter, InputKey::F(4)] {
        let mut session = package_list_state();
        let before = session.pages.management.management_state.snapshot.clone();
        session.handle_management_key(&KeyInput::new(open_key));
        assert!(session.pages.management.management_state.details_only);
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .query
                .as_ref()
                .unwrap()
                .target
                .as_deref(),
            Some("bash:amd64")
        );
        let mut snapshot = before.clone();
        let mut detailed = snapshot.rows[1].clone();
        detailed.detail = vec![("Description".into(), "A useful shell".into())];
        merge_package_details(
            &mut snapshot,
            "bash:amd64",
            Ok(ManagementSnapshot {
                rows: vec![detailed.clone()],
                ..Default::default()
            }),
        )
        .unwrap();
        assert_eq!(snapshot.rows.len(), before.rows.len());
        assert_eq!(snapshot.rows[0], before.rows[0]);
        assert_eq!(snapshot.rows[1], detailed);
        assert_eq!(snapshot.rows[2], before.rows[2]);
        let (job, _) = session.management_job();
        *job.0.snapshot.lock().unwrap() = Some(Ok(snapshot));
        session.register_management_job(&job, TaskDomain::ManagementQuery, TaskClass::Query);
        session.pages.management.management_state.query_job = Some(job);
        session.poll_management();
        assert_eq!(session.pages.management.management_state.selected, 1);
        assert!(
            session
                .to_management_view_model()
                .details
                .contains("A useful shell")
        );
        session.handle_management_key(&KeyInput::new(InputKey::Escape));
        assert!(!session.pages.management.management_state.details_only);
        assert!(
            session
                .pages
                .management
                .management_state
                .query
                .as_ref()
                .unwrap()
                .target
                .is_none()
        );
        assert_eq!(session.active_screen(), ShellScreen::Management);
        assert_eq!(
            session
                .pages
                .management
                .management_state
                .snapshot
                .rows
                .len(),
            3
        );
        assert!(!session.pages.management.management_state.actions_focused);
    }
}

#[test]
fn package_detail_scrolling_keeps_the_selection_and_list_views_keep_search_text() {
    let mut session = package_list_state();
    let search_result = session.pages.management.management_state.snapshot.clone();
    session.pages.management.management_state.filter_input = "shell".into();
    session
        .pages
        .management
        .management_state
        .query
        .as_mut()
        .unwrap()
        .filter = "shell".into();
    session.perform_management_action(
        ManagementAction {
            id: "scope_search".into(),
            ..Default::default()
        },
        None,
    );
    assert_eq!(
        session.pages.management.management_state.filter_input,
        "shell"
    );
    assert!(session.pages.management.management_state.filtering);
    assert!(
        session
            .pages
            .management
            .management_state
            .snapshot
            .rows
            .is_empty()
    );
    session.pages.management.management_state.snapshot = search_result;
    session.handle_management_key(&KeyInput::new(InputKey::Escape));
    assert!(!session.pages.management.management_state.filtering);
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .filter,
        "shell"
    );
    session.pages.management.management_state.selected = 1;
    session.handle_management_key(&KeyInput::new(InputKey::F(4)));
    session.handle_management_key(&KeyInput::new(InputKey::Down));
    assert_eq!(session.pages.management.management_state.selected, 1);
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .target
            .as_deref(),
        Some("bash:amd64")
    );
    session.handle_management_key(&KeyInput::new(InputKey::Escape));
    assert!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .target
            .is_none()
    );
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .filter,
        "shell"
    );
}

#[test]
fn unavailable_package_detail_keeps_local_rows_and_exposes_the_error() {
    let mut session = package_list_state();
    merge_package_details(
        &mut session.pages.management.management_state.snapshot,
        "bash:amd64",
        Err(ManagementError::Unavailable(
            "Repository cache is missing".into(),
        )),
    )
    .unwrap();
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .snapshot
            .rows
            .len(),
        3
    );
    assert_eq!(
        session.pages.management.management_state.snapshot.rows[1].actions[0].id,
        "remove"
    );
    assert!(
        session
            .to_management_view_model()
            .details
            .contains("Repository cache is missing")
    );
    assert_eq!(
        merge_package_details(
            &mut session.pages.management.management_state.snapshot,
            "bash:amd64",
            Err(ManagementError::Cancelled)
        ),
        Err(ManagementError::Cancelled)
    );
}

#[test]
fn package_navigation_remains_enabled_during_a_background_operation() {
    let mut session = package_list_state();
    let (job, _) = session.management_job();
    session.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    session.pages.management.management_state.operation_job = Some(job.clone());
    session.handle_management_key(&KeyInput::new(InputKey::F(3)));
    let model = session.to_management_view_model();
    assert!(
        session
            .pages
            .management
            .management_state
            .package_advanced_open
    );
    assert!(session.pages.management.management_state.form.is_none());
    let items = session.management_actions();
    let installed = items
        .iter()
        .position(|(action, _)| action.id == "scope_installed")
        .unwrap();
    assert!(model.actions[installed].1);
    let remove = items
        .iter()
        .position(|(action, _)| action.id == "remove")
        .unwrap();
    assert!(!model.actions[remove].1);
    session.handle_management_key(&KeyInput::new(InputKey::F(7)));
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .query
            .as_ref()
            .unwrap()
            .scope,
        "installed"
    );
    assert_eq!(
        session.pages.management.management_state.operation_job,
        Some(job)
    );
}

#[test]
fn configuration_snapshots_and_completion_preserve_the_package_page() {
    for action in ["history", "read"] {
        let mut session = package_list_state();
        let before = session.pages.management.management_state.snapshot.clone();
        session
            .pages
            .management
            .management_state
            .configuration_operation = true;
        session.pages.management.management_state.received_snapshot = true;
        session.pages.editor.editor_config.pending_action = Some(action.into());
        let (job, _) = session.management_job();
        job.0.events.lock().unwrap().extend([
            OperationEvent::Snapshot {
                snapshot: ManagementSnapshot {
                    backend: "config-history".into(),
                    rows: vec![ManagementRow {
                        id: "backup-1".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            },
            OperationEvent::Completed {
                message: "Configuration read".into(),
            },
        ]);
        session.register_management_job(
            &job,
            TaskDomain::ManagementOperation,
            TaskClass::Operation,
        );
        session.pages.management.management_state.operation_job = Some(job);
        session.poll_management();
        assert_eq!(session.pages.management.management_state.snapshot, before);
        assert!(session.pages.management.management_state.received_snapshot);
        assert!(session.pages.management.management_state.outcome.is_none());
        assert!(
            !session
                .pages
                .management
                .management_state
                .status
                .contains("Configuration read")
        );
        if action == "history" {
            assert_eq!(
                session.pages.editor.editor_config.history.rows[0].id,
                "backup-1"
            );
            session.cancel_management_form();
            assert!(session.pages.management.management_state.form.is_none());
        }
        assert_eq!(session.active_screen(), ShellScreen::Management);
    }
}

#[test]
fn all_management_modules_sort_headers_and_keep_action_target_after_refresh() {
    for kind in [
        ManagementKind::Services,
        ManagementKind::Processes,
        ManagementKind::Packages,
        ManagementKind::Network,
        ManagementKind::Disks,
        ManagementKind::Users,
        ManagementKind::SystemConfig,
        ManagementKind::Firewall,
        ManagementKind::Ssh,
        ManagementKind::Backups,
        ManagementKind::ScheduledTasks,
    ] {
        let mut session = shortcut_action_state(kind, "inspect");
        session.terminal_size = (120, 32);
        let mut row = session.pages.management.management_state.snapshot.rows[0].clone();
        row.cells = vec!["Zulu".into(), "2".into()];
        let mut other = row.clone();
        other.id = "other".into();
        other.cells = vec!["alpha".into(), "10".into()];
        other.identity.insert("identity".into(), "other".into());
        session.pages.management.management_state.snapshot.columns =
            vec!["Name".into(), "Count".into()];
        session.pages.management.management_state.snapshot.rows = vec![row, other];
        let original = session.pages.management.management_state.snapshot.clone();
        let model = session.to_management_view_model();
        let layout = ui::management_layout(session.management_main(), &model);
        if let Some(workspace) = &model.workspace {
            assert!(!layout.actions_panel.is_empty());
            for index in &workspace.primary_actions {
                let area = layout.actions[*index];
                assert_eq!(area.intersection(layout.detail_actions_panel), area);
            }
        } else {
            assert!(layout.actions_panel.is_empty());
            for area in layout.actions.iter().filter(|area| !area.is_empty()) {
                assert_eq!(area.intersection(layout.detail_actions_panel), *area);
            }
        }
        let header = layout.headers[0].1;
        click_management_point(&mut session, (header.x, header.y));
        assert_eq!(
            session.pages.management.management_state.snapshot.rows[0].id,
            "other"
        );
        assert_eq!(
            session.pages.management.management_state.snapshot.rows
                [session.pages.management.management_state.selected]
                .id,
            "mock-selected-item"
        );
        click_management_point(&mut session, (header.x, header.y));
        assert_eq!(
            session.pages.management.management_state.snapshot.rows[0].id,
            "mock-selected-item"
        );
        session.sort_management(1);
        assert_eq!(
            session.pages.management.management_state.snapshot.rows[0].cells[1],
            "2"
        );
        session.sort_management(1);
        let (job, _) = session.management_job();
        job.0
            .events
            .lock()
            .unwrap()
            .push_back(OperationEvent::Snapshot { snapshot: original });
        session.register_management_job(
            &job,
            TaskDomain::ManagementOperation,
            TaskClass::Operation,
        );
        session.pages.management.management_state.operation_job = Some(job);
        session.poll_management();
        assert_eq!(
            session.pages.management.management_state.snapshot.rows[0].cells[1],
            "10"
        );
        assert_eq!(
            session.pages.management.management_state.snapshot.rows
                [session.pages.management.management_state.selected]
                .identity["identity"],
            "mock"
        );
    }
}

#[test]
fn complete_details_toggle_preserves_authorized_rows_and_query() {
    for kind in [
        ManagementKind::Firewall,
        ManagementKind::Ssh,
        ManagementKind::Backups,
        ManagementKind::ScheduledTasks,
    ] {
        let mut session = shortcut_action_state(kind, "inspect");
        session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
        let state = &mut session.pages.management.management_state;
        state.snapshot.details_complete = true;
        let mut other = state.snapshot.rows[0].clone();
        other.id = "other-backup".into();
        state.snapshot.rows.push(other);
        state.snapshot.refresh_action = Some(ManagementAction {
            id: "inspect_fixture".into(),
            privileged: true,
            ..Default::default()
        });
        state.status = "authorized snapshot retained".into();
        let snapshot = state.snapshot.clone();
        let query = state.query.clone();
        session.toggle_management_details();
        assert!(session.pages.management.management_state.details_only);
        session.handle_management_key(&KeyInput::new(InputKey::Escape));
        let state = &session.pages.management.management_state;
        assert!(!state.details_only);
        assert_eq!(state.snapshot, snapshot);
        assert_eq!(state.snapshot.rows.len(), 2);
        assert_eq!(state.query, query);
        assert_eq!(state.status, "authorized snapshot retained");
        assert!(state.query_job.is_none() && state.operation_job.is_none());
    }
}

#[test]
fn authorized_refresh_uses_current_query_and_waits_for_active_operation() {
    let mut session = shortcut_action_state(ManagementKind::Backups, "inspect");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    let state = &mut session.pages.management.management_state;
    state.snapshot.refresh_action = Some(ManagementAction {
        id: "inspect_fixture".into(),
        privileged: true,
        values: BTreeMap::from([
            ("scope".into(), "old".into()),
            ("filter".into(), "old".into()),
            ("query_target".into(), "old".into()),
            ("__query_option:removed".into(), "old".into()),
            ("native".into(), "kept".into()),
        ]),
        ..Default::default()
    });
    let query = state.query.as_mut().unwrap();
    query.scope = "new".into();
    query.filter = "current filter".into();
    query.target = None;
    query.options.insert("username".into(), "demo".into());
    let request = state.refresh_command().unwrap();
    assert_eq!(request.action, "inspect_fixture");
    assert_eq!(request.values["scope"], "new");
    assert_eq!(request.values["filter"], "current filter");
    assert_eq!(request.values["__query_option:username"], "demo");
    assert_eq!(request.values["native"], "kept");
    assert!(!request.values.contains_key("query_target"));
    assert!(!request.values.contains_key("__query_option:removed"));
    let (job, _) = session.management_job();
    session.pages.management.management_state.operation_job = Some(job.clone());
    session.refresh_management();
    assert!(session.pages.management.management_state.refresh_requested);
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .operation_job
            .as_ref(),
        Some(&job)
    );
    assert!(
        session
            .pages
            .management
            .management_state
            .query_job
            .is_none()
    );
}

#[test]
fn failed_authorized_refresh_does_not_repeat_authorization() {
    let mut session = shortcut_action_state(ManagementKind::Backups, "inspect");
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    let (job, _) = session.management_job();
    session.register_management_job(&job, TaskDomain::ManagementOperation, TaskClass::Operation);
    session
        .pages
        .management
        .management_state
        .snapshot
        .refresh_action = Some(ManagementAction {
        id: "inspect_fixture".into(),
        privileged: true,
        ..Default::default()
    });
    session
        .pages
        .management
        .management_state
        .refreshing_authorized = true;
    session.pages.management.management_state.received_snapshot = false;
    job.0
        .events
        .lock()
        .unwrap()
        .push_back(OperationEvent::Failed {
            message: "inspection denied".into(),
        });
    session.pages.management.management_state.operation_job = Some(job);
    session.poll_management();
    let state = &session.pages.management.management_state;
    assert_eq!(state.status, "inspection denied");
    assert!(!state.refresh_requested && !state.refreshing_authorized);
    assert!(state.operation_job.is_none() && state.query_job.is_none());
}

#[test]
fn scheduled_task_name_and_account_source_keep_their_context_in_chinese() {
    let language = i18n::LanguageSnapshotData {
        code: "zh-CN".into(),
        generation: 1,
        current: vec![
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../ascii-assets/assets/locales/zh-CN/modules/regression-startup.ftl"
            ))
            .into(),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../ascii-assets/assets/locales/zh-CN/modules/management-cli-administration.ftl"
            ))
            .into(),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../ascii-assets/assets/locales/zh-CN/modules/management-users.ftl"
            ))
            .into(),
        ],
        english: vec![],
    }
    .restore()
    .unwrap();
    i18n::with_snapshot(&Arc::new(language), || {
        let mut session = shortcut_action_state(ManagementKind::ScheduledTasks, "create_task");
        session.pages.management.management_state.form = Some(ManagementEditor {
            instance: next_management_form_instance(),
            title: "create".into(),
            message: String::new(),
            message_scroll: 0,
            selected: 0,
            fields: vec![ManagementField {
                id: "name".into(),
                label: "任务名称".into(),
                ..Default::default()
            }],
            purpose: FormPurpose::Action(
                ManagementAction {
                    id: "create_task".into(),
                    ..Default::default()
                },
                None,
            ),
        });
        assert_eq!(
            session.to_management_view_model().form.unwrap().fields[0].label,
            "任务名称"
        );
        session.pages.management.management_state.kind = Some(ManagementKind::Users);
        session.pages.management.management_state.snapshot.columns = vec!["Source".into()];
        session.pages.management.management_state.snapshot.rows[0].detail =
            vec![("Source".into(), "local".into())];
        let model = session.to_management_view_model();
        assert_eq!(model.columns, vec!["账户来源"]);
        assert!(model.details.contains("账户来源:"));
        assert!(!model.details.contains("软件源"));
    });
}
