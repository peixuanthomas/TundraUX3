use super::*;
use std::collections::{BTreeMap, BTreeSet};

const PROPERTIES: &str = "Id,LoadState,ActiveState,UnitFileState,Unit,TimersCalendar,TimersMonotonic,NextElapseUSecRealtime,NextElapseUSecMonotonic,LastTriggerUSec,FragmentPath,DropInPaths,User,Result,ExecMainCode,ExecMainStatus,ExecMainExitTimestamp,Description";

fn ctl(scope: &str, arguments: &[&str], flag: &AtomicBool) -> Result<String, ManagementError> {
    check_cancelled(flag)?;
    if scope_id(scope) == "system" {
        super::super::capability::system_manager()?;
    }
    let args = [
        format!("--{}", scope_id(scope)),
        "--no-pager".into(),
        "--no-ask-password".into(),
    ]
    .into_iter()
    .chain(arguments.iter().map(|value| value.to_string()))
    .collect::<Vec<_>>();
    if let Some(uid) = scope.strip_prefix("user:") {
        let account = actor(
            uid.parse()
                .map_err(|_| invalid(i18n::tr!("scheduled-task-user-owner")))?,
        )?;
        let mut user_args = vec![
            "-u".into(),
            account.username.clone(),
            "--".into(),
            "/usr/bin/env".into(),
            "-i".into(),
            format!("HOME={}", account.home.display()),
            format!("USER={}", account.username),
            format!("LOGNAME={}", account.username),
            "PATH=/usr/bin:/bin".into(),
            format!("XDG_RUNTIME_DIR=/run/user/{}", account.uid),
            format!(
                "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{}/bus",
                account.uid
            ),
            "/usr/bin/systemctl".into(),
        ];
        user_args.extend(args);
        run("/usr/sbin/runuser", &user_args, flag)
    } else {
        run("/usr/bin/systemctl", &args, flag)
    }
}

fn scope_id(scope: &str) -> &str {
    if scope.starts_with("user:") {
        "user"
    } else {
        scope
    }
}

fn source_label(scope: &str) -> String {
    let scope = if scope_id(scope) == "user" {
        i18n::tr!("scheduled-task-scope-user")
    } else {
        i18n::tr!("scheduled-task-scope-system")
    };
    i18n::tr!("scheduled-task-source-systemd", scope = scope)
}

fn routed_scope(scope: &str, context: &ExecutionContext) -> String {
    if scope == "user" && unsafe { libc::geteuid() } == 0 {
        format!("user:{}", context.actor_uid)
    } else {
        scope.into()
    }
}

fn properties(
    scope: &str,
    unit: &str,
    flag: &AtomicBool,
) -> Result<BTreeMap<String, String>, ManagementError> {
    let text = ctl(
        scope,
        &["show", &format!("--property={PROPERTIES}"), "--", unit],
        flag,
    )?;
    Ok(text
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.into(), value.into()))
        .collect())
}

fn value<'a>(props: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    props.get(key).map(String::as_str).unwrap_or("")
}

fn valid_timer(unit: &str) -> bool {
    unit.ends_with(".timer")
        && unit.len() > 6
        && unit.len() <= 255
        && !unit.starts_with('-')
        && unit
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b":_.@-\\".contains(&byte))
        && !unit.ends_with("@.timer")
}

fn version(
    scope: &str,
    timer: &str,
    service: &str,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let timer_text = ctl(scope, &["cat", "--", timer], flag)?;
    let service_text = ctl(scope, &["cat", "--", service], flag)?;
    Ok(digest(
        format!("{}\0{timer_text}\0{service_text}", scope_id(scope)).as_bytes(),
    ))
}

