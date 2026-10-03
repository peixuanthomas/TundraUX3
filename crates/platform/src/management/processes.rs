//! Complete Linux process snapshots and fixed process operations.
use super::*;
use std::cmp::Ordering as CompareOrdering;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
struct ProcessRecord {
    pid: u32,
    parent: u32,
    uid: u32,
    name: String,
    command: String,
    state: char,
    nice: i32,
    threads: u32,
    started: u64,
    cpu_ticks: u64,
    memory: u64,
    cpu: Option<f64>,
}

#[derive(Debug)]
struct Stat {
    pid: u32,
    parent: u32,
    name: String,
    state: char,
    nice: i32,
    threads: u32,
    started: u64,
    cpu_ticks: u64,
    resident_pages: u64,
}

fn parse_stat(text: &str) -> Option<Stat> {
    // comm may contain spaces, parentheses, and newlines. Field 3 starts after
    // the final ')' and starttime is field 22, not a wall-clock timestamp.
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close <= open {
        return None;
    }
    let fields = text
        .get(close + 1..)?
        .split_whitespace()
        .collect::<Vec<_>>();
    Some(Stat {
        pid: text.get(..open)?.trim().parse().ok()?,
        parent: fields.get(1)?.parse().ok()?,
        name: text.get(open + 1..close)?.to_string(),
        state: fields.first()?.chars().next()?,
        nice: fields.get(16)?.parse().ok()?,
        threads: fields.get(17)?.parse().ok()?,
        started: fields.get(19)?.parse().ok()?,
        cpu_ticks: fields
            .get(11)?
            .parse::<u64>()
            .ok()?
            .checked_add(fields.get(12)?.parse().ok()?)?,
        resident_pages: fields.get(21)?.parse::<i64>().ok()?.max(0) as u64,
    })
}

fn read_process(root: &Path, pid: u32, page_size: u64) -> io::Result<ProcessRecord> {
    let directory = root.join(pid.to_string());
    let stat_text = fs::read_to_string(directory.join("stat"))?;
    let stat = parse_stat(&stat_text)
        .filter(|s| s.pid == pid)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid process stat"))?;
    let status = fs::read_to_string(directory.join("status"))?;
    let uid = status
        .lines()
        .find_map(|line| {
            line.strip_prefix("Uid:")?
                .split_whitespace()
                .next()?
                .parse::<u32>()
                .ok()
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "process UID is unavailable"))?;
    let command = fs::read(directory.join("cmdline"))
        .map(|bytes| {
            String::from_utf8_lossy(&bytes)
                .replace('\0', " ")
                .trim()
                .to_string()
        })
        .unwrap_or_default();
    let second = fs::read_to_string(directory.join("stat"))?;
    if parse_stat(&second).is_none_or(|s| s.started != stat.started || s.pid != pid) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "process changed while reading",
        ));
    }
    Ok(ProcessRecord {
        pid,
        parent: stat.parent,
        uid,
        name: stat.name,
        command,
        state: stat.state,
        nice: stat.nice,
        threads: stat.threads,
        started: stat.started,
        cpu_ticks: stat.cpu_ticks,
        memory: stat.resident_pages.saturating_mul(page_size),
        cpu: None,
    })
}

fn read_all(
    root: &Path,
    cancelled: &AtomicBool,
) -> Result<(Vec<ProcessRecord>, usize), ManagementError> {
    check_cancelled(cancelled)?;
    let entries = fs::read_dir(root)
        .map_err(|e| ManagementError::Unavailable(format!("Cannot read /proc: {e}")))?;
    // SAFETY: sysconf reads a constant; no pointers are passed.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return Err(ManagementError::Unavailable(
            "Linux page size is unavailable".into(),
        ));
    }
    let mut processes = Vec::new();
    let mut denied = 0;
    for entry in entries {
        check_cancelled(cancelled)?;
        let entry = entry.map_err(|e| ManagementError::Failed(e.to_string()))?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        match read_process(root, pid, page_size as u64) {
            Ok(process) => processes.push(process),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => denied += 1,
            // A disappearing process is normal during an enumeration.
            Err(_) => {}
        }
    }
    Ok((processes, denied))
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), ManagementError> {
    if cancelled.load(Ordering::Relaxed) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}

