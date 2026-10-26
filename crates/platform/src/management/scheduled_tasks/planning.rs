use super::*;
use std::collections::BTreeMap;

pub(super) fn calendar(values: &BTreeMap<String, String>) -> Result<String, ManagementError> {
    let get = |key: &str| values.get(key).map(String::as_str).unwrap_or("");
    let kind = get("schedule_kind");
    if kind.is_empty() || kind == "advanced" {
        let expression = get("schedule").trim();
        if expression.is_empty()
            || expression.len() > 2048
            || expression.chars().any(char::is_control)
        {
            return Err(invalid(i18n::tr!("scheduled-task-schedule-invalid")));
        }
        return Ok(expression.into());
    }
    let hour = get("hour").parse::<u8>().ok().filter(|value| *value < 24);
    let minute = get("minute").parse::<u8>().ok().filter(|value| *value < 60);
    let (Some(hour), Some(minute)) = (hour, minute) else {
        return Err(invalid(i18n::tr!("schedule-invalid-time")));
    };
    let time = format!("{hour:02}:{minute:02}:00");
    match kind {
        "daily" => Ok(format!("*-*-* {time}")),
        "weekly" => {
            let days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
                .into_iter()
                .enumerate()
                .filter(|(index, _)| get(&format!("day_{index}")) == "true")
                .map(|(_, day)| day)
                .collect::<Vec<_>>();
            if days.is_empty() {
                return Err(invalid(i18n::tr!("schedule-select-weekday")));
            }
            Ok(format!("{} *-*-* {time}", days.join(",")))
        }
        "once" => {
            let date = format!("{}-{}-{}", get("year"), get("month"), get("day"));
            let date = chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                .map_err(|_| invalid(i18n::tr!("schedule-invalid-date")))?;
            Ok(format!("{} {time}", date.format("%Y-%m-%d")))
        }
        _ => Err(invalid(i18n::tr!("schedule-invalid-kind"))),
    }
}

