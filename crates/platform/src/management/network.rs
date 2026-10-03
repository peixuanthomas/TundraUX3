//! Network management keeps the configuration owner and the running daemon separate.
//! Changes are guarded by a root-owned recovery service, never by a Shell thread.
mod config;
mod transaction;

use super::*;
use serde_json::Value as Json;
use std::collections::HashMap;
use std::io::Read;
use std::net::{IpAddr, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use zbus::blocking::{Connection, Proxy, connection::Builder};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

pub use transaction::rollback_transaction;

const NM: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";

pub fn query(
    query: &ManagementQuery,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    let addresses: Json = serde_json::from_str(&run("ip", &["-j", "address", "show"], cancelled)?)
        .map_err(|e| ManagementError::Failed(format!("Cannot read ip address output: {e}")))?;
    let mut snapshot = ManagementSnapshot {
        columns: vec![
            "Interface".into(),
            "State".into(),
            "Addresses".into(),
            "Configuration owner".into(),
        ],
        backend: "Linux network configuration owners".into(),
        ..Default::default()
    };
    if unsafe { libc::geteuid() } != 0 {
        snapshot.actions.push(ManagementAction {
            id: "inspect_network".into(),
            label: "Authorize reading protected network settings".into(),
            privileged: true,
            ..Default::default()
        });
    }
    for entry in addresses.as_array().into_iter().flatten() {
        check_cancelled(cancelled)?;
        let Some(name) = entry["ifname"].as_str() else {
            continue;
        };
        if name == "lo" || !name.contains(&query.filter) {
            continue;
        }
        let owner = config::detect(name);
        let address_list = entry["addr_info"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| {
                Some(format!(
                    "{}/{}",
                    a["local"].as_str()?,
                    a["prefixlen"].as_u64()?
                ))
            })
            .collect::<Vec<_>>()
            .join(", ");
        let mut fields = settings_fields();
        owner.populate_fields(&mut fields);
        let mut row = ManagementRow {
            id: name.into(),
            cells: vec![
                name.into(),
                entry["operstate"].as_str().unwrap_or("unknown").into(),
                address_list,
                owner.label().into(),
            ],
            detail: vec![
                ("Configuration source".into(), owner.source_display()),
                (
                    "Read-only reason".into(),
                    owner.reason.clone().unwrap_or_default(),
                ),
            ],
            identity: interface_identity(name)?,
            ..Default::default()
        };
        row.identity.insert("owner".into(), owner.label().into());
        row.identity.insert("source".into(), owner.source_display());
        if let Some(path) = owner.path.as_ref() {
            if let Ok(bytes) = std::fs::read(path) {
                row.identity
                    .insert("configuration".into(), fingerprint(&bytes));
            }
        }
        row.actions.push(ManagementAction {
            id: "configure".into(),
            label: "Configure addresses and DNS".into(),
            fields,
            confirm: true,
            privileged: true,
            disabled_reason: owner.reason.clone().or_else(|| owner.path.is_none().then(|| "There is no active persistent profile to configure; activate a native profile first".into())).or_else(recovery_unavailable),
            ..Default::default()
        });
        if owner.kind == config::OwnerKind::NetworkManager
            && std::path::Path::new(&format!("/sys/class/net/{name}/wireless")).exists()
        {
            row.actions.push(ManagementAction {
                id: "wifi-connect".into(),
                label: "Connect to Wi-Fi".into(),
                fields: vec![
                    field("ssid", "SSID", "", true, &[]),
                    field(
                        "security",
                        "Security",
                        "wpa2",
                        true,
                        &["open", "wpa2", "wpa3"],
                    ),
                    ManagementField {
                        secret: true,
                        ..field("password", "Password", "", false, &[])
                    },
                ],
                confirm: true,
                privileged: true,
                disabled_reason: owner.reason.clone().or_else(recovery_unavailable),
            });
            if let Ok(access_points) = run(
                "nmcli",
                &[
                    "--terse",
                    "--fields",
                    "SSID,SECURITY,SIGNAL",
                    "device",
                    "wifi",
                    "list",
                    "ifname",
                    name,
                ],
                cancelled,
            ) {
                row.detail
                    .push(("Visible Wi-Fi networks".into(), access_points));
            }
            if !owner.uuid.is_empty() {
                for (id, label) in [
                    ("wifi-disconnect", "Disconnect Wi-Fi"),
                    ("wifi-forget", "Forget the active Wi-Fi profile"),
                ] {
                    row.actions.push(ManagementAction {
                        id: id.into(),
                        label: label.into(),
                        confirm: true,
                        privileged: true,
                        disabled_reason: owner.reason.clone().or_else(recovery_unavailable),
                        ..Default::default()
                    });
                }
            }
        }
        row.actions.push(ManagementAction {
            id: "check".into(),
            label: "Check DNS and target connection".into(),
            fields: vec![
                field("host", "Host or IP", "", true, &[]),
                field("port", "TCP port", "443", true, &[]),
            ],
            ..Default::default()
        });
        snapshot.rows.push(row);
    }
    if let Ok(profiles) = saved_wifi_profiles() {
        let active = run(
            "nmcli",
            &["--get-values", "UUID", "connection", "show", "--active"],
            cancelled,
        )
        .unwrap_or_default();
        for profile in profiles {
            let Some(path) = profile
                .filename
                .as_ref()
                .filter(|p| config::native_nm_file(p))
            else {
                continue;
            };
            let mut identity = BTreeMap::from([
                ("uuid".into(), profile.uuid.clone()),
                ("source".into(), path.display().to_string()),
            ]);
            let mut reason = None;
            match std::fs::read(path) { Ok(bytes) => { identity.insert("configuration".into(), fingerprint(&bytes)); }, Err(_) => reason = Some("Authorize reading protected network settings before deleting this saved profile".into()) }
            if active.lines().any(|s| s == profile.uuid) {
                reason = Some(
                    "Use Forget on the active interface so disconnection has independent recovery"
                        .into(),
                );
            }
            snapshot.rows.push(ManagementRow {
                id: format!("profile:{}", profile.uuid),
                cells: vec![
                    profile.id,
                    "Saved Wi-Fi".into(),
                    String::new(),
                    "NetworkManager".into(),
                ],
                identity,
                detail: vec![
                    (
                        "Saved configuration path".into(),
                        path.display().to_string(),
                    ),
                    ("Profile UUID".into(), profile.uuid),
                ],
                actions: vec![ManagementAction {
                    id: "forget_saved_wifi".into(),
                    label: "Forget saved Wi-Fi profile".into(),
                    confirm: true,
                    privileged: true,
                    disabled_reason: reason,
                    ..Default::default()
                }],
                ..Default::default()
            });
        }
    }
    snapshot.notices.push("Changes require a configuration source that can be identified exactly and an independent system recovery service. Confirmation expires after 120 seconds.".into());
    Ok(snapshot)
}

pub fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    if command.action == "inspect_network" {
        if unsafe { libc::geteuid() } != 0 {
            return Err(ManagementError::PermissionDenied(
                "Protected network inspection requires the authorized helper".into(),
            ));
        }
        let snapshot = query(&ManagementQuery::new(ManagementKind::Network), cancelled)?;
        interaction.emit(OperationEvent::Snapshot { snapshot });
        return Ok("Protected network settings were read; passwords were omitted".into());
    }
    if command.action == "forget_saved_wifi" {
        return forget_saved_wifi(command, interaction, cancelled);
    }
    let name = command
        .target
        .as_deref()
        .ok_or_else(|| ManagementError::InvalidInput("Select a network interface".into()))?;
    validate_interface(name)?;
    verify_identity(name, &command.identity)?;
    if command.action == "check" {
        return check_target(name, &command.values, interaction, cancelled);
    }
    if !matches!(
        command.action.as_str(),
        "configure" | "wifi-connect" | "wifi-disconnect" | "wifi-forget"
    ) {
        return Err(ManagementError::InvalidInput(
            "Unknown network action".into(),
        ));
    }
    if unsafe { libc::geteuid() } != 0 {
        return Err(ManagementError::PermissionDenied(
            "Network changes require the authorized helper".into(),
        ));
    }
    let owner = config::detect(name);
    if let Some(reason) = &owner.reason {
        return Err(ManagementError::Unavailable(reason.clone()));
    }
    if command.identity.get("owner") != Some(&owner.label().to_string())
        || command.identity.get("source") != Some(&owner.source_display())
    {
        return Err(ManagementError::Conflict(
            "The interface configuration owner changed; refresh the list".into(),
        ));
    }
    if let Some(path) = &owner.path {
        let bytes = std::fs::read(path).map_err(io_error)?;
        if command.identity.get("configuration") != Some(&fingerprint(&bytes)) {
            return Err(ManagementError::Conflict(
                "Network configuration changed since it was displayed".into(),
            ));
        }
    }
    check_cancelled(cancelled)?;
    let plan = if command.action == "configure" {
        config::prepare(&owner, name, &command.values)?
    } else if command.action == "wifi-connect" {
        if owner.kind != config::OwnerKind::NetworkManager {
            return Err(ManagementError::Unavailable(
                "Personal Wi-Fi changes require native NetworkManager ownership".into(),
            ));
        }
        config::prepare_wifi(&owner, name, &command.values)?
    } else {
        config::prepare_wifi_action(&owner, name, &command.action)?
    };
    interaction.emit(OperationEvent::Output {
        text: plan.preview.clone(),
    });
    let decision = interaction.ask(
        "network-preview",
        "Apply the displayed network settings temporarily?",
        &["Apply".into(), "Cancel".into()],
        false,
    )?;
    if !decision.eq_ignore_ascii_case("Apply") {
        return Err(ManagementError::Cancelled);
    }
    verify_identity(name, &command.identity)?;
    let current = config::detect(name);
    if current.kind != owner.kind
        || current.path != owner.path
        || current.uuid != owner.uuid
        || current.reason.is_some()
    {
        return Err(ManagementError::Conflict(
            "Network ownership or the active profile changed while the preview was open".into(),
        ));
    }
    transaction::apply(plan, context, interaction, cancelled)
}

