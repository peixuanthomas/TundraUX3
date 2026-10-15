//! SSH service settings and public keys share a recoverable connection check.
use super::*;
use crate::ManagedCommand;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

mod connection;
mod transaction;
pub use transaction::{recover_pending, rollback_transaction};

const MAIN: &str = "/etc/ssh/sshd_config";
const SOCKET_DROPIN: &str = "/etc/systemd/system/ssh.socket.d/90-tundra-port.conf";
const MAX_OUTPUT: usize = 1024 * 1024;

fn failure(error: std::io::Error) -> ManagementError {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        ManagementError::PermissionDenied(error.to_string())
    } else {
        ManagementError::Failed(error.to_string())
    }
}
fn invalid(message: &str) -> ManagementError {
    ManagementError::InvalidInput(message.into())
}
fn check_cancelled(flag: &AtomicBool) -> Result<(), ManagementError> {
    if flag.load(Ordering::Acquire) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}
fn tool(name: &str) -> Result<PathBuf, ManagementError> {
    ["/usr/sbin", "/usr/bin", "/sbin", "/bin"]
        .into_iter()
        .map(|root| Path::new(root).join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            ManagementError::Unavailable(i18n::tr!("platform-ssh-tool-missing", name = name))
        })
}
fn run(name: &str, args: &[&str], flag: &AtomicBool) -> Result<String, ManagementError> {
    run_output(name, args, flag).map(|(stdout, _)| stdout)
}
fn run_output(
    name: &str,
    args: &[&str],
    flag: &AtomicBool,
) -> Result<(String, String), ManagementError> {
    use std::io::Read;
    use std::os::fd::AsRawFd;
    let mut child = Command::new(tool(name)?)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn_managed()
        .map_err(failure)?;
    let result = (|| {
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| invalid(&i18n::tr!("platform-ssh-missing-ssh-tool-output")))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| invalid(&i18n::tr!("platform-ssh-missing-ssh-tool-errors")))?;
        for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(failure(std::io::Error::last_os_error()));
            }
        }
        let mut out = Vec::new();
        let mut err = Vec::new();
        let started = Instant::now();
        let drain = |reader: &mut dyn Read, data: &mut Vec<u8>| -> Result<(), ManagementError> {
            let mut buffer = [0_u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => return Ok(()),
                    Ok(count) => {
                        data.extend_from_slice(&buffer[..count]);
                        if data.len() > MAX_OUTPUT {
                            return Err(invalid(&i18n::tr!(
                                "platform-ssh-ssh-tool-output-exceeded-1-mib"
                            )));
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(failure(error)),
                }
            }
        };
        loop {
            drain(&mut stdout, &mut out)?;
            drain(&mut stderr, &mut err)?;
            if let Some(status) = child.try_wait().map_err(failure)? {
                drain(&mut stdout, &mut out)?;
                drain(&mut stderr, &mut err)?;
                let out = String::from_utf8_lossy(&out).into_owned();
                let err = String::from_utf8_lossy(&err).into_owned();
                if !status.success() {
                    return Err(ManagementError::Failed(i18n::tr!(
                        "platform-ssh-tool-failed",
                        name = name,
                        error = runtime_log::sanitize_text(err.trim())
                    )));
                }
                return Ok((out, err));
            }
            check_cancelled(flag)?;
            if started.elapsed() > Duration::from_secs(30) {
                return Err(ManagementError::Failed(i18n::tr!(
                    "platform-ssh-tool-timeout",
                    name = name
                )));
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    if !matches!(child.try_wait(), Ok(Some(_))) {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}
fn properties(unit: &str, flag: &AtomicBool) -> Result<BTreeMap<String, String>, ManagementError> {
    Ok(run("systemctl", &["show", "--no-pager", unit], flag)?
        .lines()
        .filter_map(|line| line.split_once('=').map(|(k, v)| (k.into(), v.into())))
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Service {
    unit: String,
    socket: bool,
    socket_listen: String,
    main_pid: u32,
    reload_reason: Option<String>,
    #[serde(default)]
    original_ports: Vec<u16>,
}
fn discover(flag: &AtomicBool) -> Result<Service, ManagementError> {
    let socket = properties("ssh.socket", flag)?;
    let socket_active = socket.get("ActiveState").is_some_and(|s| s == "active");
    let mut selected = None;
    for unit in ["ssh.service", "sshd.service"] {
        let p = properties(unit, flag)?;
        if p.get("LoadState").is_some_and(|s| s == "loaded")
            && (p.get("ActiveState").is_some_and(|s| s == "active")
                || socket_active
                    && socket.get("Triggers").is_some_and(|s| {
                        s.split_whitespace()
                            .any(|name| name == p.get("Id").map(String::as_str).unwrap_or(unit))
                    }))
        {
            if selected
                .as_ref()
                .is_some_and(|(old, _): &(String, BTreeMap<String, String>)| {
                    old != &p.get("Id").cloned().unwrap_or_else(|| unit.into())
                })
            {
                return Err(ManagementError::Conflict(
                    i18n::tr!(
                        "platform-ssh-several-ssh-services-are-active-select-one-system-ssh-service"
                    )
                    .into(),
                ));
            }
            selected = Some((p.get("Id").cloned().unwrap_or_else(|| unit.into()), p));
        }
    }
    let (unit, p) = selected.ok_or_else(|| {
        ManagementError::Unavailable(
            i18n::tr!("platform-ssh-no-active-ssh-service-or-sshd-service-was-found").into(),
        )
    })?;
    let main_pid = p.get("MainPID").and_then(|v| v.parse().ok()).unwrap_or(0);
    let mut reason = None;
    if !socket_active && (main_pid == 0 || !connection::trusted_sshd(main_pid)) {
        reason =
            Some(i18n::tr!("platform-ssh-the-ssh-listener-process-could-not-be-verified").into());
    }
    if p.get("ExecStart").is_none_or(|start| custom_options(start))
        || main_pid != 0
            && fs::read(format!("/proc/{main_pid}/cmdline"))
                .is_ok_and(|cmd| custom_options(&String::from_utf8_lossy(&cmd).replace('\0', " ")))
    {
        reason = Some(
            i18n::tr!("platform-ssh-the-ssh-service-has-custom-command-line-settings-manage-them")
                .into(),
        );
    }
    if socket_active
        && (p.get("KillMode").map(String::as_str) != Some("process")
            || socket.get("Accept").map(String::as_str) != Some("no")
            || !socket
                .get("Triggers")
                .is_some_and(|value| value.split_whitespace().eq(std::iter::once(unit.as_str()))))
    {
        reason = Some(
            i18n::tr!(
                "platform-ssh-socket-activation-requires-accept-no-one-ssh-service-and-killmode"
            )
            .into(),
        );
    }
    let mut original_ports = connection::listening_ports(if socket_active { 1 } else { main_pid })
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>();
    if socket_active {
        let configured = socket_ports(socket.get("Listen").map(String::as_str).unwrap_or(""))?;
        original_ports.retain(|port| configured.contains(port));
    }
    Ok(Service {
        unit,
        socket: socket_active,
        socket_listen: socket.get("Listen").cloned().unwrap_or_default(),
        main_pid,
        reload_reason: reason,
        original_ports,
    })
}
fn custom_options(command: &str) -> bool {
    command
        .split_whitespace()
        .any(|word| word.starts_with("-f") || word.starts_with("-p") || word.starts_with("-o"))
}
fn effective(text: &str) -> BTreeMap<String, Vec<String>> {
    let mut result: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in text.lines() {
        if let Some((key, value)) = line.split_once(' ') {
            result.entry(key.into()).or_default().push(
                if key == "permitrootlogin" && value.trim() == "without-password" {
                    "prohibit-password".into()
                } else {
                    value.trim().into()
                },
            );
        }
    }
    result
}
fn first(settings: &BTreeMap<String, Vec<String>>, key: &str) -> String {
    settings
        .get(key)
        .and_then(|v| v.first())
        .cloned()
        .unwrap_or_else(|| i18n::tr!("platform-ssh-unknown"))
}
fn spec(
    username: &str,
    values: &BTreeMap<String, String>,
    port: u16,
) -> Result<String, ManagementError> {
    if username.is_empty() || username.contains([',', '\n', '\r', '\0']) {
        return Err(invalid(&i18n::tr!(
            "platform-ssh-this-account-name-cannot-be-used-in-an-ssh-connection"
        )));
    }
    let mut parts = vec![format!("user={username}"), format!("lport={port}")];
    for (key, default) in [
        ("host", "localhost"),
        ("addr", "127.0.0.1"),
        ("laddr", "127.0.0.1"),
    ] {
        let value = values.get(key).map(String::as_str).unwrap_or(default);
        if value.is_empty() || value.contains([',', '\n', '\r', '\0']) {
            return Err(invalid(&i18n::tr!(
                "platform-ssh-invalid-ssh-connection-context"
            )));
        }
        if key != "host" && value.parse::<std::net::IpAddr>().is_err() {
            return Err(invalid(&i18n::tr!(
                "platform-ssh-ssh-context-address-must-be-an-ip-address"
            )));
        }
        parts.push(format!("{key}={value}"));
    }
    Ok(parts.join(","))
}
fn account(username: &str) -> Result<users::UserAccount, ManagementError> {
    users::accounts(Some(username))?
        .into_iter()
        .find(|u| u.username == username)
        .ok_or_else(|| invalid(&i18n::tr!("platform-ssh-choose-an-existing-system-account")))
}
fn field(
    id: &str,
    label: impl Into<String>,
    value: impl Into<String>,
    choices: &[&str],
) -> ManagementField {
    ManagementField {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        choices: choices.iter().map(|s| (*s).into()).collect(),
        required: true,
        ..Default::default()
    }
}
fn action(
    id: &str,
    label: impl Into<String>,
    fields: Vec<ManagementField>,
    reason: Option<String>,
) -> ManagementAction {
    ManagementAction {
        id: id.into(),
        label: label.into(),
        fields,
        confirm: true,
        privileged: true,
        disabled_reason: reason,
        ..Default::default()
    }
}

pub fn query(
    query: &ManagementQuery,
    flag: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    let selected = query
        .target
        .as_deref()
        .or_else(|| query.options.get("username").map(String::as_str));
    let default = users::accounts(Some(&unsafe { libc::getuid() }.to_string()))?
        .into_iter()
        .next()
        .map(|user| user.username)
        .unwrap_or_else(|| "root".into());
    let username = selected.unwrap_or(&default);
    let mut request = query.clone();
    request.target = Some(username.into());
    let mut snapshot = match query_full(&request, flag) {
        Ok(snapshot) => snapshot,
        Err(error)
            if unsafe { libc::geteuid() } != 0
                && matches!(
                    &error,
                    ManagementError::PermissionDenied(_) | ManagementError::Failed(_)
                ) =>
        {
            let mut snapshot = ManagementSnapshot {
                columns: vec![
                    i18n::tr!("platform-ssh-service").into(),
                    i18n::tr!("platform-ssh-state").into(),
                ],
                notices: vec![i18n::tr!(
                    "platform-ssh-authorize-notice",
                    error = error.to_string()
                )],
                backend: i18n::tr!(
                    "platform-ssh-systemd-effective-ssh-settings-require-authorization"
                )
                .into(),
                ..Default::default()
            };
            for unit in ["ssh.service", "sshd.service", "ssh.socket"] {
                if let Ok(properties) = properties(unit, flag) {
                    snapshot.rows.push(ManagementRow {
                        id: unit.into(),
                        cells: vec![
                            unit.into(),
                            properties
                                .get("ActiveState")
                                .cloned()
                                .unwrap_or_else(|| i18n::tr!("platform-ssh-unknown").into()),
                        ],
                        ..Default::default()
                    });
                }
            }
            snapshot
        }
        Err(error) => return Err(error),
    };
    let accounts = users::accounts(None)?;
    snapshot.actions.push(ManagementAction {
        id: "set_view".into(),
        label: i18n::tr!("platform-ssh-choose-ssh-account").into(),
        fields: vec![ManagementField {
            id: "username".into(),
            label: i18n::tr!("platform-ssh-system-account").into(),
            value: username.into(),
            choices: accounts
                .into_iter()
                .filter(|user| user.local)
                .map(|user| user.username)
                .collect(),
            required: true,
            ..Default::default()
        }],
        ..Default::default()
    });
    if unsafe { libc::geteuid() } != 0 {
        snapshot.actions.push(ManagementAction {
            id: "inspect_ssh".into(),
            label: i18n::tr!("platform-ssh-authorize-reading-ssh-settings").into(),
            values: {
                let mut values = request.inspection_values();
                values.insert("username".into(), username.into());
                values
            },
            privileged: true,
            ..Default::default()
        });
    }
    Ok(snapshot)
}

fn query_full(
    query: &ManagementQuery,
    flag: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    check_cancelled(flag)?;
    let service = discover(flag)?;
    let username = query
        .target
        .as_deref()
        .or_else(|| query.options.get("username").map(String::as_str))
        .unwrap_or("root");
    let user = account(username)?;
    let context = spec(username, &query.options, 22)?;
    let data = effective(&run("sshd", &["-T", "-f", MAIN, "-C", &context], flag)?);
    let root_context = spec("root", &query.options, 22)?;
    let root = effective(&run(
        "sshd",
        &["-T", "-f", MAIN, "-C", &root_context],
        flag,
    )?);
    let ports = data.get("port").cloned().unwrap_or_default().join(", ");
    let mut details = vec![
        (
            i18n::tr!("platform-ssh-service").into(),
            service.unit.clone(),
        ),
        (
            i18n::tr!("platform-ssh-activation").into(),
            if service.socket {
                "ssh.socket".into()
            } else {
                i18n::tr!("platform-ssh-sshd-listener").into()
            },
        ),
        (
            i18n::tr!("platform-ssh-socket-listeners").into(),
            service.socket_listen.clone(),
        ),
        (i18n::tr!("platform-ssh-match-context").into(), context),
        (
            i18n::tr!("platform-ssh-authenticationmethods").into(),
            first(&data, "authenticationmethods").into(),
        ),
        (
            i18n::tr!("platform-ssh-kbdinteractiveauthentication").into(),
            first(&data, "kbdinteractiveauthentication").into(),
        ),
        (
            i18n::tr!("platform-ssh-usepam").into(),
            first(&data, "usepam").into(),
        ),
        (
            i18n::tr!("platform-ssh-authorizedkeyscommand").into(),
            first(&data, "authorizedkeyscommand").into(),
        ),
    ];
    let mut notices = vec![
        i18n::tr!("platform-ssh-effective-login-settings-are-evaluated-for-the-displayed-user-and")
            .into(),
    ];
    match connection::listening_ports(if service.socket { 1 } else { service.main_pid }) {
        Ok(ports) => {
            let socket_ports = if service.socket {
                socket_ports(&service.socket_listen)?
            } else {
                Vec::new()
            };
            let relevant: Vec<_> = ports
                .into_iter()
                .filter(|port| !service.socket || socket_ports.contains(port))
                .map(|port| port.to_string())
                .collect();
            details.push((
                i18n::tr!("platform-ssh-listening-tcp-ports").into(),
                relevant.join(", "),
            ));
        }
        Err(error) => notices.push(i18n::tr!(
            "platform-ssh-ports-unavailable",
            error = error.to_string()
        )),
    }
    match users::ssh_public_keys(&user) {
        Ok(keys) => {
            for (fingerprint, path) in keys {
                details.push((
                    i18n::tr!("platform-ssh-public-key").into(),
                    format!("{fingerprint}  {}", path.display()),
                ));
            }
        }
        Err(e) => notices.push(i18n::tr!(
            "platform-ssh-keys-unavailable",
            error = e.to_string()
        )),
    }
    if let Some(reason) = &service.reload_reason {
        notices.push(reason.clone());
    }
    let mut row = ManagementRow {
        id: username.into(),
        cells: vec![
            service.unit.clone(),
            ports.clone(),
            first(&data, "passwordauthentication").into(),
            first(&data, "pubkeyauthentication").into(),
            first(&root, "permitrootlogin").into(),
            username.into(),
        ],
        detail: details,
        identity: BTreeMap::from([
            ("uid".into(), user.uid.to_string()),
            ("gid".into(), user.gid.to_string()),
            ("home".into(), user.home.display().to_string()),
        ]),
        ..Default::default()
    };
    row.actions = vec![
        action(
            "ssh_configure",
            i18n::tr!("platform-ssh-configure-ssh"),
            vec![
                field(
                    "port",
                    i18n::tr!("platform-ssh-tcp-port"),
                    data.get("port")
                        .and_then(|p| p.first())
                        .map(String::as_str)
                        .unwrap_or("22"),
                    &[],
                ),
                field(
                    "password_authentication",
                    i18n::tr!("platform-ssh-password-authentication"),
                    first(&data, "passwordauthentication"),
                    &["yes", "no"],
                ),
                field(
                    "pubkey_authentication",
                    i18n::tr!("platform-ssh-pubkey-authentication"),
                    first(&data, "pubkeyauthentication"),
                    &["yes", "no"],
                ),
                field(
                    "permit_root_login",
                    i18n::tr!("platform-ssh-permit-root-login"),
                    first(&root, "permitrootlogin"),
                    &["yes", "prohibit-password", "forced-commands-only", "no"],
                ),
                field(
                    "open_firewall",
                    i18n::tr!("platform-ssh-allow-new-port-in-firewall"),
                    "false",
                    &["false", "true"],
                ),
            ],
            service.reload_reason.clone(),
        ),
        action(
            "user_ssh_add",
            i18n::tr!("platform-ssh-add-ssh-public-key"),
            vec![field(
                "public_key",
                i18n::tr!("platform-ssh-public-key"),
                "",
                &[],
            )],
            service.reload_reason.clone(),
        ),
        action(
            "user_ssh_remove",
            i18n::tr!("platform-ssh-remove-ssh-public-key"),
            vec![field(
                "fingerprint",
                i18n::tr!("platform-ssh-sha256-fingerprint"),
                "",
                &[],
            )],
            service.reload_reason.clone(),
        ),
    ];
    row.actions.push(ManagementAction {
        id: "edit_system_config".into(),
        label: i18n::tr!("platform-ssh-edit-ssh-configuration").into(),
        values: BTreeMap::from([
            ("path".into(), MAIN.into()),
            ("validator".into(), "sshd".into()),
            ("service".into(), service.unit.clone()),
            ("scope".into(), "system".into()),
        ]),
        ..Default::default()
    });
    Ok(ManagementSnapshot {
        columns: [
            i18n::tr!("platform-ssh-service"),
            i18n::tr!("platform-ssh-configured-ports"),
            i18n::tr!("platform-ssh-password-login"),
            i18n::tr!("platform-ssh-public-key-login"),
            i18n::tr!("platform-ssh-root-login"),
            i18n::tr!("platform-ssh-user"),
        ]
        .into_iter()
        .map(String::from)
        .collect(),
        rows: vec![row],
        notices,
        backend: i18n::tr!("platform-ssh-openssh-and-systemd").into(),
        details_complete: true,
        ..Default::default()
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Change {
    before: ConfigDocument,
    after: String,
    after_existed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Plan {
    service: Service,
    files: Vec<Change>,
    ports: Vec<u16>,
    key_command: Option<ManagementCommand>,
    backup_to_restore: Option<String>,
    seed_backup: Option<String>,
    #[serde(default)]
    remove_firewall: Vec<super::firewall::OwnedPortRule>,
    #[serde(default)]
    add_firewall: Vec<super::firewall::OwnedPortRule>,
}

fn confirmation_ports(ports: &[u16], previous: &[u16]) -> Vec<u16> {
    let added = ports
        .iter()
        .copied()
        .filter(|port| !previous.contains(port))
        .collect::<Vec<_>>();
    if added.is_empty() {
        ports.to_vec()
    } else {
        added
    }
}
fn ensure_main(files: &mut Vec<(PathBuf, String)>) -> Result<(), ManagementError> {
    if !files.iter().any(|(path, _)| path == Path::new(MAIN)) {
        files.push((MAIN.into(), system_config::read(Path::new(MAIN))?.content));
    }
    Ok(())
}
fn parse_port(value: &str) -> Result<u16, ManagementError> {
    value.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| {
        invalid(&i18n::tr!(
            "platform-ssh-ssh-port-must-be-between-1-and-65535"
        ))
    })
}
fn configured_ports(data: &BTreeMap<String, Vec<String>>) -> Result<Vec<u16>, ManagementError> {
    data.get("port")
        .ok_or_else(|| {
            invalid(&i18n::tr!(
                "platform-ssh-sshd-did-not-report-a-listening-port"
            ))
        })?
        .iter()
        .map(|p| parse_port(p))
        .collect()
}
fn directive(line: &str) -> (&str, &str) {
    let line = line.trim_start();
    let end = line
        .find(|c: char| c.is_ascii_whitespace() || c == '=')
        .unwrap_or(line.len());
    (
        &line[..end],
        line[end..].trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '='),
    )
}
fn rewrite(text: &str, main: bool, port: u16, password: &str, pubkey: &str, root: &str) -> String {
    let mut output = String::new();
    if main {
        output = format!(
            "Port {port}\nPasswordAuthentication {password}\nPubkeyAuthentication {pubkey}\nPermitRootLogin {root}\n"
        );
    }
    let mut matched = false;
    for line in text.split_inclusive('\n') {
        let (key, _) = directive(line);
        if key.eq_ignore_ascii_case("match") {
            matched = true;
        }
        if key.eq_ignore_ascii_case("port")
            || main
                && !matched
                && [
                    "PasswordAuthentication",
                    "PubkeyAuthentication",
                    "PermitRootLogin",
                ]
                .iter()
                .any(|k| key.eq_ignore_ascii_case(k))
        {
            continue;
        }
        output.push_str(line);
    }
    output
}
fn traced_files(trace: &str) -> Result<Vec<PathBuf>, ManagementError> {
    let mut files = vec![PathBuf::from(MAIN)];
    for line in trace.lines() {
        if let Some((_, rest)) = line.split_once("parse_server_config_depth: config ")
            && let Some((path, _)) = rest.rsplit_once(" len ")
        {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err(invalid(&i18n::tr!(
                    "platform-ssh-openssh-reported-a-relative-include-path"
                )));
            }
            if !files.contains(&path) {
                files.push(path);
            }
        }
    }
    Ok(files)
}
fn socket_ports(listen: &str) -> Result<Vec<u16>, ManagementError> {
    let mut ports = Vec::new();
    for listener in listen.split(" (Stream)").filter(|s| !s.trim().is_empty()) {
        let port = listener
            .trim()
            .parse::<u16>()
            .ok()
            .or_else(|| {
                listener
                    .trim()
                    .parse::<std::net::SocketAddr>()
                    .ok()
                    .map(|address| address.port())
            })
            .ok_or_else(|| {
                invalid(&i18n::tr!(
                    "platform-ssh-the-ssh-socket-has-an-unsupported-listener"
                ))
            })?;
        if !ports.contains(&port) {
            ports.push(port);
        }
    }
    Ok(ports)
}
fn socket_content(listen: &str, port: u16) -> Result<String, ManagementError> {
    let mut values = Vec::new();
    for listener in listen.split(" (Stream)").filter(|s| !s.trim().is_empty()) {
        let listener = listener.trim();
        let value = if listener.parse::<u16>().is_ok() {
            port.to_string()
        } else if let Ok(address) = listener.parse::<std::net::SocketAddr>() {
            std::net::SocketAddr::new(address.ip(), port).to_string()
        } else {
            return Err(invalid(&i18n::tr!(
                "platform-ssh-ssh-socket-has-a-listener-that-cannot-be-changed-safely"
            )));
        };
        if !values.contains(&value) {
            values.push(value);
        }
    }
    if values.is_empty() {
        return Err(invalid(&i18n::tr!(
            "platform-ssh-ssh-socket-did-not-report-its-tcp-listeners"
        )));
    }
    Ok(format!(
        "[Socket]\nListenStream=\n{}",
        values
            .into_iter()
            .map(|v| format!("ListenStream={v}\n"))
            .collect::<String>()
    ))
}
fn configure(command: &ManagementCommand, flag: &AtomicBool) -> Result<Plan, ManagementError> {
    let service = discover(flag)?;
    if let Some(reason) = &service.reload_reason {
        return Err(ManagementError::Unavailable(reason.clone()));
    }
    let username = command
        .target
        .as_deref()
        .ok_or_else(|| invalid(&i18n::tr!("platform-ssh-choose-an-ssh-account")))?;
    let selected = account(username)?;
    if command.identity.get("uid") != Some(&selected.uid.to_string())
        || command.identity.get("gid") != Some(&selected.gid.to_string())
        || command.identity.get("home") != Some(&selected.home.display().to_string())
    {
        return Err(ManagementError::Conflict(
            i18n::tr!("platform-ssh-the-ssh-account-changed-refresh-and-retry").into(),
        ));
    }
    let get = |key: &str| command.values.get(key).map(String::as_str).unwrap_or("");
    let port = parse_port(get("port"))?;
    let password = get("password_authentication");
    let pubkey = get("pubkey_authentication");
    let root = get("permit_root_login");
    if !["yes", "no"].contains(&password)
        || !["yes", "no"].contains(&pubkey)
        || !["yes", "prohibit-password", "forced-commands-only", "no"].contains(&root)
    {
        return Err(invalid(&i18n::tr!(
            "platform-ssh-choose-valid-openssh-authentication-settings"
        )));
    }
    if password == "no" && pubkey == "no" {
        return Err(invalid(&i18n::tr!(
            "platform-ssh-keep-password-or-public-key-authentication-enabled"
        )));
    }
    let (_, trace) = run_output("sshd", &["-t", "-ddd", "-f", MAIN], flag)?;
    let mut files = Vec::new();
    for path in traced_files(&trace)? {
        let before = system_config::read(&path)?;
        let after = rewrite(
            &before.content,
            path == Path::new(MAIN),
            port,
            password,
            pubkey,
            root,
        );
        if before.content != after {
            files.push(Change {
                before,
                after,
                after_existed: true,
            });
        }
    }
    let mut candidates: Vec<_> = files
        .iter()
        .map(|f| (f.before.path.clone(), f.after.clone()))
        .collect();
    ensure_main(&mut candidates)?;
    let username = command.target.as_deref().unwrap_or("root");
    account(username)?;
    let context = spec(username, &command.values, port)?;
    let data = effective(&system_config::check_ssh_candidates(
        &candidates,
        &context,
        flag,
    )?);
    let root_context = spec("root", &command.values, port)?;
    let root_data = effective(&system_config::check_ssh_candidates(
        &candidates,
        &root_context,
        flag,
    )?);
    if data.get("listenaddress").is_some_and(|values| {
        values.iter().any(|address| {
            address
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<std::net::SocketAddr>().ok())
                .is_none_or(|a| a.port() != port)
        })
    }) || configured_ports(&data)? != vec![port]
        || first(&data, "passwordauthentication") != password
        || first(&data, "pubkeyauthentication") != pubkey
        || first(&root_data, "permitrootlogin") != root
    {
        return Err(ManagementError::Conflict(i18n::tr!("platform-ssh-an-include-match-or-listenaddress-directive-overrides-these-settings-review").into()));
    }
    if service.socket {
        let before = system_config::read(Path::new(SOCKET_DROPIN))?;
        let after = socket_content(&service.socket_listen, port)?;
        if before.content != after {
            files.push(Change {
                before,
                after,
                after_existed: true,
            });
        }
    }
    Ok(Plan {
        service,
        files,
        ports: vec![port],
        key_command: None,
        backup_to_restore: None,
        seed_backup: None,
        remove_firewall: Vec::new(),
        add_firewall: Vec::new(),
    })
}
fn load(service: &Service, flag: &AtomicBool) -> Result<(), ManagementError> {
    if service.socket {
        let fresh = discover(flag)?;
        if !fresh.socket || fresh.unit != service.unit || fresh.reload_reason.is_some() {
            return Err(ManagementError::Conflict(
                i18n::tr!("platform-ssh-ssh-activation-changed-before-loading").into(),
            ));
        }
        run("systemctl", &["daemon-reload"], flag)?;
        run("systemctl", &["stop", &service.unit, "ssh.socket"], flag)?;
        run("systemctl", &["start", "ssh.socket"], flag)?;
        run("systemctl", &["start", &service.unit], flag)?;
    } else {
        let fresh = discover(flag)?;
        if fresh.socket
            || fresh.unit != service.unit
            || fresh.reload_reason.is_some()
            || !connection::trusted_sshd(fresh.main_pid)
        {
            return Err(ManagementError::Conflict(
                i18n::tr!("platform-ssh-ssh-listener-changed-before-loading").into(),
            ));
        }
        let previous = connection::listener_inodes(fresh.main_pid)?;
        if previous.is_empty() {
            return Err(ManagementError::Conflict(
                i18n::tr!("platform-ssh-the-ssh-process-owns-no-listening-tcp-socket").into(),
            ));
        }
        if unsafe { libc::kill(fresh.main_pid as i32, libc::SIGHUP) } != 0 {
            return Err(failure(std::io::Error::last_os_error()));
        }
        let started = Instant::now();
        loop {
            check_cancelled(flag)?;
            let current = connection::listener_inodes(fresh.main_pid)?;
            if !current.is_empty() && current.is_disjoint(&previous) {
                break;
            }
            if started.elapsed() > Duration::from_secs(10) {
                return Err(ManagementError::Failed(
                    i18n::tr!("platform-ssh-the-ssh-listener-did-not-finish-reloading").into(),
                ));
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        check_cancelled(flag)?;
        let p = properties(&service.unit, flag)?;
        if p.get("ActiveState").is_some_and(|s| s == "active") {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(ManagementError::Failed(
                i18n::tr!("platform-ssh-ssh-did-not-become-active-after-loading").into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn verify_plan(plan: &Plan, flag: &AtomicBool) -> Result<(), ManagementError> {
    let active = discover(flag)?;
    if active.unit != plan.service.unit || active.socket != plan.service.socket {
        return Err(ManagementError::Conflict(
            i18n::tr!("platform-ssh-ssh-activation-changed").into(),
        ));
    }
    let listeners = connection::listening_ports(if active.socket { 1 } else { active.main_pid })?;
    if active.socket && socket_ports(&active.socket_listen)? != plan.ports {
        return Err(ManagementError::Conflict(
            i18n::tr!("platform-ssh-the-ssh-socket-listeners-changed").into(),
        ));
    }
    if !plan.ports.iter().all(|p| listeners.contains(p)) {
        return Err(ManagementError::Failed(
            i18n::tr!("platform-ssh-the-ssh-listener-did-not-open-the-requested-tcp-port").into(),
        ));
    }
    for file in &plan.files {
        let actual = system_config::read(&file.before.path)?;
        if actual.content != file.after || actual.existed != file.after_existed {
            return Err(ManagementError::Conflict(
                i18n::tr!("platform-ssh-ssh-configuration-changed-while-waiting-for-confirmation")
                    .into(),
            ));
        }
    }
    Ok(())
}

pub fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    check_cancelled(flag)?;
    if unsafe { libc::geteuid() } != 0 {
        return Err(ManagementError::PermissionDenied(
            i18n::tr!("platform-ssh-ssh-changes-require-system-authorization").into(),
        ));
    }
    if command.action == "inspect_ssh" {
        let default = users::accounts(Some(&context.actor_uid.to_string()))?
            .into_iter()
            .next()
            .map(|user| user.username)
            .ok_or_else(|| {
                invalid(&i18n::tr!(
                    "platform-ssh-the-current-system-account-could-not-be-found"
                ))
            })?;
        let mut query = ManagementQuery::from_inspection(command);
        query.target = Some(
            command
                .values
                .get("username")
                .cloned()
                .or_else(|| query.target.clone())
                .unwrap_or(default),
        );
        io.emit(OperationEvent::Snapshot {
            snapshot: self::query(&query, flag)?.with_authorized_refresh(command),
        });
        return Ok(i18n::tr!("platform-ssh-protected-ssh-settings-were-read").into());
    }
    match command.action.as_str() {
        "ssh_configure" => transaction::apply(
            configure(command, flag)?,
            command
                .values
                .get("open_firewall")
                .is_some_and(|s| s == "true"),
            context,
            io,
            flag,
        ),
        "user_ssh_add" | "user_ssh_remove" => execute_key_change(command, context, io, flag),
        "ssh_confirm" => transaction::confirm(
            command
                .target
                .as_deref()
                .ok_or_else(|| invalid(&i18n::tr!("platform-ssh-choose-an-ssh-transaction")))?,
            context,
            true,
            flag,
        ),
        "ssh_cancel" => transaction::confirm(
            command
                .target
                .as_deref()
                .ok_or_else(|| invalid(&i18n::tr!("platform-ssh-choose-an-ssh-transaction")))?,
            context,
            false,
            flag,
        ),
        "ssh_status" => {
            io.emit(OperationEvent::Snapshot {
                snapshot: transaction::status(
                    command.target.as_deref().ok_or_else(|| {
                        invalid(&i18n::tr!("platform-ssh-choose-an-ssh-transaction"))
                    })?,
                    context.actor_uid,
                )?,
            });
            Ok(i18n::tr!("platform-ssh-ssh-transaction-status-read").into())
        }
        _ => Err(invalid(&i18n::tr!("platform-ssh-unknown-ssh-operation"))),
    }
}
fn key_account(command: &ManagementCommand) -> Result<users::UserAccount, ManagementError> {
    let username = command
        .target
        .as_deref()
        .ok_or_else(|| invalid(&i18n::tr!("platform-ssh-choose-an-account")))?;
    let user = account(username)?;
    if command.identity.get("uid") != Some(&user.uid.to_string())
        || command.identity.get("gid") != Some(&user.gid.to_string())
        || command.identity.get("home") != Some(&user.home.display().to_string())
    {
        return Err(ManagementError::Conflict(
            i18n::tr!("platform-ssh-the-account-changed-refresh-before-changing-keys").into(),
        ));
    }
    if !user.local {
        return Err(ManagementError::PermissionDenied(
            i18n::tr!("platform-ssh-directory-service-accounts-are-read-only").into(),
        ));
    }
    Ok(user)
}

pub(crate) fn execute_key_change(
    command: &ManagementCommand,
    context: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let user = key_account(command)?;
    let service = discover(flag)?;
    if let Some(reason) = &service.reload_reason {
        return Err(ManagementError::Unavailable(reason.clone()));
    }
    let mut files = Vec::new();
    for (path, after) in users::prepare_ssh_key_change(command, &user)? {
        files.push(Change {
            before: system_config::read(&path)?,
            after,
            after_existed: true,
        });
    }
    let context_spec = spec(&user.username, &command.values, 22)?;
    let mut candidates = Vec::new();
    ensure_main(&mut candidates)?;
    let data = effective(&system_config::check_ssh_candidates(
        &candidates,
        &context_spec,
        flag,
    )?);
    let ports = if service.socket {
        socket_ports(&service.socket_listen)?
    } else {
        configured_ports(&data)?
    };
    transaction::apply(
        Plan {
            service,
            files,
            ports,
            key_command: Some(command.clone()),
            backup_to_restore: None,
            seed_backup: None,
            remove_firewall: Vec::new(),
            add_firewall: Vec::new(),
        },
        false,
        context,
        io,
        flag,
    )
}

pub(crate) fn activate_saved_configuration(
    command: &ManagementCommand,
    context: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let id = command.values.get("backup_id").ok_or_else(|| {
        invalid(&i18n::tr!(
            "platform-ssh-reload-ssh-from-a-saved-change-with-its-backup-id"
        ))
    })?;
    super::config_backups::validate_current(id)?;
    let service = discover(flag)?;
    if let Some(reason) = &service.reload_reason {
        return Err(ManagementError::Unavailable(reason.clone()));
    }
    let mut files = Vec::new();
    for (before, after) in super::config_backups::snapshots(id)? {
        if !before.path.starts_with("/etc/ssh/") {
            return Err(invalid(&i18n::tr!(
                "platform-ssh-the-selected-backup-is-not-an-ssh-configuration-change"
            )));
        }
        files.push(Change {
            before,
            after: after.content,
            after_existed: after.existed,
        });
    }
    let mut candidates: Vec<_> = files
        .iter()
        .map(|file| (file.before.path.clone(), file.after.clone()))
        .collect();
    ensure_main(&mut candidates)?;
    let username = users::accounts(Some(&context.actor_uid.to_string()))?
        .into_iter()
        .next()
        .map(|user| user.username)
        .ok_or_else(|| {
            invalid(&i18n::tr!(
                "platform-ssh-the-current-account-could-not-be-found"
            ))
        })?;
    let data = effective(&system_config::check_ssh_candidates(
        &candidates,
        &spec(&username, &command.values, 22)?,
        flag,
    )?);
    let ports = configured_ports(&data)?;
    system_config::check_ssh_candidates(
        &candidates,
        &spec(&username, &command.values, ports[0])?,
        flag,
    )?;
    if service.socket {
        if ports.len() != 1 {
            return Err(invalid(&i18n::tr!(
                "platform-ssh-the-ssh-socket-editor-supports-one-tcp-port-use-the"
            )));
        }
        let before = system_config::read(Path::new(SOCKET_DROPIN))?;
        let after = socket_content(&service.socket_listen, ports[0])?;
        if before.content != after {
            files.push(Change {
                before,
                after,
                after_existed: true,
            });
        }
    }
    transaction::apply(
        Plan {
            service,
            files,
            ports,
            key_command: None,
            backup_to_restore: None,
            seed_backup: Some(id.clone()),
            remove_firewall: Vec::new(),
            add_firewall: Vec::new(),
        },
        false,
        context,
        io,
        flag,
    )
}

pub(crate) fn restore_backup(
    payload: &serde_json::Value,
    context: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    transaction::restore_backup(payload, context, io, flag)
}

#[cfg(test)]
#[path = "../../tests/unit/management/ssh_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/unit/management/ssh_confirmation_ports_tests.rs"]
mod confirmation_ports_tests;
