use super::config::{OwnerKind, Plan};
use super::*;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const ROOT: &str = "/run/tundraux3-network";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum State {
    Prepared,
    Applying,
    AwaitingConfirmation,
    Committed,
    Restored,
    RestoreFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Journal {
    #[serde(default)]
    actor_uid: u32,
    plan: Plan,
    state: State,
    deadline: u64,
    result: String,
    #[serde(default)]
    candidate_nm_version: Option<String>,
}

struct Lock(File);
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn lock(directory: &Path) -> Result<Lock, ManagementError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(directory.join("lock"))
        .map_err(io_error)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io_error(std::io::Error::last_os_error()));
    }
    Ok(Lock(file))
}

fn now() -> u64 {
    // A monotonic clock keeps a wall-clock/NTP correction from delaying recovery.
    let mut time = std::mem::MaybeUninit::<libc::timespec>::zeroed();
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, time.as_mut_ptr()) } == 0 {
        unsafe { time.assume_init() }.tv_sec.max(0) as u64
    } else {
        0
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id.bytes().all(|b| b.is_ascii_digit() || b == b'-')
        && id.bytes().any(|b| b.is_ascii_digit())
}

fn private_directory(path: &Path) -> Result<(), ManagementError> {
    match fs::symlink_metadata(path) {
        Ok(m)
            if m.is_dir()
                && !m.file_type().is_symlink()
                && m.uid() == 0
                && m.mode() & 0o077 == 0 =>
        {
            Ok(())
        }
        Ok(_) => Err(ManagementError::PermissionDenied(format!(
            "Recovery directory {} is not private and root-owned",
            path.display()
        ))),
        Err(e) => Err(io_error(e)),
    }
}

fn trusted_helper(path: &Path) -> Result<(), ManagementError> {
    let canonical = fs::canonicalize(path).map_err(io_error)?;
    let m = fs::symlink_metadata(path).map_err(io_error)?;
    if !path.is_absolute()
        || canonical != path
        || !m.is_file()
        || m.uid() != 0
        || m.mode() & 0o022 != 0
    {
        return Err(ManagementError::PermissionDenied(
            "Independent recovery requires an immutable root-owned helper copy".into(),
        ));
    }
    for parent in path.ancestors().skip(1) {
        let m = fs::symlink_metadata(parent).map_err(io_error)?;
        if m.uid() != 0 || m.mode() & 0o022 != 0 && m.mode() & 0o1000 == 0 {
            return Err(ManagementError::PermissionDenied(
                "Recovery helper has a parent directory that another user can modify".into(),
            ));
        }
    }
    Ok(())
}

fn write_journal(directory: &Path, journal: &Journal) -> Result<(), ManagementError> {
    let bytes = serde_json::to_vec(journal).map_err(|e| ManagementError::Failed(e.to_string()))?;
    write_atomic(&directory.join("journal.json"), &bytes, 0o600)
}

fn read_journal(directory: &Path) -> Result<Journal, ManagementError> {
    let path = directory.join("journal.json");
    let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != 0
        || metadata.mode() & 0o077 != 0
        || metadata.len() > 16 * 1024 * 1024
    {
        return Err(ManagementError::PermissionDenied(
            "Recovery journal is not a bounded root-private regular file".into(),
        ));
    }
    serde_json::from_slice(&fs::read(path).map_err(io_error)?)
        .map_err(|e| ManagementError::Failed(format!("Invalid recovery journal: {e}")))
}

