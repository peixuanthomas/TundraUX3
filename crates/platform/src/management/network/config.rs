use super::*;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value as Yaml};
use std::fs;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum OwnerKind {
    NetworkManager,
    Netplan,
    Networkd,
    Interfaces,
    Unknown,
}

#[derive(Debug, Clone)]
pub(super) struct Owner {
    pub kind: OwnerKind,
    pub path: Option<PathBuf>,
    pub id: String,
    pub uuid: String,
    pub reason: Option<String>,
}

impl Owner {
    pub fn label(&self) -> &'static str {
        match self.kind {
            OwnerKind::NetworkManager => "NetworkManager",
            OwnerKind::Netplan => "Netplan",
            OwnerKind::Networkd => "systemd-networkd",
            OwnerKind::Interfaces => "Debian interfaces",
            OwnerKind::Unknown => "Unknown / conflicting",
        }
    }
    pub fn source_display(&self) -> String {
        self.path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    }
    pub fn populate_fields(&self, fields: &mut [ManagementField]) {
        if self.kind == OwnerKind::NetworkManager && !self.uuid.is_empty() {
            let mappings = [
                ("ipv4", "ipv4.method"),
                ("address4", "ipv4.addresses"),
                ("gateway4", "ipv4.gateway"),
                ("ipv6", "ipv6.method"),
                ("address6", "ipv6.addresses"),
                ("gateway6", "ipv6.gateway"),
            ];
            for (id, property) in mappings {
                if let Ok(value) = run(
                    "nmcli",
                    &[
                        "--escape",
                        "no",
                        "--get-values",
                        property,
                        "connection",
                        "show",
                        "uuid",
                        &self.uuid,
                    ],
                    &AtomicBool::new(false),
                ) {
                    if let Some(field) = fields.iter_mut().find(|f| f.id == id) {
                        field.value = match (id, value.as_str()) {
                            ("ipv4", "auto") => "dhcp".into(),
                            ("ipv4" | "ipv6", "manual") => "static".into(),
                            ("ipv6", "ignore") => "disabled".into(),
                            _ => value,
                        };
                    }
                }
            }
            let dns = ["ipv4.dns", "ipv6.dns"]
                .iter()
                .filter_map(|p| {
                    run(
                        "nmcli",
                        &[
                            "--escape",
                            "no",
                            "--get-values",
                            p,
                            "connection",
                            "show",
                            "uuid",
                            &self.uuid,
                        ],
                        &AtomicBool::new(false),
                    )
                    .ok()
                })
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(",");
            if let Some(field) = fields.iter_mut().find(|f| f.id == "dns") {
                field.value = dns;
            }
        } else if let Some(path) = &self.path {
            if let Ok(text) = fs::read_to_string(path) {
                let values = match self.kind {
                    OwnerKind::Netplan => netplan_values(&text, &self.id),
                    OwnerKind::Networkd => networkd_values(&text),
                    OwnerKind::Interfaces => interfaces_values(&text, &self.id),
                    _ => BTreeMap::new(),
                };
                for field in fields {
                    if let Some(value) = values.get(&field.id) {
                        field.value = value.clone();
                    }
                }
            }
        }
    }
}

fn netplan_values(text: &str, id: &str) -> BTreeMap<String, String> {
    let Ok(yaml) = serde_yaml::from_str::<Yaml>(text) else {
        return BTreeMap::new();
    };
    let definition = &yaml["network"]["ethernets"][id];
    let addresses = definition["addresses"]
        .as_sequence()
        .into_iter()
        .flatten()
        .filter_map(Yaml::as_str)
        .collect::<Vec<_>>();
    let a4 = addresses
        .iter()
        .filter(|s| {
            s.split('/')
                .next()
                .is_some_and(|s| s.parse::<Ipv4Addr>().is_ok())
        })
        .copied()
        .collect::<Vec<_>>()
        .join(",");
    let a6 = addresses
        .iter()
        .filter(|s| {
            s.split('/')
                .next()
                .is_some_and(|s| s.parse::<Ipv6Addr>().is_ok())
        })
        .copied()
        .collect::<Vec<_>>()
        .join(",");
    let mut gateway4 = definition["gateway4"].as_str().unwrap_or("").to_string();
    let mut gateway6 = definition["gateway6"].as_str().unwrap_or("").to_string();
    for route in definition["routes"].as_sequence().into_iter().flatten() {
        if !matches!(route["to"].as_str(), Some("default" | "0.0.0.0/0" | "::/0")) {
            continue;
        }
        if let Some(gateway) = route["via"].as_str() {
            if gateway.parse::<Ipv4Addr>().is_ok() {
                gateway4 = gateway.into();
            } else if gateway.parse::<Ipv6Addr>().is_ok() {
                gateway6 = gateway.into();
            }
        }
    }
    let ipv4 = if definition["dhcp4"].as_bool() == Some(true) {
        "dhcp"
    } else if !a4.is_empty() {
        "static"
    } else {
        "disabled"
    };
    let ipv6 = if definition["dhcp6"].as_bool() == Some(true) {
        "dhcp"
    } else if !a6.is_empty() {
        "static"
    } else if definition["accept-ra"].as_bool() == Some(false) {
        "disabled"
    } else {
        "auto"
    };
    let dns = definition["nameservers"]["addresses"]
        .as_sequence()
        .into_iter()
        .flatten()
        .filter_map(Yaml::as_str)
        .collect::<Vec<_>>()
        .join(",");
    BTreeMap::from([
        ("ipv4".into(), ipv4.into()),
        ("ipv6".into(), ipv6.into()),
        ("address4".into(), a4),
        ("address6".into(), a6),
        ("gateway4".into(), gateway4),
        ("gateway6".into(), gateway6),
        ("dns".into(), dns),
    ])
}

