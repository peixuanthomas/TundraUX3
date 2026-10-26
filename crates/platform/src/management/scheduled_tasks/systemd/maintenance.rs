use super::*;

fn field(id: &str, label: &str, value: String) -> ManagementField {
    ManagementField {
        id: id.into(),
        label: i18n::tr!(label),
        value,
        required: true,
        ..Default::default()
    }
}

pub(super) fn manifest(text: &str) -> Option<BTreeMap<String, String>> {
    text.lines()
        .find_map(|line| line.strip_prefix("# TundraUX3-Task="))
        .and_then(|text| serde_json::from_str(text).ok())
}

pub(super) fn owns_file(path: &Path, flag: &AtomicBool) -> bool {
    use super::super::super::packages::{PackageBackend, detect_backend};
    let (program, arguments): (&str, &[&str]) = match detect_backend(flag) {
        Ok(PackageBackend::Apt) => ("/usr/bin/dpkg-query", &["--search"]),
        Ok(PackageBackend::Dnf4 | PackageBackend::Dnf5) => ("/usr/bin/rpm", &["-qf", "--"]),
        Ok(PackageBackend::Pacman) => ("/usr/bin/pacman", &["-Qo", "--"]),
        Err(_) => return true,
    };
    if !Path::new(program).is_file() {
        return true;
    }
    let args = arguments
        .iter()
        .map(|value| value.to_string())
        .chain([path.display().to_string()])
        .collect::<Vec<_>>();
    return match native(program, &args, None, flag) {
        Ok(output) if output.status.success() => true,
        Ok(output) => {
            let error = String::from_utf8_lossy(&output.stderr).to_lowercase();
            !(error.contains("no path found")
                || error.contains("not owned")
                || error.contains("no package owns"))
        }
        Err(_) => true,
    };
}

pub(super) fn editable_path(
    scope: &str,
    name: &str,
    props: &BTreeMap<String, String>,
    flag: &AtomicBool,
) -> Option<PathBuf> {
    let path = PathBuf::from(value(props, "FragmentPath"));
    if !path.is_absolute() || !path.is_file() {
        return None;
    }
    if scope_id(scope) == "user" {
        let uid = scope
            .strip_prefix("user:")
            .and_then(|uid| uid.parse().ok())
            .unwrap_or_else(|| unsafe { libc::getuid() });
        let account = actor(uid).ok()?;
        return path
            .starts_with(account.home.join(".config/systemd/user"))
            .then_some(path);
    }
    if path != Path::new("/etc/systemd/system").join(name) {
        return None;
    }
    if name.starts_with("tundra-scheduled-") || !owns_file(&path, flag) {
        Some(path)
    } else {
        None
    }
}