pub fn query(
    query: &ManagementQuery,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    let sample_started = Instant::now();
    let (previous, _) = read_all(Path::new("/proc"), cancelled)?;
    // A short second sample measures current usage rather than lifetime CPU or
    // fabricated zeroes. This runs in the management worker, never the UI loop.
    while sample_started.elapsed() < Duration::from_millis(150) {
        check_cancelled(cancelled)?;
        std::thread::sleep(Duration::from_millis(10));
    }
    let second_started = Instant::now();
    let (mut processes, denied) = read_all(Path::new("/proc"), cancelled)?;
    let elapsed = second_started.duration_since(sample_started).as_secs_f64();
    // SAFETY: sysconf has no pointer arguments.
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let previous = previous
        .into_iter()
        .map(|p| ((p.pid, p.started), p.cpu_ticks))
        .collect::<BTreeMap<_, _>>();
    if ticks_per_second > 0 && elapsed > 0.0 {
        for process in &mut processes {
            process.cpu = previous
                .get(&(process.pid, process.started))
                .and_then(|ticks| {
                    process
                        .cpu_ticks
                        .checked_sub(*ticks)
                        .map(|delta| delta as f64 * 100.0 / ticks_per_second as f64 / elapsed)
                });
        }
    }
    let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id").map_err(|e| {
        ManagementError::Unavailable(format!("Cannot read Linux boot identity: {e}"))
    })?;
    snapshot(processes, query, boot_id.trim(), denied)
}

fn snapshot(
    mut processes: Vec<ProcessRecord>,
    query: &ManagementQuery,
    boot_id: &str,
    denied: usize,
) -> Result<ManagementSnapshot, ManagementError> {
    let sort = query
        .options
        .get("sort")
        .map(String::as_str)
        .unwrap_or("cpu");
    if !matches!(sort, "pid" | "cpu" | "memory" | "uid" | "name" | "nice") {
        return Err(ManagementError::InvalidInput(
            "Unknown process sort column".into(),
        ));
    }
    let descending = boolean_option(query, "descending", true)?;
    let tree = boolean_option(query, "tree", false)?;
    let filter = query.filter.to_lowercase();
    processes.retain(|process| {
        filter.is_empty()
            || format!(
                "{} {} {} {}",
                process.pid, process.uid, process.name, process.command
            )
            .to_lowercase()
            .contains(&filter)
    });
    processes.sort_by(|a, b| {
        let order = match sort {
            "pid" => a.pid.cmp(&b.pid),
            "cpu" => a
                .cpu
                .unwrap_or(-1.0)
                .partial_cmp(&b.cpu.unwrap_or(-1.0))
                .unwrap_or(CompareOrdering::Equal),
            "memory" => a.memory.cmp(&b.memory),
            "uid" => a.uid.cmp(&b.uid),
            "name" => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            "nice" => a.nice.cmp(&b.nice),
            _ => unreachable!(),
        };
        (if descending { order.reverse() } else { order }).then_with(|| a.pid.cmp(&b.pid))
    });
    let mut rows = Vec::with_capacity(processes.len());
    if tree {
        let identities = processes.iter().map(|p| p.pid).collect::<BTreeSet<_>>();
        let mut children = BTreeMap::<u32, Vec<usize>>::new();
        let mut roots = Vec::new();
        for (index, process) in processes.iter().enumerate() {
            if process.parent != process.pid && identities.contains(&process.parent) {
                children.entry(process.parent).or_default().push(index);
            } else {
                roots.push(index);
            }
        }
        let mut visited = BTreeSet::new();
        let mut pending = roots
            .into_iter()
            .rev()
            .map(|index| (index, 0))
            .collect::<Vec<_>>();
        while let Some((index, depth)) = pending.pop() {
            let process = &processes[index];
            if !visited.insert(process.pid) {
                continue;
            }
            rows.push(row(process, boot_id, depth));
            if let Some(children) = children.get(&process.pid) {
                pending.extend(children.iter().rev().map(|index| (*index, depth + 1)));
            }
        }
        // Missing/changing parents must never hide a process, including cycles
        // in damaged fixtures or unusual namespace views.
        for process in &processes {
            if visited.insert(process.pid) {
                rows.push(row(process, boot_id, 0));
            }
        }
    } else {
        rows.extend(processes.iter().map(|p| row(p, boot_id, 0)));
    }
    let mut notices = vec!["CPU usage is sampled over a short interval; 100% means one CPU core. Nice applies to the selected process's main thread.".into()];
    if denied > 0 {
        notices.push(format!(
            "{denied} visible processes could not be read with the current Linux permissions."
        ));
    }
    if tree && !filter.is_empty() {
        notices.push(
            "The process tree includes matching processes; a filtered-out parent is omitted."
                .into(),
        );
    }
    Ok(ManagementSnapshot {
        columns: [
            "PID",
            "PPID",
            "UID",
            "State",
            "CPU %",
            "Memory",
            "Nice (main thread)",
            "Name",
        ]
        .map(String::from)
        .to_vec(),
        rows,
        backend: "Linux /proc and pidfd".into(),
        notices,
        actions: vec![ManagementAction {
            id: "set_view".into(),
            label: "Change process view".into(),
            fields: vec![
                choice_field(
                    "sort",
                    "Sort by",
                    sort,
                    &["pid", "cpu", "memory", "uid", "name", "nice"],
                ),
                choice_field(
                    "descending",
                    "Descending",
                    if descending { "true" } else { "false" },
                    &["true", "false"],
                ),
                choice_field(
                    "tree",
                    "Process tree",
                    if tree { "true" } else { "false" },
                    &["true", "false"],
                ),
            ],
            ..Default::default()
        }],
    })
}

