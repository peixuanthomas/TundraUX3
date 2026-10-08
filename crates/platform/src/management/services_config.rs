//! Service drafts are reviewed by the existing configuration editor before any write.
use super::{Scope, check_cancelled, valid_unit_name};
use crate::management::{
    ConfigDraft, ManagementAction, ManagementCommand, ManagementError, ManagementField,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn field(id: &str, label: &str, value: &str, required: bool) -> ManagementField {
    ManagementField {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        required,
        ..Default::default()
    }
}

pub(super) fn create_action(scope: Scope) -> ManagementAction {
    ManagementAction {
        id: "create_service".into(),
        label: "Create service".into(),
        group: "configuration".into(),
        fields: vec![
            field("service_name", "Service name", "", true),
            field("program", "Program (absolute path)", "", true),
            field("arguments", "Arguments", "", false),
            field("user", "Run as user", "", false),
            field("working_directory", "Working directory", "", false),
        ],
        values: BTreeMap::from([("scope".into(), scope.id().into())]),
        ..Default::default()
    }
}

pub(super) fn instance_action(scope: Scope) -> ManagementAction {
    ManagementAction {
        id: "create_instance".into(),
        label: "Create service instance".into(),
        group: "configuration".into(),
        fields: vec![field("instance", "Instance name", "", true)],
        values: BTreeMap::from([("scope".into(), scope.id().into())]),
        ..Default::default()
    }
}

fn unit_directory(scope: Scope) -> Result<PathBuf, ManagementError> {
    if scope == Scope::System {
        return Ok(PathBuf::from("/etc/systemd/system"));
    }
    let uid = unsafe { libc::getuid() };
    // Resolve the actual current account; HOME/XDG_CONFIG_HOME can come from an untrusted client.
    let mut pwd = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut found = std::ptr::null_mut();
    let mut buffer = vec![0u8; 65536];
    let code = unsafe {
        libc::getpwuid_r(
            uid,
            pwd.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut found,
        )
    };
    if code != 0 || found.is_null() {
        return Err(ManagementError::Unavailable(
            "Cannot find the current user's home directory".into(),
        ));
    }
    let pwd = unsafe { pwd.assume_init() };
    let home = unsafe { std::ffi::CStr::from_ptr(pwd.pw_dir) }
        .to_str()
        .map_err(|_| ManagementError::Unavailable("Home directory is not UTF-8".into()))?;
    if !Path::new(home).is_absolute() {
        return Err(ManagementError::Unavailable(
            "Home directory must be absolute".into(),
        ));
    }
    Ok(Path::new(home).join(".config/systemd/user"))
}

pub(super) fn edit_action(unit: &str, scope: Scope) -> ManagementAction {
    let directory = unit_directory(scope);
    let (values, disabled_reason) = match directory {
        Ok(directory) => {
            let path = directory.join(format!("{unit}.d/tundra.conf"));
            let mut values = BTreeMap::from([
                ("path".into(), path.to_string_lossy().into_owned()),
                ("validator".into(), "systemd".into()),
                ("service".into(), unit.into()),
                ("scope".into(), scope.id().into()),
            ]);
            if !path.exists() {
                values.insert("content".into(), "[Service]\n".into());
            }
            (values, None)
        }
        Err(error) => (BTreeMap::new(), Some(error.to_string())),
    };
    ManagementAction {
        id: "edit_system_config".into(),
        label: "Edit service configuration".into(),
        values,
        disabled_reason,
        group: "configuration".into(),
        ..Default::default()
    }
}

/// Build a candidate file only. This function never creates a directory, starts a service,
/// enables startup, or replaces a packaged unit.
pub fn prepare_config_draft(
    command: &ManagementCommand,
    cancelled: &AtomicBool,
) -> Result<ConfigDraft, ManagementError> {
    check_cancelled(cancelled)?;
    let scope = Scope::parse(
        command
            .values
            .get("scope")
            .or_else(|| command.identity.get("scope"))
            .map(String::as_str)
            .unwrap_or(""),
    )?;
    let directory = unit_directory(scope)?;
    let (unit, path, content) = match command.action.as_str() {
        "create_service" => {
            let mut unit = command
                .values
                .get("service_name")
                .or_else(|| command.values.get("name"))
                .map(String::as_str)
                .unwrap_or("")
                .to_string();
            if !unit.ends_with(".service") {
                unit.push_str(".service");
            }
            if !valid_unit_name(&unit) || unit.contains('@') || unit.contains('\\') {
                return Err(ManagementError::InvalidInput(
                    "Enter a plain service name; paths and templates are not accepted".into(),
                ));
            }
            let program = value(command, "program");
            if !Path::new(program).is_absolute() || program.chars().any(char::is_control) {
                return Err(ManagementError::InvalidInput(
                    "Choose an absolute program path".into(),
                ));
            }
            let args = parse_arguments(value(command, "arguments"))?;
            let user = value(command, "user");
            if !user.is_empty()
                && (!user
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                    || user.starts_with('-'))
            {
                return Err(ManagementError::InvalidInput(
                    "Choose an existing account name".into(),
                ));
            }
            if scope == Scope::User && !user.is_empty() {
                return Err(ManagementError::InvalidInput(
                    "A user service runs as the current account; leave Run as user empty".into(),
                ));
            }
            let working = value(command, "working_directory");
            if !working.is_empty()
                && (!Path::new(working).is_absolute() || working.chars().any(char::is_control))
            {
                return Err(ManagementError::InvalidInput(
                    "Working directory must be an absolute path".into(),
                ));
            }
            let mut content = format!(
                "[Unit]\nDescription={}\n\n[Service]\nType=simple\nExecStart={}",
                unit.strip_suffix(".service").unwrap_or(&unit),
                quote_argument(program)
            );
            for arg in args {
                content.push(' ');
                content.push_str(&quote_argument(&arg));
            }
            content.push('\n');
            if !user.is_empty() {
                content.push_str(&format!("User={user}\n"));
            }
            if !working.is_empty() {
                content.push_str(&format!("WorkingDirectory={}\n", quote_argument(working)));
            }
            content.push_str("\n[Install]\nWantedBy=multi-user.target\n");
            if scope == Scope::User {
                content = content.replace("WantedBy=multi-user.target", "WantedBy=default.target");
            }
            let path = directory.join(&unit);
            if path.exists() {
                return Err(ManagementError::Conflict(
                    "This service file exists; edit its configuration instead".into(),
                ));
            }
            // A new /etc unit must not shadow a distribution-provided service.
            if scope == Scope::System
                && [
                    "/usr/lib/systemd/system",
                    "/lib/systemd/system",
                    "/run/systemd/system",
                ]
                .iter()
                .any(|base| Path::new(base).join(&unit).exists())
            {
                return Err(ManagementError::Conflict(
                    "This service already exists; edit its override instead".into(),
                ));
            }
            (unit, path, content)
        }
        "create_instance" => {
            let template = command
                .target
                .as_deref()
                .filter(|target| valid_unit_name(target) && target.ends_with("@.service"))
                .ok_or_else(|| {
                    ManagementError::InvalidInput(
                        "Choose a service template ending in @.service".into(),
                    )
                })?;
            if command.identity.get("unit").map(String::as_str) != Some(template) {
                return Err(ManagementError::Conflict(
                    "The template changed; refresh and try again".into(),
                ));
            }
            let instance = value(command, "instance");
            if instance.is_empty()
                || instance.len() > 128
                || !instance
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
                || instance.starts_with('-')
            {
                return Err(ManagementError::InvalidInput(
                    "Use letters, numbers, dots, underscores or hyphens for the instance".into(),
                ));
            }
            let unit = template.replace("@.service", &format!("@{instance}.service"));
            let path = directory.join(format!("{unit}.d/tundra.conf"));
            if path.exists() {
                return Err(ManagementError::Conflict(
                    "This instance override exists; edit it instead".into(),
                ));
            }
            (
                unit,
                path,
                "[Service]\n# Instance-specific settings\n".into(),
            )
        }
        _ => {
            return Err(ManagementError::InvalidInput(
                "Unknown service draft operation".into(),
            ));
        }
    };
    check_cancelled(cancelled)?;
    Ok(ConfigDraft {
        path,
        content,
        validator: "systemd".into(),
        service: Some(unit),
        scope: scope.id().into(),
        expected_content: None,
    })
}

fn value<'a>(command: &'a ManagementCommand, key: &str) -> &'a str {
    command.values.get(key).map(String::as_str).unwrap_or("")
}

