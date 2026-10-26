use super::*;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::time::{SystemTime, UNIX_EPOCH};

const ROOT: &str = "/var/lib/tundraux3/ssh";
const BOOT_UNIT: &str = "/etc/systemd/system/tundra-ssh-recovery.service";

pub(crate) fn active_backup_ids() -> Result<std::collections::BTreeSet<String>, ManagementError> {
    let mut ids = std::collections::BTreeSet::new();
    if !Path::new(ROOT).exists() {
        return Ok(ids);
    }
    private(Path::new(ROOT))?;
    for entry in fs::read_dir(ROOT).map_err(failure)? {
        let entry = entry.map_err(failure)?;
        if !entry.file_type().map_err(failure)?.is_dir() {
            continue;
        }
        let journal = read(&entry.path())?;
        if !finished(journal.state) {
            ids.insert(journal.backup_id);
            ids.extend(journal.rollback_backup_id);
            ids.extend(journal.plan.seed_backup);
        }
    }
    Ok(ids)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum State {
    Prepared,
    Applying,
    AwaitingConfirmation,
    Committed,
    Restored,
    RestoreFailed,
}
fn state_text(state: State) -> String {
    match state {
        State::Prepared => i18n::tr!("platform-ssh-state-prepared"),
        State::Applying => i18n::tr!("platform-ssh-state-applying"),
        State::AwaitingConfirmation => i18n::tr!("platform-ssh-state-awaiting"),
        State::Committed => i18n::tr!("platform-ssh-state-committed"),
        State::Restored => i18n::tr!("platform-ssh-state-restored"),
        State::RestoreFailed => i18n::tr!("platform-ssh-state-failed"),
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Journal {
    actor_uid: u32,
    owner_pid: u32,
    owner_start_ticks: u64,
    boot_id: String,
    state: State,
    deadline: u64,
    applied_ticks: u64,
    backup_id: String,
    #[serde(default)]
    rollback_backup_id: Option<String>,
    plan: Plan,
    firewall: Vec<super::super::firewall::OwnedPortRule>,
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
        .map_err(failure)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(failure(std::io::Error::last_os_error()));
    }
    Ok(Lock(file))
}
fn root() -> Result<(), ManagementError> {
    if unsafe { libc::geteuid() } != 0 {
        Err(ManagementError::PermissionDenied(
            i18n::tr!("platform-ssh-ssh-recovery-requires-the-authorized-root-helper").into(),
        ))
    } else {
        Ok(())
    }
}
fn private(path: &Path) -> Result<(), ManagementError> {
    let m = fs::symlink_metadata(path).map_err(failure)?;
    if !m.is_dir() || m.file_type().is_symlink() || m.uid() != 0 || m.mode() & 0o077 != 0 {
        return Err(ManagementError::PermissionDenied(i18n::tr!(
            "platform-ssh-private-directory",
            path = path.display().to_string()
        )));
    }
    Ok(())
}
fn prepare_root() -> Result<(), ManagementError> {
    for path in ["/var/lib/tundraux3", ROOT] {
        if !Path::new(path).exists() {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(path)
                .map_err(failure)?;
        }
        if path == ROOT {
            private(Path::new(path))?;
        } else {
            let m = fs::symlink_metadata(path).map_err(failure)?;
            if !m.is_dir() || m.file_type().is_symlink() || m.uid() != 0 || m.mode() & 0o022 != 0 {
                return Err(ManagementError::PermissionDenied(
                    i18n::tr!("platform-ssh-unsafe-ssh-recovery-parent-directory").into(),
                ));
            }
        }
    }
    Ok(())
}
fn now() -> u64 {
    let mut time = std::mem::MaybeUninit::<libc::timespec>::zeroed();
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, time.as_mut_ptr()) } == 0 {
        unsafe { time.assume_init() }.tv_sec.max(0) as u64
    } else {
        0
    }
}
fn boot_id() -> Result<String, ManagementError> {
    fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map(|s| s.trim().into())
        .map_err(failure)
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 80
        && id.bytes().all(|b| b.is_ascii_digit() || b == b'-')
        && id.bytes().any(|b| b.is_ascii_digit())
}
fn atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), ManagementError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid(&i18n::tr!("platform-ssh-missing-recovery-parent-directory")))?;
    crate::document::validate_no_follow_path(path, false)
        .map_err(|e| ManagementError::Failed(e.to_string()))?;
    let mut file = tempfile::Builder::new()
        .prefix(".tundra-ssh-")
        .tempfile_in(parent)
        .map_err(failure)?;
    file.write_all(bytes).map_err(failure)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(mode))
        .map_err(failure)?;
    file.as_file().sync_all().map_err(failure)?;
    file.persist(path).map_err(|e| failure(e.error))?;
    File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(failure)
}
fn write(directory: &Path, journal: &Journal) -> Result<(), ManagementError> {
    atomic(
        &directory.join("journal.json"),
        &serde_json::to_vec(journal).map_err(|e| ManagementError::Failed(e.to_string()))?,
        0o600,
    )
}
fn read(directory: &Path) -> Result<Journal, ManagementError> {
    private(directory)?;
    let path = directory.join("journal.json");
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(failure)?;
    let m = file.metadata().map_err(failure)?;
    if !m.is_file() || m.uid() != 0 || m.mode() & 0o077 != 0 || m.len() > 16 * 1024 * 1024 {
        return Err(invalid(&i18n::tr!(
            "platform-ssh-ssh-recovery-journal-is-not-a-bounded-root-private-file"
        )));
    }
    serde_json::from_reader(file).map_err(|e| {
        ManagementError::Failed(i18n::tr!(
            "platform-ssh-invalid-journal",
            error = e.to_string()
        ))
    })
}
fn helper(path: &Path) -> Result<(), ManagementError> {
    let canonical = fs::canonicalize(path).map_err(failure)?;
    let m = fs::symlink_metadata(path).map_err(failure)?;
    if canonical != path || !m.is_file() || m.uid() != 0 || m.mode() & 0o022 != 0 {
        return Err(ManagementError::PermissionDenied(
            i18n::tr!("platform-ssh-independent-ssh-recovery-needs-a-protected-root-owned-helper")
                .into(),
        ));
    }
    for parent in path.ancestors().skip(1) {
        let m = fs::symlink_metadata(parent).map_err(failure)?;
        if m.file_type().is_symlink()
            || m.uid() != 0
            || m.mode() & 0o022 != 0 && m.mode() & 0o1000 == 0
        {
            return Err(ManagementError::PermissionDenied(
                i18n::tr!(
                    "platform-ssh-the-ssh-helper-directory-can-be-changed-by-another-account"
                )
                .into(),
            ));
        }
    }
    Ok(())
}
fn install_boot_recovery(
    context: &ExecutionContext,
    flag: &AtomicBool,
) -> Result<PathBuf, ManagementError> {
    helper(&context.helper_path)?;
    let destination = Path::new(ROOT).join("helper");
    atomic(
        &destination,
        &fs::read(&context.helper_path).map_err(failure)?,
        0o755,
    )?;
    let unit = format!(
        "[Unit]\nDescription=Restore unconfirmed Tundra SSH changes\nDefaultDependencies=no\nAfter=local-fs.target\nBefore=ssh.service sshd.service ssh.socket ufw.service firewalld.service\n[Service]\nType=oneshot\nExecStart={} __ssh-recover-pending\n[Install]\nWantedBy=multi-user.target\n",
        destination.display()
    );
    if Path::new(BOOT_UNIT).exists() && fs::read_to_string(BOOT_UNIT).map_err(failure)? != unit {
        return Err(ManagementError::Conflict(
            i18n::tr!("platform-ssh-the-ssh-recovery-service-was-changed-outside-tundra-review-it")
                .into(),
        ));
    }
    atomic(Path::new(BOOT_UNIT), unit.as_bytes(), 0o644)?;
    run("systemctl", &["daemon-reload"], flag)?;
    run(
        "systemctl",
        &["enable", "tundra-ssh-recovery.service"],
        flag,
    )?;
    Ok(destination)
}
fn actor_directory(id: &str, uid: u32) -> Result<PathBuf, ManagementError> {
    root()?;
    if !valid_id(id) {
        return Err(invalid(&i18n::tr!(
            "platform-ssh-invalid-ssh-transaction-id"
        )));
    }
    private(Path::new(ROOT))?;
    let directory = Path::new(ROOT).join(id);
    if read(&directory)?.actor_uid != uid && uid != 0 {
        return Err(ManagementError::PermissionDenied(
            i18n::tr!("platform-ssh-this-ssh-change-belongs-to-another-account").into(),
        ));
    }
    Ok(directory)
}
fn owner_alive(journal: &Journal) -> bool {
    super::connection::process_matches(journal.owner_pid, journal.owner_start_ticks)
}
fn finished(state: State) -> bool {
    matches!(state, State::Committed | State::Restored)
}