fn boolean_option(
    query: &ManagementQuery,
    key: &str,
    default: bool,
) -> Result<bool, ManagementError> {
    match query.options.get(key).map(String::as_str) {
        None => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        _ => Err(ManagementError::InvalidInput(format!(
            "{key} must be true or false"
        ))),
    }
}

fn choice_field(id: &str, label: &str, value: &str, choices: &[&str]) -> ManagementField {
    ManagementField {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        required: true,
        choices: choices.iter().map(|s| (*s).into()).collect(),
        ..Default::default()
    }
}

fn row(process: &ProcessRecord, boot_id: &str, depth: usize) -> ManagementRow {
    let caller = unsafe { libc::getuid() };
    let protected = process.pid <= 1 || process.pid == std::process::id();
    let actions = [
        ("term", "Terminate (TERM)"),
        ("kill", "Force termination (KILL)"),
        ("stop", "Pause (STOP)"),
        ("cont", "Continue (CONT)"),
        ("nice", "Change main-thread nice"),
    ]
    .into_iter()
    .map(|(id, label)| ManagementAction {
        id: id.into(),
        label: label.into(),
        confirm: true,
        // Raising priority can require CAP_SYS_NICE even for the caller's own process.
        privileged: id == "nice" || process.uid != caller,
        disabled_reason: protected
            .then(|| "The init process and the management process are protected".into()),
        fields: if id == "nice" {
            vec![ManagementField {
                id: "nice".into(),
                label: "Main-thread nice (-20..19; lower is higher priority)".into(),
                value: process.nice.to_string(),
                required: true,
                ..Default::default()
            }]
        } else {
            Vec::new()
        },
    })
    .collect();
    ManagementRow {
        id: process.pid.to_string(),
        cells: vec![
            process.pid.to_string(),
            process.parent.to_string(),
            process.uid.to_string(),
            process.state.to_string(),
            process
                .cpu
                .map(|cpu| format!("{cpu:.1}"))
                .unwrap_or_else(|| "—".into()),
            format!("{:.1} MiB", process.memory as f64 / 1048576.0),
            process.nice.to_string(),
            format!(
                "{}{}",
                "  ".repeat(depth.min(32)),
                runtime_log::sanitize_text(&process.name)
            ),
        ],
        detail: vec![
            (
                "Command".into(),
                runtime_log::sanitize_text(&process.command),
            ),
            ("Threads".into(), process.threads.to_string()),
            (
                "Start time (ticks since boot)".into(),
                process.started.to_string(),
            ),
        ],
        actions,
        identity: BTreeMap::from([
            ("pid".into(), process.pid.to_string()),
            ("start_time_ticks".into(), process.started.to_string()),
            ("uid".into(), process.uid.to_string()),
            ("boot_id".into(), boot_id.into()),
        ]),
    }
}

