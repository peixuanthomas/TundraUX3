use super::*;

fn session() -> ShellSession {
    let mut session = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (180, 60),
        ShellHomeMode::User,
    );
    session.settings_task_runtime = ShellSettingsTaskRuntime::unavailable();
    session.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::LocalAccount,
            session_id: "schedule-form".into(),
            user_id: "alice".into(),
            username: "alice".into(),
            role: UserRole::User,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    session.enter_screen(ShellScreen::Management);
    session.pages.management.management_state.kind = Some(ManagementKind::ScheduledTasks);
    session.pages.management.management_state.query =
        Some(ManagementQuery::new(ManagementKind::ScheduledTasks));
    while session.notification_dismiss_active_modal_without_response() {}
    session
}

fn click(session: &mut ShellSession, area: Rect) {
    assert!(!area.is_empty());
    session.refresh_hit_map();
    let point = (area.x, area.y);
    session.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    session.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
}
fn next(session: &mut ShellSession) {
    let layout = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    );
    click(session, layout.submit);
}

#[test]
fn script_creation_reaches_time_settings_with_unmodified_pointer_clicks() {
    let directory = std::env::temp_dir().join(format!(
        "tundra-schedule-form-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let script = directory.join("每日任务.sh");
    std::fs::write(&script, "#!/bin/sh\ntrue\n").unwrap();
    let mut session = session();
    let action = ManagementAction {
        id: "create_task".into(),
        fields: vec![
            text("name", "scheduled-task-field-name", "每日任务".into(), true),
            text(
                "working_directory",
                "scheduled-task-field-directory",
                directory.display().to_string(),
                true,
            ),
            choice(
                "user",
                "scheduled-task-field-user",
                "alice".into(),
                vec!["alice".into()],
            ),
        ],
        ..Default::default()
    };
    session.activate_management_item(action, None);
    assert!(
        matches!(&session.pages.management.management_state.form.as_ref().unwrap().purpose,FormPurpose::Scheduled(wizard) if wizard.step==ScheduleStep::Operation)
    );
    next(&mut session);
    let layout = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    );
    click(&mut session, layout.fields[0].1);
    let index = session
        .pages
        .management
        .management_state
        .form
        .as_ref()
        .unwrap()
        .fields[0]
        .choices
        .iter()
        .position(|path| path == script.to_str().unwrap())
        .unwrap();
    let layout = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    );
    let area = layout
        .choice_rows
        .iter()
        .find(|(candidate, _)| *candidate == index)
        .unwrap()
        .1;
    click(&mut session, area);
    next(&mut session);
    next(&mut session);
    next(&mut session);
    let model = session.to_management_view_model();
    let form = model.form.unwrap();
    assert!(form.fields.iter().any(|field| field.id == "hour"));
    assert!(form.fields.iter().any(|field| field.id == "minute"));
    assert!(
        form.fields
            .iter()
            .any(|field| field.id == "persistent" && field.value == "false")
    );
    assert!(form.cancel_label.is_some());
    assert!(
        matches!(&session.pages.management.management_state.form.as_ref().unwrap().purpose,FormPurpose::Scheduled(wizard) if wizard.get("script_path")==script.to_str().unwrap())
    );
    let layout = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    );
    click(&mut session, layout.cancel);
    assert!(
        matches!(&session.pages.management.management_state.form.as_ref().unwrap().purpose,FormPurpose::Scheduled(wizard) if wizard.step==ScheduleStep::Kind)
    );
    std::fs::remove_file(script).unwrap();
    std::fs::remove_dir(directory).unwrap();
}