fn settings_fields() -> Vec<ManagementField> {
    vec![
        field(
            "ipv4",
            "IPv4 mode",
            "dhcp",
            true,
            &["dhcp", "static", "disabled"],
        ),
        field(
            "address4",
            "IPv4 addresses (CIDR, comma separated)",
            "",
            false,
            &[],
        ),
        field("gateway4", "IPv4 gateway", "", false, &[]),
        field(
            "ipv6",
            "IPv6 mode",
            "auto",
            true,
            &["auto", "dhcp", "static", "disabled"],
        ),
        field(
            "address6",
            "IPv6 addresses (CIDR, comma separated)",
            "",
            false,
            &[],
        ),
        field("gateway6", "IPv6 gateway", "", false, &[]),
        field(
            "dns",
            "DNS server IPs (comma separated; empty uses automatic DNS)",
            "",
            false,
            &[],
        ),
    ]
}

fn field(id: &str, label: &str, value: &str, required: bool, choices: &[&str]) -> ManagementField {
    ManagementField {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        required,
        choices: choices.iter().map(|s| (*s).into()).collect(),
        ..Default::default()
    }
}

fn recovery_unavailable() -> Option<String> {
    if !std::path::Path::new("/run/systemd/system").is_dir() || program("systemd-run").is_err() {
        Some("No systemd system manager is available to run recovery independently of SSH and the UI".into())
    } else {
        None
    }
}

