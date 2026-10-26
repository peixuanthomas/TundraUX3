use super::*;

pub(super) fn directory(uid: u32) -> PathBuf {
    Path::new(RESULTS).join(uid.to_string()).join("history")
}

pub(super) fn prepare(uid: u32, gid: u32) -> Result<(), ManagementError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    prepare_results(uid, gid)?;
    let path = directory(uid);
    match fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(io_error(error)),
    }
    let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
    if !metadata.is_dir() || metadata.uid() != uid && metadata.uid() != 0 {
        return Err(invalid(i18n::tr!("scheduled-task-result-owner")));
    }
    let path_c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| invalid(i18n::tr!("scheduled-task-result-path")))?;
    if unsafe { libc::chown(path_c.as_ptr(), uid, gid) } != 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error)
}

pub(super) fn commands(unit: &str, label: &str, uid: u32) -> (String, String) {
    let (start, stop) = commands_at(&directory(uid), unit, label);
    (
        start,
        format!(
            "{stop}; /bin/sh -c {}",
            cron::shell_quote(&result_script(unit, uid))
        ),
    )
}

fn commands_at(root: &Path, unit: &str, label: &str) -> (String, String) {
    let root = cron::shell_quote(&root.display().to_string());
    let name = cron::shell_quote(&serde_json::to_string(label).expect("task label serializes"));
    let operation = cron::shell_quote(
        &serde_json::to_string(&format!("systemd:system:{unit}.service")).expect("unit serializes"),
    );
    // Only systemd's hexadecimal invocation id becomes a file name.
    let prefix = format!(
        "umask 077; history={root}; id=\"$INVOCATION_ID\"; case \"$id\" in ''|*[!0-9a-f]*) exit 1;; esac; boot=$(/usr/bin/cat /proc/sys/kernel/random/boot_id); "
    );
    let running_format = cron::shell_quote(
        "{\"format_version\":1,\"id\":\"%s\",\"name\":%s,\"source\":\"scheduled-tasks\",\"origin\":\"scheduled-tasks\",\"started_at\":%s,\"finished_at\":null,\"status\":\"running\",\"process_id\":%s,\"process_started_at\":%s,\"operation_id\":%s,\"boot_id\":\"%s\",\"output_complete\":true}\n",
    );
    let finished_format = cron::shell_quote(
        "{\"format_version\":1,\"id\":\"%s\",\"name\":%s,\"source\":\"scheduled-tasks\",\"origin\":\"scheduled-tasks\",\"started_at\":%s,\"finished_at\":%s,\"status\":\"%s\",\"operation_id\":%s,\"result\":\"%s; %s; %s\",\"boot_id\":\"%s\",\"output_complete\":true}\n",
    );
    let start = format!(
        "{prefix}exec >> \"$history/$id.output\" 2>&1; started=$(/usr/bin/date +%s); /usr/bin/printf '%s' \"$started\" > \"$history/$id.started\"; ticks=$(/usr/bin/awk '{{print $22}}' /proc/$$/stat); ticks=${{ticks:-null}}; /usr/bin/printf {running_format} \"$id\" {name} \"$started\" \"$$\" \"$ticks\" {operation} \"$boot\" > \"$history/$id.json.tmp\" && /usr/bin/mv -f -- \"$history/$id.json.tmp\" \"$history/$id.json\"; "
    );
    let stop = format!(
        "{prefix}finished=$(/usr/bin/date +%s); started=$(/usr/bin/cat -- \"$history/$id.started\" 2>/dev/null); started=${{started:-$finished}}; status=failed; [ \"$SERVICE_RESULT\" = success ] && status=succeeded; case \"$EXIT_CODE:$EXIT_STATUS\" in killed:TERM|killed:INT|killed:KILL|killed:15|killed:2|killed:9) status=cancelled;; esac; /usr/bin/printf {finished_format} \"$id\" {name} \"$started\" \"$finished\" \"$status\" {operation} \"$SERVICE_RESULT\" \"$EXIT_CODE\" \"$EXIT_STATUS\" \"$boot\" > \"$history/$id.json.tmp\" && /usr/bin/mv -f -- \"$history/$id.json.tmp\" \"$history/$id.json\" && /usr/bin/rm -f -- \"$history/$id.started\""
    );
    (start, stop)
}

#[cfg(test)]
#[path = "../../../tests/unit/management/scheduled_tasks_history.rs"]
mod tests;

fn result_script(key: &str, uid: u32) -> String {
    let path = format!("{RESULTS}/{uid}/{key}.result");
    format!(
        "/usr/bin/printf '%s\\n' \"$(/usr/bin/date --iso-8601=seconds)\" \"$SERVICE_RESULT\" \"$EXIT_CODE\" \"$EXIT_STATUS\" > {path}.tmp.$$ && /usr/bin/mv -f -- {path}.tmp.$$ {path}"
    )
}