pub(super) fn append_actions(
    actions: &mut Vec<ManagementAction>,
    scope: &str,
    timer: &str,
    service: &str,
    props: &BTreeMap<String, String>,
    service_props: &BTreeMap<String, String>,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    let text = ctl(scope, &["cat", "--", service], flag)?;
    let own_values = manifest(&text);
    let mut complex = false;
    let mut fields = if let Some(mut values) = own_values.clone() {
        values.insert("unit_name".into(), timer.trim_end_matches(".timer").into());
        [
            ("name", "scheduled-task-field-name"),
            ("command", "scheduled-task-field-command"),
            ("working_directory", "scheduled-task-field-directory"),
            ("schedule", "scheduled-task-field-schedule"),
            ("user", "scheduled-task-field-user"),
        ]
        .into_iter()
        .map(|(id, label)| field(id, label, values.get(id).cloned().unwrap_or_default()))
        .collect()
    } else {
        let mut calendar = Vec::new();
        for line in ctl(scope, &["cat", "--", timer], flag)?.lines() {
            if let Some(expression) = line.trim().strip_prefix("OnCalendar=") {
                if expression.is_empty() {
                    calendar.clear();
                } else {
                    calendar.push(expression.to_owned());
                }
            }
        }
        complex = calendar.len() != 1;
        vec![field(
            "schedule",
            "scheduled-task-field-schedule",
            calendar.last().cloned().unwrap_or_default(),
        )]
    };
    fields.push(ManagementField {
        id: "persistent".into(),
        label: i18n::tr!("schedule-catch-up"),
        value: matches!(value(props, "Persistent"), "yes" | "true" | "1").to_string(),
        control: FieldControl::Checkbox,
        ..Default::default()
    });
    if fields.last().is_some_and(|field| field.value.is_empty()) {
        fields.last_mut().unwrap().value = "false".into();
    }
    complex |= own_values.is_none()
        && (!value(props, "TimersMonotonic").is_empty()
            || value(props, "OnClockChange") == "yes"
            || value(props, "OnTimezoneChange") == "yes");
    actions.push(ManagementAction {
        id: "edit_task".into(),
        label: i18n::tr!("schedule-edit"),
        fields,
        values: own_values.unwrap_or_default(),
        privileged: true,
        confirm: true,
        group: "configuration".into(),
        disabled_reason: complex.then(|| i18n::tr!("schedule-use-advanced")),
        ..Default::default()
    });
    let directory = if scope_id(scope) == "system" {
        PathBuf::from("/etc/systemd/system")
    } else {
        let uid = scope
            .strip_prefix("user:")
            .and_then(|uid| uid.parse().ok())
            .unwrap_or_else(|| unsafe { libc::getuid() });
        actor(uid)?.home.join(".config/systemd/user")
    };
    let override_path = directory.join(format!("{timer}.d/tundra-schedule.conf"));
    let mut values = BTreeMap::from([
        ("path".into(), override_path.display().to_string()),
        ("validator".into(), "systemd".into()),
        ("service".into(), timer.into()),
        ("scope".into(), scope_id(scope).into()),
    ]);
    if !override_path.exists() {
        values.insert("content".into(), "[Timer]\n".into());
    }
    actions.push(ManagementAction {
        id: "edit_system_config".into(),
        label: i18n::tr!("schedule-advanced-timer"),
        values,
        group: "configuration".into(),
        ..Default::default()
    });
    let override_path = directory.join(format!("{service}.d/tundra-scheduled.conf"));
    let mut values = BTreeMap::from([
        ("path".into(), override_path.display().to_string()),
        ("validator".into(), "systemd".into()),
        ("service".into(), service.into()),
        ("scope".into(), scope_id(scope).into()),
    ]);
    if !override_path.exists() {
        values.insert("content".into(), "[Service]\n".into());
    }
    actions.push(ManagementAction {
        id: "edit_system_config".into(),
        label: i18n::tr!("schedule-advanced-command"),
        values,
        group: "configuration".into(),
        ..Default::default()
    });
    let owned = editable_path(scope, timer, props, flag);
    actions.push(ManagementAction {
        id: "delete_task".into(),
        label: i18n::tr!("schedule-delete"),
        privileged: true,
        confirm: true,
        group: "configuration".into(),
        disabled_reason: if owned.is_none() {
            Some(i18n::tr!("schedule-preinstalled"))
        } else if matches!(
            value(service_props, "ActiveState"),
            "active" | "activating" | "deactivating"
        ) {
            Some(i18n::tr!("schedule-running-delete"))
        } else {
            None
        },
        ..Default::default()
    });
    Ok(())
}