pub fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    check_cancelled(cancelled)?;
    let signal = match command.action.as_str() {
        "term" => Some(libc::SIGTERM),
        "kill" => Some(libc::SIGKILL),
        "stop" => Some(libc::SIGSTOP),
        "cont" => Some(libc::SIGCONT),
        "nice" => None,
        _ => {
            return Err(ManagementError::InvalidInput(
                "Unknown process operation".into(),
            ));
        }
    };
    let pid = command
        .target
        .as_deref()
        .and_then(|pid| pid.parse::<u32>().ok())
        .filter(|pid| *pid > 1 && *pid <= i32::MAX as u32)
        .ok_or_else(|| {
            ManagementError::InvalidInput(
                "A process operation requires a PID greater than 1".into(),
            )
        })?;
    if pid == std::process::id() {
        return Err(ManagementError::InvalidInput(
            "The management process is protected".into(),
        ));
    }
    let actual_uid = unsafe { libc::getuid() };
    if actual_uid != 0 && actual_uid != context.actor_uid {
        return Err(ManagementError::PermissionDenied(
            "The request does not belong to the current Linux user".into(),
        ));
    }
    let nice = if signal.is_none() {
        Some(
            command
                .values
                .get("nice")
                .and_then(|value| value.parse::<i32>().ok())
                .filter(|value| (-20..=19).contains(value))
                .ok_or_else(|| {
                    ManagementError::InvalidInput("Nice must be an integer from -20 to 19".into())
                })?,
        )
    } else {
        None
    };
    let pidfd = open_pidfd(pid)?;
    let current = validate_identity(command, pid)?;
    if actual_uid != 0 && current.uid != actual_uid {
        return Err(ManagementError::PermissionDenied(
            "Managing another user's process requires temporary administrator authorization".into(),
        ));
    }
    check_cancelled(cancelled)?;
    interaction.emit(OperationEvent::Progress {
        message: format!("Applying {} to PID {pid}", command.action),
        percent: None,
    });
    if let Some(signal) = signal {
        // The descriptor stays bound to this process even if its numeric PID
        // is later reused. Never fall back to kill(pid) on old kernels.
        let result = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                pidfd.as_raw_fd(),
                signal,
                std::ptr::null::<libc::siginfo_t>(),
                0_u32,
            )
        };
        if result < 0 {
            return Err(process_error("send signal", io::Error::last_os_error()));
        }
        Ok(format!(
            "{} sent to PID {pid}",
            command.action.to_uppercase()
        ))
    } else {
        // Linux has no pidfd-based setpriority. Recheck immediately before the
        // numeric-PID syscall, keep the pidfd alive, and detect a changed target
        // afterward. The narrow kernel race cannot be claimed as eliminated.
        validate_identity(command, pid)?;
        if unsafe { libc::setpriority(libc::PRIO_PROCESS, pid, nice.unwrap()) } != 0 {
            return Err(process_error(
                "change main-thread nice",
                io::Error::last_os_error(),
            ));
        }
        validate_identity(command, pid)?;
        Ok(format!(
            "PID {pid} main-thread nice set to {}. Other threads are unchanged.",
            nice.unwrap()
        ))
    }
}

fn open_pidfd(pid: u32) -> Result<OwnedFd, ManagementError> {
    let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0_u32) };
    if descriptor < 0 {
        return Err(process_error(
            "open process handle",
            io::Error::last_os_error(),
        ));
    }
    // SAFETY: a successful syscall returned an owned descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(descriptor as i32) })
}

fn validate_identity(
    command: &ManagementCommand,
    pid: u32,
) -> Result<ProcessRecord, ManagementError> {
    let expected = |key: &str| {
        command.identity.get(key).ok_or_else(|| {
            ManagementError::InvalidInput(format!("Missing process identity field: {key}"))
        })
    };
    if expected("pid")?.parse::<u32>().ok() != Some(pid) {
        return Err(ManagementError::Conflict(
            "The selected PID changed; refresh the process list".into(),
        ));
    }
    let boot_id = fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(|e| process_error("read boot identity", e))?;
    if expected("boot_id")? != boot_id.trim() {
        return Err(ManagementError::Conflict(
            "The machine restarted; refresh the process list".into(),
        ));
    }
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    let current = read_process(Path::new("/proc"), pid, page_size.max(1) as u64)
        .map_err(|e| process_error("read selected process", e))?;
    if expected("start_time_ticks")?.parse::<u64>().ok() != Some(current.started)
        || expected("uid")?.parse::<u32>().ok() != Some(current.uid)
    {
        return Err(ManagementError::Conflict(
            "The selected process exited or its identity changed; refresh before retrying".into(),
        ));
    }
    Ok(current)
}

fn process_error(operation: &str, error: io::Error) -> ManagementError {
    match error.raw_os_error() {
        Some(libc::EPERM | libc::EACCES) => {
            ManagementError::PermissionDenied(format!("Cannot {operation}: {error}"))
        }
        Some(libc::ESRCH | libc::ENOENT) => {
            ManagementError::Conflict(format!("The process is no longer available: {error}"))
        }
        Some(libc::ENOSYS) => ManagementError::Unavailable(
            "This Linux kernel does not support safe pidfd process operations".into(),
        ),
        _ => ManagementError::Failed(format!("Cannot {operation}: {error}")),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/management/processes_tests.rs"]
mod tests;