pub(super) fn validate_interface(name: &str) -> Result<(), ManagementError> {
    if name.is_empty()
        || name.len() > 15
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        || name.starts_with('-')
    {
        Err(ManagementError::InvalidInput(
            "Invalid network interface name".into(),
        ))
    } else {
        Ok(())
    }
}

fn interface_identity(name: &str) -> Result<BTreeMap<String, String>, ManagementError> {
    validate_interface(name)?;
    let root = std::path::PathBuf::from(format!("/sys/class/net/{name}"));
    let index = std::fs::read_to_string(root.join("ifindex")).map_err(io_error)?;
    let address = std::fs::read_to_string(root.join("address")).map_err(io_error)?;
    Ok(BTreeMap::from([
        ("ifindex".into(), index.trim().into()),
        ("mac".into(), address.trim().into()),
    ]))
}

fn verify_identity(name: &str, expected: &BTreeMap<String, String>) -> Result<(), ManagementError> {
    let current = interface_identity(name)?;
    if ["ifindex", "mac"]
        .iter()
        .any(|key| expected.get(*key) != current.get(*key))
    {
        Err(ManagementError::Conflict(
            "The network interface changed; refresh the list".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn fingerprint(bytes: &[u8]) -> String {
    // Only a display-time change detector. Root-private backups contain the actual bytes.
    let hash = bytes.iter().fold(0xcbf29ce484222325_u64, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}:{}", bytes.len())
}

pub(super) fn program(name: &str) -> Result<PathBuf, ManagementError> {
    for directory in ["/usr/sbin", "/usr/bin", "/sbin", "/bin"] {
        let path = PathBuf::from(directory).join(name);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(ManagementError::Unavailable(format!(
        "Required system tool {name} is not installed"
    )))
}

pub(super) fn run(
    name: &str,
    args: &[&str],
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    let mut child = Command::new(program(name)?)
        .args(args)
        .env_clear()
        .env("LC_ALL", "C")
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(io_error)?;
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io_error(std::io::Error::last_os_error()));
        }
    }
    fn drain(pipe: &mut impl Read, bytes: &mut Vec<u8>) -> Result<(), ManagementError> {
        let mut buffer = [0_u8; 8192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => return Ok(()),
                Ok(length) => {
                    if bytes.len() + length > 2 * 1024 * 1024 {
                        return Err(ManagementError::Failed(
                            "System command output exceeded its limit".into(),
                        ));
                    }
                    bytes.extend_from_slice(&buffer[..length]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(e) => return Err(io_error(e)),
            }
        }
    }
    let mut output = Vec::new();
    let mut error = Vec::new();
    let started = Instant::now();
    let status = loop {
        if let Err(e) = drain(&mut stdout, &mut output).and_then(|_| drain(&mut stderr, &mut error))
        {
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            let _ = child.wait();
            return Err(e);
        }
        if let Some(status) = child.try_wait().map_err(io_error)? {
            drain(&mut stdout, &mut output)?;
            drain(&mut stderr, &mut error)?;
            break status;
        }
        if cancelled.load(Ordering::Acquire) || started.elapsed() > Duration::from_secs(45) {
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
            }
            let _ = child.wait();
            return if cancelled.load(Ordering::Acquire) {
                Err(ManagementError::Cancelled)
            } else {
                Err(ManagementError::Failed(format!("{name} timed out")))
            };
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = String::from_utf8_lossy(&output).trim().to_string();
    let error = String::from_utf8_lossy(&error).trim().to_string();
    if !status.success() {
        return Err(ManagementError::Failed(format!("{name} failed: {error}")));
    }
    Ok(output)
}

pub(super) fn check_cancelled(cancelled: &AtomicBool) -> Result<(), ManagementError> {
    if cancelled.load(Ordering::Acquire) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}

pub(super) fn io_error(error: std::io::Error) -> ManagementError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        ManagementError::PermissionDenied(error.to_string())
    } else {
        ManagementError::Failed(error.to_string())
    }
}

pub(super) fn bus() -> Result<Connection, ManagementError> {
    Builder::system()
        .map_err(dbus_error)?
        .method_timeout(Duration::from_secs(15))
        .build()
        .map_err(dbus_error)
}

pub(super) fn dbus_error(error: zbus::Error) -> ManagementError {
    ManagementError::Unavailable(format!("NetworkManager D-Bus: {error}"))
}

#[derive(Debug, Clone, Default)]
pub(super) struct NmDevice {
    pub path: String,
    pub uuid: String,
    pub connection: String,
    pub filename: Option<PathBuf>,
    pub managed: bool,
}

pub(super) fn nm_device(name: &str) -> Result<NmDevice, ManagementError> {
    let bus = bus()?;
    let names = zbus::blocking::fdo::DBusProxy::new(&bus).map_err(dbus_error)?;
    if !names
        .name_has_owner(NM.try_into().map_err(|_| {
            ManagementError::Unavailable("Invalid NetworkManager service name".into())
        })?)
        .map_err(|e| dbus_error(e.into()))?
    {
        return Err(ManagementError::Unavailable(
            "NetworkManager is not running".into(),
        ));
    }
    let proxy = Proxy::new(&bus, NM, NM_PATH, NM).map_err(dbus_error)?;
    let path: OwnedObjectPath = proxy
        .call("GetDeviceByIpIface", &(name,))
        .map_err(dbus_error)?;
    let device = Proxy::new(
        &bus,
        NM,
        path.as_str(),
        "org.freedesktop.NetworkManager.Device",
    )
    .map_err(dbus_error)?;
    let managed: bool = device.get_property("Managed").map_err(dbus_error)?;
    let active: OwnedObjectPath = device
        .get_property("ActiveConnection")
        .map_err(dbus_error)?;
    let mut result = NmDevice {
        path: path.to_string(),
        managed,
        ..Default::default()
    };
    if active.as_str() != "/" {
        let active = Proxy::new(
            &bus,
            NM,
            active.as_str(),
            "org.freedesktop.NetworkManager.Connection.Active",
        )
        .map_err(dbus_error)?;
        result.uuid = active.get_property::<String>("Uuid").map_err(dbus_error)?;
        let connection: OwnedObjectPath = active.get_property("Connection").map_err(dbus_error)?;
        let settings = Proxy::new(
            &bus,
            NM,
            connection.as_str(),
            "org.freedesktop.NetworkManager.Settings.Connection",
        )
        .map_err(dbus_error)?;
        let filename: String = settings.get_property("Filename").map_err(dbus_error)?;
        result.connection = connection.to_string();
        result.filename = (!filename.is_empty()).then(|| PathBuf::from(filename));
    }
    Ok(result)
}

pub(super) fn add_wifi(
    name: &str,
    uuid: &str,
    ssid: &str,
    security: &str,
    password: &str,
) -> Result<(), ManagementError> {
    let bus = bus()?;
    let device = nm_device(name)?;
    let mut settings = HashMap::<&str, HashMap<&str, Value<'_>>>::new();
    settings.insert(
        "connection",
        HashMap::from([
            ("id", Value::from(ssid)),
            ("uuid", Value::from(uuid)),
            ("type", Value::from("802-11-wireless")),
            ("interface-name", Value::from(name)),
            ("autoconnect", Value::from(false)),
        ]),
    );
    settings.insert(
        "802-11-wireless",
        HashMap::from([
            ("ssid", Value::from(ssid.as_bytes().to_vec())),
            ("mode", Value::from("infrastructure")),
        ]),
    );
    settings.insert("ipv4", HashMap::from([("method", Value::from("auto"))]));
    settings.insert("ipv6", HashMap::from([("method", Value::from("auto"))]));
    if security != "open" {
        let mut wireless_security = HashMap::from([
            (
                "key-mgmt",
                Value::from(if security == "wpa3" { "sae" } else { "wpa-psk" }),
            ),
            ("psk", Value::from(password)),
            ("proto", Value::from(vec!["rsn"])),
        ]);
        if security == "wpa3" {
            wireless_security.insert("pmf", Value::from(3_u32));
        }
        settings.insert("802-11-wireless-security", wireless_security);
    }
    let proxy = Proxy::new(&bus, NM, NM_PATH, NM).map_err(dbus_error)?;
    let options = HashMap::from([
        ("persist", Value::from("memory")),
        ("bind-activation", Value::from("none")),
    ]);
    let _: (
        OwnedObjectPath,
        OwnedObjectPath,
        HashMap<String, OwnedValue>,
    ) = proxy
        .call(
            "AddAndActivateConnection2",
            &(
                settings,
                OwnedObjectPath::try_from(device.path)
                    .map_err(|e| ManagementError::Failed(e.to_string()))?,
                OwnedObjectPath::try_from("/").unwrap(),
                options,
            ),
        )
        .map_err(dbus_error)?;
    Ok(())
}

pub(super) fn save_nm(uuid: &str) -> Result<(), ManagementError> {
    let bus = bus()?;
    let settings = Proxy::new(
        &bus,
        NM,
        "/org/freedesktop/NetworkManager/Settings",
        "org.freedesktop.NetworkManager.Settings",
    )
    .map_err(dbus_error)?;
    let path: OwnedObjectPath = settings
        .call("GetConnectionByUuid", &(uuid,))
        .map_err(dbus_error)?;
    let connection = Proxy::new(
        &bus,
        NM,
        path.as_str(),
        "org.freedesktop.NetworkManager.Settings.Connection",
    )
    .map_err(dbus_error)?;
    connection.call::<_, _, ()>("Save", &()).map_err(dbus_error)
}

struct SavedWifi {
    uuid: String,
    id: String,
    filename: Option<PathBuf>,
}

fn saved_wifi_profiles() -> Result<Vec<SavedWifi>, ManagementError> {
    let bus = bus()?;
    let names = zbus::blocking::fdo::DBusProxy::new(&bus).map_err(dbus_error)?;
    if !names
        .name_has_owner(NM.try_into().unwrap())
        .map_err(|e| dbus_error(e.into()))?
    {
        return Ok(Vec::new());
    }
    let proxy = Proxy::new(
        &bus,
        NM,
        "/org/freedesktop/NetworkManager/Settings",
        "org.freedesktop.NetworkManager.Settings",
    )
    .map_err(dbus_error)?;
    let connections: Vec<OwnedObjectPath> =
        proxy.call("ListConnections", &()).map_err(dbus_error)?;
    let mut output = Vec::new();
    for path in connections.into_iter().take(256) {
        let profile = Proxy::new(
            &bus,
            NM,
            path.as_str(),
            "org.freedesktop.NetworkManager.Settings.Connection",
        )
        .map_err(dbus_error)?;
        let settings: HashMap<String, HashMap<String, OwnedValue>> =
            match profile.call("GetSettings", &()) {
                Ok(v) => v,
                Err(_) => continue,
            };
        let get = |key: &str| {
            settings
                .get("connection")
                .and_then(|c| c.get(key))
                .and_then(|v| v.try_clone().ok())
                .and_then(|v| String::try_from(v).ok())
                .unwrap_or_default()
        };
        if get("type") != "802-11-wireless" {
            continue;
        }
        let filename = profile
            .get_property::<String>("Filename")
            .unwrap_or_default();
        output.push(SavedWifi {
            uuid: get("uuid"),
            id: get("id"),
            filename: (!filename.is_empty()).then(|| PathBuf::from(filename)),
        });
    }
    Ok(output)
}

fn forget_saved_wifi(
    command: &ManagementCommand,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(ManagementError::PermissionDenied(
            "Deleting protected Wi-Fi profiles requires the authorized helper".into(),
        ));
    }
    let uuid = command
        .target
        .as_deref()
        .and_then(|s| s.strip_prefix("profile:"))
        .filter(|s| s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))
        .ok_or_else(|| ManagementError::InvalidInput("Select a saved Wi-Fi profile".into()))?;
    let profile = saved_wifi_profiles()?
        .into_iter()
        .find(|p| p.uuid == uuid)
        .ok_or_else(|| {
            ManagementError::Conflict("Saved Wi-Fi profile changed or disappeared".into())
        })?;
    let path = profile
        .filename
        .filter(|p| config::native_nm_file(p))
        .ok_or_else(|| {
            ManagementError::Unavailable(
                "Only native persistent NetworkManager Wi-Fi profiles can be removed".into(),
            )
        })?;
    config::checked_file(&path)?;
    let bytes = std::fs::read(&path).map_err(io_error)?;
    if command.identity.get("uuid").map(String::as_str) != Some(uuid)
        || command.identity.get("source") != Some(&path.display().to_string())
        || command.identity.get("configuration") != Some(&fingerprint(&bytes))
    {
        return Err(ManagementError::Conflict(
            "The saved Wi-Fi profile changed after it was displayed".into(),
        ));
    }
    let active = run(
        "nmcli",
        &["--get-values", "UUID", "connection", "show", "--active"],
        cancelled,
    )?;
    if active.lines().any(|s| s == uuid) {
        return Err(ManagementError::Conflict("This profile became active; forget it through the interface so network recovery is armed".into()));
    }
    let backup = transaction::backup_saved_wifi(uuid, &bytes)?;
    interaction.emit(OperationEvent::Output {
        text: format!(
            "Delete saved Wi-Fi profile {:?} ({uuid}) at {}. A private backup is retained at {}.",
            profile.id,
            path.display(),
            backup.display()
        ),
    });
    let decision = interaction.ask(
        "forget-saved-wifi",
        "Delete the displayed inactive saved Wi-Fi profile?",
        &["Forget".into(), "Cancel".into()],
        false,
    )?;
    if !decision.eq_ignore_ascii_case("Forget") {
        return Err(ManagementError::Cancelled);
    }
    check_cancelled(cancelled)?;
    if std::fs::read(&path).map_err(io_error)? != bytes {
        return Err(ManagementError::Conflict(
            "The saved Wi-Fi file changed while confirmation was open".into(),
        ));
    }
    let active = run(
        "nmcli",
        &["--get-values", "UUID", "connection", "show", "--active"],
        cancelled,
    )?;
    if active.lines().any(|s| s == uuid) {
        return Err(ManagementError::Conflict("The saved profile became active while confirmation was open; use the interface action instead".into()));
    }
    run("nmcli", &["connection", "delete", "uuid", uuid], cancelled)?;
    if saved_wifi_profiles()?.iter().any(|p| p.uuid == uuid) || path.exists() {
        return Err(ManagementError::Failed(
            "NetworkManager reported deletion, but the saved profile is still present".into(),
        ));
    }
    Ok(format!(
        "Forgot saved Wi-Fi profile {:?}; private backup retained at {}",
        profile.id,
        backup.display()
    ))
}