pub(super) fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let route = validate_action(command, context, flag)?;
    let timer = &command.identity["timer"];
    let service = &command.identity["service"];
    let props = properties(&route, timer, flag)?;
    let service_props = properties(&route, service, flag)?;
    let scope = command.identity["scope"].as_str();
    let mut candidates = Vec::new();
    let mut removals = Vec::new();
    let mut prepare_identity = None;
    if command.action == "delete_task" {
        if matches!(
            value(&service_props, "ActiveState"),
            "active" | "activating" | "deactivating"
        ) {
            return Err(invalid(i18n::tr!("schedule-running-delete")));
        }
        let path = editable_path(&route, timer, &props, flag)
            .ok_or_else(|| invalid(i18n::tr!("schedule-preinstalled")))?;
        removals.push(super::super::super::system_config::read_stored(&path)?);
        if timer.starts_with("tundra-scheduled-") {
            let service_path = Path::new("/etc/systemd/system").join(service);
            let timer_text = ctl(&route, &["cat", "--", timer], flag)?;
            let service_text = ctl(&route, &["cat", "--", service], flag)?;
            if scope == "system"
                && manifest(&service_text).is_some()
                && timer_text
                    .lines()
                    .any(|line| line == format!("Unit={service}"))
                && value(&service_props, "FragmentPath") == service_path.display().to_string()
            {
                let users = ctl(
                    &route,
                    &["show", "--property=TriggeredBy", "--value", "--", service],
                    flag,
                )?;
                if users.split_whitespace().all(|unit| unit == timer) {
                    removals.push(super::super::super::system_config::read_stored(
                        &service_path,
                    )?);
                }
            }
        }
    } else {
        let original = ctl(&route, &["cat", "--", service], flag)?;
        if let Some(mut values) = manifest(&original) {
            values.extend(command.values.clone());
            values.insert("unit_name".into(), timer.trim_end_matches(".timer").into());
            let definition = definition(&values, context.actor_uid)?;
            validate(&definition, flag)?;
            prepare_identity = Some((definition.uid, definition.gid));
            for (path, content) in [
                (
                    Path::new("/etc/systemd/system").join(service),
                    definition.service,
                ),
                (
                    Path::new("/etc/systemd/system").join(timer),
                    definition.timer,
                ),
            ] {
                let current = super::super::super::system_config::read(&path)?;
                candidates.push(ConfigCandidate {
                    path,
                    expected_version: current.version,
                    content,
                });
            }
        } else {
            let schedule = planning::calendar(&command.values)?;
            let calendar = run(
                "/usr/bin/systemd-analyze",
                &[
                    "calendar".into(),
                    "--iterations=5".into(),
                    "--".into(),
                    schedule.clone(),
                ],
                flag,
            )?;
            io.emit(OperationEvent::Output { text: calendar });
            let directory = if scope == "system" {
                PathBuf::from("/etc/systemd/system")
            } else {
                actor(context.actor_uid)?.home.join(".config/systemd/user")
            };
            let path = directory.join(format!("{timer}.d/tundra-schedule.conf"));
            let current = super::super::super::system_config::read(&path)?;
            let persistent = planning::persistent(&command.values)?;
            let content = format!(
                "[Timer]\nOnCalendar=\nOnCalendar={}\nPersistent={persistent}\n",
                schedule.replace('%', "%%")
            );
            let checked =
                super::super::super::system_config::check(&path, &content, "systemd", flag)?;
            if checked != ConfigCheck::Passed {
                return Err(invalid(match checked {
                    ConfigCheck::Failed(message) | ConfigCheck::Unavailable(message) => message,
                    _ => i18n::tr!("schedule-check-unavailable"),
                }));
            }
            candidates.push(ConfigCandidate {
                path,
                expected_version: current.version,
                content,
            });
        }
    }
    let paths = candidates
        .iter()
        .map(|candidate| candidate.path.clone())
        .chain(removals.iter().map(|stored| stored.document.path.clone()))
        .collect::<Vec<_>>();
    super::super::super::configuration::confirm_preview(
        "scheduled_task_change",
        &i18n::tr!("schedule-change-review"),
        &paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>(),
        io,
    )?;
    validate_action(command, context, flag)?;
    let backup = super::super::super::config_backups::begin("scheduled-tasks", &paths)?;
    let mut payload = serde_json::json!({"type":"timer-state","scope":scope,"timer":timer,"before":state(&props),"after":state(&props)});
    backup.set_payload(payload.clone())?;
    let result = (|| {
        if let Some((uid, gid)) = prepare_identity {
            history::prepare(uid, gid)?;
        }
        if command.action == "delete_task" {
            ctl(&route, &["disable", "--now", "--", timer], flag)?;
            for mut stored in removals {
                let expected = stored.document.version.clone();
                stored.document.existed = false;
                stored.document.content.clear();
                super::super::super::system_config::restore_snapshot(&stored, &expected)?;
            }
        } else {
            super::super::super::system_config::install_batch_reviewed(&candidates, flag)?;
        }
        ctl(&route, &["daemon-reload"], flag)?;
        if command.action != "delete_task" && value(&props, "ActiveState") == "active" {
            ctl(&route, &["restart", "--", timer], flag)?;
        }
        Ok(i18n::tr!("schedule-change-saved"))
    })();
    if let Ok(after) = properties(&route, timer, &AtomicBool::new(false)) {
        payload["after"] = state(&after);
        if value(&after, "LoadState") != "not-found" {
            if let Ok(version) = version(&route, timer, service, &AtomicBool::new(false)) {
                payload["version"] = version.into();
            }
        }
        backup.set_payload(payload)?;
    }
    backup.finish(if result.is_ok() {
        "completed"
    } else {
        "failed"
    })?;
    result.map(|message| {
        i18n::tr!(
            "schedule-change-with-backup",
            message = message,
            backup = backup.id()
        )
    })
}