pub(super) fn rows(scope: &str, flag: &AtomicBool) -> Result<Vec<ManagementRow>, ManagementError> {
    let installed = ctl(
        scope,
        &["list-unit-files", "--type=timer", "--no-legend", "--plain"],
        flag,
    )?;
    let loaded = ctl(
        scope,
        &[
            "list-units",
            "--all",
            "--type=timer",
            "--no-legend",
            "--plain",
        ],
        flag,
    )?;
    let names = installed
        .lines()
        .chain(loaded.lines())
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| valid_timer(name))
        .collect::<BTreeSet<_>>();
    let mut rows = Vec::new();
    for name in names {
        check_cancelled(flag)?;
        match row(scope, name, flag) {
            Ok(row) => rows.push(row),
            Err(error) => rows.push(ManagementRow {
                id: format!("systemd:{}:{name}", scope_id(scope)),
                cells: vec![
                    name.into(),
                    source_label(scope),
                    i18n::tr!("scheduled-task-unknown").into(),
                    i18n::tr!("scheduled-task-unknown").into(),
                    error.to_string(),
                    i18n::tr!("scheduled-task-unknown").into(),
                    i18n::tr!("scheduled-task-unknown").into(),
                ],
                detail: vec![(
                    i18n::tr!("scheduled-task-unavailable").into(),
                    error.to_string(),
                )],
                ..Default::default()
            }),
        }
    }
    Ok(rows)
}

fn row(scope: &str, name: &str, flag: &AtomicBool) -> Result<ManagementRow, ManagementError> {
    let props = properties(scope, name, flag)?;
    if value(&props, "LoadState") == "not-found" {
        return Err(ManagementError::Conflict(
            i18n::tr!("scheduled-task-timer-missing").into(),
        ));
    }
    let service = value(&props, "Unit");
    if !services::valid_unit_name(service) {
        return Err(invalid(i18n::tr!("scheduled-task-unsupported-service")));
    }
    let service_props = properties(scope, service, flag)?;
    let user = if scope_id(scope) == "user" {
        actor(
            scope
                .strip_prefix("user:")
                .and_then(|uid| uid.parse().ok())
                .unwrap_or_else(|| unsafe { libc::getuid() }),
        )?
        .username
    } else if value(&service_props, "User").is_empty() {
        "root".into()
    } else {
        value(&service_props, "User").into()
    };
    let active = value(&props, "ActiveState");
    let enabled = value(&props, "UnitFileState");
    let state = format!("{active}; {enabled}");
    let calendar = value(&props, "TimersCalendar");
    let schedule = if calendar.is_empty() {
        value(&props, "TimersMonotonic")
    } else {
        calendar
    };
    let next = value(&props, "NextElapseUSecRealtime");
    let next = if active != "active" {
        i18n::tr!("scheduled-task-inactive").into()
    } else if !next.is_empty() && next != "n/a" {
        next.into()
    } else {
        i18n::tr!(
            "scheduled-task-monotonic",
            time = value(&props, "NextElapseUSecMonotonic")
        )
    };
    let last = if name.starts_with("tundra-scheduled-") {
        local_identity(&user)
            .map(|(uid, _)| result_detail(name.trim_end_matches(".timer"), uid))
            .unwrap_or_else(|| i18n::tr!("scheduled-task-no-result").into())
    } else if value(&service_props, "ExecMainExitTimestamp").is_empty() {
        i18n::tr!("scheduled-task-no-result").into()
    } else {
        i18n::tr!(
            "scheduled-task-native-result",
            time = value(&service_props, "ExecMainExitTimestamp"),
            result = value(&service_props, "Result"),
            status = value(&service_props, "ExecMainStatus")
        )
    };
    let config_version = version(scope, name, service, flag);
    let identity = BTreeMap::from([
        ("source".into(), "systemd".into()),
        ("scope".into(), scope_id(scope).into()),
        ("timer".into(), name.into()),
        ("service".into(), service.into()),
        (
            "version".into(),
            config_version.as_ref().cloned().unwrap_or_default(),
        ),
    ]);
    let masked =
        matches!(enabled, "masked" | "masked-runtime") || value(&props, "LoadState") == "masked";
    let unsupported = config_version
        .err()
        .map(|error| i18n::tr!("scheduled-task-cannot-verify", error = error.to_string()));
    let actions = [
        (
            "pause",
            i18n::tr!("scheduled-task-pause"),
            active != "active",
        ),
        (
            "resume",
            i18n::tr!("scheduled-task-resume"),
            active == "active" || masked,
        ),
        ("run_now", i18n::tr!("scheduled-task-run-now"), masked),
    ]
    .into_iter()
    .map(|(id, label, disabled)| ManagementAction {
        id: id.into(),
        label: label.into(),
        confirm: true,
        privileged: true,
        disabled_reason: unsupported.clone().or_else(|| {
            disabled.then(|| i18n::tr!("scheduled-task-timer-operation-disabled").into())
        }),
        primary: true,
        ..Default::default()
    })
    .collect();
    Ok(ManagementRow {
        id: format!("systemd:{}:{name}", scope_id(scope)),
        cells: vec![
            name.into(),
            source_label(scope),
            user,
            schedule.into(),
            state,
            last.clone(),
            next,
        ],
        detail: vec![
            (
                i18n::tr!("scheduled-task-detail-description"),
                value(&props, "Description").into(),
            ),
            (i18n::tr!("scheduled-task-detail-timer"), name.into()),
            (i18n::tr!("scheduled-task-detail-service"), service.into()),
            (
                i18n::tr!("scheduled-task-detail-definition"),
                value(&props, "FragmentPath").into(),
            ),
            (
                i18n::tr!("scheduled-task-detail-last-trigger"),
                value(&props, "LastTriggerUSec").into(),
            ),
            (i18n::tr!("scheduled-task-column-last").into(), last),
            (
                i18n::tr!("scheduled-task-detail-service-state"),
                value(&service_props, "ActiveState").into(),
            ),
            (
                i18n::tr!("scheduled-task-pause-behavior").into(),
                i18n::tr!("scheduled-task-pause-behavior-detail").into(),
            ),
        ],
        identity,
        actions,
    })
}