fn networkd_values(text: &str) -> BTreeMap<String, String> {
    let mut a4 = Vec::new();
    let mut a6 = Vec::new();
    let mut dns = Vec::new();
    let mut gateway4 = String::new();
    let mut gateway6 = String::new();
    let mut dhcp = "no";
    let mut ra = "yes";
    for line in text.lines().map(str::trim) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "Address" => {
                if value
                    .split('/')
                    .next()
                    .is_some_and(|s| s.parse::<Ipv4Addr>().is_ok())
                {
                    a4.push(value);
                } else {
                    a6.push(value);
                }
            }
            "Gateway" => {
                if value.parse::<Ipv4Addr>().is_ok() {
                    gateway4 = value.into();
                } else {
                    gateway6 = value.into();
                }
            }
            "DNS" => dns.extend(value.split_whitespace()),
            "DHCP" => dhcp = value,
            "IPv6AcceptRA" => ra = value,
            _ => {}
        }
    }
    BTreeMap::from([
        (
            "ipv4".into(),
            if matches!(dhcp, "yes" | "true" | "ipv4") {
                "dhcp"
            } else if !a4.is_empty() {
                "static"
            } else {
                "disabled"
            }
            .into(),
        ),
        (
            "ipv6".into(),
            if matches!(dhcp, "yes" | "true" | "ipv6") {
                "dhcp"
            } else if !a6.is_empty() {
                "static"
            } else if matches!(ra, "no" | "false") {
                "disabled"
            } else {
                "auto"
            }
            .into(),
        ),
        ("address4".into(), a4.join(",")),
        ("address6".into(), a6.join(",")),
        ("gateway4".into(), gateway4),
        ("gateway6".into(), gateway6),
        ("dns".into(), dns.join(",")),
    ])
}

fn interfaces_values(text: &str, name: &str) -> BTreeMap<String, String> {
    let mut result = BTreeMap::<String, String>::new();
    let mut family = "";
    let mut netmask = None;
    for line in text.lines() {
        let p = line
            .split('#')
            .next()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>();
        if p.is_empty() {
            continue;
        }
        if p[0] == "iface" {
            family = if p.len() == 4 && p[1] == name {
                if p[2] == "inet" { "4" } else { "6" }
            } else {
                ""
            };
            if !family.is_empty() {
                result.insert(
                    format!("ipv{family}"),
                    if p[3] == "manual" { "disabled" } else { p[3] }.into(),
                );
            }
        } else if matches!(
            p[0],
            "auto" | "allow-hotplug" | "source" | "source-directory" | "mapping"
        ) {
            family = "";
        } else if !family.is_empty() && p.len() >= 2 {
            match p[0] {
                "address" => {
                    result.insert(format!("address{family}"), p[1].into());
                }
                "gateway" => {
                    result.insert(format!("gateway{family}"), p[1].into());
                }
                "netmask" if family == "4" => netmask = p[1].parse::<Ipv4Addr>().ok(),
                "dns-nameservers" => {
                    result.insert("dns".into(), p[1..].join(","));
                }
                _ => {}
            }
        }
    }
    if let Some(address) = result.get_mut("address4") {
        if !address.contains('/') {
            if let Some(netmask) = netmask {
                address.push_str(&format!("/{}", u32::from(netmask).count_ones()));
            }
        }
    }
    result
}

fn readonly(
    kind: OwnerKind,
    path: Option<PathBuf>,
    id: String,
    reason: impl Into<String>,
) -> Owner {
    Owner {
        kind,
        path,
        id,
        uuid: String::new(),
        reason: Some(reason.into()),
    }
}

pub(super) fn detect(name: &str) -> Owner {
    if validate_interface(name).is_err() {
        return readonly(
            OwnerKind::Unknown,
            None,
            String::new(),
            "Invalid interface name",
        );
    }
    let nm = nm_device(name).ok().filter(|d| d.managed);
    let netplan = netplan_owner(name);
    let interfaces = interfaces_owner(name);
    let networkd = networkd_source(name);
    // A generated renderer file is evidence of Netplan ownership, not permission to edit it.
    let generated = nm
        .as_ref()
        .and_then(|d| d.filename.as_ref())
        .into_iter()
        .chain(networkd.iter())
        .any(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().contains("netplan"))
        });
    if let Some(mut owner) = netplan {
        if interfaces.is_some() {
            owner.reason = Some("Both Netplan and Debian interfaces define this device; resolve ownership outside Tundra first".into());
        }
        return owner;
    }
    if generated {
        return readonly(
            OwnerKind::Netplan,
            None,
            String::new(),
            "A Netplan-generated profile exists, but its original YAML definition cannot be identified exactly",
        );
    }
    if let Some(owner) = interfaces {
        if nm.is_some() || networkd.is_some() {
            return readonly(
                OwnerKind::Unknown,
                owner.path,
                name.into(),
                "More than one network manager claims this device",
            );
        }
        return owner;
    }
    if let Some(device) = nm {
        if networkd.is_some() {
            return readonly(
                OwnerKind::Unknown,
                None,
                name.into(),
                "Both NetworkManager and networkd claim this device",
            );
        }
        let reason = match device.filename.as_deref() {
            Some(p) if native_nm_file(p) => None,
            Some(_) => Some("NetworkManager's profile is generated or belongs to another configuration source".into()),
            None if device.uuid.is_empty() => None,
            None => Some("The active NetworkManager profile has no persistent source; only its owner can safely replace it".into()),
        };
        return Owner {
            kind: OwnerKind::NetworkManager,
            path: device.filename,
            id: name.into(),
            uuid: device.uuid,
            reason,
        };
    }
    if let Some(path) = networkd {
        let reason = if !path.starts_with("/etc/systemd/network") {
            Some(
                "networkd uses a generated or vendor-owned file; Tundra does not take over it"
                    .into(),
            )
        } else if has_networkd_dropins(&path) {
            Some("networkd drop-in files affect this interface; editing the main file alone is ambiguous".into())
        } else {
            fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|s| simple_networkd(&s, name))
                .err()
        };
        return Owner {
            kind: OwnerKind::Networkd,
            path: Some(path),
            id: name.into(),
            uuid: String::new(),
            reason,
        };
    }
    readonly(
        OwnerKind::Unknown,
        None,
        name.into(),
        "No existing configuration owner can be established; Tundra will not enable a different network manager",
    )
}

