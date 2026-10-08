//! Extra information is read only for the selected PID and checked against start time.
use super::{ProcessRecord, check_cancelled, parse_stat};
use crate::management::{ManagementAction, ManagementError, ManagementField, ManagementRow};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::sync::atomic::AtomicBool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct IoBytes {
    read: u64,
    written: u64,
}

pub(super) fn read_io(root: &Path, pid: u32) -> io::Result<IoBytes> {
    let text = fs::read_to_string(root.join(pid.to_string()).join("io"))?;
    parse_io(&text).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Disk I/O counters are unavailable",
        )
    })
}

fn parse_io(text: &str) -> Option<IoBytes> {
    let fields = text
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name, value.trim()))
        .collect::<BTreeMap<_, _>>();
    Some(IoBytes {
        read: fields.get("read_bytes")?.parse().ok()?,
        written: fields.get("write_bytes")?.parse().ok()?,
    })
}

fn same_process(root: &Path, process: &ProcessRecord) -> Result<(), ManagementError> {
    let text =
        fs::read_to_string(root.join(process.pid.to_string()).join("stat")).map_err(|error| {
            match error.kind() {
                io::ErrorKind::PermissionDenied => ManagementError::PermissionDenied(
                    "Process details require permission; retry with authorization".into(),
                ),
                _ => ManagementError::Conflict("The process exited; refresh the list".into()),
            }
        })?;
    if parse_stat(&text)
        .is_none_or(|stat| stat.pid != process.pid || stat.started != process.started)
    {
        return Err(ManagementError::Conflict(
            "The PID now belongs to another process; refresh the list".into(),
        ));
    }
    Ok(())
}