fn quote_argument(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('$', "$$")
            .replace('%', "%%")
    )
}

// Quotes group literal arguments; no variable, shell, glob or command expansion is performed.
fn parse_arguments(text: &str) -> Result<Vec<String>, ManagementError> {
    if text.len() > 16384 || text.chars().any(char::is_control) {
        return Err(ManagementError::InvalidInput(
            "Arguments are too long or contain control characters".into(),
        ));
    }
    let mut args = Vec::new();
    let mut arg = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut present = false;
    for ch in text.chars() {
        if escaped {
            arg.push(ch);
            escaped = false;
            present = true;
            continue;
        }
        if ch == '\\' && quote != Some('\'') {
            escaped = true;
            present = true;
            continue;
        }
        if let Some(expected) = quote {
            if ch == expected {
                quote = None;
            } else {
                arg.push(ch);
            }
            present = true;
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            present = true;
        } else if ch.is_whitespace() {
            if present {
                args.push(std::mem::take(&mut arg));
                present = false;
            }
        } else {
            arg.push(ch);
            present = true;
        }
    }
    if quote.is_some() || escaped {
        return Err(ManagementError::InvalidInput(
            "Close the argument quote or escape before continuing".into(),
        ));
    }
    if present {
        args.push(arg);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arguments_are_literal_and_systemd_expansion_is_escaped() {
        assert_eq!(
            parse_arguments("--name 'two words' \"\" '$HOME' $(id)").unwrap(),
            ["--name", "two words", "", "$HOME", "$(id)"]
        );
        assert_eq!(
            quote_argument("$HOME %i \"hello\""),
            "\"$$HOME %%i \\\"hello\\\"\""
        );
        assert!(parse_arguments("'unfinished").is_err());
        assert!(parse_arguments("x\ny").is_err());
    }
    #[test]
    fn service_draft_has_no_automatic_start_or_enable_and_does_not_write() {
        let name = format!("tundra-draft-test-{}", std::process::id());
        let command = ManagementCommand {
            kind: crate::management::ManagementKind::Services,
            action: "create_service".into(),
            target: None,
            values: BTreeMap::from([
                ("name".into(), name.clone()),
                ("program".into(), "/usr/bin/sleep".into()),
                ("arguments".into(), "30".into()),
            ]),
            identity: BTreeMap::new(),
        };
        let draft = prepare_config_draft(&command, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            draft.path,
            Path::new("/etc/systemd/system").join(format!("{name}.service"))
        );
        assert!(!draft.path.exists());
        assert!(
            draft
                .content
                .contains("ExecStart=\"/usr/bin/sleep\" \"30\"")
        );
        assert_eq!(draft.validator, "systemd");
    }
}