pub(super) fn native_nm_file(path: &Path) -> bool {
    path.parent() == Some(Path::new("/etc/NetworkManager/system-connections"))
        || (path.parent() == Some(Path::new("/etc/sysconfig/network-scripts"))
            && path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("ifcfg-")))
}

fn networkd_source(name: &str) -> Option<PathBuf> {
    let index = fs::read_to_string(format!("/sys/class/net/{name}/ifindex")).ok()?;
    let link = fs::read_to_string(format!("/run/systemd/netif/links/{}", index.trim())).ok()?;
    link.lines()
        .find_map(|l| l.strip_prefix("NETWORK_FILE="))
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn has_networkd_dropins(path: &Path) -> bool {
    let Some(base) = path.file_name() else {
        return true;
    };
    [
        "/etc/systemd/network",
        "/run/systemd/network",
        "/usr/lib/systemd/network",
    ]
    .iter()
    .any(|dir| {
        let sub = Path::new(dir).join(format!("{}.d", base.to_string_lossy()));
        fs::read_dir(sub).is_ok_and(|mut d| d.next().is_some())
    })
}

fn simple_networkd(text: &str, interface: &str) -> Result<(), String> {
    let mut section = "";
    let mut exact_match = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = &line[1..line.len() - 1];
            if !matches!(section, "Match" | "Network") {
                return Err(format!(
                    "The [{section}] section needs a more detailed network editor"
                ));
            }
            continue;
        }
        let (key, value) = line.split_once('=').ok_or("Invalid networkd directive")?;
        match (section, key.trim()) {
            ("Match", "Name") if value.trim() == interface => exact_match = true,
            (
                "Network",
                "DHCP" | "Address" | "Gateway" | "DNS" | "IPv6AcceptRA" | "LinkLocalAddressing",
            ) => {}
            _ => {
                return Err(format!(
                    "networkd directive {} needs a more detailed network editor",
                    key.trim()
                ));
            }
        }
    }
    if !exact_match {
        return Err("networkd's match does not select exactly this interface".into());
    }
    Ok(())
}

fn yaml_key(s: &str) -> Yaml {
    Yaml::String(s.into())
}

fn netplan_owner(name: &str) -> Option<Owner> {
    let mut found = Vec::new();
    let mac = fs::read_to_string(format!("/sys/class/net/{name}/address")).unwrap_or_default();
    let mut unreadable = false;
    for root in ["/lib/netplan", "/etc/netplan", "/run/netplan"] {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path
                .extension()
                .is_some_and(|e| matches!(e.to_str(), Some("yaml" | "yml")))
            {
                continue;
            }
            let text = match fs::read_to_string(&path) {
                Ok(t) => t,
                Err(_) => {
                    unreadable = true;
                    continue;
                }
            };
            let yaml: Yaml = match serde_yaml::from_str(&text) {
                Ok(v) => v,
                Err(_) => {
                    unreadable = true;
                    continue;
                }
            };
            for category in ["ethernets", "wifis", "bridges", "bonds", "vlans"] {
                let Some(definitions) = yaml["network"][category].as_mapping() else {
                    continue;
                };
                for (key, definition) in definitions {
                    let Some(id) = key.as_str() else { continue };
                    let match_name = definition["match"]["name"].as_str();
                    let match_mac = definition["match"]["macaddress"].as_str();
                    let set_name = definition["set-name"].as_str();
                    let matches = set_name == Some(name)
                        || (set_name.is_none()
                            && (id == name && definition["match"].is_null()
                                || match_name == Some(name)
                                || match_mac.is_some_and(|m| m.eq_ignore_ascii_case(mac.trim()))));
                    if !matches {
                        continue;
                    }
                    let reason = if category != "ethernets" {
                        Some("This Netplan device is not a plain Ethernet definition; personal Wi-Fi editing is available for native NetworkManager only".into())
                    } else if root != "/etc/netplan" {
                        Some("The definition is supplied by a vendor or runtime generator".into())
                    } else if text.contains("cloud-init") || text.contains("will not persist") {
                        Some("This Netplan file is generated by cloud-init; edit its owning source outside Tundra".into())
                    } else {
                        None
                    };
                    found.push(Owner {
                        kind: OwnerKind::Netplan,
                        path: Some(path.clone()),
                        id: id.into(),
                        uuid: String::new(),
                        reason,
                    });
                }
            }
        }
    }
    if found.len() > 1 {
        return Some(readonly(
            OwnerKind::Netplan,
            None,
            name.into(),
            "Several Netplan definitions match this device; merged ownership cannot be edited safely",
        ));
    }
    let mut owner = found.pop()?;
    if unreadable {
        owner.reason = Some("Some Netplan files could not be read or parsed, so configuration ownership is incomplete".into());
    }
    Some(owner)
}