fn check_target(
    name: &str,
    values: &BTreeMap<String, String>,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    let host = values
        .get("host")
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 253
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b))
        })
        .ok_or_else(|| ManagementError::InvalidInput("Enter a host name or IP address".into()))?;
    let port = values
        .get("port")
        .and_then(|p| p.parse::<u16>().ok())
        .filter(|p| *p != 0)
        .ok_or_else(|| {
            ManagementError::InvalidInput("TCP port must be between 1 and 65535".into())
        })?;
    let ips = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![ip]
    } else {
        let result = run("getent", &["ahosts", host], cancelled)?;
        let mut ips = result
            .lines()
            .filter_map(|l| l.split_whitespace().next()?.parse::<IpAddr>().ok())
            .collect::<Vec<_>>();
        ips.sort();
        ips.dedup();
        interaction.emit(OperationEvent::Output {
            text: format!("System DNS resolved {host}: {ips:?}"),
        });
        ips
    };
    if ips.is_empty() {
        return Err(ManagementError::Failed("DNS returned no address".into()));
    }
    let mut errors = Vec::new();
    for ip in ips.iter().take(8) {
        check_cancelled(cancelled)?;
        let route: Json = serde_json::from_str(&run(
            "ip",
            &["-j", "route", "get", &ip.to_string()],
            cancelled,
        )?)
        .map_err(|e| ManagementError::Failed(e.to_string()))?;
        if route[0]["dev"].as_str() != Some(name) {
            errors.push(format!("{ip}: system route uses another interface"));
            continue;
        }
        match TcpStream::connect_timeout(
            &std::net::SocketAddr::new(*ip, port),
            Duration::from_secs(5),
        ) {
            Ok(_) => {
                return Ok(format!(
                    "DNS/route check succeeded; TCP {host}:{port} was reachable through {name}"
                ));
            }
            Err(e) => errors.push(format!("{ip}: {e}")),
        }
    }
    Err(ManagementError::Failed(errors.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_interface_option_and_path_injection() {
        for name in ["", "-a", "eth0/other", "eth0\n", "1234567890123456"] {
            assert!(validate_interface(name).is_err());
        }
        for name in ["eth0", "enp0s1", "wlan0", "vlan.2"] {
            assert!(validate_interface(name).is_ok());
        }
    }
    #[test]
    fn configuration_change_detector_tracks_content() {
        assert_ne!(fingerprint(b"one"), fingerprint(b"two"));
    }
}
