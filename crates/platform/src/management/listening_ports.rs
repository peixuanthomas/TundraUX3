//! TCP listeners and unconnected UDP sockets in the current network namespace.
use super::{ManagementError, ManagementRow, network::check_cancelled};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::sync::atomic::AtomicBool;

pub(crate) fn socket_inode(target: &str) -> Option<u64> {
    target
        .strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

pub(crate) fn endpoint(value: &str, ipv6: bool) -> Option<String> {
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
                    .to_ne_bytes(),
            );
        }
        Some(format!("[{}]:{port}", Ipv6Addr::from(bytes)))
    } else {
        Some(format!(
            "{}:{port}",
            Ipv4Addr::from(u32::from_str_radix(address, 16).ok()?.to_ne_bytes())
        ))
    }
}

#[derive(Debug)]
struct Listener {
    protocol: String,
    family: &'static str,
    address: String,
    port: u16,
    inode: u64,
}

fn parse_table(text: &str, protocol: &str, ipv6: bool) -> Vec<Listener> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            let state = *fields.get(3)?;
            if (protocol == "tcp" && state != "0A") || (protocol == "udp" && state != "07") {
                return None;
            }
            let local = fields.get(1)?;
            let address = endpoint(local, ipv6)?;
            let port = u16::from_str_radix(local.split_once(':')?.1, 16).ok()?;
            Some(Listener {
                protocol: protocol.into(),
                family: if ipv6 { "IPv6" } else { "IPv4" },
                address,
                port,
                inode: fields.get(9)?.parse().ok()?,
            })
        })
        .collect()
}

pub(crate) fn query(
    root: &Path,
    filter: &str,
    cancelled: &AtomicBool,
) -> Result<(Vec<ManagementRow>, Vec<String>), ManagementError> {
    let mut sockets = Vec::new();
    let mut notices = vec![i18n::tr!("platform-firewall-listener-scope").into()];
    for (table, protocol, ipv6) in [
        ("tcp", "tcp", false),
        ("tcp6", "tcp", true),
        ("udp", "udp", false),
        ("udp6", "udp", true),
    ] {
        check_cancelled(cancelled)?;
        match fs::read_to_string(root.join("net").join(table)) {
            Ok(text) => sockets.extend(parse_table(&text, protocol, ipv6)),
            Err(error) => notices.push(i18n::tr!(
                "platform-firewall-listener-read-error",
                table = table.to_string(),
                error = error.to_string()
            )),
        }
    }
    let wanted = sockets
        .iter()
        .map(|socket| socket.inode)
        .collect::<BTreeSet<_>>();
    let mut owners = BTreeMap::<u64, BTreeSet<String>>::new();
    let mut denied = false;
    for entry in fs::read_dir(root).map_err(|error| ManagementError::Failed(error.to_string()))? {
        check_cancelled(cancelled)?;
        let Ok(entry) = entry else {
            continue;
        };
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let directory = entry.path();
        let Ok(before) = fs::read_to_string(directory.join("stat")) else {
            continue;
        };
        let Ok(descriptors) = fs::read_dir(directory.join("fd")) else {
            denied = true;
            continue;
        };
        let mut held = BTreeSet::new();
        for descriptor in descriptors.flatten() {
            if let Ok(target) = fs::read_link(descriptor.path()) {
                if let Some(inode) =
                    socket_inode(&target.to_string_lossy()).filter(|inode| wanted.contains(inode))
                {
                    held.insert(inode);
                }
            }
        }
        if held.is_empty() {
            continue;
        }
        let name = fs::read_to_string(directory.join("comm"))
            .unwrap_or_else(|_| i18n::tr!("platform-firewall-unknown").into())
            .trim()
            .to_string();
        let Ok(after) = fs::read_to_string(directory.join("stat")) else {
            continue;
        };
        // The counters change while reading; only PID and start time identify the owner.
        if process_identity(&before) != process_identity(&after)
            || process_identity(&before).is_none()
        {
            continue;
        }
        for inode in held {
            owners
                .entry(inode)
                .or_default()
                .insert(format!("{pid} / {name}"));
        }
    }
    if denied {
        notices.push(i18n::tr!("platform-firewall-listener-permission").into());
    }
    sockets.sort_by(|a, b| {
        (&a.protocol, a.port, a.family, &a.address, a.inode).cmp(&(
            &b.protocol,
            b.port,
            b.family,
            &b.address,
            b.inode,
        ))
    });
    let rows = sockets
        .into_iter()
        .map(|socket| {
            let owner = owners
                .get(&socket.inode)
                .map(|values| values.iter().cloned().collect::<Vec<_>>().join(", "))
                .unwrap_or_else(|| i18n::tr!("platform-firewall-unknown-owner").into());
            ManagementRow {
                id: format!("{}:{}:{}", socket.protocol, socket.family, socket.inode),
                cells: vec![socket.protocol, socket.family.into(), socket.address, owner],
                detail: vec![
                    (
                        i18n::tr!("platform-firewall-port").into(),
                        socket.port.to_string(),
                    ),
                    (
                        i18n::tr!("platform-firewall-inode").into(),
                        socket.inode.to_string(),
                    ),
                ],
                ..Default::default()
            }
        })
        .filter(|row| {
            filter.is_empty()
                || row
                    .cells
                    .iter()
                    .any(|cell| cell.to_lowercase().contains(&filter.to_lowercase()))
        })
        .collect();
    Ok((rows, notices))
}

fn process_identity(text: &str) -> Option<(&str, &str)> {
    Some((
        text.split_once('(')?.0.trim(),
        text.get(text.rfind(')')? + 1..)?
            .split_whitespace()
            .nth(19)?,
    ))
}

#[cfg(test)]
#[path = "../../tests/unit/management/listening_ports_tests.rs"]
mod tests;