fn interfaces_files() -> (Vec<PathBuf>, Option<String>) {
    let root = PathBuf::from("/etc/network/interfaces");
    if !root.exists() {
        return (Vec::new(), None);
    }
    let mut files = vec![root];
    let mut position = 0;
    let mut reason = None;
    while position < files.len() {
        let text = match fs::read_to_string(&files[position]) {
            Ok(t) => t,
            Err(_) => {
                reason = Some("Some included interfaces files cannot be read; configuration ownership is incomplete".into());
                position += 1;
                continue;
            }
        };
        position += 1;
        for line in text.lines() {
            let words = line
                .split('#')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect::<Vec<_>>();
            if words.is_empty() || !matches!(words[0], "source" | "source-directory") {
                continue;
            }
            if words.len() != 2
                || !(words[0] == "source" && words[1] == "/etc/network/interfaces.d/*"
                    || words[0] == "source-directory" && words[1] == "/etc/network/interfaces.d")
            {
                reason = Some("interfaces uses custom include paths or patterns; edit it through its owning administrator".into());
                continue;
            }
            match fs::read_dir("/etc/network/interfaces.d") {
                Ok(entries) => {
                    for entry in entries {
                        let Ok(entry) = entry else {
                            reason = Some(
                                "An interfaces include directory could not be read completely"
                                    .into(),
                            );
                            continue;
                        };
                        let path = entry.path();
                        if words[0] == "source-directory"
                            && !entry
                                .file_name()
                                .to_string_lossy()
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                        {
                            continue;
                        }
                        if !files.contains(&path)
                            && fs::symlink_metadata(&path).is_ok_and(|m| m.is_file())
                        {
                            files.push(path);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => reason = Some("The interfaces include directory cannot be read".into()),
            }
        }
        if files.len() > 256 {
            reason = Some("Too many interfaces include files to establish ownership safely".into());
            break;
        }
    }
    (files, reason)
}

fn interfaces_owner(name: &str) -> Option<Owner> {
    let mut found = Vec::new();
    let (files, include_reason) = interfaces_files();
    for path in files {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if text.lines().any(|l| {
            let p = l.split_whitespace().collect::<Vec<_>>();
            p.len() >= 4 && p[0] == "iface" && p[1] == name
        }) {
            found.push((path, text));
        }
    }
    if found.len() > 1 {
        return Some(readonly(
            OwnerKind::Interfaces,
            None,
            name.into(),
            "Several interfaces files define this device",
        ));
    }
    let Some((path, text)) = found.pop() else {
        return include_reason
            .map(|reason| readonly(OwnerKind::Unknown, None, name.into(), reason));
    };
    let reason = include_reason.or_else(|| simple_interfaces(&text, name).err());
    Some(Owner {
        kind: OwnerKind::Interfaces,
        path: Some(path),
        id: name.into(),
        uuid: String::new(),
        reason,
    })
}

fn simple_interfaces(text: &str, interface: &str) -> Result<(), String> {
    let mut target = false;
    let mut families = BTreeMap::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let p = line.split_whitespace().collect::<Vec<_>>();
        if p.is_empty() {
            continue;
        }
        if p[0] == "iface" {
            target = p.get(1).copied() == Some(interface);
            if target {
                if p.len() != 4
                    || !matches!(p[2], "inet" | "inet6")
                    || !matches!(p[3], "dhcp" | "static" | "manual" | "auto")
                {
                    return Err(
                        "Only plain DHCP/static IPv4/IPv6 interfaces stanzas are supported".into(),
                    );
                }
                if families.insert(p[2], ()).is_some() {
                    return Err("Several stanzas define the same address family".into());
                }
            }
        } else if matches!(
            p[0],
            "auto" | "allow-hotplug" | "source" | "source-directory" | "mapping"
        ) {
            target = false;
        } else if target && !matches!(p[0], "address" | "netmask" | "gateway" | "dns-nameservers") {
            return Err(format!(
                "interfaces directive {} may run scripts or affect other settings; use the owning editor",
                p[0]
            ));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FileChange {
    pub path: PathBuf,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
    pub mode: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Plan {
    pub operation: String,
    pub identity: BTreeMap<String, String>,
    pub kind: OwnerKind,
    pub interface: String,
    pub uuid: String,
    pub new_uuid: String,
    pub files: Vec<FileChange>,
    pub arguments: Vec<String>,
    pub preview: String,
    pub wifi: Option<Wifi>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Wifi {
    pub ssid: String,
    pub security: String,
    pub password: String,
}

#[derive(Debug, Clone, PartialEq)]
struct Addresses {
    ipv4: String,
    ipv6: String,
    address4: Vec<String>,
    address6: Vec<String>,
    gateway4: String,
    gateway6: String,
    dns: Vec<IpAddr>,
}

fn addresses(values: &BTreeMap<String, String>) -> Result<Addresses, ManagementError> {
    let get = |key: &str| values.get(key).map(String::as_str).unwrap_or("").trim();
    let ipv4 = get("ipv4");
    let ipv6 = get("ipv6");
    if !matches!(ipv4, "dhcp" | "static" | "disabled")
        || !matches!(ipv6, "auto" | "dhcp" | "static" | "disabled")
    {
        return Err(ManagementError::InvalidInput(
            "Choose a supported IPv4/IPv6 mode".into(),
        ));
    }
    let address4 = cidrs(get("address4"), false)?;
    let address6 = cidrs(get("address6"), true)?;
    if ipv4 == "static" && address4.is_empty() || ipv6 == "static" && address6.is_empty() {
        return Err(ManagementError::InvalidInput(
            "Static mode requires at least one CIDR address".into(),
        ));
    }
    let gateway4 = get("gateway4");
    let gateway6 = get("gateway6");
    if !gateway4.is_empty() && (gateway4.parse::<Ipv4Addr>().is_err() || ipv4 != "static")
        || !gateway6.is_empty() && (gateway6.parse::<Ipv6Addr>().is_err() || ipv6 != "static")
    {
        return Err(ManagementError::InvalidInput(
            "The gateway must match a static address family".into(),
        ));
    }
    let dns = get("dns")
        .split([',', ' '])
        .filter(|s| !s.is_empty())
        .map(|s| {
            s.parse::<IpAddr>().map_err(|_| {
                ManagementError::InvalidInput("DNS servers must be IP addresses".into())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if dns.len() > 8 {
        return Err(ManagementError::InvalidInput(
            "At most eight DNS servers are supported".into(),
        ));
    }
    Ok(Addresses {
        ipv4: ipv4.into(),
        ipv6: ipv6.into(),
        address4: if ipv4 == "static" {
            address4
        } else {
            Vec::new()
        },
        address6: if ipv6 == "static" {
            address6
        } else {
            Vec::new()
        },
        gateway4: gateway4.into(),
        gateway6: gateway6.into(),
        dns,
    })
}

fn cidrs(text: &str, ipv6: bool) -> Result<Vec<String>, ManagementError> {
    let mut result = Vec::new();
    for address in text.split([',', ' ']).filter(|s| !s.is_empty()) {
        let (ip, prefix) = address.split_once('/').ok_or_else(|| {
            ManagementError::InvalidInput(
                "Addresses require an IP and prefix length, for example 192.168.1.20/24".into(),
            )
        })?;
        let ip = ip
            .parse::<IpAddr>()
            .map_err(|_| ManagementError::InvalidInput("Invalid IP address".into()))?;
        let prefix = prefix
            .parse::<u8>()
            .map_err(|_| ManagementError::InvalidInput("Invalid prefix length".into()))?;
        if ip.is_ipv6() != ipv6
            || prefix > if ipv6 { 128 } else { 32 }
            || ip.is_unspecified()
            || ip.is_multicast()
        {
            return Err(ManagementError::InvalidInput(
                "Invalid address family or prefix".into(),
            ));
        }
        result.push(format!("{ip}/{prefix}"));
    }
    if result.len() > 8 {
        return Err(ManagementError::InvalidInput(
            "At most eight addresses per family are supported".into(),
        ));
    }
    Ok(result)
}

pub(super) fn prepare(
    owner: &Owner,
    name: &str,
    values: &BTreeMap<String, String>,
) -> Result<Plan, ManagementError> {
    let a = addresses(values)?;
    let mut plan = Plan {
        operation: "configure".into(),
        identity: interface_identity(name)?,
        kind: owner.kind,
        interface: name.into(),
        uuid: owner.uuid.clone(),
        new_uuid: String::new(),
        files: Vec::new(),
        arguments: Vec::new(),
        preview: format!(
            "Interface: {name}\nOwner: {}\nIPv4: {} {} via {}\nIPv6: {} {} via {}\nDNS: {:?}\nUnconfirmed changes will be restored by the independent recovery service after 120 seconds.",
            owner.label(),
            a.ipv4,
            a.address4.join(","),
            a.gateway4,
            a.ipv6,
            a.address6.join(","),
            a.gateway6,
            a.dns
        ),
        wifi: None,
    };
    let path = owner.path.as_deref().ok_or_else(|| {
        ManagementError::Unavailable(
            "The active configuration has no identifiable persistent source".into(),
        )
    })?;
    let metadata = checked_file(path)?;
    let before = fs::read(path).map_err(io_error)?;
    let after = match owner.kind {
        OwnerKind::NetworkManager => {
            if owner.uuid.is_empty() {
                return Err(ManagementError::Unavailable("Activate an existing native NetworkManager profile before editing its addresses".into()));
            }
            plan.arguments = nm_arguments(&a);
            before.clone()
        }
        OwnerKind::Netplan => {
            if !a.dns.is_empty()
                && !nm_device(name).is_ok_and(|d| d.managed)
                && !resolved_available()
            {
                return Err(ManagementError::Unavailable("The Netplan renderer has no verified resolver integration; DNS changes cannot be applied reliably".into()));
            }
            netplan_edit(&before, &owner.id, &a)?
        }
        OwnerKind::Networkd => {
            if !a.dns.is_empty() && !resolved_available() {
                return Err(ManagementError::Unavailable("systemd-resolved is not running; networkd DNS would not necessarily be used by this system".into()));
            }
            simple_networkd(
                std::str::from_utf8(&before).map_err(|_| {
                    ManagementError::InvalidInput("networkd file is not UTF-8".into())
                })?,
                name,
            )
            .map_err(ManagementError::Unavailable)?;
            networkd_text(name, &a).into_bytes()
        }
        OwnerKind::Interfaces => {
            if !a.dns.is_empty() && program("resolvconf").is_err() {
                return Err(ManagementError::Unavailable(
                    "resolvconf is absent; interfaces DNS directives cannot be applied reliably"
                        .into(),
                ));
            }
            interfaces_edit(
                std::str::from_utf8(&before).map_err(|_| {
                    ManagementError::InvalidInput("interfaces file is not UTF-8".into())
                })?,
                name,
                &a,
            )?
            .into_bytes()
        }
        OwnerKind::Unknown => {
            return Err(ManagementError::Unavailable(
                "Unknown configuration owner".into(),
            ));
        }
    };
    if before != after {
        let preview = if owner.kind == OwnerKind::Netplan {
            redact_yaml(&after)?
        } else {
            String::from_utf8_lossy(&after).into_owned()
        };
        plan.preview.push_str(&format!(
            "\nFile: {}\nProposed contents:\n{preview}",
            path.display()
        ));
    }
    plan.files.push(FileChange {
        path: path.into(),
        before,
        after,
        mode: metadata.mode() & 0o777,
    });
    Ok(plan)
}

pub(super) fn checked_file(path: &Path) -> Result<std::fs::Metadata, ManagementError> {
    for parent in path.ancestors().skip(1) {
        let metadata = fs::symlink_metadata(parent).map_err(io_error)?;
        if metadata.uid() != 0
            || !metadata.file_type().is_symlink()
                && (!metadata.is_dir() || metadata.mode() & 0o022 != 0)
        {
            return Err(ManagementError::Unavailable(
                "Configuration has a parent directory that another user can replace or write"
                    .into(),
            ));
        }
    }
    let canonical = fs::canonicalize(path).map_err(io_error)?;
    for parent in canonical.ancestors().skip(1) {
        let metadata = fs::symlink_metadata(parent).map_err(io_error)?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(ManagementError::Unavailable(
                "Resolved configuration directory is writable by another user".into(),
            ));
        }
    }
    let m = fs::symlink_metadata(path).map_err(io_error)?;
    if !m.is_file() || m.file_type().is_symlink() || m.uid() != 0 || m.mode() & 0o022 != 0 {
        return Err(ManagementError::Unavailable(
            "Configuration must be a root-owned regular file with no group/world write permission"
                .into(),
        ));
    }
    if m.len() > 1024 * 1024 {
        return Err(ManagementError::Unavailable(
            "Configuration file is too large for safe preview".into(),
        ));
    }
    Ok(m)
}

fn resolved_available() -> bool {
    std::fs::read_link("/etc/resolv.conf")
        .is_ok_and(|p| p.to_string_lossy().contains("systemd/resolve"))
        && run(
            "systemctl",
            &["is-active", "systemd-resolved.service"],
            &AtomicBool::new(false),
        )
        .is_ok()
}

fn nm_arguments(a: &Addresses) -> Vec<String> {
    let dns4 = a
        .dns
        .iter()
        .filter(|d| d.is_ipv4())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let dns6 = a
        .dns
        .iter()
        .filter(|d| d.is_ipv6())
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    [
        "ipv4.method".into(),
        match a.ipv4.as_str() {
            "dhcp" => "auto",
            "static" => "manual",
            _ => "disabled",
        }
        .into(),
        "ipv4.addresses".into(),
        a.address4.join(","),
        "ipv4.gateway".into(),
        a.gateway4.clone(),
        "ipv6.method".into(),
        match a.ipv6.as_str() {
            "static" => "manual",
            other => other,
        }
        .into(),
        "ipv6.addresses".into(),
        a.address6.join(","),
        "ipv6.gateway".into(),
        a.gateway6.clone(),
        "ipv4.dns".into(),
        dns4,
        "ipv6.dns".into(),
        dns6,
        "ipv4.ignore-auto-dns".into(),
        (!a.dns.is_empty()).to_string(),
        "ipv6.ignore-auto-dns".into(),
        (!a.dns.is_empty()).to_string(),
    ]
    .into()
}

fn networkd_text(name: &str, a: &Addresses) -> String {
    let dhcp = match (a.ipv4 == "dhcp", a.ipv6 == "dhcp") {
        (true, true) => "yes",
        (true, false) => "ipv4",
        (false, true) => "ipv6",
        _ => "no",
    };
    let mut text = format!(
        "[Match]\nName={name}\n\n[Network]\nDHCP={dhcp}\nIPv6AcceptRA={}\nLinkLocalAddressing={}\n",
        if matches!(a.ipv6.as_str(), "auto" | "dhcp") {
            "yes"
        } else {
            "no"
        },
        if a.ipv6 == "disabled" { "no" } else { "ipv6" }
    );
    for address in a.address4.iter().chain(&a.address6) {
        text.push_str(&format!("Address={address}\n"));
    }
    for gateway in [&a.gateway4, &a.gateway6]
        .into_iter()
        .filter(|s| !s.is_empty())
    {
        text.push_str(&format!("Gateway={gateway}\n"));
    }
    for dns in &a.dns {
        text.push_str(&format!("DNS={dns}\n"));
    }
    text
}

fn netplan_edit(bytes: &[u8], id: &str, a: &Addresses) -> Result<Vec<u8>, ManagementError> {
    let mut yaml: Yaml = serde_yaml::from_slice(bytes)
        .map_err(|e| ManagementError::InvalidInput(format!("Invalid Netplan YAML: {e}")))?;
    let definition = yaml["network"]["ethernets"][id]
        .as_mapping_mut()
        .ok_or_else(|| ManagementError::Conflict("Netplan definition changed".into()))?;
    definition.insert(yaml_key("dhcp4"), Yaml::Bool(a.ipv4 == "dhcp"));
    definition.insert(yaml_key("dhcp6"), Yaml::Bool(a.ipv6 == "dhcp"));
    definition.insert(
        yaml_key("accept-ra"),
        Yaml::Bool(matches!(a.ipv6.as_str(), "auto" | "dhcp")),
    );
    definition.insert(
        yaml_key("link-local"),
        Yaml::Sequence(if a.ipv6 == "disabled" {
            Vec::new()
        } else {
            vec![yaml_key("ipv6")]
        }),
    );
    definition.insert(
        yaml_key("addresses"),
        Yaml::Sequence(
            a.address4
                .iter()
                .chain(&a.address6)
                .map(|s| yaml_key(s))
                .collect(),
        ),
    );
    definition.remove(yaml_key("gateway4"));
    definition.remove(yaml_key("gateway6"));
    let mut routes = definition
        .get(yaml_key("routes"))
        .and_then(Yaml::as_sequence)
        .cloned()
        .unwrap_or_default();
    routes.retain(|r| !matches!(r["to"].as_str(), Some("default" | "0.0.0.0/0" | "::/0")));
    for (gateway, to) in [(&a.gateway4, "0.0.0.0/0"), (&a.gateway6, "::/0")] {
        if !gateway.is_empty() {
            routes.push(Yaml::Mapping(Mapping::from_iter([
                (yaml_key("to"), yaml_key(to)),
                (yaml_key("via"), yaml_key(gateway)),
            ])));
        }
    }
    definition.insert(yaml_key("routes"), Yaml::Sequence(routes));
    if !a.dns.is_empty() {
        let nameservers = definition
            .entry(yaml_key("nameservers"))
            .or_insert_with(|| Yaml::Mapping(Mapping::new()))
            .as_mapping_mut()
            .ok_or_else(|| {
                ManagementError::InvalidInput("Netplan nameservers must be a mapping".into())
            })?;
        nameservers.insert(
            yaml_key("addresses"),
            Yaml::Sequence(a.dns.iter().map(|s| yaml_key(&s.to_string())).collect()),
        );
        for key in ["dhcp4-overrides", "dhcp6-overrides"] {
            let overrides = definition
                .entry(yaml_key(key))
                .or_insert_with(|| Yaml::Mapping(Mapping::new()))
                .as_mapping_mut()
                .ok_or_else(|| {
                    ManagementError::InvalidInput("Netplan DHCP overrides must be mappings".into())
                })?;
            overrides.insert(yaml_key("use-dns"), Yaml::Bool(false));
        }
    } else {
        if let Some(nameservers) = definition
            .get_mut(yaml_key("nameservers"))
            .and_then(Yaml::as_mapping_mut)
        {
            nameservers.remove(yaml_key("addresses"));
        }
        for key in ["dhcp4-overrides", "dhcp6-overrides"] {
            if let Some(overrides) = definition
                .get_mut(yaml_key(key))
                .and_then(Yaml::as_mapping_mut)
            {
                overrides.remove(yaml_key("use-dns"));
            }
        }
    }
    serde_yaml::to_string(&yaml)
        .map(String::into_bytes)
        .map_err(|e| ManagementError::Failed(e.to_string()))
}

fn redact_yaml(bytes: &[u8]) -> Result<String, ManagementError> {
    fn redact(value: &mut Yaml) {
        match value {
            Yaml::Mapping(map) => {
                for (key, value) in map.iter_mut() {
                    let key = key.as_str().unwrap_or("").to_ascii_lowercase();
                    if key.contains("password")
                        || key.contains("secret")
                        || key == "psk"
                        || key.contains("private-key")
                    {
                        *value = yaml_key("[redacted]");
                    } else {
                        redact(value);
                    }
                }
            }
            Yaml::Sequence(values) => {
                for value in values {
                    redact(value);
                }
            }
            _ => {}
        }
    }
    let mut yaml: Yaml =
        serde_yaml::from_slice(bytes).map_err(|e| ManagementError::Failed(e.to_string()))?;
    redact(&mut yaml);
    serde_yaml::to_string(&yaml).map_err(|e| ManagementError::Failed(e.to_string()))
}

fn interfaces_edit(text: &str, name: &str, a: &Addresses) -> Result<String, ManagementError> {
    simple_interfaces(text, name).map_err(ManagementError::Unavailable)?;
    if a.address4.len() > 1 || a.address6.len() > 1 || a.ipv6 == "disabled" {
        return Err(ManagementError::Unavailable("Debian interfaces supports one address per family here; disabling IPv6 globally is outside this app".into()));
    }
    let mut output = String::new();
    let mut skip = false;
    for raw in text.lines() {
        let p = raw.split_whitespace().collect::<Vec<_>>();
        if !p.is_empty()
            && (p[0] == "iface"
                || matches!(
                    p[0],
                    "auto" | "allow-hotplug" | "source" | "source-directory" | "mapping"
                ))
        {
            skip = p[0] == "iface" && p.get(1).copied() == Some(name);
        }
        if !skip {
            output.push_str(raw);
            output.push('\n');
        }
    }
    output.push('\n');
    for (family, mode, address, gateway) in [
        ("inet", &a.ipv4, &a.address4, &a.gateway4),
        ("inet6", &a.ipv6, &a.address6, &a.gateway6),
    ] {
        let mode = match mode.as_str() {
            "disabled" => "manual",
            "auto" => "auto",
            other => other,
        };
        output.push_str(&format!("iface {name} {family} {mode}\n"));
        if mode == "static" {
            output.push_str(&format!("    address {}\n", address[0]));
        }
        if !gateway.is_empty() {
            output.push_str(&format!("    gateway {gateway}\n"));
        }
        if family == "inet" && !a.dns.is_empty() {
            output.push_str(&format!(
                "    dns-nameservers {}\n",
                a.dns
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
    }
    Ok(output)
}

pub(super) fn prepare_wifi(
    owner: &Owner,
    name: &str,
    values: &BTreeMap<String, String>,
) -> Result<Plan, ManagementError> {
    let ssid = values.get("ssid").cloned().unwrap_or_default();
    let security = values.get("security").cloned().unwrap_or_default();
    let password = values.get("password").cloned().unwrap_or_default();
    if ssid.is_empty()
        || ssid.len() > 32
        || ssid.contains(['\0', '\n', '\r'])
        || !matches!(security.as_str(), "open" | "wpa2" | "wpa3")
    {
        return Err(ManagementError::InvalidInput(
            "Enter an SSID of 1–32 bytes and choose open, WPA2 or WPA3 personal security".into(),
        ));
    }
    validate_wifi_password(&security, &password)?;
    let new_uuid = fs::read_to_string("/proc/sys/kernel/random/uuid")
        .map_err(io_error)?
        .trim()
        .into();
    Ok(Plan {
        operation: "wifi-connect".into(),
        identity: interface_identity(name)?,
        kind: OwnerKind::NetworkManager,
        interface: name.into(),
        uuid: owner.uuid.clone(),
        new_uuid,
        files: Vec::new(),
        arguments: Vec::new(),
        preview: format!(
            "Connect {name} to SSID {ssid:?} using {security}. A new profile is first kept in memory; it is saved only after confirmation. The previous connection will be restored if unconfirmed."
        ),
        wifi: Some(Wifi {
            ssid,
            security,
            password,
        }),
    })
}

fn validate_wifi_password(security: &str, password: &str) -> Result<(), ManagementError> {
    if password.contains(['\0', '\n', '\r']) {
        return Err(ManagementError::InvalidInput(
            "The password contains a character that cannot be entered in this form".into(),
        ));
    }
    let valid = match security {
        "open" => true,
        "wpa2" => {
            password.is_ascii()
                && ((8..=63).contains(&password.len())
                    || password.len() == 64 && password.bytes().all(|b| b.is_ascii_hexdigit()))
        }
        "wpa3" => !password.is_empty() && password.len() <= 4096,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(ManagementError::InvalidInput("WPA2 requires an 8–63 character ASCII passphrase or 64 hexadecimal digits; WPA3 requires a nonempty passphrase".into()))
    }
}

pub(super) fn prepare_wifi_action(
    owner: &Owner,
    name: &str,
    operation: &str,
) -> Result<Plan, ManagementError> {
    if owner.kind != OwnerKind::NetworkManager
        || owner.uuid.is_empty()
        || !Path::new(&format!("/sys/class/net/{name}/wireless")).exists()
    {
        return Err(ManagementError::Unavailable(
            "Disconnect/forget requires an active native NetworkManager Wi-Fi profile".into(),
        ));
    }
    let mut files = Vec::new();
    if operation == "wifi-forget" {
        let path = owner.path.as_deref().ok_or_else(|| {
            ManagementError::Unavailable(
                "The active profile has no persistent source to back up".into(),
            )
        })?;
        let metadata = checked_file(path)?;
        files.push(FileChange {
            path: path.into(),
            before: fs::read(path).map_err(io_error)?,
            after: Vec::new(),
            mode: metadata.mode() & 0o777,
        });
    }
    Ok(Plan {
        operation: operation.into(),
        identity: interface_identity(name)?,
        kind: OwnerKind::NetworkManager,
        interface: name.into(),
        uuid: owner.uuid.clone(),
        new_uuid: String::new(),
        files,
        arguments: Vec::new(),
        preview: format!(
            "{operation} on {name}, active profile {}. The previous profile/connection is restored unless confirmed within 120 seconds.",
            owner.uuid
        ),
        wifi: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn values() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("ipv4".into(), "static".into()),
            ("ipv6".into(), "auto".into()),
            ("address4".into(), "192.0.2.7/24".into()),
            ("gateway4".into(), "192.0.2.1".into()),
            ("dns".into(), "192.0.2.53".into()),
        ])
    }
    #[test]
    fn rejects_invalid_address_families_prefixes_and_dns() {
        for (key, bad) in [
            ("address4", "2001:db8::1/64"),
            ("address4", "192.0.2.1/33"),
            ("gateway4", "$(whoami)"),
            ("dns", "example.org"),
        ] {
            let mut input = values();
            input.insert(key.into(), bad.into());
            assert!(addresses(&input).is_err());
        }
    }
    #[test]
    fn netplan_preserves_other_devices_and_non_default_routes() {
        let before = b"network:\n  version: 2\n  ethernets:\n    eth0:\n      mtu: 1400\n      routes:\n        - to: 198.51.100.0/24\n          via: 192.0.2.2\n    eth1:\n      dhcp4: true\n";
        let after = netplan_edit(before, "eth0", &addresses(&values()).unwrap()).unwrap();
        let result: Yaml = serde_yaml::from_slice(&after).unwrap();
        assert_eq!(
            result["network"]["ethernets"]["eth1"]["dhcp4"],
            Yaml::Bool(true)
        );
        assert_eq!(
            result["network"]["ethernets"]["eth0"]["mtu"].as_u64(),
            Some(1400)
        );
        assert_eq!(
            result["network"]["ethernets"]["eth0"]["routes"][0]["to"].as_str(),
            Some("198.51.100.0/24")
        );
    }
    #[test]
    fn rejects_networkd_matches_affecting_multiple_interfaces() {
        assert!(simple_networkd("[Match]\nName=en*\n[Network]\nDHCP=yes\n", "ens3").is_err());
    }
    #[test]
    fn interfaces_edit_keeps_other_stanzas_and_rejects_hooks() {
        let before = "auto eth0 eth1\niface eth0 inet dhcp\niface eth1 inet static\n    address 198.51.100.2/24\n";
        let after = interfaces_edit(before, "eth0", &addresses(&values()).unwrap()).unwrap();
        assert!(after.contains("iface eth1 inet static\n    address 198.51.100.2/24"));
        assert!(
            simple_interfaces(
                "iface eth0 inet dhcp\n    pre-up arbitrary-command\n",
                "eth0"
            )
            .is_err()
        );
    }
    #[test]
    fn native_form_defaults_report_current_addresses_and_gateways() {
        let netplan = netplan_values(
            "network:\n  ethernets:\n    eth0:\n      addresses: [192.0.2.7/24, '2001:db8::2/64']\n      routes:\n        - to: default\n          via: 192.0.2.1\n",
            "eth0",
        );
        assert_eq!(netplan["ipv4"], "static");
        assert_eq!(netplan["address6"], "2001:db8::2/64");
        assert_eq!(netplan["gateway4"], "192.0.2.1");
        let interfaces = interfaces_values(
            "iface eth0 inet static\n    address 192.0.2.7\n    netmask 255.255.255.0\n    gateway 192.0.2.1\n",
            "eth0",
        );
        assert_eq!(interfaces["address4"], "192.0.2.7/24");
    }
    #[test]
    fn netplan_dns_edits_preserve_search_and_other_dhcp_options_and_redact_secrets() {
        let before = b"network:\n  ethernets:\n    eth0:\n      nameservers:\n        search: [example.org]\n      dhcp4-overrides:\n        use-routes: false\n  wifis:\n    wlan0:\n      access-points:\n        ssid:\n          password: secret-password\n";
        let after = netplan_edit(before, "eth0", &addresses(&values()).unwrap()).unwrap();
        let yaml: Yaml = serde_yaml::from_slice(&after).unwrap();
        assert_eq!(
            yaml["network"]["ethernets"]["eth0"]["nameservers"]["search"][0].as_str(),
            Some("example.org")
        );
        assert_eq!(
            yaml["network"]["ethernets"]["eth0"]["dhcp4-overrides"]["use-routes"].as_bool(),
            Some(false)
        );
        assert!(!redact_yaml(&after).unwrap().contains("secret-password"));
    }
    #[test]
    fn wifi_password_validation_distinguishes_wpa2_and_sae() {
        assert!(validate_wifi_password("wpa2", "short").is_err());
        assert!(validate_wifi_password("wpa2", &"a".repeat(64)).is_ok());
        assert!(validate_wifi_password("wpa2", &"z".repeat(64)).is_err());
        assert!(validate_wifi_password("wpa3", "x").is_ok());
        assert!(validate_wifi_password("wpa3", "").is_err());
    }
}