pub(super) fn append_details(
    root: &Path,
    process: &ProcessRecord,
    row: &mut ManagementRow,
    before: Option<IoBytes>,
    elapsed: f64,
    cancelled: &AtomicBool,
) -> Result<(), ManagementError> {
    check_cancelled(cancelled)?;
    if let Err(error) = same_process(root, process) {
        row.detail
            .push(("Process details".into(), error.to_string()));
        return Ok(());
    }
    // Build a separate result and attach only after a final identity check. A reused PID
    // can otherwise mix one process's list row with another process's files or sockets.
    let mut detail = Vec::new();
    let mut actions = Vec::new();
    let directory = root.join(process.pid.to_string());
    match read_io(root, process.pid) {
        Ok(now) => {
            detail.push(("Disk bytes read".into(), now.read.to_string()));
            detail.push(("Disk bytes written".into(), now.written.to_string()));
            for (label, current, previous) in [
                ("Disk read bytes/s", now.read, before.map(|io| io.read)),
                (
                    "Disk write bytes/s",
                    now.written,
                    before.map(|io| io.written),
                ),
            ] {
                let rate = previous
                    .and_then(|old| current.checked_sub(old))
                    .filter(|_| elapsed > 0.0)
                    .map(|delta| format!("{:.0}", delta as f64 / elapsed))
                    .unwrap_or_else(|| "Unknown; wait for the next sample".into());
                detail.push((label.into(), rate));
            }
        }
        Err(error) => detail.push(("Disk I/O".into(), unknown(&error))),
    }
    let mut inodes = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut files = Vec::new();
    match fs::read_dir(directory.join("fd")) {
        Ok(entries) => {
            for (index, entry) in entries.enumerate() {
                check_cancelled(cancelled)?;
                if index >= 2048 {
                    files.push("List truncated at 2048 open descriptors".into());
                    break;
                }
                let Ok(entry) = entry else {
                    files.push("A file descriptor could not be read".into());
                    continue;
                };
                match fs::read_link(entry.path()) {
                    Ok(path) => {
                        let target = path.to_string_lossy();
                        if let Some(inode) = socket_inode(&target) {
                            inodes.insert(inode);
                        } else {
                            files.push(format!(
                                "{}: {}",
                                entry.file_name().to_string_lossy(),
                                runtime_log::sanitize_text(&target)
                            ));
                            if path.is_absolute() && !target.ends_with(" (deleted)") {
                                paths.insert(target.into_owned());
                            }
                        }
                    }
                    Err(error) => files.push(format!(
                        "{}: {}",
                        entry.file_name().to_string_lossy(),
                        unknown(&error)
                    )),
                }
            }
            detail.push((
                "Open files".into(),
                if files.is_empty() {
                    "No file descriptors found".into()
                } else {
                    files.join("\n")
                },
            ));
            if !paths.is_empty() {
                let choices = paths.into_iter().collect::<Vec<_>>();
                actions.push(ManagementAction {
                    id: "open_path".into(),
                    label: "Locate open file".into(),
                    group: "inspect".into(),
                    fields: vec![ManagementField {
                        id: "path".into(),
                        label: "Open file".into(),
                        value: choices[0].clone(),
                        choices,
                        required: true,
                        ..Default::default()
                    }],
                    ..Default::default()
                });
            }
            let mut ports = Vec::new();
            let mut identified = BTreeSet::new();
            for (name, ipv6) in [
                ("tcp", false),
                ("tcp6", true),
                ("udp", false),
                ("udp6", true),
            ] {
                check_cancelled(cancelled)?;
                match fs::read_to_string(directory.join("net").join(name)) {
                    Ok(text) => {
                        identified.extend(table_socket_inodes(&text, 9));
                        ports.extend(owned_ports(&text, name, ipv6, &inodes));
                    }
                    Err(error) => ports.push(format!("{name}: {}", unknown(&error))),
                }
            }
            // Unix sockets have no TCP/UDP port. Other unmatched sockets can
            // belong to another network namespace, so absence is not a zero.
            if let Ok(text) = fs::read_to_string(directory.join("net/unix")) {
                identified.extend(table_socket_inodes(&text, 6));
            }
            let unmatched = inodes.difference(&identified).count();
            if unmatched > 0 {
                ports.push(format!(
                    "{unmatched} socket(s): protocol or network namespace unknown"
                ));
            }
            detail.push((
                "TCP/UDP ports".into(),
                if ports.is_empty() {
                    "No TCP/UDP sockets found".into()
                } else {
                    ports.join("\n")
                },
            ));
        }
        Err(error) => {
            detail.push(("Open files".into(), unknown(&error)));
            detail.push((
                "TCP/UDP ports".into(),
                "Unknown; cannot read this process's file descriptors".into(),
            ));
        }
    }
    match fs::read_to_string(directory.join("cgroup")) {
        Ok(text) => {
            if let Some((unit, scope)) = cgroup_service(&text) {
                detail.push(("Service".into(), format!("{unit} ({scope})")));
                actions.push(ManagementAction {
                    id: "open_service".into(),
                    label: "Open service".into(),
                    group: "inspect".into(),
                    values: BTreeMap::from([
                        ("unit".into(), unit.clone()),
                        ("scope".into(), scope.into()),
                    ]),
                    ..Default::default()
                });
                actions.push(service_log_action(
                    unit,
                    scope,
                    row.identity.get("boot_id").cloned().unwrap_or_default(),
                ));
            } else {
                detail.push((
                    "Service".into(),
                    "Unknown; no service unit appears in the process cgroup".into(),
                ));
            }
        }
        Err(error) => detail.push(("Service".into(), unknown(&error))),
    }
    check_cancelled(cancelled)?;
    if let Err(error) = same_process(root, process) {
        row.detail
            .push(("Process details".into(), error.to_string()));
        return Ok(());
    }
    row.detail.extend(detail);
    row.actions.extend(actions);
    Ok(())
}

fn service_log_action(unit: String, scope: &str, boot_id: String) -> ManagementAction {
    ManagementAction {
        id: "view_logs".into(),
        label: "View service logs".into(),
        group: "inspect".into(),
        values: BTreeMap::from([
            ("unit".into(), unit),
            ("scope".into(), scope.into()),
            ("boot_id".into(), boot_id),
        ]),
        ..Default::default()
    }
}

fn table_socket_inodes(text: &str, column: usize) -> BTreeSet<u64> {
    text.lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().nth(column)?.parse().ok())
        .collect()
}

fn unknown(error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::PermissionDenied => "Unknown; permission is required to read details".into(),
        io::ErrorKind::NotFound => "Unknown; process or descriptor exited".into(),
        _ => format!("Unknown: {error}"),
    }
}

fn socket_inode(target: &str) -> Option<u64> {
    target
        .strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

fn owned_ports(text: &str, protocol: &str, ipv6: bool, owned: &BTreeSet<u64>) -> Vec<String> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if !owned.contains(&fields.get(9)?.parse().ok()?) {
                return None;
            }
            let local = endpoint(fields.get(1)?, ipv6)?;
            let remote = endpoint(fields.get(2)?, ipv6)?;
            let state = match *fields.get(3)? {
                "0A" => "listening",
                "01" => "established",
                "07" if protocol.starts_with("udp") => "bound",
                value => value,
            };
            Some(format!("{protocol} {local} -> {remote} ({state})"))
        })
        .collect()
}