pub(super) fn create_actions(uid: u32) -> Result<Vec<ManagementAction>, ManagementError> {
    let current = actor(uid)?;
    let choices = users::accounts(None)?
        .into_iter()
        .filter(|account| account.local)
        .map(|account| account.username)
        .collect::<Vec<_>>();
    let fields = vec![
        ManagementField {
            id: "name".into(),
            label: i18n::tr!("scheduled-task-field-name").into(),
            required: true,
            ..Default::default()
        },
        ManagementField {
            id: "command".into(),
            label: i18n::tr!("scheduled-task-field-command").into(),
            required: true,
            ..Default::default()
        },
        ManagementField {
            id: "working_directory".into(),
            label: i18n::tr!("scheduled-task-field-directory").into(),
            value: current.home.display().to_string(),
            required: true,
            ..Default::default()
        },
        ManagementField {
            id: "schedule".into(),
            label: i18n::tr!("scheduled-task-field-schedule").into(),
            value: "daily".into(),
            required: true,
            ..Default::default()
        },
        ManagementField {
            id: "user".into(),
            label: i18n::tr!("scheduled-task-field-user").into(),
            value: current.username,
            required: true,
            choices,
            ..Default::default()
        },
    ];
    Ok([
        ("preview_task", i18n::tr!("scheduled-task-preview"), false),
        ("create_task", i18n::tr!("scheduled-task-create"), true),
    ]
    .into_iter()
    .map(|(id, label, privileged)| ManagementAction {
        id: id.into(),
        label: label.into(),
        fields: fields.clone(),
        privileged,
        confirm: privileged,
        primary: true,
        ..Default::default()
    })
    .collect())
}