pub(super) fn apply(
    plan: Plan,
    open_firewall: bool,
    context: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    root()?;
    check_cancelled(flag)?;
    prepare_root()?;
    let global = lock(Path::new(ROOT))?;
    for entry in fs::read_dir(ROOT).map_err(failure)? {
        let entry = entry.map_err(failure)?;
        if entry.file_type().map_err(failure)?.is_dir() && !finished(read(&entry.path())?.state) {
            return Err(ManagementError::Conflict(i18n::tr!(
                "platform-ssh-earlier-pending",
                id = entry.file_name().to_string_lossy().into_owned()
            )));
        }
    }
    let persistent_helper = install_boot_recovery(context, flag)?;
    let id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ManagementError::Failed(e.to_string()))?
            .as_nanos()
    );
    let directory = Path::new(ROOT).join(&id);
    let mut firewall = if open_firewall {
        super::super::firewall::prepare_ssh_ports(&id, &plan.ports, flag)?
    } else {
        Vec::new()
    };
    let present = super::super::firewall::present_ssh_rules(&plan.add_firewall, flag)?;
    for rule in &plan.add_firewall {
        if !present.contains(rule) && !firewall.contains(rule) {
            firewall.push(rule.clone());
        }
    }
    let mut paths = plan
        .files
        .iter()
        .map(|file| file.before.path.clone())
        .collect::<Vec<_>>();
    for path in super::super::firewall::ssh_backup_paths(&firewall)
        .into_iter()
        .chain(super::super::firewall::ssh_backup_paths(
            &plan.remove_firewall,
        ))
    {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    let session = if let Some(id) = &plan.seed_backup {
        super::super::config_backups::begin_from_backup("ssh", id, &paths)?
    } else {
        super::super::config_backups::begin("ssh", &paths)?
    };
    session.set_payload(serde_json::json!({"transaction_id":id,"backup_id":session.id(),"firewall_observed":true,"firewall_restore":{"add":plan.remove_firewall,"remove":firewall}}))?;
    let mut journal = Journal {
        actor_uid: context.actor_uid,
        owner_pid: context.caller_pid,
        owner_start_ticks: context.caller_start_ticks,
        boot_id: boot_id()?,
        state: State::Prepared,
        deadline: now() + 120,
        applied_ticks: 0,
        backup_id: session.id().into(),
        rollback_backup_id: None,
        plan,
        firewall,
        result: String::new(),
    };
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(failure)?;
    if let Err(error) = write(&directory, &journal) {
        let _ = fs::remove_dir(&directory);
        return Err(error);
    }
    if let Err(error) = run(
        "systemd-run",
        &[
            "--quiet",
            "--collect",
            "--service-type=exec",
            "--unit",
            &format!("tundra-ssh-rollback-{id}"),
            "--property=Restart=on-failure",
            "--property=RestartSec=1",
            "--",
            persistent_helper
                .to_str()
                .ok_or_else(|| invalid(&i18n::tr!("platform-ssh-invalid-helper-path")))?,
            "__ssh-rollback",
            &id,
        ],
        flag,
    ) {
        journal.state = State::Restored;
        journal.result = i18n::tr!("platform-ssh-task-not-started", error = error.to_string());
        write(&directory, &journal)?;
        session.finish(&journal.result)?;
        return Err(error);
    }
    let started = Instant::now();
    while !directory.join("ready").is_file() {
        if started.elapsed() > Duration::from_secs(15) {
            return Err(ManagementError::Unavailable(
                i18n::tr!("platform-ssh-ssh-recovery-task-did-not-become-ready-no-ssh-settings")
                    .into(),
            ));
        }
        check_cancelled(flag)?;
        std::thread::sleep(Duration::from_millis(30));
    }
    {
        let _lock = lock(&directory)?;
        journal = read(&directory)?;
        if journal.state != State::Prepared || now() >= journal.deadline {
            return Err(ManagementError::Conflict(
                i18n::tr!("platform-ssh-ssh-recovery-already-started").into(),
            ));
        }
        if let Some(id) = &journal.plan.seed_backup {
            super::super::config_backups::validate_current(id)?;
        }
        for file in &journal.plan.files {
            let actual = system_config::read(&file.before.path)?;
            if journal.plan.seed_backup.is_some()
                && actual.content == file.after
                && actual.existed == file.after_existed
            {
                continue;
            }
            if actual.version != file.before.version {
                return Err(ManagementError::Conflict(
                    i18n::tr!("platform-ssh-ssh-files-changed-before-applying-refresh-and-retry")
                        .into(),
                ));
            }
        }
        journal.state = State::Applying;
        write(&directory, &journal)?;
        let outcome: Result<(), ManagementError> = (|| {
            super::super::firewall::apply_ssh_ports_unrecorded(&journal.firewall, flag)?;
            super::super::firewall::rollback_ssh_ports_unrecorded(
                &journal.plan.remove_firewall,
                flag,
            )?;
            if let Some(id) = &journal.plan.backup_to_restore {
                super::super::config_backups::restore_selected_files_unrecorded(
                    id,
                    &journal
                        .plan
                        .files
                        .iter()
                        .map(|file| file.before.path.clone())
                        .collect::<Vec<_>>(),
                )?;
            } else if let Some(command) = &journal.plan.key_command {
                let user = key_account(command)?;
                users::apply_prepared_ssh_key_change(
                    &user,
                    &journal
                        .plan
                        .files
                        .iter()
                        .map(|file| (file.before.clone(), file.after.clone()))
                        .collect::<Vec<_>>(),
                    io,
                )?;
            } else {
                for file in &journal.plan.files {
                    let actual = system_config::read(&file.before.path)?;
                    if journal.plan.seed_backup.is_some()
                        && actual.content == file.after
                        && actual.existed == file.after_existed
                    {
                        continue;
                    }
                    system_config::install_reviewed(
                        &file.before.path,
                        &file.before.version,
                        &file.after,
                    )?;
                }
            }
            // Record the clock only after installing. A connection opened during
            // preparation cannot prove that the new settings work.
            load(&journal.plan.service, flag)?;
            verify_plan(&journal.plan, flag)?;
            journal.applied_ticks = connection::ticks()?;
            session.finish("awaiting SSH connection confirmation")?;
            Ok(())
        })();
        if let Err(error) = outcome {
            let recovery = restore(&directory, &mut journal, false);
            return match recovery {
                Ok(()) => Err(ManagementError::Failed(i18n::tr!(
                    "platform-ssh-error-restored",
                    error = error.to_string()
                ))),
                Err(recovery) => Err(ManagementError::Failed(i18n::tr!(
                    "platform-ssh-error-recovery-id",
                    error = error.to_string(),
                    recovery = recovery.to_string(),
                    id = id
                ))),
            };
        }
        journal.state = State::AwaitingConfirmation;
        journal.deadline = now() + 120;
        write(&directory, &journal)?;
    }
    drop(global);
    io.emit(OperationEvent::Transaction {
        kind: ManagementKind::Ssh,
        id: id.clone(),
    });
    io.emit(OperationEvent::Output {
        text: i18n::tr!(
            "platform-ssh-waiting-connection",
            id = id.clone(),
            ports = confirmation_ports(&journal.plan.ports, &journal.plan.service.original_ports)
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    });
    loop {
        {
            let _lock = lock(&directory)?;
            let mut journal = read(&directory)?;
            if finished(journal.state) {
                return if journal.state == State::Committed {
                    Ok(journal.result)
                } else {
                    Err(ManagementError::Failed(journal.result))
                };
            }
            if flag.load(Ordering::Acquire) {
                restore(&directory, &mut journal, false)?;
                return Err(ManagementError::Cancelled);
            }
            if now() >= journal.deadline {
                restore(&directory, &mut journal, false)?;
                return Err(ManagementError::Failed(
                    i18n::tr!(
                        "platform-ssh-ssh-confirmation-timed-out-previous-settings-were-restored"
                    )
                    .into(),
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn restore(directory: &Path, journal: &mut Journal, boot: bool) -> Result<(), ManagementError> {
    if finished(journal.state) {
        return Ok(());
    }
    if journal.state == State::Prepared {
        journal.state = State::Restored;
        journal.result = i18n::tr!("platform-ssh-ssh-change-cancelled-before-writing").into();
        write(directory, journal)?;
        let _ = super::super::config_backups::finish(&journal.backup_id, &journal.result);
        return Ok(());
    }
    if journal.rollback_backup_id.is_none() {
        let mut paths = journal
            .plan
            .files
            .iter()
            .map(|file| file.before.path.clone())
            .collect::<Vec<_>>();
        for path in super::super::firewall::ssh_backup_paths(&journal.firewall)
            .into_iter()
            .chain(super::super::firewall::ssh_backup_paths(
                &journal.plan.remove_firewall,
            ))
        {
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        let session = super::super::config_backups::begin("ssh", &paths)?;
        let id = directory
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid(&i18n::tr!("platform-ssh-invalid-ssh-recovery-directory")))?;

        let observation = (|| {
            let flag = AtomicBool::new(false);
            let (add, present) = if boot {
                (
                    super::super::firewall::present_ssh_rules_boot(&journal.firewall, &flag)?,
                    super::super::firewall::present_ssh_rules_boot(
                        &journal.plan.remove_firewall,
                        &flag,
                    )?,
                )
            } else {
                (
                    super::super::firewall::present_ssh_rules(&journal.firewall, &flag)?,
                    super::super::firewall::present_ssh_rules(
                        &journal.plan.remove_firewall,
                        &flag,
                    )?,
                )
            };
            let remove = journal
                .plan
                .remove_firewall
                .iter()
                .filter(|rule| !present.contains(rule))
                .cloned()
                .collect::<Vec<_>>();
            Ok::<_, ManagementError>((add, remove))
        })();
        let (add, remove, observed) = match observation {
            Ok((add, remove)) => (add, remove, true),
            Err(_) => (
                journal.firewall.clone(),
                journal.plan.remove_firewall.clone(),
                false,
            ),
        };
        session.set_payload(serde_json::json!({"transaction_id":id,"backup_id":session.id(),"firewall_observed":observed,"firewall_restore":{"add":add,"remove":remove}}))?;
        journal.rollback_backup_id = Some(session.id().into());
        write(directory, journal)?;
    }
    let flag = AtomicBool::new(false);
    let result = (|| {
        let expected = journal
            .plan
            .files
            .iter()
            .map(|f| (f.before.path.clone(), (f.after.clone(), f.after_existed)))
            .collect();
        super::super::config_backups::restore_pending_selected_files_unrecorded(
            &journal.backup_id,
            &expected,
        )?;
        let firewall_result = (|| {
            let present = if boot {
                super::super::firewall::rollback_ssh_ports_boot_unrecorded(
                    &journal.firewall,
                    &flag,
                )?;
                super::super::firewall::present_ssh_rules_boot(
                    &journal.plan.remove_firewall,
                    &flag,
                )?
            } else {
                super::super::firewall::rollback_ssh_ports_unrecorded(&journal.firewall, &flag)?;
                super::super::firewall::present_ssh_rules(&journal.plan.remove_firewall, &flag)?
            };
            let missing = journal
                .plan
                .remove_firewall
                .iter()
                .filter(|rule| !present.contains(rule))
                .cloned()
                .collect::<Vec<_>>();
            if boot {
                super::super::firewall::apply_ssh_ports_boot_unrecorded(&missing, &flag)
            } else {
                super::super::firewall::apply_ssh_ports_unrecorded(&missing, &flag)
            }
        })();
        if boot {
            run("systemctl", &["daemon-reload"], &flag)?;
        } else {
            // Socket restart may have failed after stopping the listener. The
            // recorded KillMode was checked before any file was changed.
            restore_load(&journal.plan.service, &flag)?;
        }
        firewall_result?;
        for file in &journal.plan.files {
            let actual = system_config::read(&file.before.path)?;
            if actual.content != file.before.content || actual.existed != file.before.existed {
                return Err(ManagementError::Failed(
                    i18n::tr!("platform-ssh-restored-ssh-file-did-not-match-the-backup").into(),
                ));
            }
        }
        Ok(())
    })();
    match &result {
        Ok(()) => {
            journal.state = State::Restored;
            journal.result = i18n::tr!(
                "platform-ssh-previous-ssh-settings-and-this-change-s-added-firewall-rules"
            )
            .into();
        }
        Err(error) => {
            journal.state = State::RestoreFailed;
            journal.result = i18n::tr!(
                "platform-ssh-recovery-failed",
                error = error.to_string(),
                backup = journal.backup_id.clone()
            );
        }
    }
    write(directory, journal)?;
    if result.is_ok() {
        super::super::config_backups::finish(&journal.backup_id, &journal.result)?;
    } else {
        super::super::config_backups::set_result(&journal.backup_id, &journal.result)?;
    }
    if let Some(id) = &journal.rollback_backup_id {
        if result.is_ok() {
            super::super::config_backups::finish(id, &journal.result)?;
        } else {
            super::super::config_backups::set_result(id, &journal.result)?;
        }
    }
    result
}
fn restore_load(service: &Service, flag: &AtomicBool) -> Result<(), ManagementError> {
    if service.socket {
        let properties = properties(&service.unit, flag)?;
        if properties.get("KillMode").map(String::as_str) != Some("process") {
            return Err(ManagementError::Conflict(
                i18n::tr!("platform-ssh-the-ssh-service-killmode-changed-recovery-refuses-to-kill-existing")
                    .into(),
            ));
        }
        run("systemctl", &["daemon-reload"], flag)?;
        run("systemctl", &["stop", &service.unit, "ssh.socket"], flag)?;
        run("systemctl", &["start", "ssh.socket"], flag)?;
        run("systemctl", &["start", &service.unit], flag)?;
    } else {
        let p = properties(&service.unit, flag)?;
        let pid = p.get("MainPID").and_then(|v| v.parse().ok()).unwrap_or(0);
        if connection::trusted_sshd(pid) {
            let before = connection::listener_inodes(pid)?;
            if unsafe { libc::kill(pid as i32, libc::SIGHUP) } != 0 {
                return Err(failure(std::io::Error::last_os_error()));
            }
            let started = Instant::now();
            loop {
                let current = connection::listener_inodes(pid)?;
                if !current.is_empty() && current.is_disjoint(&before) {
                    break;
                }
                if started.elapsed() > Duration::from_secs(10) {
                    return Err(ManagementError::Failed(
                        i18n::tr!("platform-ssh-the-ssh-listener-did-not-finish-restoring-its-configuration").into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        } else {
            run("systemctl", &["start", &service.unit], flag)?;
        }
    }
    let started = Instant::now();
    loop {
        let p = properties(&service.unit, flag)?;
        let pid = p
            .get("MainPID")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let listeners =
            connection::listening_ports(if service.socket { 1 } else { pid }).unwrap_or_default();
        if p.get("ActiveState").map(String::as_str) == Some("active")
            && !listeners.is_empty()
            && service
                .original_ports
                .iter()
                .all(|port| listeners.contains(port))
        {
            break;
        }
        if started.elapsed() > Duration::from_secs(10) {
            return Err(ManagementError::Failed(
                i18n::tr!("platform-ssh-ssh-did-not-restore-its-original-listening-tcp-ports")
                    .into(),
            ));
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    Ok(())
}

pub fn rollback_transaction(id: &str) -> Result<(), ManagementError> {
    root()?;
    if !valid_id(id) {
        return Err(invalid(&i18n::tr!("platform-ssh-invalid-ssh-recovery-id")));
    }
    private(Path::new(ROOT))?;
    let directory = Path::new(ROOT).join(id);
    {
        let _lock = lock(&directory)?;
        read(&directory)?;
        atomic(&directory.join("ready"), b"ready\n", 0o600)?;
    }
    loop {
        {
            let _lock = lock(&directory)?;
            let mut journal = read(&directory)?;
            if finished(journal.state) {
                return Ok(());
            }
            let reboot = journal.boot_id != boot_id()?;
            if reboot
                || now() >= journal.deadline
                || !owner_alive(&journal)
                || journal.state == State::RestoreFailed
            {
                return restore(&directory, &mut journal, reboot);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
pub fn recover_pending() -> Result<(), ManagementError> {
    root()?;
    if !Path::new(ROOT).exists() {
        return Ok(());
    }
    private(Path::new(ROOT))?;
    let _global = lock(Path::new(ROOT))?;
    let mut errors = Vec::new();
    for entry in fs::read_dir(ROOT).map_err(failure)? {
        let entry = entry.map_err(failure)?;
        if !entry.file_type().map_err(failure)?.is_dir() {
            continue;
        }
        let _lock = lock(&entry.path())?;
        let mut journal = read(&entry.path())?;
        if !finished(journal.state)
            && let Err(error) = restore(&entry.path(), &mut journal, true)
        {
            errors.push(error.to_string());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ManagementError::Failed(errors.join("; ")))
    }
}
pub(super) fn confirm(
    id: &str,
    context: &ExecutionContext,
    keep: bool,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let directory = actor_directory(id, context.actor_uid)?;
    let _lock = lock(&directory)?;
    let mut journal = read(&directory)?;
    if journal.state != State::AwaitingConfirmation {
        return Err(ManagementError::Conflict(i18n::tr!(
            "platform-ssh-transaction-state",
            state = state_text(journal.state),
            result = journal.result.clone()
        )));
    }
    if !keep || now() >= journal.deadline || journal.boot_id != boot_id()? {
        restore(&directory, &mut journal, false)?;
        return if keep {
            Err(ManagementError::Conflict(
                i18n::tr!("platform-ssh-ssh-confirmation-expired-previous-settings-were-restored")
                    .into(),
            ))
        } else {
            Ok(journal.result)
        };
    }
    let validation = connection::verify_new_connection(
        context,
        journal.applied_ticks,
        &confirmation_ports(&journal.plan.ports, &journal.plan.service.original_ports),
    )
    .and_then(|_| verify_plan(&journal.plan, flag));
    if let Err(error) = validation {
        return match restore(&directory, &mut journal, false) {
            Ok(()) => Err(ManagementError::Failed(i18n::tr!(
                "platform-ssh-error-restored",
                error = error.to_string()
            ))),
            Err(recovery) => Err(ManagementError::Failed(i18n::tr!(
                "platform-ssh-error-transaction",
                error = error.to_string(),
                recovery = recovery.to_string(),
                id = id
            ))),
        };
    }
    journal.state = State::Committed;
    journal.result =
        i18n::tr!("platform-ssh-ssh-settings-confirmed-through-a-new-connection-and-saved").into();
    write(&directory, &journal)?;
    super::super::config_backups::finish(&journal.backup_id, &journal.result)?;
    Ok(journal.result)
}
pub(super) fn status(id: &str, uid: u32) -> Result<ManagementSnapshot, ManagementError> {
    let directory = actor_directory(id, uid)?;
    let _lock = lock(&directory)?;
    let j = read(&directory)?;
    Ok(ManagementSnapshot {
        columns: vec![
            i18n::tr!("platform-ssh-transaction").into(),
            i18n::tr!("platform-ssh-state").into(),
            i18n::tr!("platform-ssh-seconds-remaining").into(),
        ],
        rows: vec![ManagementRow {
            id: id.into(),
            cells: vec![
                id.into(),
                state_text(j.state),
                j.deadline.saturating_sub(now()).to_string(),
            ],
            detail: vec![
                (i18n::tr!("platform-ssh-result").into(), j.result),
                (i18n::tr!("platform-ssh-backup").into(), j.backup_id),
            ],
            ..Default::default()
        }],
        backend: i18n::tr!("platform-ssh-ssh-recovery-journal").into(),
        ..Default::default()
    })
}

pub(super) fn restore_backup(
    payload: &serde_json::Value,
    context: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let id = payload
        .get("transaction_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            invalid(&i18n::tr!(
                "platform-ssh-this-ssh-backup-has-no-recovery-transaction"
            ))
        })?;
    let directory = actor_directory(id, context.actor_uid)?;
    let source = read(&directory)?;
    if !finished(source.state) {
        return Err(ManagementError::Conflict(
            i18n::tr!("platform-ssh-confirm-or-cancel-the-pending-ssh-change-before-restoring-its")
                .into(),
        ));
    }
    let service = discover(flag)?;
    if let Some(reason) = &service.reload_reason {
        return Err(ManagementError::Unavailable(reason.clone()));
    }
    let backup_id = payload
        .get("backup_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&source.backup_id)
        .to_string();
    super::super::config_backups::validate_current(&backup_id)?;
    let mut files = Vec::new();
    for (desired, _) in super::super::config_backups::snapshots(&backup_id)? {
        if !source
            .plan
            .files
            .iter()
            .any(|file| file.before.path == desired.path)
        {
            continue;
        }
        files.push(Change {
            before: system_config::read(&desired.path)?,
            after: desired.content,
            after_existed: desired.existed,
        });
    }
    let mut candidates: Vec<_> = files
        .iter()
        .filter(|f| f.before.path == Path::new(MAIN) || f.before.path.starts_with("/etc/ssh"))
        .map(|f| (f.before.path.clone(), f.after.clone()))
        .collect();
    ensure_main(&mut candidates)?;
    let data = effective(&system_config::check_ssh_candidates(
        &candidates,
        &spec("root", &BTreeMap::new(), 22)?,
        flag,
    )?);
    let ports = configured_ports(&data)?;
    if payload
        .get("firewall_observed")
        .and_then(serde_json::Value::as_bool)
        == Some(false)
    {
        return Err(ManagementError::Conflict(
            i18n::tr!("platform-ssh-the-firewall-state-before-this-recovery-could-not-be-read")
                .into(),
        ));
    }
    let remove_rules: Vec<super::super::firewall::OwnedPortRule> = serde_json::from_value(
        payload
            .pointer("/firewall_restore/remove")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )
    .map_err(|e| ManagementError::Failed(e.to_string()))?;
    let add_rules: Vec<super::super::firewall::OwnedPortRule> = serde_json::from_value(
        payload
            .pointer("/firewall_restore/add")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )
    .map_err(|e| ManagementError::Failed(e.to_string()))?;
    let remove_firewall = super::super::firewall::present_ssh_rules(&remove_rules, flag)?;

    apply(
        Plan {
            service,
            files,
            ports,
            key_command: None,
            backup_to_restore: Some(backup_id),
            seed_backup: None,
            remove_firewall,
            add_firewall: add_rules,
        },
        false,
        context,
        io,
        flag,
    )
}

#[cfg(test)]
#[path = "../../../tests/unit/management/ssh_transaction_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../tests/unit/management/ssh_transaction_isolated.rs"]
pub(super) mod real_tests;