pub(super) fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), ManagementError> {
    let parent = path
        .parent()
        .ok_or_else(|| ManagementError::InvalidInput("File has no parent".into()))?;
    let temporary = parent.join(format!(
        ".tundra-network-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode & 0o777)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&temporary)
            .map_err(io_error)?;
        file.write_all(bytes).map_err(io_error)?;
        file.set_permissions(fs::Permissions::from_mode(mode & 0o777))
            .map_err(io_error)?;
        file.sync_all().map_err(io_error)?;
        fs::rename(&temporary, path).map_err(io_error)?;
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(io_error)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn validate_stage(
    plan: &Plan,
    directory: &Path,
    cancelled: &AtomicBool,
) -> Result<(), ManagementError> {
    if plan.kind == OwnerKind::Netplan {
        let stage = directory.join("stage");
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&stage)
            .map_err(io_error)?;
        for root in ["lib/netplan", "etc/netplan", "run/netplan"] {
            let destination = stage.join(root);
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&destination)
                .map_err(io_error)?;
            if let Ok(entries) = fs::read_dir(Path::new("/").join(root)) {
                for entry in entries {
                    let entry = entry.map_err(io_error)?;
                    if !entry
                        .path()
                        .extension()
                        .is_some_and(|e| matches!(e.to_str(), Some("yaml" | "yml")))
                    {
                        continue;
                    }
                    config::checked_file(&entry.path())?;
                    fs::copy(entry.path(), destination.join(entry.file_name()))
                        .map_err(io_error)?;
                }
            }
        }
        for change in &plan.files {
            let relative = change.path.strip_prefix("/").map_err(|_| {
                ManagementError::InvalidInput("Configuration path must be absolute".into())
            })?;
            write_atomic(&stage.join(relative), &change.after, change.mode)?;
        }
        run(
            "netplan",
            &[
                "generate",
                "--root-dir",
                stage
                    .to_str()
                    .ok_or_else(|| ManagementError::InvalidInput("Invalid staging path".into()))?,
            ],
            cancelled,
        )?;
    } else if plan.kind == OwnerKind::Interfaces {
        let file = directory.join("interfaces-candidate");
        write_atomic(&file, &plan.files[0].after, 0o600)?;
        run(
            "ifquery",
            &["--interfaces", file.to_str().unwrap(), &plan.interface],
            cancelled,
        )?;
    }
    Ok(())
}

pub(super) fn apply(
    plan: Plan,
    context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    if let Some(reason) = recovery_unavailable() {
        return Err(ManagementError::Unavailable(reason));
    }
    trusted_helper(&context.helper_path)?;
    // One unfinished transaction system-wide avoids overlapping generated-file and DNS edits.
    if !Path::new(ROOT).exists() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(ROOT)
            .map_err(io_error)?;
    }
    private_directory(Path::new(ROOT))?;
    let _global = lock(Path::new(ROOT))?;
    for entry in fs::read_dir(ROOT).map_err(io_error)?.flatten() {
        if !entry.file_type().map_err(io_error)?.is_dir() {
            continue;
        }
        if let Ok(journal) = read_journal(&entry.path()) {
            if !matches!(journal.state, State::Committed | State::Restored) {
                return Err(ManagementError::Conflict(format!(
                    "An earlier network change still needs recovery: {}",
                    entry.file_name().to_string_lossy()
                )));
            }
        }
    }
    let id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let directory = Path::new(ROOT).join(&id);
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(io_error)?;
    let mut stored = plan.clone();
    if let Some(wifi) = stored.wifi.as_mut() {
        use zeroize::Zeroize;
        wifi.password.zeroize();
    }
    let mut journal = Journal {
        actor_uid: context.actor_uid,
        plan: stored,
        state: State::Prepared,
        deadline: now() + 180,
        result: String::new(),
        candidate_nm_version: None,
    };
    write_journal(&directory, &journal)?;
    if let Err(error) = validate_stage(&plan, &directory, cancelled) {
        journal.state = State::Restored;
        journal.result = format!("No system files were changed: {error}");
        write_journal(&directory, &journal)?;
        return Err(error);
    }
    let helper = context
        .helper_path
        .to_str()
        .ok_or_else(|| ManagementError::InvalidInput("Recovery helper path is not UTF-8".into()))?;
    let unit = format!("tundra-network-rollback-{id}");
    interaction.emit(OperationEvent::Progress {
        message: "Starting independent network recovery before applying changes".into(),
        percent: None,
    });
    if let Err(error) = run(
        "systemd-run",
        &[
            "--quiet",
            "--collect",
            "--service-type=exec",
            "--unit",
            &unit,
            "--property=Restart=on-failure",
            "--property=RestartSec=1",
            "--property=RuntimeMaxSec=600",
            "--",
            helper,
            "__network-rollback",
            &id,
        ],
        cancelled,
    ) {
        journal.state = State::Restored;
        journal.result =
            format!("Recovery service did not start; no changes were applied: {error}");
        write_journal(&directory, &journal)?;
        return Err(error);
    }
    let readiness = Instant::now();
    while !directory.join("ready").is_file() {
        if readiness.elapsed() > Duration::from_secs(15) {
            return Err(ManagementError::Unavailable(format!(
                "Recovery service did not become ready; no changes were applied. Journal: {id}"
            )));
        }
        check_cancelled(cancelled)?;
        std::thread::sleep(Duration::from_millis(30));
    }
    {
        let _lock = lock(&directory)?;
        journal = read_journal(&directory)?;
        if journal.state != State::Prepared || now() >= journal.deadline {
            return Err(ManagementError::Conflict(
                "Recovery already started before applying changes".into(),
            ));
        }
        if let Err(error) = validate_plan(&plan) {
            journal.state = State::Restored;
            journal.result = format!("No network changes were applied: {error}");
            write_journal(&directory, &journal)?;
            return Err(error);
        }
        journal.state = State::Applying;
        write_journal(&directory, &journal)?;
        let mut candidate_nm_version = None;
        guarded_apply(
            || {
                apply_configuration(&plan, cancelled).and_then(|_| {
                    if let Some(wifi) = &plan.wifi {
                        verify_wifi(&plan, wifi, cancelled)
                    } else {
                        Ok(())
                    }
                })?;
                if plan.kind == OwnerKind::NetworkManager {
                    let uuid = expected_active_uuid(&plan);
                    if !uuid.is_empty() {
                        candidate_nm_version = Some(nm_configuration_version(uuid)?);
                    }
                }
                Ok(())
            },
            || restore(&mut journal, &directory),
            &directory,
        )?;
        journal.candidate_nm_version = candidate_nm_version;
        journal.state = State::AwaitingConfirmation;
        journal.deadline = now() + 120;
        write_journal(&directory, &journal)?;
    }
    drop(_global);
    interaction.emit(OperationEvent::Progress {
        message: if plan.wifi.is_some() {
            "Selected Wi-Fi and address verified. Saving the connection.".into()
        } else {
            "Applied temporarily. Confirm within 120 seconds.".into()
        },
        percent: None,
    });
    interaction.emit(OperationEvent::Output {
        text: format!("Network transaction: {id}"),
    });
    if plan.defer_confirmation && plan.wifi.is_none() {
        return Ok(format!(
            "Network transaction {id} awaits confirmation for 120 seconds. Run network confirm with this ID after checking the connection."
        ));
    }
    let answer = if plan.wifi.is_some() {
        Ok("Keep".into())
    } else {
        interaction.ask(
        "network-confirm-120",
        "Keep this network configuration? Unconfirmed changes will be restored after 120 seconds.",
        &["Keep".into(), "Restore".into()],
        false,
    )
    };
    let _lock = lock(&directory)?;
    journal = read_journal(&directory)?;
    if matches!(journal.state, State::Restored | State::RestoreFailed) {
        return if journal.state == State::Restored {
            Ok("Confirmation expired; the previous network configuration was restored".into())
        } else {
            Err(ManagementError::Failed(journal.result))
        };
    }
    let keep = answer.is_ok_and(|a| a.eq_ignore_ascii_case("Keep"))
        && !cancelled.load(Ordering::Acquire)
        && now() < journal.deadline;
    if !keep {
        restore(&mut journal, &directory)?;
        return Ok("Previous network configuration was restored".into());
    }
    validate_pending_configuration(&journal)?;
    if plan.kind == OwnerKind::NetworkManager
        && matches!(plan.operation.as_str(), "configure" | "wifi-connect")
    {
        let uuid = if let Some(wifi) = &plan.wifi {
            if !wifi.saved_uuid.is_empty() {
                &wifi.saved_uuid
            } else {
                &plan.new_uuid
            }
        } else if plan.new_uuid.is_empty() {
            &plan.uuid
        } else {
            &plan.new_uuid
        };
        if plan.operation == "wifi-connect" && !plan.new_uuid.is_empty() {
            if let Err(error) = run(
                "nmcli",
                &[
                    "connection",
                    "modify",
                    "--temporary",
                    "uuid",
                    uuid,
                    "connection.autoconnect",
                    "yes",
                ],
                &AtomicBool::new(false),
            ) {
                restore(&mut journal, &directory)?;
                return Err(error);
            }
        }
        if (plan
            .wifi
            .as_ref()
            .is_none_or(|wifi| wifi.saved_uuid.is_empty()))
            && let Err(error) = save_nm(uuid)
        {
            restore(&mut journal, &directory)?;
            return Err(error);
        }
    }
    journal.state = State::Committed;
    journal.result = if plan.wifi.is_some() {
        "Wi-Fi connected. Selected network and required addresses verified.".into()
    } else {
        "Network configuration confirmed and saved".into()
    };
    write_journal(&directory, &journal)?;
    // The recovery service observes the terminal state before exiting. Backups remain
    // private until then; retaining this small journal also records the actual outcome.
    Ok(journal.result)
}

