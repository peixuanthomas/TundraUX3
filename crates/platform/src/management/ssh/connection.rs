use super::*;
use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Process {
    parent: u32,
    start_ticks: u64,
}
fn parse_stat(text: &str) -> Result<Process, ManagementError> {
    let tail = text
        .rsplit_once(") ")
        .ok_or_else(|| invalid(&i18n::tr!("platform-ssh-invalid-process-information")))?
        .1;
    let fields: Vec<_> = tail.split_whitespace().collect();
    Ok(Process {
        parent: fields
            .get(1)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| invalid(&i18n::tr!("platform-ssh-missing-process-parent")))?,
        start_ticks: fields
            .get(19)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| invalid(&i18n::tr!("platform-ssh-missing-process-creation-time")))?,
    })
}
fn process(pid: u32) -> Result<Process, ManagementError> {
    parse_stat(&fs::read_to_string(format!("/proc/{pid}/stat")).map_err(failure)?)
}
#[cfg(test)]
pub(super) fn start_ticks(pid: u32) -> Result<u64, ManagementError> {
    Ok(process(pid)?.start_ticks)
}
pub(super) fn process_matches(pid: u32, start_ticks: u64) -> bool {
    process(pid).is_ok_and(|process| process.start_ticks == start_ticks)
}
pub(super) fn ticks() -> Result<u64, ManagementError> {
    let mut time = std::mem::MaybeUninit::<libc::timespec>::zeroed();
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, time.as_mut_ptr()) } != 0 {
        return Err(failure(std::io::Error::last_os_error()));
    }
    let time = unsafe { time.assume_init() };
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if hz <= 0 {
        return Err(invalid(&i18n::tr!(
            "platform-ssh-cannot-read-kernel-clock-ticks"
        )));
    }
    Ok(time.tv_sec as u64 * hz as u64 + time.tv_nsec as u64 * hz as u64 / 1_000_000_000)
}
pub(super) fn trusted_sshd(pid: u32) -> bool {
    let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) else {
        return false;
    };
    let root = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .is_some_and(|line| line.split_whitespace().nth(1) == Some("0"));
    let Ok(exe) = fs::read_link(format!("/proc/{pid}/exe")) else {
        return false;
    };
    let Ok(meta) = fs::metadata(&exe) else {
        return false;
    };
    root && meta.uid() == 0
        && meta.mode() & 0o022 == 0
        && matches!(
            exe.file_name().and_then(|s| s.to_str()),
            Some("sshd" | "sshd-session" | "sshd-auth")
        )
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Tcp {
    port: u16,
    inode: u64,
    established: bool,
}
fn parse_tcp(text: &str) -> Vec<Tcp> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let columns: Vec<_> = line.split_whitespace().collect();
            let port = u16::from_str_radix(columns.get(1)?.rsplit_once(':')?.1, 16).ok()?;
            let state = *columns.get(3)?;
            if !matches!(state, "01" | "0A") {
                return None;
            }
            Some(Tcp {
                port,
                inode: columns.get(9)?.parse().ok()?,
                established: state == "01",
            })
        })
        .collect()
}
fn tcp() -> Result<Vec<Tcp>, ManagementError> {
    let mut data = Vec::new();
    for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
        match fs::read_to_string(path) {
            Ok(text) => data.extend(parse_tcp(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(failure(e)),
        }
    }
    Ok(data)
}
fn sockets(pid: u32) -> Result<BTreeSet<u64>, ManagementError> {
    let mut result = BTreeSet::new();
    for entry in fs::read_dir(format!("/proc/{pid}/fd")).map_err(failure)? {
        let entry = entry.map_err(failure)?;
        let Ok(path) = fs::read_link(entry.path()) else {
            continue;
        };
        if let Some(inode) = path
            .to_str()
            .and_then(|s| s.strip_prefix("socket:["))
            .and_then(|s| s.strip_suffix(']'))
            .and_then(|s| s.parse().ok())
        {
            result.insert(inode);
        }
    }
    Ok(result)
}
pub(super) fn listener_inodes(pid: u32) -> Result<BTreeSet<u64>, ManagementError> {
    let owned = sockets(pid)?;
    Ok(tcp()?
        .into_iter()
        .filter(|tcp| !tcp.established && owned.contains(&tcp.inode))
        .map(|tcp| tcp.inode)
        .collect())
}
pub(super) fn listening_ports(pid: u32) -> Result<BTreeSet<u16>, ManagementError> {
    let owned = sockets(pid)?;
    Ok(tcp()?
        .into_iter()
        .filter(|tcp| !tcp.established && owned.contains(&tcp.inode))
        .map(|tcp| tcp.port)
        .collect())
}
fn new_connection(
    process_start: u64,
    applied_ticks: u64,
    port: u16,
    ports: &[u16],
    established: bool,
) -> bool {
    process_start > applied_ticks && established && ports.contains(&port)
}

pub(super) fn verify_new_connection(
    context: &ExecutionContext,
    applied_ticks: u64,
    ports: &[u16],
) -> Result<(), ManagementError> {
    let initial = process(context.caller_pid)?;
    if initial.start_ticks != context.caller_start_ticks {
        return Err(ManagementError::Conflict(
            i18n::tr!("platform-ssh-the-confirmation-caller-process-changed").into(),
        ));
    }
    let connections = tcp()?;
    let mut pid = context.caller_pid;
    let mut seen = BTreeSet::new();
    while pid > 1 && seen.insert(pid) && seen.len() <= 128 {
        let before = process(pid)?;
        if trusted_sshd(pid) && before.start_ticks > applied_ticks {
            let owned = sockets(pid)?;
            if connections.iter().any(|tcp| {
                owned.contains(&tcp.inode)
                    && new_connection(
                        before.start_ticks,
                        applied_ticks,
                        tcp.port,
                        ports,
                        tcp.established,
                    )
            }) {
                if process(pid)? != before
                    || process(context.caller_pid)?.start_ticks != context.caller_start_ticks
                {
                    return Err(ManagementError::Conflict(
                        i18n::tr!("platform-ssh-the-ssh-connection-changed-during-confirmation")
                            .into(),
                    ));
                }
                return Ok(());
            }
        }
        pid = before.parent;
    }
    Err(ManagementError::PermissionDenied(
        i18n::tr!("platform-ssh-confirm-from-a-new-ssh-connection-to-the-new-port").into(),
    ))
}

#[cfg(test)]
#[path = "../../../tests/unit/management/ssh_connection_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../tests/unit/management/ssh_connection_isolated.rs"]
mod real_tests;