pub(super) fn operation(
    values: &BTreeMap<String, String>,
    flag: &AtomicBool,
) -> Result<(String, bool), ManagementError> {
    let get = |key: &str| values.get(key).map(String::as_str).unwrap_or("");
    match get("task_type") {
        "" | "command" => {
            let command = get("command");
            if command.trim().is_empty() || command.len() > 65536 || command.contains('\0') {
                return Err(invalid(i18n::tr!("scheduled-task-command-invalid")));
            }
            Ok((format!("/bin/sh -c {}", cron::shell_quote(command)), false))
        }
        "script" => {
            use std::os::unix::fs::OpenOptionsExt;
            let path = Path::new(get("script_path"));
            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(path)
                .map_err(io_error)?;
            if !path.is_absolute() || !file.metadata().map_err(io_error)?.is_file() {
                return Err(invalid(i18n::tr!("schedule-invalid-script")));
            }
            let mut bytes = Vec::new();
            file.take(512).read_to_end(&mut bytes).map_err(io_error)?;
            let header = String::from_utf8_lossy(&bytes);
            let interpreter = if let Some(line) = header
                .lines()
                .next()
                .and_then(|line| line.strip_prefix("#!"))
            {
                shlex::split(line)
                    .filter(|words| !words.is_empty())
                    .ok_or_else(|| invalid(i18n::tr!("schedule-invalid-interpreter")))?
            } else {
                vec!["/bin/sh".into()]
            };
            if !Path::new(&interpreter[0]).is_absolute() || !Path::new(&interpreter[0]).is_file() {
                return Err(invalid(i18n::tr!("schedule-invalid-interpreter")));
            }
            let args = shlex::split(get("arguments"))
                .ok_or_else(|| invalid(i18n::tr!("schedule-invalid-arguments")))?;
            let command = interpreter
                .into_iter()
                .chain([path.display().to_string()])
                .chain(args)
                .map(|word| cron::shell_quote(&word))
                .collect::<Vec<_>>()
                .join(" ");
            Ok((command, false))
        }
        "service_restart" => {
            let unit = get("service_unit");
            if !services::valid_unit_name(unit) {
                return Err(invalid(i18n::tr!("schedule-select-service")));
            }
            let load = run(
                "/usr/bin/systemctl",
                &[
                    "--system".into(),
                    "show".into(),
                    "--property=LoadState".into(),
                    "--value".into(),
                    "--".into(),
                    unit.into(),
                ],
                flag,
            )?;
            if load.trim() != "loaded" {
                return Err(invalid(i18n::tr!("schedule-select-service")));
            }
            Ok((
                format!(
                    "/usr/bin/systemctl --system --no-ask-password restart -- {}",
                    cron::shell_quote(unit)
                ),
                true,
            ))
        }
        "packages_update" => {
            let commands = super::super::packages::scheduled_update_commands(flag)?;
            let commands = commands
                .into_iter()
                .map(|spec| {
                    std::iter::once(spec.program.display().to_string())
                        .chain(spec.args)
                        .map(|word| cron::shell_quote(&word))
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
                .join(" && ");
            let script = format!(
                "export DEBIAN_FRONTEND=noninteractive DEBIAN_PRIORITY=critical; {commands}"
            );
            Ok((format!("/bin/sh -c {}", cron::shell_quote(&script)), true))
        }
        _ => Err(invalid(i18n::tr!("schedule-invalid-operation"))),
    }
}

pub(super) fn persistent(values: &BTreeMap<String, String>) -> Result<bool, ManagementError> {
    match values.get("persistent").map(String::as_str) {
        None | Some("false") => Ok(false),
        Some("true") => Ok(true),
        _ => Err(invalid(i18n::tr!("schedule-invalid-persistent"))),
    }
}

pub(super) fn cron_calendar(values: &BTreeMap<String, String>) -> Result<String, ManagementError> {
    let get = |key: &str| values.get(key).map(String::as_str).unwrap_or("");
    match get("schedule_kind") {
        "daily" | "weekly" => {
            calendar(values)?;
            let minute = get("minute")
                .parse::<u8>()
                .map_err(|_| invalid(i18n::tr!("schedule-invalid-time")))?;
            let hour = get("hour")
                .parse::<u8>()
                .map_err(|_| invalid(i18n::tr!("schedule-invalid-time")))?;
            let days = if get("schedule_kind") == "daily" {
                "*".into()
            } else {
                (0..7)
                    .filter(|index| get(&format!("day_{index}")) == "true")
                    .map(|index| ((index + 1) % 7).to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            };
            Ok(format!("{minute} {hour} * * {days}"))
        }
        "once" => Err(invalid(i18n::tr!("schedule-cron-once-unavailable"))),
        _ => {
            let expression = get("schedule");
            if expression.is_empty() || expression.chars().any(char::is_control) {
                return Err(invalid(i18n::tr!("scheduled-task-schedule-invalid")));
            }
            Ok(expression.into())
        }
    }
}

pub(super) fn description(values: &BTreeMap<String, String>) -> String {
    let get = |key: &str| values.get(key).cloned().unwrap_or_default();
    let time = format!("{}:{}", get("hour"), get("minute"));
    match get("schedule_kind").as_str() {
        "daily" => i18n::tr!("schedule-description-daily", time = time),
        "weekly" => {
            let days = [
                "schedule-mon",
                "schedule-tue",
                "schedule-wed",
                "schedule-thu",
                "schedule-fri",
                "schedule-sat",
                "schedule-sun",
            ]
            .into_iter()
            .enumerate()
            .filter(|(index, _)| get(&format!("day_{index}")) == "true")
            .map(|(_, id)| i18n::tr!(id))
            .collect::<Vec<_>>()
            .join(", ");
            i18n::tr!("schedule-description-weekly", days = days, time = time)
        }
        "once" => i18n::tr!(
            "schedule-description-once",
            date = format!("{}-{}-{}", get("year"), get("month"), get("day")),
            time = time
        ),
        _ => get("schedule"),
    }
}

pub(super) fn operation_description(values: &BTreeMap<String, String>) -> String {
    let get = |key: &str| values.get(key).cloned().unwrap_or_default();
    match get("task_type").as_str() {
        "script" => i18n::tr!(
            "schedule-review-script",
            path = get("script_path"),
            arguments = get("arguments"),
            directory = get("working_directory")
        ),
        "service_restart" => i18n::tr!("schedule-review-service", service = get("service_unit")),
        "packages_update" => i18n::tr!("schedule-packages-help"),
        _ if get("__source") == "native-timer" => i18n::tr!("schedule-review-native"),
        _ => i18n::tr!("schedule-review-command", command = get("command")),
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/management/scheduled_tasks_planning.rs"]
mod tests;