fn validate_plan(plan: &Plan) -> Result<(), ManagementError> {
    verify_identity(&plan.interface, &plan.identity)?;
    if plan.kind == OwnerKind::NetworkManager && nm_device(&plan.interface)?.uuid != plan.uuid {
        return Err(ManagementError::Conflict(
            "The active NetworkManager connection changed while recovery was being prepared".into(),
        ));
    }
    for change in &plan.files {
        config::checked_file(&change.path)?;
        if fs::read(&change.path).map_err(io_error)? != change.before {
            return Err(ManagementError::Conflict(
                "Configuration changed during preview".into(),
            ));
        }
    }
    Ok(())
}

fn guarded_apply(
    apply_and_verify: impl FnOnce() -> Result<(), ManagementError>,
    recover: impl FnOnce() -> Result<(), ManagementError>,
    directory: &Path,
) -> Result<(), ManagementError> {
    match apply_and_verify() {
        Ok(()) => Ok(()),
        Err(error) => Err(ManagementError::Failed(match recover() {
            Ok(()) => {
                format!("Network change failed. The previous connection was restored.\n{error}")
            }
            Err(recovery) => format!(
                "Network change failed. Recovery failed; inspect the retained backup at {}.\n{error}\n{recovery}",
                directory.display()
            ),
        })),
    }
}

fn apply_configuration(plan: &Plan, cancelled: &AtomicBool) -> Result<(), ManagementError> {
    if plan.operation == "wifi-disconnect" {
        run(
            "nmcli",
            &["device", "disconnect", &plan.interface],
            cancelled,
        )?;
        return Ok(());
    }
    if plan.operation == "wifi-forget" {
        run(
            "nmcli",
            &["connection", "delete", "uuid", &plan.uuid],
            cancelled,
        )?;
        return Ok(());
    }
    if let Some(wifi) = &plan.wifi {
        if wifi.saved_uuid.is_empty() {
            return add_wifi(&plan.interface, &plan.new_uuid, wifi);
        }
        // Wait for activation, including association and address acquisition.
        run(
            "nmcli",
            &[
                "--wait",
                "35",
                "connection",
                "up",
                "uuid",
                if wifi.saved_uuid.is_empty() {
                    &plan.new_uuid
                } else {
                    &wifi.saved_uuid
                },
                "ifname",
                &plan.interface,
            ],
            cancelled,
        )?;
        return Ok(());
    }
    if plan.kind == OwnerKind::NetworkManager {
        let mut args = vec![
            "connection",
            "modify",
            "--temporary",
            "uuid",
            plan.uuid.as_str(),
        ];
        args.extend(plan.arguments.iter().map(String::as_str));
        run("nmcli", &args, cancelled)?;
        run(
            "nmcli",
            &[
                "--wait",
                "35",
                "connection",
                "up",
                "uuid",
                &plan.uuid,
                "ifname",
                &plan.interface,
            ],
            cancelled,
        )?;
    } else {
        if plan.kind == OwnerKind::Interfaces {
            deactivate_interfaces(&plan.interface, cancelled)?;
        }
        for change in &plan.files {
            write_atomic(&change.path, &change.after, change.mode)?;
        }
        activate(plan, cancelled)?;
    }
    Ok(())
}