pub(super) fn quote(text: &str) -> String {
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

struct Definition {
    name: String,
    service: String,
    timer: String,
    schedule: String,
    uid: u32,
    gid: u32,
}

fn definition(
    values: &BTreeMap<String, String>,
    current_uid: u32,
) -> Result<Definition, ManagementError> {
    let name = values.get("name").map(String::as_str).unwrap_or("").trim();
    if name.is_empty()
        || name.len() > 120
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
    {
        return Err(invalid(i18n::tr!("scheduled-task-name-invalid")));
    }
    let command = values.get("command").map(String::as_str).unwrap_or("");
    if command.trim().is_empty() || command.len() > 65536 || command.contains('\0') {
        return Err(invalid(i18n::tr!("scheduled-task-command-invalid")));
    }
    let directory = values
        .get("working_directory")
        .map(String::as_str)
        .unwrap_or("");
    if !Path::new(directory).is_absolute()
        || !Path::new(directory).is_dir()
        || directory.chars().any(char::is_control)
    {
        return Err(invalid(i18n::tr!("scheduled-task-directory-invalid")));
    }
    let schedule = values
        .get("schedule")
        .map(String::as_str)
        .unwrap_or("")
        .trim();
    if schedule.is_empty() || schedule.len() > 2048 || schedule.chars().any(char::is_control) {
        return Err(invalid(i18n::tr!("scheduled-task-schedule-invalid")));
    }
    let username = values
        .get("user")
        .filter(|value| !value.is_empty())
        .cloned()
        .unwrap_or(actor(current_uid)?.username);
    let user = users::accounts(Some(&username))?
        .into_iter()
        .find(|account| account.local && account.username == username)
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-user-invalid")))?;
    let name = format!("tundra-scheduled-{name}");
    // The recorder must still start when the chosen working directory disappears.
    let script = format!(
        "cd -- {} && exec /bin/sh -c {}",
        super::cron::shell_quote(directory),
        super::cron::shell_quote(command)
    );
    let service = format!(
        "[Unit]\nDescription=Tundra scheduled task {name}\n\n[Service]\nType=oneshot\nUser={}\nGroup={}\nWorkingDirectory=/\nEnvironment={} {} {} {}\nExecStart=:/bin/sh -c {}\nExecStopPost={}\nTimeoutStartSec=infinity\n",
        user.uid,
        user.gid,
        quote(&format!("HOME={}", user.home.display())),
        quote(&format!("USER={}", user.username)),
        quote(&format!("LOGNAME={}", user.username)),
        quote("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"),
        quote(&script),
        result_command(&name, user.uid)
    );
    let timer = format!(
        "[Unit]\nDescription=Tundra schedule {name}\n\n[Timer]\nOnCalendar={}\nUnit={name}.service\nPersistent=false\n\n[Install]\nWantedBy=timers.target\n",
        schedule.replace('%', "%%")
    );
    Ok(Definition {
        name,
        service,
        timer,
        schedule: schedule.into(),
        uid: user.uid,
        gid: user.gid,
    })
}

fn validate(definition: &Definition, flag: &AtomicBool) -> Result<String, ManagementError> {
    let calendar = run(
        "/usr/bin/systemd-analyze",
        &[
            "calendar".into(),
            "--iterations=5".into(),
            "--".into(),
            definition.schedule.clone(),
        ],
        flag,
    )?;
    let normalized = calendar
        .lines()
        .find_map(|line| line.trim().strip_prefix("Normalized form:"))
        .unwrap_or("")
        .trim();
    if normalized
        .split_whitespace()
        .last()
        .is_none_or(|time| time.matches(':').count() != 2)
    {
        return Err(invalid(i18n::tr!("scheduled-task-system-time-zone")));
    }
    let directory = tempfile::tempdir().map_err(io_error)?;
    let service = directory
        .path()
        .join(format!("{}.service", definition.name));
    let timer = directory.path().join(format!("{}.timer", definition.name));
    fs::write(&service, &definition.service).map_err(io_error)?;
    fs::write(&timer, &definition.timer).map_err(io_error)?;
    run(
        "/usr/bin/systemd-analyze",
        &[
            "verify".into(),
            "--".into(),
            service.display().to_string(),
            timer.display().to_string(),
        ],
        flag,
    )?;
    Ok(calendar)
}

pub(super) fn create(
    command: &ManagementCommand,
    context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let definition = definition(&command.values, context.actor_uid)?;
    let calendar = validate(&definition, flag)?;
    interaction.emit(OperationEvent::Output {
        text: i18n::tr!(
            "scheduled-task-definition-preview",
            name = definition.name.clone(),
            service = definition.service.clone(),
            timer = definition.timer.clone(),
            calendar = calendar
        ),
    });
    if command.action == "preview_task" {
        return Ok(i18n::tr!("scheduled-task-preview-validated").into());
    }
    require_root()?;
    if command.values.get("yes").map(String::as_str) != Some("true") {
        if command.values.get("non_interactive").map(String::as_str) == Some("true") {
            return Err(invalid(i18n::tr!("scheduled-task-confirmation-required")));
        }
        let save = i18n::tr!("scheduled-task-save-enable");
        let choices = [save.clone(), i18n::tr!("scheduled-task-cancel")];
        let answer = interaction.ask(
            "scheduled_task_save",
            &i18n::tr!("scheduled-task-confirm-save"),
            &choices,
            false,
        )?;
        if !answer.eq_ignore_ascii_case(&save) {
            return Err(ManagementError::Cancelled);
        }
    }
    check_cancelled(flag)?;
    let timer_name = format!("{}.timer", definition.name);
    let service_name = format!("{}.service", definition.name);
    // Check systemd's complete unit search path, including vendor and runtime units.
    for name in [&timer_name, &service_name] {
        let props = properties("system", name, flag)?;
        if value(&props, "LoadState") != "not-found" {
            return Err(ManagementError::Conflict(i18n::tr!(
                "scheduled-task-name-exists",
                name = name.as_str()
            )));
        }
    }
    let service = Path::new("/etc/systemd/system").join(&service_name);
    let timer = Path::new("/etc/systemd/system").join(&timer_name);
    let backup =
        super::super::config_backups::begin("scheduled-tasks", &[service.clone(), timer.clone()])?;
    let mut payload = serde_json::json!({"type":"created-timer", "timer":timer_name,"scope":"system","before":{"active":"inactive","enabled":"disabled"},"after":{"active":"inactive","enabled":"disabled"}});
    backup.set_payload(payload.clone())?;
    let result = (|| {
        prepare_results(definition.uid, definition.gid)?;
        write_new(&service, &definition.service)?;
        write_new(&timer, &definition.timer)?;
        ctl("system", &["daemon-reload"], flag)?;
        ctl("system", &["enable", "--now", "--", &timer_name], flag)?;
        let props = properties("system", &timer_name, flag)?;
        if value(&props, "ActiveState") != "active" || value(&props, "UnitFileState") != "enabled" {
            return Err(ManagementError::Failed(
                i18n::tr!("scheduled-task-activation-unconfirmed").into(),
            ));
        }
        Ok(i18n::tr!(
            "scheduled-task-created",
            timer = timer_name.as_str(),
            backup = backup.id()
        ))
    })();
    if let Ok(props) = properties("system", &timer_name, &AtomicBool::new(false)) {
        payload["after"] = state(&props);
        if let Ok(version) = version(
            "system",
            &timer_name,
            &service_name,
            &AtomicBool::new(false),
        ) {
            payload["version"] = serde_json::Value::String(version);
        }
        backup.set_payload(payload)?;
    }
    backup.finish(if result.is_ok() {
        "completed"
    } else {
        "failed"
    })?;
    result
}

fn write_new(path: &Path, text: &str) -> Result<(), ManagementError> {
    super::super::system_config::install_reviewed(path, "missing", text).map(|_| ())
}

fn state(props: &BTreeMap<String, String>) -> serde_json::Value {
    serde_json::json!({"active":value(props,"ActiveState"), "enabled":value(props,"UnitFileState")})
}

pub(super) fn validate_action(
    command: &ManagementCommand,
    context: &ExecutionContext,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let scope = command
        .identity
        .get("scope")
        .map(String::as_str)
        .unwrap_or("");
    if !matches!(scope, "system" | "user") {
        return Err(invalid(i18n::tr!("scheduled-task-timer-scope")));
    }
    require_root()?;
    let timer = command
        .identity
        .get("timer")
        .filter(|timer| valid_timer(timer))
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-select-timer")))?;
    if command.target.as_deref() != Some(&format!("systemd:{scope}:{timer}")) {
        return Err(invalid(i18n::tr!("scheduled-task-timer-identity")));
    }
    let route = routed_scope(scope, context);
    let fresh = row(&route, timer, flag)?;
    if fresh.identity != command.identity {
        return Err(ManagementError::Conflict(
            i18n::tr!("scheduled-task-definitions-changed").into(),
        ));
    }
    if let Some(reason) = fresh
        .actions
        .iter()
        .find(|action| action.id == command.action)
        .and_then(|action| action.disabled_reason.as_ref())
    {
        return Err(invalid(reason.clone()));
    }
    Ok(route)
}

pub(super) fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    _interaction: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let route = validate_action(command, context, flag)?;
    let scope = command.identity["scope"].as_str();
    let timer = &command.identity["timer"];
    let service = command
        .identity
        .get("service")
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-service-identity")))?;
    if command.action == "run_now" {
        ctl(&route, &["start", "--no-block", "--", service], flag)?;
        return Ok(i18n::tr!(
            "scheduled-task-service-started",
            service = service.as_str()
        ));
    }
    let before = properties(&route, timer, flag)?;
    let backup = super::super::config_backups::begin("scheduled-tasks", &[])?;
    let mut payload = serde_json::json!({"type":"timer-state","scope":scope,"timer":timer,"version":command.identity.get("version"),"before":state(&before),"after":state(&before)});
    backup.set_payload(payload.clone())?;
    let result = change_state(
        &route,
        timer,
        &command.action,
        value(&before, "UnitFileState"),
        flag,
    );
    let after = properties(&route, timer, &AtomicBool::new(false));
    if let Ok(after) = &after {
        payload["after"] = state(after);
        backup.set_payload(payload)?;
    }
    backup.finish(if result.is_ok() && after.is_ok() {
        "completed"
    } else {
        "failed"
    })?;
    result?;
    after?;
    Ok(i18n::tr!(
        "scheduled-task-state-completed",
        action = state_action_label(&command.action),
        timer = timer.as_str(),
        backup = backup.id()
    ))
}

