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
    plan: Plan,
    state: State,
    deadline: u64,
    result: String,
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
        wifi.password.clear();
    }
    let mut journal = Journal {
        plan: stored,
        state: State::Prepared,
        deadline: now() + 180,
        result: String::new(),
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
        let result = apply_configuration(&plan, cancelled);
        if let Err(error) = result {
            let restored = restore(&mut journal, &directory);
            return Err(ManagementError::Failed(match restored {
                Ok(_) => {
                    format!("Network change failed; previous configuration was restored: {error}")
                }
                Err(rollback) => format!(
                    "Network change failed: {error}. Recovery also failed: {rollback}. Backups retained at {}",
                    directory.display()
                ),
            }));
        }
        journal.state = State::AwaitingConfirmation;
        journal.deadline = now() + 120;
        write_journal(&directory, &journal)?;
    }
    drop(_global);
    interaction.emit(OperationEvent::Progress { message: "Applied temporarily. Confirm within 120 seconds; recovery continues if SSH or the UI disconnects.".into(), percent: None });
    let answer = interaction.ask(
        "network-confirm-120",
        "Keep this network configuration? Unconfirmed changes will be restored after 120 seconds.",
        &["Keep".into(), "Restore".into()],
        false,
    );
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
    if plan.kind == OwnerKind::NetworkManager
        && matches!(plan.operation.as_str(), "configure" | "wifi-connect")
    {
        let uuid = if plan.new_uuid.is_empty() {
            &plan.uuid
        } else {
            &plan.new_uuid
        };
        if plan.operation == "wifi-connect" {
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
        if let Err(error) = save_nm(uuid) {
            restore(&mut journal, &directory)?;
            return Err(error);
        }
    }
    journal.state = State::Committed;
    journal.result = "Network configuration confirmed and saved".into();
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
        add_wifi(
            &plan.interface,
            &plan.new_uuid,
            &wifi.ssid,
            &wifi.security,
            &wifi.password,
        )?;
        // Wait for activation, including association and address acquisition.
        run(
            "nmcli",
            &[
                "--wait",
                "35",
                "connection",
                "up",
                "uuid",
                &plan.new_uuid,
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_ids_cannot_name_paths_or_options() {
        assert!(valid_id("123-456"));
        for id in ["", "../123", "123/456", "--help", "one", "123\n"] {
            assert!(!valid_id(id));
        }
    }
    #[test]
    fn journal_does_not_store_wifi_password() {
        let mut plan = Plan {
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
            }),
        };
        if let Some(wifi) = plan.wifi.as_mut() {
            wifi.password.clear();
        }
        let bytes = serde_json::to_string(&Journal {
            plan,
            state: State::Prepared,
            deadline: 123,
            result: String::new(),
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
            plan: Plan {
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
            plan: Plan {
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
        };
        assert!(matches!(
            restore(&mut journal, &directory),
            Err(ManagementError::Conflict(_))
        ));
        assert_eq!(journal.state, State::RestoreFailed);
        assert_eq!(fs::read(file).unwrap(), b"new");
        fs::remove_dir_all(directory).unwrap();
    }
}