fn activate(plan: &Plan, cancelled: &AtomicBool) -> Result<(), ManagementError> {
    match plan.kind {
        OwnerKind::Netplan => {
            run("netplan", &["apply"], cancelled)?;
        }
        OwnerKind::Networkd => {
            run("networkctl", &["reload"], cancelled)?;
            run("networkctl", &["reconfigure", &plan.interface], cancelled)?;
        }
        OwnerKind::Interfaces => {
            run("ifup", &[&plan.interface], cancelled)?;
        }
        _ => {}
    }
    Ok(())
}

fn wifi_addresses_ready(wifi: &config::Wifi, addresses: &Json) -> bool {
    let valid = addresses
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|device| device["addr_info"].as_array().into_iter().flatten())
        .filter(|a| {
            (a["scope"].as_str() == Some("global")
                || (a["family"].as_str() == Some("inet") && wifi.ipv4 == "link-local")
                || (a["family"].as_str() == Some("inet6") && wifi.ipv6 == "link-local"))
                && !a["flags"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|f| matches!(f.as_str(), Some("tentative" | "dadfailed")))
        })
        .filter_map(|a| a["local"].as_str()?.parse::<IpAddr>().ok())
        .filter(|ip| !ip.is_unspecified() && !ip.is_loopback())
        .collect::<Vec<_>>();
    let v4 = valid.iter().any(IpAddr::is_ipv4);
    let v6 = valid.iter().any(IpAddr::is_ipv6);
    let enabled4 = !matches!(wifi.ipv4.as_str(), "disabled" | "");
    let enabled6 = !matches!(wifi.ipv6.as_str(), "disabled" | "ignore" | "");
    (!wifi.require_ipv4 || !enabled4 || v4)
        && (!wifi.require_ipv6 || !enabled6 || v6)
        && (v4 || v6 || !enabled4 && !enabled6)
}