fn change_state(
    scope: &str,
    timer: &str,
    action: &str,
    enabled: &str,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    if action == "pause" {
        ctl(scope, &["stop", "--", timer], flag)?;
        if matches!(enabled, "enabled" | "enabled-runtime") {
            ctl(scope, &["disable", "--", timer], flag)?;
        }
    } else {
        if matches!(enabled, "disabled" | "enabled" | "enabled-runtime") {
            ctl(scope, &["enable", "--", timer], flag)?;
        }
        ctl(scope, &["start", "--", timer], flag)?;
    }
    Ok(())
}

pub(super) fn restore(
    payload: &serde_json::Value,
    context: &ExecutionContext,
    _interaction: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let scope = payload
        .get("scope")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let timer = payload
        .get("timer")
        .and_then(serde_json::Value::as_str)
        .filter(|name| valid_timer(name))
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-recovery-target")))?;
    if !matches!(scope, "system" | "user") {
        return Err(invalid(i18n::tr!("scheduled-task-recovery-scope")));
    }
    let route = routed_scope(scope, context);
    let scope = route.as_str();
    ctl(scope, &["daemon-reload"], flag)?;
    if payload.get("type").and_then(serde_json::Value::as_str) == Some("created-timer") {
        return Ok(i18n::tr!(
            "scheduled-task-removed-timer-restored",
            timer = timer
        ));
    }
    let props = properties(scope, timer, flag)?;
    let before = payload
        .get("before")
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-previous-state")))?;
    let enabled = before
        .get("enabled")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if enabled == "enabled-runtime" {
        ctl(scope, &["enable", "--runtime", "--", timer], flag)?;
    } else if enabled == "enabled" {
        ctl(scope, &["enable", "--", timer], flag)?;
    } else if matches!(
        value(&props, "UnitFileState"),
        "enabled" | "enabled-runtime"
    ) {
        ctl(scope, &["disable", "--", timer], flag)?;
    }
    ctl(
        scope,
        &[
            if before.get("active").and_then(serde_json::Value::as_str) == Some("active") {
                "start"
            } else {
                "stop"
            },
            "--",
            timer,
        ],
        flag,
    )?;
    let after = properties(scope, timer, flag)?;
    if value(&after, "ActiveState")
        != before
            .get("active")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
    {
        return Err(ManagementError::Failed(
            i18n::tr!("scheduled-task-recovery-unconfirmed").into(),
        ));
    }
    Ok(i18n::tr!("scheduled-task-timer-restored", timer = timer))
}