fn endpoint(value: &str, ipv6: bool) -> Option<String> {
    let (address, port) = value.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    if ipv6 {
        if address.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for (index, word) in address.as_bytes().chunks_exact(8).enumerate() {
            bytes[index * 4..index * 4 + 4].copy_from_slice(
                &u32::from_str_radix(std::str::from_utf8(word).ok()?, 16)
                    .ok()?
                    .to_le_bytes(),
            );
        }
        Some(format!("[{}]:{port}", Ipv6Addr::from(bytes)))
    } else {
        Some(format!(
            "{}:{port}",
            Ipv4Addr::from(u32::from_str_radix(address, 16).ok()?.to_le_bytes())
        ))
    }
}

fn cgroup_service(text: &str) -> Option<(String, &'static str)> {
    let mut found = None;
    for line in text.lines() {
        let mut fields = line.splitn(3, ':');
        let hierarchy = fields.next()?;
        let controllers = fields.next()?;
        let path = fields.next()?;
        if hierarchy != "0"
            && !controllers
                .split(',')
                .any(|controller| controller == "name=systemd")
        {
            continue;
        }
        let user_scope = path
            .split('/')
            .any(|part| part.starts_with("user@") && part.ends_with(".service"));
        // Choose the innermost service. user@UID.service itself remains a system service.
        let unit = path
            .split('/')
            .rev()
            .find(|part| crate::management::services::valid_unit_name(part))?;
        found = Some((
            unit.to_string(),
            if user_scope && !unit.starts_with("user@") {
                "user"
            } else {
                "system"
            },
        ));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn service_log_jump_uses_the_log_viewer_unit_and_boot_fields() {
        let action = service_log_action("demo.service".into(), "user", "selected-boot".into());
        assert_eq!(
            action.values.get("unit").map(String::as_str),
            Some("demo.service")
        );
        assert_eq!(
            action.values.get("boot_id").map(String::as_str),
            Some("selected-boot")
        );
        assert_eq!(action.values.get("scope").map(String::as_str), Some("user"));
        assert!(!action.privileged);
    }
    #[test]
    fn only_sockets_owned_by_the_selected_process_are_reported() {
        let text = "header\n 0: 0100007F:1F90 00000000:0000 0A 0:0 0:0 0 1000 0 42\n 1: 00000000:0016 00000000:0000 0A 0:0 0:0 0 1000 0 99\n";
        let result = owned_ports(text, "tcp", false, &BTreeSet::from([42]));
        assert_eq!(result, ["tcp 127.0.0.1:8080 -> 0.0.0.0:0 (listening)"]);
        assert_eq!(
            endpoint("00000000000000000000000001000000:0016", true),
            Some("[::1]:22".into())
        );
    }
    #[test]
    fn io_uses_disk_counters_and_never_fabricates_missing_zeroes() {
        assert_eq!(
            parse_io("rchar: 9000\nread_bytes: 2048\nwrite_bytes: 4096\n"),
            Some(IoBytes {
                read: 2048,
                written: 4096
            })
        );
        assert!(parse_io("rchar: 9000\n").is_none());
        assert!(parse_io("read_bytes: invalid\nwrite_bytes: 0\n").is_none());
    }
    #[test]
    fn user_cgroup_selects_leaf_service_and_ignores_scope_and_unrelated_controllers() {
        assert_eq!(
            cgroup_service(
                "0::/user.slice/user-1000.slice/user@1000.service/app.slice/demo.service"
            ),
            Some(("demo.service".into(), "user"))
        );
        assert_eq!(
            cgroup_service("0::/system.slice/sshd.service/workers"),
            Some(("sshd.service".into(), "system"))
        );
        assert_eq!(
            cgroup_service("1:cpu:/not-real.service\n0::/user.slice/session-1.scope"),
            None
        );
    }
    #[test]
    fn live_selected_process_details_include_real_owned_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let pid = std::process::id();
        let process = super::super::read_process(Path::new("/proc"), pid, 4096).unwrap();
        let mut row = super::super::row(&process, "boot", 0);
        append_details(
            Path::new("/proc"),
            &process,
            &mut row,
            None,
            0.1,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(
            row.detail.iter().any(|(key, value)| key == "TCP/UDP ports"
                && value.contains(&format!("127.0.0.1:{port}")))
        );
        assert!(row.detail.iter().any(|(key, _)| key == "Disk bytes read"));
        let mut stale = process;
        stale.started += 1;
        let mut row = super::super::row(&stale, "boot", 0);
        append_details(
            Path::new("/proc"),
            &stale,
            &mut row,
            None,
            0.1,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(!row.detail.iter().any(|(key, _)| key == "Open files"));
    }
}
