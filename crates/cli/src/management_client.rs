//! The CLI and Shell use the same authorized helper and task connection.
use crate::management_command::ManagementCli;
#[cfg(target_os = "linux")]
use crate::management_command::{ManagementRequest, is_draft, kind};
#[cfg(target_os = "linux")]
use platform::management::*;
#[cfg(target_os = "linux")]
use std::collections::BTreeMap;
use std::io::Write;
#[cfg(target_os = "linux")]
use std::sync::atomic::AtomicBool;

pub(crate) fn run(call: ManagementCli, stdout: &mut impl Write, stderr: &mut impl Write) -> i32 {
    if let ManagementCli::Help(group) = &call {
        return crate::management_command::help(stdout, group).map_or(1, |_| 0);
    }
    let ManagementCli::Run(request) = call else {
        unreachable!()
    };
    #[cfg(target_os = "linux")]
    {
        match linux::execute(&request, stdout, stderr) {
            Ok(code) => code,
            Err(error) => {
                let code = problem::OperationProblem::from_error(&error).exit_code;
                let _ = writeln!(stderr, "{error}");
                if request.json {
                    let _ = serde_json::to_writer(
                        &mut *stdout,
                        &serde_json::json!({"status":"failed","problem":problem::OperationProblem::from_error(&error)}),
                    );
                    let _ = writeln!(stdout);
                }
                code
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = stdout;
        let _ = request;
        let _ = writeln!(stderr, "Linux management is unavailable on this platform.");
        4
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::fs::File;
    use std::io::{IsTerminal, Read};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};
    use zeroize::Zeroizing;

    #[derive(Default)]
    struct ResultData {
        code: i32,
        state: String,
        message: String,
        kind: Option<ManagementKind>,
        action: String,
        operation_id: Option<String>,
        transaction_id: Option<String>,
        snapshot: Option<ManagementSnapshot>,
        document: Option<ConfigDocument>,
        preview_diff: Option<String>,
        problem: Option<problem::OperationProblem>,
    }
    enum ClientEvent {
        Event(OperationEvent),
        End(Result<(), ManagementError>),
    }

    fn observer_group(
        process: &watchdog::ProcessWatchdog,
    ) -> Result<watchdog::ManagedTaskGroup, ManagementError> {
        process
            .register_app(watchdog::AppDescriptor::new(
                watchdog::AppId::from_static("cli"),
                "Tundra CLI",
                env!("CARGO_PKG_VERSION"),
                watchdog::AppCriticality::ProcessCritical,
            ))
            .map(|app| app.task_group("management"))
            .map_err(|error| ManagementError::Failed(error.to_string()))
    }

    fn spawn_observer<F>(
        group: &watchdog::ManagedTaskGroup,
        observe: F,
    ) -> Result<watchdog::ManagedThreadHandle<()>, ManagementError>
    where
        F: FnOnce() + Send + 'static,
    {
        // An observer may have already started a system operation when it panics.
        // Never restart its factory or replay the command. Reconnect by task ID.
        let mut observe = Some(observe);
        group
            .spawn_thread(
                watchdog::TaskSpec::one_shot(watchdog::TaskId::from_static("operation-observer")),
                move || {
                    let observe = observe
                        .take()
                        .expect("one-shot operation observer cannot be restarted");
                    observe();
                },
            )
            .map_err(|error| ManagementError::Failed(error.to_string()))
    }

    pub(super) fn execute(
        request: &ManagementRequest,
        stdout: &mut impl Write,
        stderr: &mut impl Write,
    ) -> Result<i32, ManagementError> {
        let authority = authorization::PrivilegeSession::default();
        if request.group == "operations" {
            return operations(request, &authority, stdout, stderr);
        }
        let kind = kind(&request.group)
            .ok_or_else(|| ManagementError::InvalidInput("Unknown management group".into()))?;
        if kind == ManagementKind::SystemConfig {
            return configuration(request, &authority, stdout, stderr);
        }
        if kind == ManagementKind::Users && request.verb == "shells" {
            output_json_or_text(
                request.json,
                &serde_json::json!(users::allowed_shells()?),
                stdout,
            )?;
            return Ok(0);
        }
        let mut query = ManagementQuery::new(kind);
        query.scope = request.values.get("scope").cloned().unwrap_or_default();
        if kind == ManagementKind::Services && query.scope.is_empty() {
            query.scope = "system".into();
        }
        query.filter = request.values.get("filter").cloned().unwrap_or_default();
        if kind == ManagementKind::Packages {
            query.scope = match request.verb.as_str() {
                "search" | "install" => "search",
                "updates" => "updates",
                "sources" | "source-enable" | "source-disable" | "source-remove" | "source-add" => {
                    "sources"
                }
                "conflicts" => "conflicts",
                "status" => "status",
                _ => {
                    if query.scope.is_empty() {
                        "installed"
                    } else {
                        &query.scope
                    }
                }
            }
            .to_string();
            if request.verb == "search" {
                query.filter = request.target.clone().unwrap_or_default();
            }
            if matches!(
                request.verb.as_str(),
                "show" | "install" | "remove" | "upgrade"
            ) {
                query.target = request.target.clone();
            }
        }
        if kind == ManagementKind::Users {
            if request.verb == "groups" || request.verb.starts_with("group-") {
                query.scope = "groups".into();
            } else if let Some(name) = &request.target {
                if request.verb != "create" {
                    query.options.insert("username".into(), name.clone());
                }
            }
        }
        if kind == ManagementKind::Processes {
            for key in ["sort", "descending", "tree"] {
                if let Some(value) = request.values.get(key) {
                    query.options.insert(key.into(), value.clone());
                }
            }
        }
        if matches!(
            kind,
            ManagementKind::Services | ManagementKind::Processes | ManagementKind::Disks
        ) {
            query.target = request.target.clone();
        }
        let pure_query = matches!(
            (request.group.as_str(), request.verb.as_str()),
            ("services", "list" | "show" | "dependencies")
                | (
                    "processes",
                    "list" | "show" | "files" | "ports" | "io" | "service"
                )
                | (
                    "packages",
                    "list" | "show" | "search" | "updates" | "status" | "conflicts" | "sources"
                )
                | ("network", "list" | "show" | "wifi-list")
                | ("disks", "list" | "show" | "inodes")
                | ("users", "list" | "show" | "groups" | "lookup")
        );
        if pure_query {
            let mut snapshot = platform::management::query(&query, &AtomicBool::new(false))?;
            if let Some(target) = &request.target {
                if request.verb != "search" {
                    snapshot.rows.retain(|row| row_matches(row, target));
                }
            }
            if request.verb == "wifi-list" {
                snapshot.rows.retain(|row| {
                    row.id.starts_with("wifi:")
                        && request.values.get("interface").is_none_or(|interface| {
                            row.identity.get("interface") == Some(interface)
                        })
                });
            }
            if request.target.is_some() && request.verb != "search" && snapshot.rows.is_empty() {
                return Err(ManagementError::Conflict(
                    "The target is not in the current list; refresh and use its exact ID".into(),
                ));
            }
            print_snapshot(&snapshot, request.json, stdout)?;
            return Ok(0);
        }
        // Transactions and new account/directory requests do not refer to an inventory row.
        if kind == ManagementKind::Network
            && matches!(request.verb.as_str(), "confirm" | "transaction-status")
        {
            let command = ManagementCommand {
                kind,
                action: request.verb.clone(),
                target: request.target.clone(),
                values: request.values.clone(),
                identity: BTreeMap::new(),
            };
            return execute_and_print(
                command,
                true,
                request.verb == "confirm",
                request,
                &authority,
                stdout,
                stderr,
            );
        }
        if kind == ManagementKind::Disks && request.verb == "scan" {
            let mut values = request.values.clone();
            values.insert("directory".into(), request.target.clone().unwrap());
            let command = ManagementCommand {
                kind,
                action: "scan".into(),
                target: None,
                values,
                identity: BTreeMap::new(),
            };
            return execute_and_print(command, false, false, request, &authority, stdout, stderr);
        }
        let mut snapshot = platform::management::query(&query, &AtomicBool::new(false))?;
        let action_id = backend_action(kind, &request.verb, &snapshot.backend);
        let mut selected = select_action(&snapshot, request, &action_id)?;
        if kind == ManagementKind::Network && selected.0.disabled_reason.is_some() {
            let inspect = ManagementCommand {
                kind,
                action: "inspect_network".into(),
                target: None,
                values: BTreeMap::new(),
                identity: BTreeMap::new(),
            };
            let result = operate(Some(inspect), true, None, request, &authority, stderr)?;
            if result.code != 0 {
                print_result(&result, request.json, stdout)?;
                return Ok(result.code);
            }
            snapshot = result.snapshot.ok_or_else(|| {
                ManagementError::Failed(
                    "Authorized network inspection did not return a list".into(),
                )
            })?;
            selected = select_action(&snapshot, request, &action_id)?;
        }
        let (action, row) = selected;
        if let Some(reason) = &action.disabled_reason {
            return Err(ManagementError::Unavailable(reason.clone()));
        }
        let mut values = action.values.clone();
        // Action defaults match the displayed form. Explicit CLI values replace them.
        for field in &action.fields {
            if !field.secret && !field.value.is_empty() {
                values.insert(field.id.clone(), field.value.clone());
            }
        }
        values.extend(
            request
                .values
                .iter()
                .filter(|(key, _)| !matches!(key.as_str(), "filter" | "scope"))
                .map(|(key, value)| (key.clone(), value.clone())),
        );
        if kind == ManagementKind::Services {
            values.insert("scope".into(), query.scope.clone());
        }
        if kind == ManagementKind::Users && request.verb == "create" {
            values.insert("username".into(), request.target.clone().unwrap());
        }
        if kind == ManagementKind::Users && request.verb == "group-create" {
            values.insert("name".into(), request.target.clone().unwrap());
        }
        if kind == ManagementKind::Services && request.verb == "create" {
            values.insert("service_name".into(), request.target.clone().unwrap());
        }
        if kind == ManagementKind::Packages && request.verb == "source-add" {
            if let Some(name) = values.get("name").cloned() {
                values.insert("source_name".into(), name);
            }
        }
        for field in &action.fields {
            if field.secret {
                // Optional hidden-network passwords are required only for personal encryption.
                let required = field.required
                    || (field.id == "password"
                        && values
                            .get("security")
                            .is_some_and(|security| security != "open"));
                if required || request.password_fd.is_some() {
                    match secret(
                        request.password_fd,
                        request.non_interactive,
                        &field.label,
                        stderr,
                    )? {
                        Some(value) => {
                            values.insert(field.id.clone(), value.to_string());
                        }
                        None => return input_required(request, stdout, stderr, &field.label),
                    }
                }
            } else if field.required && values.get(&field.id).is_none_or(|value| value.is_empty()) {
                return input_required(
                    request,
                    stdout,
                    stderr,
                    &format!("Missing --{}", field.id.replace('_', "-")),
                );
            }
        }
        if kind == ManagementKind::Packages && !is_draft(&request.group, &request.verb) {
            values.insert(
                "non_interactive".into(),
                request.non_interactive.to_string(),
            );
            values.insert("yes".into(), request.yes.to_string());
        }
        if kind == ManagementKind::Network && request.non_interactive && request.verb == "configure"
        {
            values.insert("defer_confirmation".into(), "true".into());
        }
        let command = ManagementCommand {
            kind,
            action: action.id.clone(),
            target: row.as_ref().map(|row| row.id.clone()),
            values,
            identity: row.map(|row| row.identity).unwrap_or_default(),
        };
        if is_draft(&request.group, &request.verb) {
            let draft = match kind {
                ManagementKind::Services if request.verb == "config" => {
                    let path = command
                        .values
                        .get("path")
                        .map(PathBuf::from)
                        .ok_or_else(|| {
                            ManagementError::InvalidInput("No service configuration path".into())
                        })?;
                    let content = match command.values.get("content") {
                        Some(text) => text.clone(),
                        None => match read_document(&path, request, &authority, stdout, stderr)? {
                            (Some(document), _) => document.content,
                            (None, code) => return Ok(code),
                        },
                    };
                    ConfigDraft {
                        path,
                        content,
                        validator: "systemd".into(),
                        service: command.values.get("service").cloned(),
                        scope: query.scope.clone(),
                        expected_content: None,
                    }
                }
                ManagementKind::Services => {
                    services::prepare_config_draft(&command, &AtomicBool::new(false))?
                }
                ManagementKind::Packages => {
                    packages::prepare_config_draft(&command, &AtomicBool::new(false))?
                }
                ManagementKind::Disks => {
                    disks::automatic_mount_draft(&command, &AtomicBool::new(false))?
                }
                _ => unreachable!(),
            };
            return save_or_print_draft(draft, request, &authority, stdout, stderr);
        }
        execute_and_print(
            command,
            action.privileged,
            action.confirm,
            request,
            &authority,
            stdout,
            stderr,
        )
    }

    fn row_matches(row: &ManagementRow, target: &str) -> bool {
        row.id == target
            || row.id == format!("group:{target}")
            || row.identity.get("name").is_some_and(|name| name == target)
    }

    fn backend_action(kind: ManagementKind, verb: &str, backend: &str) -> String {
        match kind {
            ManagementKind::Services => match verb {
                "create" => "create_service",
                "create-instance" => "create_instance",
                "config" => "edit_system_config",
                _ => verb,
            }
            .replace('-', "_"),
            ManagementKind::Processes => verb.into(),
            ManagementKind::Packages => match (backend, verb) {
                ("pacman", "install") => "pacman_install",
                ("pacman", "upgrade") => "pacman_upgrade",
                ("pacman", "upgrade-all") => "pacman_upgrade_all",
                (_, "check") => "check_database",
                _ => verb,
            }
            .replace('-', "_"),
            ManagementKind::Users => match verb {
                "create" => "user_create",
                "info" => "user_info",
                "password" => "user_password",
                "lock" => "user_lock",
                "unlock" => "user_unlock",
                "delete" => "user_delete",
                "set-groups" => "user_groups",
                "primary-group" => "user_primary_group",
                "shell" => "user_shell",
                "expiry" => "user_expiry",
                "ssh-keys" => "user_ssh_keys",
                "ssh-add" => "user_ssh_add",
                "ssh-remove" => "user_ssh_remove",
                _ => verb,
            }
            .replace('-', "_"),
            ManagementKind::Disks if verb == "automatic-mount" => "auto_mount".into(),
            _ => verb.into(),
        }
    }

    fn select_action(
        snapshot: &ManagementSnapshot,
        request: &ManagementRequest,
        id: &str,
    ) -> Result<(ManagementAction, Option<ManagementRow>), ManagementError> {
        let global = matches!(
            request.verb.as_str(),
            "daemon-reload"
                | "create"
                | "source-add"
                | "upgrade-all"
                | "refresh"
                | "repair-configure"
                | "repair-dependencies"
                | "check"
                | "group-create"
        ) && !(request.group == "network" && request.verb == "check");
        if global {
            if let Some(action) = snapshot.actions.iter().find(|action| action.id == id) {
                return Ok((action.clone(), None));
            }
        }
        let target = request.target.as_deref().unwrap_or("");
        let mut rows = snapshot
            .rows
            .iter()
            .filter(|row| {
                row_matches(row, target)
                    || (request.group == "network"
                        && request.verb == "wifi-connect"
                        && request
                            .values
                            .get("ssid")
                            .is_some_and(|ssid| row.cells.first() == Some(ssid))
                        && row
                            .identity
                            .get("interface")
                            .is_some_and(|interface| interface == target))
            })
            .collect::<Vec<_>>();
        if request.group == "network"
            && request.verb == "wifi-connect"
            && request.values.contains_key("ssid")
            && rows.iter().any(|row| row.id.starts_with("wifi:"))
        {
            rows.retain(|row| row.id.starts_with("wifi:"));
        }
        if rows.len() > 1 {
            return Err(ManagementError::Conflict(
                "Several items match; choose an exact ID from the list".into(),
            ));
        }
        if let Some(row) = rows.first() {
            if let Some(action) = row.actions.iter().find(|action| action.id == id) {
                if request.group == "network" && request.verb == "wifi-connect" {
                    return Ok((
                        wifi_connect_action(row, action, request)?,
                        Some((*row).clone()),
                    ));
                }
                return Ok((action.clone(), Some((*row).clone())));
            }
            // Explicit SSID on an interface also supports a hidden personal network.
            if request.group == "network"
                && request.verb == "wifi-connect"
                && request.values.contains_key("ssid")
            {
                if let Some(action) = row.actions.iter().find(|action| action.id == "wifi-hidden") {
                    return Ok((
                        wifi_connect_action(row, action, request)?,
                        Some((*row).clone()),
                    ));
                }
            }
        }
        // APT recovery/check is also available from installed/search lists.
        if request.group == "packages"
            && matches!(
                request.verb.as_str(),
                "check" | "repair-configure" | "repair-dependencies"
            )
        {
            let mut query = ManagementQuery::new(ManagementKind::Packages);
            query.scope = "status".into();
            let status = platform::management::query(&query, &AtomicBool::new(false))?;
            if let Some(action) = status.actions.iter().find(|action| action.id == id) {
                return Ok((action.clone(), None));
            }
        }
        Err(ManagementError::Unavailable(
            "This action is unavailable for the target; inspect its current details".into(),
        ))
    }

    fn wifi_connect_action(
        row: &ManagementRow,
        action: &ManagementAction,
        request: &ManagementRequest,
    ) -> Result<ManagementAction, ManagementError> {
        if request.password_fd.is_none() {
            return Ok(action.clone());
        }
        let security = if action.id == "wifi-hidden" {
            request.values.get("security").or_else(|| {
                action
                    .fields
                    .iter()
                    .find(|field| field.id == "security")
                    .map(|field| &field.value)
            })
        } else {
            row.identity.get("security")
        };
        if security.is_some_and(|security| security == "open") {
            return Err(ManagementError::InvalidInput(
                "Open Wi-Fi does not use a password; remove --password-fd".into(),
            ));
        }
        Ok(row
            .actions
            .iter()
            .find(|action| action.id == "wifi-connect-password")
            .unwrap_or(action)
            .clone())
    }

    fn execute_and_print(
        command: ManagementCommand,
        privileged: bool,
        confirm: bool,
        request: &ManagementRequest,
        authority: &authorization::PrivilegeSession,
        stdout: &mut impl Write,
        stderr: &mut impl Write,
    ) -> Result<i32, ManagementError> {
        if confirm && !request.yes {
            if request.non_interactive {
                return input_required(
                    request,
                    stdout,
                    stderr,
                    "Review this change, then pass --yes",
                );
            }
            if !confirm_change(
                &format!("{} {} {:?}", request.group, request.verb, request.target),
                stderr,
            )? {
                return Ok(130);
            }
        }
        let result = operate(Some(command), privileged, None, request, authority, stderr)?;
        print_result(&result, request.json, stdout)?;
        Ok(result.code)
    }

    fn configuration(
        request: &ManagementRequest,
        authority: &authorization::PrivilegeSession,
        stdout: &mut impl Write,
        stderr: &mut impl Write,
    ) -> Result<i32, ManagementError> {
        let path = PathBuf::from(request.target.as_ref().unwrap());
        if request.verb == "diff" {
            let document = match read_document(&path, request, authority, stdout, stderr)? {
                (Some(document), _) => document,
                (None, code) => return Ok(code),
            };
            let candidate = candidate(request)?;
            let changes = diff(&document.content, &candidate);
            if request.json {
                output_json_or_text(
                    true,
                    &serde_json::json!({"path":path,"expected_version":document.version,"diff":changes}),
                    stdout,
                )?;
            } else {
                writeln!(stdout, "{changes}").map_err(failure)?;
            }
            return Ok(0);
        }
        if request.verb == "read" {
            let document = match read_document(&path, request, authority, stdout, stderr)? {
                (Some(document), _) => document,
                (None, code) => return Ok(code),
            };
            print_document(&document, request.json, true, stdout)?;
            return Ok(0);
        }
        let recovery_original = if request.verb == "preview-restore" {
            match read_document(&path, request, authority, stdout, stderr)? {
                (Some(document), _) => Some(document),
                (None, code) => return Ok(code),
            }
        } else {
            None
        };
        let mut values = request.values.clone();
        values.remove("filter");
        values.insert("path".into(), path.display().to_string());
        if request.input.is_some() {
            values.insert("content".into(), candidate(request)?);
        }
        if request.verb == "check" || request.verb == "permissions" {
            let document = match read_document(&path, request, authority, stdout, stderr)? {
                (Some(document), _) => document,
                (None, code) => return Ok(code),
            };
            if request.verb == "check" {
                values.entry("content".into()).or_insert(document.content);
                values
                    .entry("expected_version".into())
                    .or_insert(document.version);
            } else {
                values.insert("content".into(), document.content);
            }
        }
        let command = ManagementCommand {
            kind: ManagementKind::SystemConfig,
            action: request.verb.replace('-', "_"),
            target: Some(path.display().to_string()),
            values,
            identity: BTreeMap::new(),
        };
        if let Some(original) = recovery_original {
            let mut result = operate(Some(command), true, None, request, authority, stderr)?;
            if result.code == 0 {
                if let Some(recovery) = &result.document {
                    result.preview_diff = Some(recovery_difference(&original, recovery));
                } else {
                    result.code = 7;
                    result.state = "unknown".into();
                    result.message =
                        "Recovery preview returned no document; reconnect before retrying".into();
                }
            }
            print_result(&result, request.json, stdout)?;
            return Ok(result.code);
        }
        execute_and_print(
            command,
            configuration_privileged(
                &request.verb,
                request.values.get("scope").map(String::as_str),
            ),
            matches!(
                request.verb.as_str(),
                "apply" | "permissions" | "restore" | "reload"
            ),
            request,
            authority,
            stdout,
            stderr,
        )
    }

    fn configuration_privileged(verb: &str, scope: Option<&str>) -> bool {
        !(verb == "reload" && scope == Some("user"))
    }
    fn recovery_difference(original: &ConfigDocument, recovery: &ConfigDocument) -> String {
        let mut text = diff(&original.content, &recovery.content);
        for (name, before, after) in [
            (
                "Owner UID",
                original.uid.to_string(),
                recovery.uid.to_string(),
            ),
            (
                "Group GID",
                original.gid.to_string(),
                recovery.gid.to_string(),
            ),
            (
                "Mode",
                format!("{:04o}", original.mode),
                format!("{:04o}", recovery.mode),
            ),
        ] {
            if before != after {
                text.push_str(&format!("\n{name}: {before} -> {after}"));
            }
        }
        if original.existed != recovery.existed {
            text.push_str(if recovery.existed {
                "\nFile will be restored"
            } else {
                "\nFile will be removed to restore its previous absence"
            });
        }
        text
    }

    fn candidate(request: &ManagementRequest) -> Result<String, ManagementError> {
        let path = request
            .input
            .as_ref()
            .ok_or_else(|| ManagementError::InvalidInput("Supply --input CANDIDATE".into()))?;
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(failure)?
            .take(256 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(failure)?;
        if bytes.len() > 256 * 1024 || bytes.contains(&0) {
            return Err(ManagementError::InvalidInput(
                "Candidate must be a text file smaller than 256 KiB".into(),
            ));
        }
        String::from_utf8(bytes)
            .map_err(|_| ManagementError::InvalidInput("Candidate must use UTF-8".into()))
    }

    fn read_document(
        path: &Path,
        request: &ManagementRequest,
        authority: &authorization::PrivilegeSession,
        stdout: &mut impl Write,
        stderr: &mut impl Write,
    ) -> Result<(Option<ConfigDocument>, i32), ManagementError> {
        match system_config::read(path) {
            Ok(document) => return Ok((Some(document), 0)),
            Err(ManagementError::PermissionDenied(_)) => {}
            Err(error) => return Err(error),
        }
        let command = ManagementCommand {
            kind: ManagementKind::SystemConfig,
            action: "read".into(),
            target: Some(path.display().to_string()),
            values: BTreeMap::new(),
            identity: BTreeMap::new(),
        };
        let result = operate(Some(command), true, None, request, authority, stderr)?;
        if result.code != 0 {
            print_result(&result, request.json, stdout)?;
            return Ok((None, result.code));
        }
        result
            .document
            .map(|document| (Some(document), 0))
            .ok_or_else(|| {
                ManagementError::Failed("Read returned no configuration document".into())
            })
    }

    fn save_or_print_draft(
        draft: ConfigDraft,
        request: &ManagementRequest,
        authority: &authorization::PrivilegeSession,
        stdout: &mut impl Write,
        stderr: &mut impl Write,
    ) -> Result<i32, ManagementError> {
        if !request.apply {
            if request.json {
                output_json_or_text(
                    true,
                    &serde_json::to_value(&draft)
                        .map_err(|error| ManagementError::Failed(error.to_string()))?,
                    stdout,
                )?;
            } else {
                writeln!(
                    stdout,
                    "# File: {}\n# Validator: {}\n{}",
                    draft.path.display(),
                    draft.validator,
                    draft.content
                )
                .map_err(failure)?;
            }
            return Ok(0);
        }
        let previous = match read_document(&draft.path, request, authority, stdout, stderr)? {
            (Some(document), _) => document,
            (None, code) => return Ok(code),
        };
        if draft
            .expected_content
            .as_ref()
            .is_some_and(|expected| expected != &previous.content)
        {
            return Err(ManagementError::Conflict(
                "The file changed after the draft was prepared; generate it again".into(),
            ));
        }
        if request.values.get("expected_version") != Some(&previous.version) {
            return Err(ManagementError::Conflict(
                "The supplied file version changed; read it and review the draft again".into(),
            ));
        }
        let mut values = BTreeMap::from([
            ("path".into(), draft.path.display().to_string()),
            ("content".into(), draft.content),
            ("validator".into(), draft.validator),
            ("expected_version".into(), previous.version),
            ("scope".into(), draft.scope),
        ]);
        if let Some(service) = draft.service {
            values.insert("service".into(), service);
        }
        if let Some(value) = request.values.get("allow_unvalidated") {
            values.insert("allow_unvalidated".into(), value.clone());
        }
        execute_and_print(
            ManagementCommand {
                kind: ManagementKind::SystemConfig,
                action: "apply".into(),
                target: Some(draft.path.display().to_string()),
                values,
                identity: BTreeMap::new(),
            },
            true,
            true,
            request,
            authority,
            stdout,
            stderr,
        )
    }

    fn operations(
        request: &ManagementRequest,
        authority: &authorization::PrivilegeSession,
        stdout: &mut impl Write,
        stderr: &mut impl Write,
    ) -> Result<i32, ManagementError> {
        if request.verb == "list" {
            let rows = helper::recoverable_operations(unsafe { libc::getuid() }).into_iter().filter(|path| path.exists()).map(|path| serde_json::json!({"operation_id":path,"kind":helper::operation_kind(&path)})).collect::<Vec<_>>();
            output_json_or_text(request.json, &serde_json::json!(rows), stdout)?;
            return Ok(0);
        }
        let result = operate(
            None,
            false,
            request.target.as_ref().map(PathBuf::from),
            request,
            authority,
            stderr,
        )?;
        print_result(&result, request.json, stdout)?;
        Ok(result.code)
    }

    fn operate(
        command: Option<ManagementCommand>,
        privileged: bool,
        socket: Option<PathBuf>,
        request: &ManagementRequest,
        authority: &authorization::PrivilegeSession,
        stderr: &mut impl Write,
    ) -> Result<ResultData, ManagementError> {
        let pending_network = command.as_ref().is_some_and(|command| {
            command.kind == ManagementKind::Network
                && command.action == "configure"
                && command
                    .values
                    .get("defer_confirmation")
                    .is_some_and(|value| value == "true")
        });
        let mut config_check = command.as_ref().is_some_and(|command| {
            command.kind == ManagementKind::SystemConfig && command.action == "check"
        });
        let detached = Arc::new(AtomicBool::new(false));
        struct DetachOnDrop(Arc<AtomicBool>);
        impl Drop for DetachOnDrop {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let _detach_guard = DetachOnDrop(detached.clone());
        let worker_detached = detached.clone();
        let worker_authority = authority.clone();
        let (send, receive) = mpsc::channel();
        let (input_send, input_receive) = mpsc::channel();
        let process = watchdog::ProcessWatchdog::global().ok_or_else(|| {
            ManagementError::Unavailable("Start management commands with tundra-cli".into())
        })?;
        let group = observer_group(&process)?;
        let worker = spawn_observer(&group, move || {
            let callback = |event| {
                let _ = send.send(ClientEvent::Event(event));
            };
            let result = client::run(
                &worker_authority,
                command,
                privileged,
                socket,
                input_receive,
                &worker_detached,
                &callback,
            );
            let _ = send.send(ClientEvent::End(result));
        })?;
        let started = Instant::now();
        let mut result = ResultData {
            code: 7,
            state: "unknown".into(),
            ..Default::default()
        };
        let mut terminal = false;
        let mut completed = false;
        loop {
            if (result.operation_id.is_some()
                && started.elapsed() >= Duration::from_secs(request.wait_seconds))
                || started.elapsed() >= Duration::from_secs(request.wait_seconds.saturating_add(60))
            {
                result.code = 7;
                result.state = "unfinished".into();
                result.message = "Stopped waiting; the operation may still be running. Use operations attach with its ID.".into();
                break;
            }
            match receive.recv_timeout(Duration::from_millis(30)) {
                Ok(ClientEvent::Event(event)) => match event {
                    OperationEvent::Connected { operation_id } => {
                        let _ = writeln!(stderr, "Operation: {operation_id}");
                        result.operation_id = Some(operation_id);
                        result.state = "running".into();
                    }
                    OperationEvent::Snapshot { snapshot } => result.snapshot = Some(snapshot),
                    OperationEvent::ConfigDocument { document } => result.document = Some(document),
                    OperationEvent::Output { text: message } => {
                        if let Some(id) = message.strip_prefix("Network transaction: ") {
                            result.transaction_id = Some(id.trim().into());
                        }
                        let _ = writeln!(stderr, "{}", runtime_log::sanitize_text(&message));
                    }
                    OperationEvent::Progress { message, .. }
                    | OperationEvent::Disconnected { message } => {
                        let _ = writeln!(stderr, "{}", runtime_log::sanitize_text(&message));
                    }
                    OperationEvent::TerminalOutput { bytes } => {
                        terminal = true;
                        if !request.json && !request.non_interactive {
                            let _ = stderr.write_all(&bytes);
                            let _ = stderr.flush();
                        } else {
                            let _ = writeln!(
                                stderr,
                                "{}",
                                runtime_log::sanitize_text(&String::from_utf8_lossy(&bytes))
                            );
                        }
                    }
                    OperationEvent::Question {
                        id,
                        prompt,
                        choices,
                        secret: confidential,
                    } => {
                        let answer = if confidential {
                            secret(
                                if id == "sudo-password" {
                                    request.authorization_fd
                                } else {
                                    request.password_fd
                                },
                                request.non_interactive,
                                &prompt,
                                stderr,
                            )
                            .map(|answer| answer.map(|value| value.to_string()))
                        } else if request.non_interactive || !std::io::stdin().is_terminal() {
                            Ok(None)
                        } else {
                            read_line(&format!("{prompt} {}", choices.join(" / ")), stderr)
                                .map(Some)
                        };
                        let answer = match answer {
                            Ok(answer) => answer,
                            Err(error) => {
                                client_error(&mut result, error);
                                break;
                            }
                        };
                        if let Some(value) = answer {
                            if input_send
                                .send(OperationInput::Answer { id, value })
                                .is_err()
                            {
                                client_error(
                                    &mut result,
                                    ManagementError::Failed(
                                        "Input channel closed; reconnect before retrying".into(),
                                    ),
                                );
                                break;
                            }
                        } else {
                            result.code = 6;
                            result.state = "input_required".into();
                            result.message = prompt;
                            break;
                        }
                    }
                    OperationEvent::Problem { problem } => {
                        if result
                            .problem
                            .as_ref()
                            .is_none_or(|old| old.native_exit_code.is_none())
                            || problem.native_exit_code.is_some()
                        {
                            result.code = problem.exit_code;
                            result.problem = Some(problem);
                        }
                    }
                    OperationEvent::Completed { message } => {
                        complete_result(&mut result, pending_network, config_check, message);
                        completed = true;
                        break;
                    }
                    OperationEvent::Failed { message } => {
                        if result.problem.is_none() {
                            result.code = 1;
                        }
                        result.state = failure_state(result.problem.as_ref()).into();
                        result.message = message;
                        break;
                    }
                    OperationEvent::Started { kind, action, .. } => {
                        config_check = kind == ManagementKind::SystemConfig && action == "check";
                        result.kind = Some(kind);
                        result.action = action;
                        result.state = "running".into();
                    }
                },
                Ok(ClientEvent::End(Ok(()))) => break,
                Ok(ClientEvent::End(Err(error))) => {
                    result.code = if matches!(error, ManagementError::PermissionDenied(_)) {
                        3
                    } else if result.operation_id.is_some() {
                        7
                    } else {
                        problem::OperationProblem::from_error(&error).exit_code
                    };
                    result.message = error.to_string();
                    result.state = "unknown".into();
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => {
                    result.message =
                        "Operation observer disconnected; reconnect before retrying".into();
                    break;
                }
            }
            if terminal && !request.non_interactive && stdin_ready() {
                let mut buffer = [0u8; 4096];
                let count = match std::io::stdin().read(&mut buffer) {
                    Ok(count) => count,
                    Err(error) => {
                        client_error(&mut result, failure(error));
                        break;
                    }
                };
                if count > 0 {
                    let _ = input_send.send(OperationInput::Terminal {
                        bytes: buffer[..count].to_vec(),
                    });
                }
            }
        }
        detached.store(true, Ordering::Release);
        drop(input_send);
        // The shared client has bounded reads and never cancels the helper on detach.
        // Joining does not wait for the operation itself or kill package writes.
        let _ = worker.join();
        if completed
            && result.kind == Some(ManagementKind::Network)
            && result.action == "configure"
            && result.transaction_id.is_some()
        {
            let mut status_request = request.clone();
            status_request.wait_seconds = request.wait_seconds.clamp(10, 900);
            let transaction = result.transaction_id.clone().unwrap();
            let status = operate(
                Some(ManagementCommand {
                    kind: ManagementKind::Network,
                    action: "transaction-status".into(),
                    target: Some(transaction),
                    values: Default::default(),
                    identity: Default::default(),
                }),
                true,
                None,
                &status_request,
                authority,
                stderr,
            );
            match status {
                Ok(status) => merge_transaction_result(&mut result, status),
                Err(error) => client_error(&mut result, error),
            }
        }
        Ok(result)
    }

    fn client_error(result: &mut ResultData, error: ManagementError) {
        let mut problem = problem::OperationProblem::from_error(&error);
        let unknown = result.operation_id.is_some() && matches!(error, ManagementError::Failed(_));
        result.code = if unknown { 7 } else { problem.exit_code };
        result.state = if unknown { "unknown" } else { "failed" }.into();
        result.message = error.to_string();
        problem.exit_code = result.code;
        result.problem = Some(problem);
    }

    fn merge_transaction_result(result: &mut ResultData, status: ResultData) {
        if status.code != 0 {
            result.code = status.code;
            result.state = if status.code == 7 {
                "unknown"
            } else {
                "failed"
            }
            .into();
            result.message = status.message;
            result.problem = status.problem;
            return;
        }
        let row = status.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .rows
                .iter()
                .find(|row| Some(&row.id) == result.transaction_id.as_ref())
        });
        let state = row.and_then(|row| row.cells.get(2)).map(String::as_str);
        let (code, name) = match state {
            Some("AwaitingConfirmation") => (7, "awaiting_confirmation"),
            Some("Prepared" | "Applying") => (7, "running"),
            Some("Committed") => (0, "succeeded"),
            Some("Restored") => (1, "rolled_back"),
            Some("RestoreFailed") => (1, "partial"),
            _ => (7, "unknown"),
        };
        result.code = code;
        result.state = name.into();
        result.message = row
            .and_then(|row| row.detail.iter().find(|(key, _)| key == "Result"))
            .map(|(_, value)| value.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                format!("Network transaction is {name}; use network transaction-status with its ID")
            });
        result.snapshot = status.snapshot;
    }

    fn stdin_ready() -> bool {
        let mut fd = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        unsafe { libc::poll(&mut fd, 1, 0) > 0 && fd.revents & libc::POLLIN != 0 }
    }
    fn failure_state(problem: Option<&problem::OperationProblem>) -> &'static str {
        if problem.is_some_and(|problem| problem.code == "package_partial") {
            "partial"
        } else {
            "failed"
        }
    }
    fn complete_result(
        result: &mut ResultData,
        pending_network: bool,
        config_check: bool,
        message: String,
    ) {
        result.code = if pending_network { 7 } else { 0 };
        result.state = if pending_network {
            "awaiting_confirmation"
        } else {
            "succeeded"
        }
        .into();
        result.message = message;
        if !config_check {
            return;
        }
        let Some(document) = &result.document else {
            result.code = 7;
            result.state = "unknown".into();
            result.message =
                "Configuration check returned no result; reconnect before retrying".into();
            return;
        };
        match &document.check {
            ConfigCheck::Passed => {}
            ConfigCheck::Failed(message) => {
                result.code = 1;
                result.state = "failed".into();
                result.message = message.clone();
            }
            ConfigCheck::Unavailable(message) => {
                result.code = 4;
                result.state = "unsupported".into();
                result.message = message.clone();
            }
            ConfigCheck::NotChecked => {
                result.code = 4;
                result.state = "unchecked".into();
                result.message = "No configuration checker is available".into();
            }
        }
    }
    fn failure(error: std::io::Error) -> ManagementError {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            ManagementError::PermissionDenied(error.to_string())
        } else {
            ManagementError::Failed(error.to_string())
        }
    }

    fn print_result(
        result: &ResultData,
        json: bool,
        stdout: &mut impl Write,
    ) -> Result<(), ManagementError> {
        if json {
            output_json_or_text(
                true,
                &serde_json::json!({"status":result.state,"exit_code":result.code,"operation_id":result.operation_id,"transaction_id":result.transaction_id,"kind":result.kind,"action":result.action,"message":result.message,"problem":result.problem,"snapshot":result.snapshot,"document":result.document,"diff":result.preview_diff}),
                stdout,
            )
        } else {
            if let Some(snapshot) = &result.snapshot {
                print_snapshot(snapshot, false, stdout)?;
            }
            if let Some(document) = &result.document {
                print_document(document, false, false, stdout)?;
            }
            if let Some(difference) = &result.preview_diff {
                writeln!(stdout, "{difference}").map_err(failure)?;
            }
            writeln!(stdout, "{}", runtime_log::sanitize_text(&result.message)).map_err(failure)?;
            if result.code == 7 {
                if let Some(id) = &result.operation_id {
                    writeln!(stdout, "operation_id={id}").map_err(failure)?;
                }
            }
            if let Some(id) = &result.transaction_id {
                writeln!(stdout, "network_transaction={id}").map_err(failure)?;
            }
            Ok(())
        }
    }
    fn print_snapshot(
        snapshot: &ManagementSnapshot,
        json: bool,
        stdout: &mut impl Write,
    ) -> Result<(), ManagementError> {
        if json {
            return output_json_or_text(
                true,
                &serde_json::to_value(snapshot)
                    .map_err(|error| ManagementError::Failed(error.to_string()))?,
                stdout,
            );
        }
        writeln!(stdout, "ID\t{}", snapshot.columns.join("\t")).map_err(failure)?;
        for row in &snapshot.rows {
            writeln!(
                stdout,
                "{}\t{}",
                runtime_log::sanitize_text(&row.id),
                row.cells
                    .iter()
                    .map(|cell| runtime_log::sanitize_text(cell))
                    .collect::<Vec<_>>()
                    .join("\t")
            )
            .map_err(failure)?;
            for (name, value) in &row.detail {
                if snapshot.rows.len() == 1 {
                    writeln!(
                        stdout,
                        "{}: {}",
                        runtime_log::sanitize_text(name),
                        runtime_log::sanitize_text(value)
                    )
                    .map_err(failure)?;
                }
            }
        }
        Ok(())
    }
    fn print_document(
        document: &ConfigDocument,
        json: bool,
        content: bool,
        stdout: &mut impl Write,
    ) -> Result<(), ManagementError> {
        if json {
            return output_json_or_text(
                true,
                &serde_json::to_value(document)
                    .map_err(|error| ManagementError::Failed(error.to_string()))?,
                stdout,
            );
        }
        writeln!(
            stdout,
            "File: {}\nVersion: {}\nOwner: {}:{}\nMode: {:04o}\nCheck: {:?}",
            document.path.display(),
            document.version,
            document.uid,
            document.gid,
            document.mode,
            document.check
        )
        .map_err(failure)?;
        if content {
            write!(stdout, "{}", document.content).map_err(failure)?;
        }
        Ok(())
    }
    fn output_json_or_text(
        json: bool,
        value: &serde_json::Value,
        stdout: &mut impl Write,
    ) -> Result<(), ManagementError> {
        if json {
            serde_json::to_writer(&mut *stdout, value)
                .map_err(|error| ManagementError::Failed(error.to_string()))?;
        } else if let Some(array) = value.as_array() {
            for entry in array {
                if let Some(text) = entry.as_str() {
                    writeln!(stdout, "{text}").map_err(failure)?;
                } else {
                    writeln!(stdout, "{entry}").map_err(failure)?;
                }
            }
        } else {
            write!(stdout, "{value}").map_err(failure)?;
        }
        writeln!(stdout).map_err(failure)
    }
    fn input_required(
        request: &ManagementRequest,
        stdout: &mut impl Write,
        stderr: &mut impl Write,
        message: &str,
    ) -> Result<i32, ManagementError> {
        let _ = writeln!(stderr, "{message}. Supply the required input and retry.");
        if request.json {
            output_json_or_text(
                true,
                &serde_json::json!({"status":"input_required","exit_code":6,"message":message}),
                stdout,
            )?;
        }
        Ok(6)
    }

    fn read_line(prompt: &str, stderr: &mut impl Write) -> Result<String, ManagementError> {
        if !std::io::stdin().is_terminal() {
            return Err(ManagementError::InvalidInput(
                "Interactive input needs a terminal; use --non-interactive and explicit options"
                    .into(),
            ));
        }
        write!(stderr, "{prompt}: ").map_err(failure)?;
        stderr.flush().map_err(failure)?;
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).map_err(failure)?;
        Ok(line.trim_end_matches(['\n', '\r']).into())
    }
    fn confirm_change(prompt: &str, stderr: &mut impl Write) -> Result<bool, ManagementError> {
        Ok(matches!(
            read_line(&format!("{prompt}. Continue? [y/N]"), stderr)?.as_str(),
            "y" | "Y" | "yes"
        ))
    }

    fn secret(
        fd: Option<i32>,
        non_interactive: bool,
        prompt: &str,
        stderr: &mut impl Write,
    ) -> Result<Option<Zeroizing<String>>, ManagementError> {
        if let Some(fd) = fd {
            let cloned = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
            if cloned < 0 {
                return Err(failure(std::io::Error::last_os_error()));
            }
            let file = unsafe { File::from_raw_fd(cloned) };
            return read_secret_file(file).map(Some);
        }
        if non_interactive || !std::io::stdin().is_terminal() {
            return Ok(None);
        }
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .map_err(failure)?;
        let fd = tty.as_raw_fd();
        let mut previous = std::mem::MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(fd, previous.as_mut_ptr()) } != 0 {
            return Err(failure(std::io::Error::last_os_error()));
        }
        let previous = unsafe { previous.assume_init() };
        let mut hidden = previous;
        hidden.c_lflag &= !libc::ECHO;
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &hidden) } != 0 {
            return Err(failure(std::io::Error::last_os_error()));
        }
        struct Restore {
            fd: i32,
            state: libc::termios,
        }
        impl Drop for Restore {
            fn drop(&mut self) {
                unsafe {
                    libc::tcsetattr(self.fd, libc::TCSANOW, &self.state);
                }
            }
        }
        let restore = Restore {
            fd,
            state: previous,
        };
        write!(stderr, "{prompt}: ").map_err(failure)?;
        stderr.flush().map_err(failure)?;
        let result = read_secret_file(tty);
        drop(restore);
        let _ = writeln!(stderr);
        result.map(Some)
    }
    fn read_secret_file(mut file: File) -> Result<Zeroizing<String>, ManagementError> {
        let mut bytes = Zeroizing::new(Vec::new());
        // Do not buffer past one line: the next secret on the same descriptor must
        // remain available for a later question rather than disappearing on drop.
        loop {
            let mut byte = [0u8; 1];
            if file.read(&mut byte).map_err(failure)? == 0 {
                break;
            }
            bytes.push(byte[0]);
            if byte[0] == b'\n' || bytes.len() > 8192 {
                break;
            }
        }
        if bytes.len() > 8192 {
            return Err(ManagementError::InvalidInput(
                "Secret input is too long".into(),
            ));
        }
        while bytes
            .last()
            .is_some_and(|byte| matches!(byte, b'\n' | b'\r'))
        {
            bytes.pop();
        }
        let value = std::str::from_utf8(&bytes)
            .map_err(|_| ManagementError::InvalidInput("Secret input must use UTF-8".into()))?;
        Ok(Zeroizing::new(value.to_string()))
    }
    fn diff(before: &str, after: &str) -> String {
        if before == after {
            return "No changes".into();
        }
        let mut text = String::from("--- current\n+++ candidate\n");
        for line in before.lines() {
            text.push('-');
            text.push_str(line);
            text.push('\n');
        }
        for line in after.lines() {
            text.push('+');
            text.push_str(line);
            text.push('\n');
        }
        text
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn explicit_wifi_password_selects_replacement_and_open_networks_reject_it() {
            let ManagementCli::Run(mut request) = crate::management_command::parse_management(
                "network",
                &["wifi-connect", "wifi:test:ap", "--password-fd", "9"].map(String::from),
            )
            .unwrap() else {
                panic!("expected a management request");
            };
            let password = ManagementField {
                id: "password".into(),
                required: true,
                secret: true,
                ..Default::default()
            };
            let mut snapshot = ManagementSnapshot {
                rows: vec![ManagementRow {
                    id: "wifi:test:ap".into(),
                    identity: [("security".into(), "wpa2".into())].into(),
                    actions: vec![
                        ManagementAction {
                            id: "wifi-connect".into(),
                            ..Default::default()
                        },
                        ManagementAction {
                            id: "wifi-connect-password".into(),
                            fields: vec![password.clone()],
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            };
            let (selected, _) = select_action(&snapshot, &request, "wifi-connect").unwrap();
            assert_eq!(selected.id, "wifi-connect-password");
            assert!(selected.fields.iter().any(|field| field.secret));
            request.password_fd = None;
            let (selected, _) = select_action(&snapshot, &request, "wifi-connect").unwrap();
            assert_eq!(selected.id, "wifi-connect");
            assert!(selected.fields.is_empty());
            request.password_fd = Some(9);
            snapshot.rows[0].actions.pop();
            snapshot.rows[0].actions[0].fields.push(password);
            let (selected, _) = select_action(&snapshot, &request, "wifi-connect").unwrap();
            assert_eq!(selected.id, "wifi-connect");
            assert!(selected.fields.iter().any(|field| field.secret));
            snapshot.rows[0]
                .identity
                .insert("security".into(), "open".into());
            snapshot.rows[0].actions[0].fields.clear();
            assert!(matches!(
                select_action(&snapshot, &request, "wifi-connect"),
                Err(ManagementError::InvalidInput(message)) if message.contains("remove --password-fd")
            ));
            snapshot.rows[0].identity.clear();
            snapshot.rows[0].actions[0].id = "wifi-hidden".into();
            request.values.insert("ssid".into(), "hidden".into());
            request.values.insert("security".into(), "open".into());
            assert!(matches!(
                select_action(&snapshot, &request, "wifi-connect"),
                Err(ManagementError::InvalidInput(_))
            ));
        }
        #[test]
        fn managed_observer_reports_panic_without_replaying_and_can_reconnect() {
            let root = std::env::temp_dir()
                .join(format!("tundra-cli-observer-test-{}", std::process::id()));
            let config = watchdog::WatchdogConfig::new(
                root.join("reports"),
                root.join("fallback"),
                root.join("state"),
                "cli-observer-test",
                env!("CARGO_PKG_VERSION"),
            )
            .with_unclean_exit_tracking(false);
            let (runtime, process) = watchdog::WatchdogRuntime::start_isolated(config).unwrap();
            let group = observer_group(&process).unwrap();
            let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let observer_attempts = attempts.clone();
            let (send, receive) = mpsc::channel();
            let worker = spawn_observer(&group, move || {
                observer_attempts.fetch_add(1, Ordering::SeqCst);
                send.send("operation-id").unwrap();
                panic!("observer disconnected after starting an operation");
            })
            .unwrap();
            assert_eq!(worker.join().unwrap(), None);
            assert_eq!(attempts.load(Ordering::SeqCst), 1);
            assert_eq!(receive.recv().unwrap(), "operation-id");
            assert!(receive.recv().is_err());
            let deadline = Instant::now() + Duration::from_secs(2);
            let incident = loop {
                if let Some(incident) = runtime.try_recv_incident() {
                    break incident;
                }
                assert!(
                    Instant::now() < deadline,
                    "watchdog did not report the panic"
                );
                std::thread::sleep(Duration::from_millis(5));
            };
            assert_eq!(
                incident.task_id.as_ref().map(watchdog::TaskId::as_str),
                Some("operation-observer")
            );
            assert!(!incident.recovery.is_recovered());
            // Reconnection starts another reader; it must be possible to reuse
            // the completed task slot without restarting the original command.
            let (send, receive) = mpsc::channel();
            let reconnected = spawn_observer(&group, move || {
                send.send("read existing operation-id").unwrap();
            })
            .unwrap();
            assert_eq!(reconnected.join().unwrap(), Some(()));
            assert_eq!(receive.recv().unwrap(), "read existing operation-id");
            assert_eq!(attempts.load(Ordering::SeqCst), 1);
            runtime.shutdown().unwrap();
            std::fs::remove_dir_all(root).unwrap();
        }
        #[test]
        fn user_service_reload_keeps_the_original_user_and_does_not_elevate() {
            assert!(!configuration_privileged("reload", Some("user")));
            assert!(configuration_privileged("reload", Some("system")));
            assert!(configuration_privileged("reload", None));
            for action in ["apply", "check", "permissions", "history", "restore"] {
                assert!(configuration_privileged(action, Some("user")), "{action}");
            }
        }
        #[test]
        fn secret_descriptor_keeps_the_next_line_and_original_handle_open() {
            let mut descriptors = [0; 2];
            assert_eq!(
                unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC) },
                0
            );
            let reader = unsafe { File::from_raw_fd(descriptors[0]) };
            let mut writer = unsafe { File::from_raw_fd(descriptors[1]) };
            writer
                .write_all(b"operation-secret\nsecond-secret\n")
                .unwrap();
            drop(writer);
            let mut output = Vec::new();
            assert_eq!(
                &**secret(Some(reader.as_raw_fd()), true, "Secret", &mut output)
                    .unwrap()
                    .as_ref()
                    .unwrap(),
                "operation-secret"
            );
            assert_eq!(
                &**secret(Some(reader.as_raw_fd()), true, "Secret", &mut output)
                    .unwrap()
                    .as_ref()
                    .unwrap(),
                "second-secret"
            );
            assert!(unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_GETFD) } >= 0);
            assert!(output.is_empty());
        }
        #[test]
        fn noninteractive_secret_request_returns_input_required_without_stdin() {
            assert!(
                secret(None, true, "Secret", &mut Vec::new())
                    .unwrap()
                    .is_none()
            );
        }
        #[test]
        fn partial_package_results_remain_distinct_from_plain_failure() {
            let mut problem = problem::OperationProblem::from_error(&ManagementError::Failed(
                "native operation failed".into(),
            ));
            assert_eq!(failure_state(Some(&problem)), "failed");
            problem.code = "package_partial".into();
            problem.native_exit_code = Some(100);
            assert_eq!(failure_state(Some(&problem)), "partial");
            assert_eq!(problem.exit_code, 1);
            problem.code = "package_busy".into();
            assert_eq!(failure_state(Some(&problem)), "failed");
            assert_eq!(failure_state(None), "failed");
        }
        #[test]
        fn reconnected_configuration_check_keeps_its_result_and_operation_id() {
            for (check, expected, state) in [
                (ConfigCheck::Passed, 0, "succeeded"),
                (ConfigCheck::Failed("Bad configuration".into()), 1, "failed"),
                (
                    ConfigCheck::Unavailable("Tool missing".into()),
                    4,
                    "unsupported",
                ),
                (ConfigCheck::NotChecked, 4, "unchecked"),
            ] {
                let mut result = ResultData {
                    operation_id: Some("test-operation".into()),
                    document: Some(ConfigDocument {
                        path: "/test.conf".into(),
                        content: "candidate".into(),
                        version: "version".into(),
                        existed: true,
                        uid: 0,
                        gid: 0,
                        mode: 0o600,
                        validator: "sshd".into(),
                        check,
                        backup_id: None,
                    }),
                    ..Default::default()
                };
                complete_result(
                    &mut result,
                    false,
                    true,
                    "Configuration check finished".into(),
                );
                assert_eq!(result.code, expected);
                assert_eq!(result.state, state);
                assert_eq!(result.operation_id.as_deref(), Some("test-operation"));
            }
            let mut missing = ResultData::default();
            complete_result(&mut missing, false, true, "Finished".into());
            assert_eq!(missing.code, 7);
            let mut reading = ResultData::default();
            complete_result(&mut reading, false, false, "Read".into());
            assert_eq!(reading.code, 0);
        }
        #[test]
        fn network_reconnect_uses_current_transaction_state_and_keeps_original_ids() {
            for (state, code, expected) in [
                ("AwaitingConfirmation", 7, "awaiting_confirmation"),
                ("Applying", 7, "running"),
                ("Committed", 0, "succeeded"),
                ("Restored", 1, "rolled_back"),
                ("RestoreFailed", 1, "partial"),
            ] {
                let mut result = ResultData {
                    operation_id: Some("original-operation".into()),
                    transaction_id: Some("transaction".into()),
                    ..Default::default()
                };
                let status = ResultData {
                    operation_id: Some("status-query".into()),
                    snapshot: Some(ManagementSnapshot {
                        rows: vec![ManagementRow {
                            id: "transaction".into(),
                            cells: vec![
                                "transaction".into(),
                                "test-interface".into(),
                                state.into(),
                                "30".into(),
                            ],
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                };
                merge_transaction_result(&mut result, status);
                assert_eq!(result.code, code);
                assert_eq!(result.state, expected);
                assert_eq!(result.operation_id.as_deref(), Some("original-operation"));
                assert_eq!(result.transaction_id.as_deref(), Some("transaction"));
            }
        }
        #[test]
        fn input_and_transaction_query_errors_preserve_reconnect_identifiers() {
            let mut result = ResultData {
                operation_id: Some("original-operation".into()),
                transaction_id: Some("transaction".into()),
                ..Default::default()
            };
            client_error(
                &mut result,
                ManagementError::Failed("Secret descriptor closed".into()),
            );
            assert_eq!(result.code, 7);
            assert_eq!(result.state, "unknown");
            assert_eq!(result.operation_id.as_deref(), Some("original-operation"));
            merge_transaction_result(
                &mut result,
                ResultData {
                    code: 3,
                    message: "Authorization denied".into(),
                    ..Default::default()
                },
            );
            assert_eq!(result.code, 3);
            assert_eq!(result.operation_id.as_deref(), Some("original-operation"));
            assert_eq!(result.transaction_id.as_deref(), Some("transaction"));
        }
        #[test]
        fn recovery_preview_reports_content_attributes_and_previous_absence() {
            let original = ConfigDocument {
                path: "/test.conf".into(),
                content: "Port 22\n".into(),
                version: "version".into(),
                existed: true,
                uid: 1000,
                gid: 1000,
                mode: 0o644,
                validator: "sshd".into(),
                check: ConfigCheck::NotChecked,
                backup_id: None,
            };
            let mut recovered = original.clone();
            recovered.content = "Port 2222\n".into();
            recovered.mode = 0o600;
            let difference = recovery_difference(&original, &recovered);
            assert!(difference.contains("-Port 22"));
            assert!(difference.contains("+Port 2222"));
            assert!(difference.contains("Mode: 0644 -> 0600"));
            recovered.existed = false;
            recovered.content.clear();
            assert!(recovery_difference(&original, &recovered).contains("previous absence"));
            let result = ResultData {
                document: Some(recovered),
                preview_diff: Some(difference),
                ..Default::default()
            };
            let mut output = Vec::new();
            print_result(&result, false, &mut output).unwrap();
            assert!(String::from_utf8(output).unwrap().contains("-Port 22"));
        }
    }
}