fn verify_wifi(
    plan: &Plan,
    wifi: &config::Wifi,
    cancelled: &AtomicBool,
) -> Result<(), ManagementError> {
    let deadline = Instant::now() + Duration::from_secs(35);
    let expected_uuid = if wifi.saved_uuid.is_empty() {
        &plan.new_uuid
    } else {
        &wifi.saved_uuid
    };
    let bus = bus()?;
    loop {
        check_cancelled(cancelled)?;
        verify_identity(&plan.interface, &plan.identity)?;
        let device = nm_device(&plan.interface)?;
        let proxy = Proxy::new(
            &bus,
            NM,
            device.path.as_str(),
            "org.freedesktop.NetworkManager.Device",
        )
        .map_err(dbus_error)?;
        let state: u32 = proxy.get_property("State").map_err(dbus_error)?;
        if state == 120 {
            return Err(ManagementError::Failed(
                "Wi-Fi authentication or addressing failed. Check the password and try again."
                    .into(),
            ));
        }
        if state == 100 && &device.uuid == expected_uuid {
            let wireless = Proxy::new(
                &bus,
                NM,
                device.path.as_str(),
                "org.freedesktop.NetworkManager.Device.Wireless",
            )
            .map_err(dbus_error)?;
            let path: OwnedObjectPath = wireless
                .get_property("ActiveAccessPoint")
                .map_err(dbus_error)?;
            if path.as_str() != "/" {
                let ap = Proxy::new(
                    &bus,
                    NM,
                    path.as_str(),
                    "org.freedesktop.NetworkManager.AccessPoint",
                )
                .map_err(dbus_error)?;
                let actual: Vec<u8> = ap.get_property("Ssid").map_err(dbus_error)?;
                let addresses: Json = serde_json::from_str(&run(
                    "ip",
                    &["-j", "address", "show", "dev", &plan.interface],
                    cancelled,
                )?)
                .map_err(|e| ManagementError::Failed(e.to_string()))?;
                if wifi_observation_ready(
                    wifi,
                    expected_uuid,
                    state,
                    &device.uuid,
                    &actual,
                    &addresses,
                ) {
                    return Ok(());
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(ManagementError::Failed(
                "Wi-Fi did not acquire the required address. Check DHCP or the saved profile."
                    .into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn wifi_observation_ready(
    wifi: &config::Wifi,
    expected_uuid: &str,
    state: u32,
    actual_uuid: &str,
    actual_ssid: &[u8],
    addresses: &Json,
) -> bool {
    state == 100
        && actual_uuid == expected_uuid
        && actual_ssid == wifi.ssid_bytes
        && wifi_addresses_ready(wifi, addresses)
}

fn deactivate_interfaces(interface: &str, cancelled: &AtomicBool) -> Result<(), ManagementError> {
    match run("ifdown", &[interface], cancelled) {
        Ok(_) => Ok(()),
        Err(e) if e.to_string().contains("not configured") => Ok(()),
        Err(e) => Err(e),
    }
}

fn restore(journal: &mut Journal, directory: &Path) -> Result<(), ManagementError> {
    if journal.state == State::Prepared {
        journal.state = State::Restored;
        journal.result = "No network changes were applied".into();
        return write_journal(directory, journal);
    }
    let result = (|| {
        // Check before any ifdown/profile deletion/file replacement, so a device
        // that reused the old interface name cannot be changed by recovery.
        verify_identity(&journal.plan.interface, &journal.plan.identity)?;
        let cancelled = AtomicBool::new(false);
        let down_error = if journal.plan.kind == OwnerKind::Interfaces {
            deactivate_interfaces(&journal.plan.interface, &cancelled).err()
        } else {
            None
        };
        for change in &journal.plan.files {
            // Refuse to overwrite a third party's later edits, but report recovery as failed.
            let removed_by_us = journal.plan.operation == "wifi-forget" && !change.path.exists();
            if !removed_by_us {
                config::checked_file(&change.path)?;
            }
            let current = if removed_by_us {
                Vec::new()
            } else {
                fs::read(&change.path).map_err(io_error)?
            };
            if current != change.before && current != change.after {
                return Err(ManagementError::Conflict(format!(
                    "Configuration changed outside this transaction: {}",
                    change.path.display()
                )));
            }
            write_atomic(&change.path, &change.before, change.mode)?;
            if fs::read(&change.path).map_err(io_error)? != change.before {
                return Err(ManagementError::Failed(
                    "Restored file readback did not match its backup".into(),
                ));
            }
        }
        if journal.plan.kind == OwnerKind::NetworkManager {
            if !journal.plan.new_uuid.is_empty() {
                // Only the profile created by this transaction is deleted.
                if run(
                    "nmcli",
                    &["connection", "show", "uuid", &journal.plan.new_uuid],
                    &cancelled,
                )
                .is_ok()
                {
                    run(
                        "nmcli",
                        &["connection", "delete", "uuid", &journal.plan.new_uuid],
                        &cancelled,
                    )?;
                }
            }
            run("nmcli", &["connection", "reload"], &cancelled)?;
            if journal.plan.uuid.is_empty() {
                run(
                    "nmcli",
                    &["device", "disconnect", &journal.plan.interface],
                    &cancelled,
                )?;
            } else {
                run(
                    "nmcli",
                    &[
                        "--wait",
                        "35",
                        "connection",
                        "up",
                        "uuid",
                        &journal.plan.uuid,
                        "ifname",
                        &journal.plan.interface,
                    ],
                    &cancelled,
                )?;
                if nm_device(&journal.plan.interface)?.uuid != journal.plan.uuid {
                    return Err(ManagementError::Failed(
                        "The previous NetworkManager connection was not restored".into(),
                    ));
                }
            }
        } else {
            activate(&journal.plan, &cancelled)?;
        }
        if let Some(error) = down_error {
            return Err(ManagementError::Failed(format!(
                "Previous files were restored and activated, but removing the temporary interface configuration failed: {error}"
            )));
        }
        run(
            "ip",
            &["-j", "address", "show", "dev", &journal.plan.interface],
            &cancelled,
        )?;
        Ok(())
    })();
    match &result {
        Ok(()) => {
            journal.state = State::Restored;
            journal.result = "Previous network configuration restored; configuration files matched backups and the backend completed activation".into();
        }
        Err(error) => {
            journal.state = State::RestoreFailed;
            journal.result = format!(
                "Recovery failed: {error}. Backups retained at {}",
                directory.display()
            );
        }
    }
    write_journal(directory, journal)?;
    result
}

pub(super) fn backup_saved_wifi(uuid: &str, bytes: &[u8]) -> Result<PathBuf, ManagementError> {
    if !Path::new(ROOT).exists() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(ROOT)
            .map_err(io_error)?;
    }
    private_directory(Path::new(ROOT))?;
    let directory = Path::new(ROOT).join("forgotten-wifi");
    if !directory.exists() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(io_error)?;
    }
    private_directory(&directory)?;
    let file = directory.join(format!(
        "{uuid}-{}.nmconnection",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    write_atomic(&file, bytes, 0o600)?;
    Ok(file)
}

/// Fixed root-only helper entry. Its journal cannot be supplied by the original user.
pub fn rollback_transaction(id: &str) -> Result<(), ManagementError> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(ManagementError::PermissionDenied(
            "Network recovery must run in the root helper".into(),
        ));
    }
    if !valid_id(id) {
        return Err(ManagementError::InvalidInput(
            "Invalid recovery transaction ID".into(),
        ));
    }
    private_directory(Path::new(ROOT))?;
    let directory = Path::new(ROOT).join(id);
    private_directory(&directory)?;
    {
        let _lock = lock(&directory)?;
        let journal = read_journal(&directory)?;
        validate_interface(&journal.plan.interface)?;
        write_atomic(&directory.join("ready"), b"ready\n", 0o600)?;
    }
    loop {
        {
            let _lock = lock(&directory)?;
            let mut journal = read_journal(&directory)?;
            if matches!(journal.state, State::Committed | State::Restored) {
                return Ok(());
            }
            if now() >= journal.deadline || journal.state == State::RestoreFailed {
                return restore(&mut journal, &directory);
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn actor_transaction(id: &str, actor_uid: u32) -> Result<PathBuf, ManagementError> {
    if unsafe { libc::geteuid() } != 0 {
        return Err(ManagementError::PermissionDenied(
            "Network transaction access requires the authorized helper.".into(),
        ));
    }
    if !valid_id(id) {
        return Err(ManagementError::InvalidInput(
            "Invalid network transaction ID.".into(),
        ));
    }
    private_directory(Path::new(ROOT))?;
    let directory = Path::new(ROOT).join(id);
    private_directory(&directory)?;
    if read_journal(&directory)?.actor_uid != actor_uid {
        return Err(ManagementError::PermissionDenied(
            "This network transaction belongs to another user.".into(),
        ));
    }
    Ok(directory)
}

/// Queries the recovery journal; does not start a new network operation.
pub fn transaction_status(id: &str, actor_uid: u32) -> Result<ManagementSnapshot, ManagementError> {
    let directory = actor_transaction(id, actor_uid)?;
    let _lock = lock(&directory)?;
    let journal = read_journal(&directory)?;
    Ok(ManagementSnapshot {
        columns: vec![
            "Transaction".into(),
            "Interface".into(),
            "State".into(),
            "Seconds remaining".into(),
        ],
        backend: "Network recovery journal".into(),
        rows: vec![ManagementRow {
            id: id.into(),
            cells: vec![
                id.into(),
                journal.plan.interface,
                format!("{:?}", journal.state),
                journal.deadline.saturating_sub(now()).to_string(),
            ],
            detail: vec![("Result".into(), journal.result)],
            ..Default::default()
        }],
        ..Default::default()
    })
}

/// Confirms a deferred script operation. The independent recovery process still
/// enforces the original deadline and this function never extends it.
pub fn confirm_transaction(
    id: &str,
    actor_uid: u32,
    keep: bool,
) -> Result<String, ManagementError> {
    let directory = actor_transaction(id, actor_uid)?;
    let _lock = lock(&directory)?;
    let mut journal = read_journal(&directory)?;
    if journal.state != State::AwaitingConfirmation {
        return Err(ManagementError::Conflict(format!(
            "Network transaction is {:?}. Query its result before retrying.",
            journal.state
        )));
    }
    if !keep || now() >= journal.deadline {
        return restore_confirmation_outcome(keep, || {
            restore(&mut journal, &directory)?;
            Ok(journal.result.clone())
        });
    }
    validate_pending_configuration(&journal)?;
    if journal.plan.kind == OwnerKind::NetworkManager && journal.plan.operation == "configure" {
        if let Err(error) = save_nm(&journal.plan.uuid) {
            restore(&mut journal, &directory)?;
            return Err(error);
        }
    }
    journal.state = State::Committed;
    journal.result = "Network configuration confirmed and saved".into();
    write_journal(&directory, &journal)?;
    Ok(journal.result)
}

fn expected_active_uuid(plan: &Plan) -> &str {
    if matches!(plan.operation.as_str(), "wifi-disconnect" | "wifi-forget") {
        ""
    } else if let Some(wifi) = &plan.wifi {
        if wifi.saved_uuid.is_empty() {
            &plan.new_uuid
        } else {
            &wifi.saved_uuid
        }
    } else {
        &plan.uuid
    }
}

fn nm_configuration_version(uuid: &str) -> Result<String, ManagementError> {
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
    // GetSettings omits secrets. Only its version is retained in the protected
    // journal; profile content never enters progress output or normal logs.
    let settings: HashMap<String, HashMap<String, OwnedValue>> =
        connection.call("GetSettings", &()).map_err(dbus_error)?;
    let settings =
        serde_json::to_value(settings).map_err(|e| ManagementError::Failed(e.to_string()))?;
    Ok(nm_version_from_settings(settings))
}

fn nm_version_from_settings(mut settings: Json) -> String {
    // Last-activation time is maintained by the daemon and can change without
    // any configuration edit. Other settings must retain their exact version.
    if let Some(connection) = settings.get_mut("connection").and_then(Json::as_object_mut) {
        connection.remove("timestamp");
    }
    fingerprint(canonical_nm_json(settings).to_string().as_bytes())
}

fn canonical_nm_json(value: Json) -> Json {
    match value {
        Json::Object(object) => Json::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, canonical_nm_json(value)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Json::Array(array) => Json::Array(array.into_iter().map(canonical_nm_json).collect()),
        value => value,
    }
}

fn matching_pending_version(
    expected_uuid: &str,
    actual_uuid: &str,
    expected_version: &str,
    actual_version: &str,
) -> Result<(), ManagementError> {
    if expected_uuid != actual_uuid || expected_version != actual_version {
        Err(ManagementError::Conflict("The connection or its configuration changed while confirmation was pending. Review the current settings before retrying.".into()))
    } else {
        Ok(())
    }
}

fn matching_candidate_file(expected: &[u8], actual: &[u8]) -> Result<(), ManagementError> {
    if expected == actual {
        Ok(())
    } else {
        Err(ManagementError::Conflict("Network configuration changed while confirmation was pending. Review the current file before retrying.".into()))
    }
}

fn validate_pending_configuration(journal: &Journal) -> Result<(), ManagementError> {
    let plan = &journal.plan;
    verify_identity(&plan.interface, &plan.identity)?;
    for change in &plan.files {
        if plan.operation == "wifi-forget" {
            match fs::symlink_metadata(&change.path) {
                Err(error) if error.kind()==std::io::ErrorKind::NotFound=>continue,
                _=>return Err(ManagementError::Conflict("The removed Wi-Fi configuration path was recreated. Review it before confirming.".into())),
            }
        }
        config::checked_file(&change.path)?;
        matching_candidate_file(&change.after, &fs::read(&change.path).map_err(io_error)?)?;
    }
    if plan.kind == OwnerKind::NetworkManager {
        let expected = expected_active_uuid(plan);
        let actual = nm_device(&plan.interface)?.uuid;
        if expected.is_empty() {
            matching_pending_version(expected, &actual, "", "")?;
        } else {
            let version=journal.candidate_nm_version.as_deref().ok_or_else(||ManagementError::Conflict("The pending network configuration has no recorded version. Restore it and try again.".into()))?;
            matching_pending_version(
                expected,
                &actual,
                version,
                &nm_configuration_version(expected)?,
            )?;
        }
    }
    Ok(())
}

fn restore_confirmation_outcome(
    keep_requested: bool,
    recover: impl FnOnce() -> Result<String, ManagementError>,
) -> Result<String, ManagementError> {
    let result = recover()?;
    if keep_requested {
        Err(ManagementError::Conflict(
            "The confirmation deadline expired. The previous network configuration was restored."
                .into(),
        ))
    } else {
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wifi() -> config::Wifi {
        config::Wifi {
            ssid: "selected".into(),
            ssid_bytes: b"selected".to_vec(),
            security: "wpa2".into(),
            password: "secret".into(),
            saved_uuid: String::new(),
            ap_path: String::new(),
            ipv4: "auto".into(),
            ipv6: "auto".into(),
            require_ipv4: true,
            require_ipv6: false,
        }
    }
    #[test]
    fn wifi_commit_requires_the_selected_network_and_required_address() {
        let wifi = wifi();
        let acquired = serde_json::json!([{"addr_info":[{"family":"inet","scope":"global","local":"192.0.2.2"}]}]);
        assert!(wifi_observation_ready(
            &wifi,
            "new",
            100,
            "new",
            b"selected",
            &acquired
        ));
        assert!(!wifi_observation_ready(
            &wifi,
            "new",
            100,
            "old",
            b"selected",
            &acquired
        ));
        assert!(!wifi_observation_ready(
            &wifi, "new", 100, "new", b"other", &acquired
        ));
        assert!(!wifi_observation_ready(
            &wifi,
            "new",
            120,
            "new",
            b"selected",
            &acquired
        ));
        assert!(!wifi_observation_ready(
            &wifi,
            "new",
            100,
            "new",
            b"selected",
            &serde_json::json!([])
        ));
        let tentative = serde_json::json!([{"addr_info":[{"family":"inet","scope":"global","local":"192.0.2.2","flags":["tentative"]}]}]);
        assert!(!wifi_observation_ready(
            &wifi,
            "new",
            100,
            "new",
            b"selected",
            &tentative
        ));
    }
    #[test]
    fn saved_wifi_address_requirements_allow_optional_and_ipv6_only_profiles() {
        let mut wifi = wifi();
        wifi.require_ipv4 = false;
        let ipv6 = serde_json::json!([{"addr_info":[{"family":"inet6","scope":"global","local":"2001:db8::2"}]}]);
        assert!(wifi_addresses_ready(&wifi, &ipv6));
        wifi.ipv4 = "disabled".into();
        wifi.require_ipv6 = true;
        assert!(wifi_addresses_ready(&wifi, &ipv6));
        let ipv4 = serde_json::json!([{"addr_info":[{"family":"inet","scope":"global","local":"192.0.2.2"}]}]);
        assert!(!wifi_addresses_ready(&wifi, &ipv4));
    }
    #[test]
    fn wrong_password_timeout_and_cancellation_restore_before_reporting_failure() {
        for error in [
            ManagementError::Failed("wrong password".into()),
            ManagementError::Failed("address acquisition timed out".into()),
            ManagementError::Cancelled,
        ] {
            let restored = std::cell::Cell::new(false);
            let result = guarded_apply(
                || Err(error),
                || {
                    restored.set(true);
                    Ok(())
                },
                Path::new("/private/backup"),
            );
            assert!(restored.get());
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("previous connection was restored")
            );
        }
        let restored = std::cell::Cell::new(false);
        assert!(
            guarded_apply(
                || Ok(()),
                || {
                    restored.set(true);
                    Ok(())
                },
                Path::new("/private/backup")
            )
            .is_ok()
        );
        assert!(!restored.get());
        let result = guarded_apply(
            || Err(ManagementError::Failed("apply".into())),
            || Err(ManagementError::Failed("restore".into())),
            Path::new("/private/backup"),
        );
        assert!(result.unwrap_err().to_string().contains("Recovery failed"));
    }
    #[test]
    fn recovery_ids_cannot_name_paths_or_options() {
        assert!(valid_id("123-456"));
        for id in ["", "../123", "123/456", "--help", "one", "123\n"] {
            assert!(!valid_id(id));
        }
    }
    #[test]
    fn journal_does_not_store_wifi_password() {
        let plan = Plan {
            defer_confirmation: false,
            operation: "wifi-connect".into(),
            identity: BTreeMap::new(),
            kind: OwnerKind::NetworkManager,
            interface: "wlan0".into(),
            uuid: "old".into(),
            new_uuid: "new".into(),
            files: Vec::new(),
            arguments: Vec::new(),
            preview: String::new(),
            wifi: Some(config::Wifi {
                ssid: "ssid".into(),
                security: "wpa2".into(),
                password: "secret-password".into(),
                ssid_bytes: b"ssid".to_vec(),
                saved_uuid: String::new(),
                ap_path: String::new(),
                ipv4: "auto".into(),
                ipv6: "auto".into(),
                require_ipv4: true,
                require_ipv6: false,
            }),
        };
        assert!(!format!("{plan:?}").contains("secret-password"));
        let bytes = serde_json::to_string(&Journal {
            actor_uid: 1000,
            plan,
            state: State::Prepared,
            deadline: 123,
            result: String::new(),
            candidate_nm_version: None,
        })
        .unwrap();
        assert!(!bytes.contains("secret-password"));
    }
    #[test]
    fn recovery_before_application_preserves_files_and_does_not_activate_network() {
        let directory = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!(
                "network-prepared-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("interfaces");
        fs::write(&file, b"outside edit").unwrap();
        let mut journal = Journal {
            actor_uid: 1000,
            plan: Plan {
                defer_confirmation: false,
                operation: "configure".into(),
                identity: BTreeMap::new(),
                kind: OwnerKind::Interfaces,
                interface: "nonexistent".into(),
                uuid: String::new(),
                new_uuid: String::new(),
                files: vec![config::FileChange {
                    path: file.clone(),
                    before: b"old".to_vec(),
                    after: b"new".to_vec(),
                    mode: 0o600,
                }],
                arguments: Vec::new(),
                preview: String::new(),
                wifi: None,
            },
            state: State::Prepared,
            deadline: 0,
            result: String::new(),
            candidate_nm_version: None,
        };
        restore(&mut journal, &directory).unwrap();
        assert_eq!(journal.state, State::Restored);
        assert_eq!(fs::read(file).unwrap(), b"outside edit");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn recovery_checks_device_identity_before_replacing_files() {
        let directory = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!(
                "network-identity-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("profile");
        fs::write(&file, b"new").unwrap();
        let mut journal = Journal {
            actor_uid: 1000,
            plan: Plan {
                defer_confirmation: false,
                operation: "configure".into(),
                identity: BTreeMap::from([
                    ("ifindex".into(), "0".into()),
                    ("mac".into(), "different-device".into()),
                ]),
                kind: OwnerKind::NetworkManager,
                interface: "lo".into(),
                uuid: String::new(),
                new_uuid: String::new(),
                files: vec![config::FileChange {
                    path: file.clone(),
                    before: b"old".to_vec(),
                    after: b"new".to_vec(),
                    mode: 0o600,
                }],
                arguments: Vec::new(),
                preview: String::new(),
                wifi: None,
            },
            state: State::Applying,
            deadline: 0,
            result: String::new(),
            candidate_nm_version: None,
        };
        assert!(matches!(
            restore(&mut journal, &directory),
            Err(ManagementError::Conflict(_))
        ));
        assert_eq!(journal.state, State::RestoreFailed);
        assert_eq!(fs::read(file).unwrap(), b"new");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn keeping_requires_the_pending_connection_and_configuration_version() {
        assert!(matching_pending_version("pending", "pending", "candidate", "candidate").is_ok());
        assert!(matches!(
            matching_pending_version("pending", "other", "candidate", "candidate"),
            Err(ManagementError::Conflict(_))
        ));
        assert!(matches!(
            matching_pending_version("pending", "pending", "candidate", "external edit"),
            Err(ManagementError::Conflict(_))
        ));
        assert!(matching_candidate_file(b"candidate", b"candidate").is_ok());
        assert!(matches!(
            matching_candidate_file(b"candidate", b"external edit"),
            Err(ManagementError::Conflict(_))
        ));
    }

    #[test]
    fn network_manager_version_ignores_only_activation_time_and_object_order() {
        let before = serde_json::json!({"connection":{"id":"profile","timestamp":100},"ipv4":{"method":"manual","address-data":[{"address":"192.0.2.2","prefix":24}]}});
        let later = serde_json::json!({"ipv4":{"address-data":[{"prefix":24,"address":"192.0.2.2"}],"method":"manual"},"connection":{"timestamp":200,"id":"profile"}});
        assert_eq!(
            nm_version_from_settings(before.clone()),
            nm_version_from_settings(later)
        );
        let mut changed = before.clone();
        changed["ipv4"]["method"] = serde_json::json!("auto");
        assert_ne!(
            nm_version_from_settings(before),
            nm_version_from_settings(changed)
        );
        let typed = HashMap::from([(
            "connection".to_string(),
            HashMap::from([("timestamp".to_string(), OwnedValue::from(123_u64))]),
        )]);
        let encoded = serde_json::to_value(typed).unwrap();
        assert!(encoded["connection"].is_object());
        assert_eq!(
            nm_version_from_settings(encoded),
            nm_version_from_settings(serde_json::json!({"connection":{}}))
        );
    }

    #[test]
    fn expired_keep_restores_before_returning_conflict_but_explicit_restore_succeeds() {
        for keep in [false, true] {
            let restored = std::cell::Cell::new(false);
            let result = restore_confirmation_outcome(keep, || {
                restored.set(true);
                Ok("Restored".into())
            });
            assert!(restored.get());
            if keep {
                assert!(matches!(result, Err(ManagementError::Conflict(_))));
            } else {
                assert_eq!(result.unwrap(), "Restored");
            }
        }
        assert!(matches!(
            restore_confirmation_outcome(true, || Err(ManagementError::Failed(
                "Recovery failed".into()
            ))),
            Err(ManagementError::Failed(_))
        ));
    }
}