pub(super) fn before_restore(
    payload: &serde_json::Value,
    context: &ExecutionContext,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    if payload.get("type").and_then(serde_json::Value::as_str) != Some("created-timer") {
        return Ok(());
    }
    let timer = payload
        .get("timer")
        .and_then(serde_json::Value::as_str)
        .filter(|name| valid_timer(name))
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-recovery-target")))?;
    let scope = routed_scope("system", context);
    let props = properties(&scope, timer, flag)?;
    if value(&props, "LoadState") != "not-found" {
        ctl(&scope, &["disable", "--now", "--", timer], flag)?;
    }
    Ok(())
}

pub(super) fn recovery_after(
    payload: &serde_json::Value,
    context: &ExecutionContext,
    flag: &AtomicBool,
) -> Result<Option<(serde_json::Value, Option<String>)>, ManagementError> {
    if !matches!(
        payload.get("type").and_then(serde_json::Value::as_str),
        Some("timer-state" | "created-timer")
    ) {
        return Ok(None);
    }
    let scope = payload
        .get("scope")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-recovery-scope-missing")))?;
    let timer = payload
        .get("timer")
        .and_then(serde_json::Value::as_str)
        .filter(|name| valid_timer(name))
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-recovery-target")))?;
    let scope = routed_scope(scope, context);
    let props = properties(&scope, timer, flag)?;
    let version = if value(&props, "LoadState") == "not-found" {
        None
    } else {
        Some(version(&scope, timer, value(&props, "Unit"), flag)?)
    };
    Ok(Some((state(&props), version)))
}

pub(super) fn restore_preflight(
    payload: &serde_json::Value,
    context: &ExecutionContext,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    let scope = payload
        .get("scope")
        .and_then(serde_json::Value::as_str)
        .filter(|scope| matches!(*scope, "system" | "user"))
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-recovery-scope")))?;
    let timer = payload
        .get("timer")
        .and_then(serde_json::Value::as_str)
        .filter(|name| valid_timer(name))
        .ok_or_else(|| invalid(i18n::tr!("scheduled-task-recovery-target")))?;
    let scope = routed_scope(scope, context);
    let props = properties(&scope, timer, flag)?;
    if Some(&state(&props)) != payload.get("after") {
        return Err(ManagementError::Conflict(
            i18n::tr!("scheduled-task-state-conflict").into(),
        ));
    }
    if let Some(expected) = payload.get("version").and_then(serde_json::Value::as_str) {
        let actual = version(&scope, timer, value(&props, "Unit"), flag)?;
        if expected != actual {
            return Err(ManagementError::Conflict(
                i18n::tr!("scheduled-task-definitions-conflict").into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/management/scheduled_tasks_systemd_tests.rs"]
mod tests;