#[test]
fn changed_fields_and_replaced_forms_reject_old_calendar_previews() {
    let mut session = session();
    session.begin_scheduled_wizard(
        ManagementAction {
            id: "create_task".into(),
            fields: vec![
                text("name", "scheduled-task-field-name", "test".into(), true),
                text(
                    "working_directory",
                    "scheduled-task-field-directory",
                    "/".into(),
                    true,
                ),
                text("user", "scheduled-task-field-user", "alice".into(), true),
            ],
            ..Default::default()
        },
        None,
    );
    let FormPurpose::Scheduled(mut wizard) = session
        .pages
        .management
        .management_state
        .form
        .as_ref()
        .unwrap()
        .purpose
        .clone()
    else {
        panic!("wizard");
    };
    wizard.step = ScheduleStep::Time;
    wizard.preview_values = Some(wizard.values.clone());
    let (job, _) = session.management_job();
    wizard.preview_id = Some(job.0.id);
    *job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        details: vec![(String::new(), "obsolete preview".into())],
        ..Default::default()
    }));
    session.show_scheduled_step(wizard, String::new());
    session.pages.management.management_state.scheduled_preview = Some(job.clone());
    session
        .pages
        .management
        .management_state
        .form
        .as_mut()
        .unwrap()
        .fields
        .iter_mut()
        .find(|field| field.id == "hour")
        .unwrap()
        .value = "10".into();
    session.poll_scheduled_preview();
    assert!(job.0.cancelled.load(Ordering::Relaxed));
    assert!(
        !session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .message
            .contains("obsolete")
    );
    let (job, _) = session.management_job();
    *job.0.snapshot.lock().unwrap() = Some(Ok(ManagementSnapshot {
        details: vec![(String::new(), "another obsolete preview".into())],
        ..Default::default()
    }));
    session.pages.management.management_state.scheduled_preview = Some(job);
    let form = session
        .pages
        .management
        .management_state
        .form
        .as_mut()
        .unwrap();
    form.purpose = FormPurpose::Answer("current".into());
    form.message = "current prompt".into();
    session.poll_scheduled_preview();
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap()
            .message,
        "current prompt"
    );
}

#[test]
fn existing_native_tasks_use_common_time_fields_and_keep_complex_rules() {
    for (source, schedule) in [("crontab", "30 9 * * 1-5"), ("systemd", "Mon..Fri 09:30")] {
        let mut session = session();
        let row = ManagementRow {
            cells: vec!["existing task".into(), String::new(), "alice".into()],
            identity: BTreeMap::from([("source".into(), source.into())]),
            ..Default::default()
        };
        let action = ManagementAction {
            id: "edit_task".into(),
            fields: vec![
                text(
                    "schedule",
                    "scheduled-task-field-schedule",
                    schedule.into(),
                    true,
                ),
                text(
                    "command",
                    "scheduled-task-field-command",
                    "echo keep".into(),
                    true,
                ),
                boolean("persistent", "schedule-catch-up", "true".into()),
            ],
            ..Default::default()
        };
        session.activate_management_item(action, Some(row));
        let form = session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap();
        let FormPurpose::Scheduled(wizard) = &form.purpose else {
            panic!("time wizard");
        };
        assert_eq!(wizard.step, ScheduleStep::Kind);
        assert_eq!(wizard.get("schedule_kind"), "weekly");
        assert_eq!(wizard.get("hour"), "09");
        assert_eq!(wizard.get("minute"), "30");
        if source == "crontab" {
            assert!(!form.fields[0].choices.iter().any(|value| value == "once"));
        }
        next(&mut session);
        let form = session
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .unwrap();
        assert!(
            form.fields
                .iter()
                .any(|field| field.id == "day_0" && field.value == "true")
        );
        assert_eq!(
            form.fields.iter().any(|field| field.id == "persistent"),
            source == "systemd"
        );
    }
    let mut values = BTreeMap::from([("schedule".into(), "*-01,04,07,10-01 03:15:27".into())]);
    seed_existing_schedule(&mut values, false);
    assert_eq!(values["schedule_kind"], "advanced");
    assert_eq!(values["schedule"], "*-01,04,07,10-01 03:15:27");
}
