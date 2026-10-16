//! Persistent network history keeps native UUIDs beside protected file backups.
use super::super::config_backups;
use super::config::{OwnerKind, Plan};
use super::*;
use std::collections::BTreeSet;
use std::path::Path;

fn invalid(message: &str) -> ManagementError {
    ManagementError::InvalidInput(message.into())
}
fn profile_uuids(flag: &AtomicBool) -> Result<BTreeSet<String>, ManagementError> {
    Ok(run(
        "nmcli",
        &["--get-values", "UUID", "connection", "show"],
        flag,
    )?
    .lines()
    .map(String::from)
    .collect())
}
pub(super) fn profile_exists(uuid: &str, flag: &AtomicBool) -> Result<bool, ManagementError> {
    Ok(profile_uuids(flag)?.contains(uuid))
}
pub(super) fn refresh_files(plan: &mut Plan) -> Result<(), ManagementError> {
    for file in &mut plan.files {
        file.after = match fs_read(&file.path) {
            Ok(bytes) => bytes,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && (plan.operation == "wifi-forget" || plan.restore_record.is_some()) =>
            {
                Vec::new()
            }
            Err(error) => return Err(io_error(error)),
        };
    }
    Ok(())
}
fn fs_read(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}
fn profile_filename(uuid: &str) -> Result<PathBuf, ManagementError> {
    profile_file(uuid)?.0.ok_or_else(|| {
        ManagementError::Conflict(
            i18n::tr!("platform-network-history-the-saved-networkmanager-profile-has-no-verified-file-for-its").into(),
        )
    })
}
fn profile_file(uuid: &str) -> Result<(Option<PathBuf>, bool), ManagementError> {
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
    let profile = Proxy::new(
        &bus,
        NM,
        path.as_str(),
        "org.freedesktop.NetworkManager.Settings.Connection",
    )
    .map_err(dbus_error)?;
    let values: HashMap<String, HashMap<String, OwnedValue>> =
        profile.call("GetSettings", &()).map_err(dbus_error)?;
    let actual = values
        .get("connection")
        .and_then(|section| section.get("uuid"))
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| String::try_from(value).ok());
    let filename: String = profile.get_property("Filename").map_err(dbus_error)?;
    let unsaved: bool = profile.get_property("Unsaved").map_err(dbus_error)?;
    if actual.as_deref() != Some(uuid) {
        return Err(ManagementError::Conflict(
            i18n::tr!("platform-network-history-the-saved-networkmanager-profile-has-no-verified-file-for-its").into(),
        ));
    }
    if filename.is_empty() {
        return Ok((None, true));
    }
    let path = PathBuf::from(filename);
    if path.starts_with("/run/NetworkManager") {
        return Ok((None, true));
    }
    if !config::native_nm_file(&path) {
        return Err(ManagementError::Unavailable(
            i18n::tr!("platform-network-history-only-native-keyfile-networkmanager-profiles-support-persistent-recovery").into(),
        ));
    }
    config::checked_file(&path)?;
    Ok((Some(path), unsaved))
}
fn uuids(plan: &Plan) -> BTreeSet<String> {
    [
        plan.uuid.as_str(),
        plan.new_uuid.as_str(),
        plan.wifi
            .as_ref()
            .map(|wifi| wifi.saved_uuid.as_str())
            .unwrap_or(""),
        &plan.restore_uuid,
        &plan.remove_uuid,
    ]
    .into_iter()
    .filter(|uuid| !uuid.is_empty())
    .map(String::from)
    .collect()
}
fn versions(
    ids: &BTreeSet<String>,
    flag: &AtomicBool,
) -> Result<BTreeMap<String, Option<String>>, ManagementError> {
    let existing = profile_uuids(flag)?;
    ids.iter()
        .map(|id| {
            Ok((
                id.clone(),
                if existing.contains(id) {
                    Some(transaction::nm_configuration_version(id)?)
                } else {
                    None
                },
            ))
        })
        .collect()
}
pub(super) fn begin(
    plan: &Plan,
    actor_uid: u32,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let mut paths = plan
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let native = if plan.kind == OwnerKind::NetworkManager {
        let ids = uuids(plan);
        let before = versions(&ids, flag)?;
        if !plan.new_uuid.is_empty() && before.get(&plan.new_uuid).is_some_and(Option::is_some) {
            return Err(ManagementError::Conflict(
                i18n::tr!("platform-network-history-the-new-networkmanager-uuid-already-exists")
                    .into(),
            ));
        }
        for (uuid, version) in &before {
            if version.is_some() {
                let path = profile_filename(uuid)?;
                if !paths.contains(&path) {
                    paths.push(path.clone());
                }
                if path.parent() == Some(Path::new("/etc/sysconfig/network-scripts")) {
                    let name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .and_then(|name| name.strip_prefix("ifcfg-"))
                        .ok_or_else(|| invalid(&i18n::tr!("platform-network-history-invalid-native-networkmanager-profile-path")))?;
                    let keys = path.with_file_name(format!("keys-{name}"));
                    if !paths.contains(&keys) {
                        paths.push(keys);
                    }
                }
            }
        }
        serde_json::to_value(before).map_err(|e| ManagementError::Failed(e.to_string()))?
    } else {
        serde_json::Value::Null
    };
    let session = config_backups::begin("network", &paths)?;
    session.set_payload(serde_json::json!({"mode":"transaction","backup_id":session.id(),"actor_uid":actor_uid,"plan":plan,"before_versions":native,"expected_versions":native,"expected_active_uuid":plan.uuid}))?;
    Ok(session.id().into())
}
pub(super) fn finish(
    id: &str,
    plan: &Plan,
    actor_uid: u32,
    result: &str,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    let after = if plan.kind == OwnerKind::NetworkManager {
        serde_json::to_value(versions(&uuids(plan), flag)?)
            .map_err(|e| ManagementError::Failed(e.to_string()))?
    } else {
        serde_json::Value::Null
    };
    if plan.kind == OwnerKind::NetworkManager
        && !plan.new_uuid.is_empty()
        && profile_uuids(flag)?.contains(&plan.new_uuid)
    {
        let path = profile_filename(&plan.new_uuid)?;
        config_backups::append_created_path(id, &path)?;
    }
    let active = if plan.kind == OwnerKind::NetworkManager {
        nm_device(&plan.interface)?.uuid
    } else {
        String::new()
    };
    let mut payload = config_backups::payload(id)?;
    payload["plan"] =
        serde_json::to_value(plan).map_err(|e| ManagementError::Failed(e.to_string()))?;
    payload["actor_uid"] = serde_json::Value::from(actor_uid);
    payload["expected_versions"] = after;
    payload["expected_active_uuid"] = serde_json::Value::from(active);
    payload["result"] = serde_json::Value::from(result);
    config_backups::set_payload(id, payload)?;
    config_backups::finish(id, result)
}
fn rollback_plan(
    original: &Plan,
    active: String,
    existing: &BTreeMap<String, Option<String>>,
) -> Plan {
    let mut plan = original.clone();
    plan.restore_record = None;
    plan.operation = "rollback".into();
    plan.defer_confirmation = false;
    plan.uuid = active;
    plan.new_uuid = if original.kind == OwnerKind::NetworkManager
        && !original.uuid.is_empty()
        && existing.get(&original.uuid).is_some_and(Option::is_none)
    {
        original.uuid.clone()
    } else {
        String::new()
    };
    plan.restore_uuid = original.uuid.clone();
    plan.remove_uuid = if original.new_uuid.is_empty() {
        original
            .wifi
            .as_ref()
            .map(|wifi| wifi.saved_uuid.clone())
            .unwrap_or_default()
    } else {
        original.new_uuid.clone()
    };
    plan.wifi = None;
    plan.arguments.clear();
    plan.preview = i18n::tr!("platform-network-history-restore-the-saved-configuration-from-before-automatic-network-recovery-confirm").into();
    plan
}
pub(super) fn begin_rollback(
    original: &Plan,
    source: &str,
    actor_uid: u32,
    flag: &AtomicBool,
) -> Result<Option<(String, Plan)>, ManagementError> {
    let before = config_backups::before_snapshots(source)?;
    let mut paths = before
        .iter()
        .map(|document| document.path.clone())
        .collect::<BTreeSet<_>>();
    let active = if original.kind == OwnerKind::NetworkManager {
        nm_device(&original.interface)?.uuid
    } else {
        String::new()
    };
    let mut ids = uuids(original);
    if !active.is_empty() {
        ids.insert(active.clone());
    }
    let native = if original.kind == OwnerKind::NetworkManager {
        versions(&ids, flag)?
    } else {
        BTreeMap::new()
    };
    let mut temporary = false;
    for (uuid, version) in &native {
        if version.is_some() {
            let (path, unsaved) = profile_file(uuid)?;
            temporary |= unsaved;
            if let Some(path) = path {
                if path.parent() == Some(Path::new("/etc/sysconfig/network-scripts")) {
                    let name = path.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_prefix("ifcfg-")).ok_or_else(|| invalid(&i18n::tr!("platform-network-history-invalid-native-networkmanager-profile-path")))?;
                    paths.insert(path.with_file_name(format!("keys-{name}")));
                }
                paths.insert(path);
            }
        }
    }
    let mut plan = rollback_plan(original, active, &native);
    plan.files.clear();
    let mut changed = false;
    for path in &paths {
        let current = super::super::system_config::read(path)?;
        let previous = before.iter().find(|document| document.path == *path);
        changed |= previous.map_or(current.existed, |document| {
            document.version != current.version
        });
        plan.files.push(config::FileChange {
            path: path.clone(),
            before: current.content.into_bytes(),
            after: previous
                .map(|document| document.content.as_bytes().to_vec())
                .unwrap_or_default(),
            mode: current.mode,
        });
    }
    if !changed {
        return Ok(None);
    }
    let session = config_backups::begin("network", &paths.into_iter().collect::<Vec<_>>())?;
    let mut payload = serde_json::json!({"mode":"transaction","backup_id":session.id(),"actor_uid":actor_uid,"plan":plan,"before_versions":native,"expected_versions":native,"expected_active_uuid":plan.uuid});
    if temporary {
        payload["restore_disabled_reason"] = serde_json::Value::from(i18n::tr!(
            "platform-network-history-an-affected-connection-had-temporary-or-unsaved-settings-before-automatic"
        ));
    }
    session.set_payload(payload)?;
    Ok(Some((session.id().into(), plan)))
}
pub(super) fn candidates(
    plan: &Plan,
) -> Result<BTreeMap<PathBuf, (String, bool)>, ManagementError> {
    if let Some(id) = &plan.restore_record {
        return Ok(config_backups::snapshots(id)?
            .into_iter()
            .map(|(before, _)| (before.path, (before.content, before.existed)))
            .collect());
    }
    plan.files
        .iter()
        .map(|file| {
            Ok((
                file.path.clone(),
                (
                    String::from_utf8(file.after.clone()).map_err(|_| {
                        invalid(&i18n::tr!(
                            "platform-network-history-network-configurations-must-be-utf-8-text"
                        ))
                    })?,
                    plan.operation != "wifi-forget",
                ),
            ))
        })
        .collect()
}
fn decode(payload: &serde_json::Value) -> Result<Plan, ManagementError> {
    serde_json::from_value(payload.get("plan").cloned().ok_or_else(|| {
        invalid(&i18n::tr!(
            "platform-network-history-this-network-backup-has-no-saved-plan"
        ))
    })?)
    .map_err(|e| {
        ManagementError::Failed(i18n::tr!(
            "platform-network-history-invalid-plan",
            error = e.to_string()
        ))
    })
}
fn check_versions(payload: &serde_json::Value, flag: &AtomicBool) -> Result<(), ManagementError> {
    let expected: BTreeMap<String, Option<String>> = serde_json::from_value(
        payload
            .get("expected_versions")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    )
    .map_err(|_| {
        invalid(&i18n::tr!(
            "platform-network-history-the-network-backup-has-no-native-profile-versions"
        ))
    })?;
    if versions(&expected.keys().cloned().collect(), flag)? != expected {
        return Err(ManagementError::Conflict(i18n::tr!("platform-network-history-networkmanager-profiles-changed-after-this-operation-inspect-their-current-settings").into()));
    }
    Ok(())
}
pub(crate) fn restore_preflight(
    payload: &serde_json::Value,
    context: &ExecutionContext,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    if let Some(reason) = payload
        .get("restore_disabled_reason")
        .and_then(serde_json::Value::as_str)
    {
        return Err(ManagementError::Unavailable(reason.into()));
    }
    if unsafe { libc::geteuid() } != 0 {
        return Err(ManagementError::PermissionDenied(
            i18n::tr!("platform-network-history-network-recovery-requires-system-authorization")
                .into(),
        ));
    }
    let owner = payload
        .get("actor_uid")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            invalid(&i18n::tr!(
                "platform-network-history-the-network-backup-has-no-initiating-account"
            ))
        })?;
    if context.actor_uid != 0 && owner != context.actor_uid as u64 {
        return Err(ManagementError::PermissionDenied(
            i18n::tr!("platform-network-history-this-network-operation-belongs-to-another-account")
                .into(),
        ));
    }
    let id = payload
        .get("backup_id")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            invalid(&i18n::tr!(
                "platform-network-history-this-network-backup-has-no-id"
            ))
        })?;
    config_backups::validate_current(id)?;
    if payload.get("mode").and_then(serde_json::Value::as_str) == Some("inactive-wifi") {
        return check_versions(payload, flag);
    }
    if payload.get("result").is_none() {
        return Err(ManagementError::Conflict(i18n::tr!("platform-network-history-this-network-operation-has-not-finished-confirm-or-recover-it").into()));
    }
    let plan = decode(payload)?;
    verify_identity(&plan.interface, &plan.identity)?;
    if plan.kind == OwnerKind::NetworkManager {
        check_versions(payload, flag)?;
        if nm_device(&plan.interface)?.uuid
            != payload
                .get("expected_active_uuid")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
        {
            return Err(ManagementError::Conflict(
                i18n::tr!(
                    "platform-network-history-the-active-network-changed-after-this-operation"
                )
                .into(),
            ));
        }
    }
    Ok(())
}
pub(crate) fn restore_backup(
    payload: &serde_json::Value,
    context: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    restore_preflight(payload, context, flag)?;
    let id = payload["backup_id"].as_str().ok_or_else(|| {
        invalid(&i18n::tr!(
            "platform-network-history-missing-network-backup-id"
        ))
    })?;
    if payload.get("mode").and_then(serde_json::Value::as_str) == Some("inactive-wifi") {
        return restore_inactive(payload, id, context, flag);
    }
    let original = decode(payload)?;
    let active = if original.kind == OwnerKind::NetworkManager {
        nm_device(&original.interface)?.uuid
    } else {
        String::new()
    };
    let files = config_backups::snapshots(id)?
        .into_iter()
        .map(|(before, after)| config::FileChange {
            path: after.path,
            before: after.content.into_bytes(),
            after: before.content.into_bytes(),
            mode: after.mode,
        })
        .collect();
    let plan=Plan{restore_record:Some(id.into()),restore_uuid:original.uuid.clone(),remove_uuid:original.new_uuid.clone(),defer_confirmation:false,operation:"restore".into(),identity:original.identity,kind:original.kind,interface:original.interface,uuid:active,new_uuid:String::new(),files,arguments:Vec::new(),preview:i18n::tr!("platform-network-history-restore-the-selected-saved-network-operation-confirm-the-restored-connection").into(),wifi:None};
    transaction::apply(plan, context, io, flag)
}
pub(super) fn begin_inactive(
    uuid: &str,
    path: &Path,
    actor_uid: u32,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let mut paths = vec![path.into()];
    if path.parent() == Some(Path::new("/etc/sysconfig/network-scripts")) {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix("ifcfg-"))
            .ok_or_else(|| {
                invalid(&i18n::tr!(
                    "platform-network-history-invalid-networkmanager-profile-filename"
                ))
            })?;
        paths.push(path.with_file_name(format!("keys-{name}")));
    }
    let session = config_backups::begin("network", &paths)?;
    let before = versions(&BTreeSet::from([uuid.into()]), flag)?;
    session.set_payload(serde_json::json!({"mode":"inactive-wifi","backup_id":session.id(),"actor_uid":actor_uid,"uuid":uuid,"before_versions":before,"expected_versions":before}))?;
    Ok(session.id().into())
}
pub(super) fn finish_inactive(
    id: &str,
    uuid: &str,
    result: &str,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    let mut payload = config_backups::payload(id)?;
    payload["expected_versions"] =
        serde_json::to_value(versions(&BTreeSet::from([uuid.into()]), flag)?)
            .map_err(|e| ManagementError::Failed(e.to_string()))?;
    config_backups::set_payload(id, payload)?;
    config_backups::finish(id, result)
}
fn restore_inactive(
    payload: &serde_json::Value,
    id: &str,
    context: &ExecutionContext,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let uuid = payload
        .get("uuid")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            invalid(&i18n::tr!(
                "platform-network-history-missing-wi-fi-profile-uuid"
            ))
        })?;
    let active_before = run(
        "nmcli",
        &["--get-values", "UUID", "connection", "show", "--active"],
        flag,
    )?;
    if active_before.lines().any(|active| active == uuid) {
        return Err(ManagementError::Conflict(
            i18n::tr!(
                "platform-network-history-the-saved-wi-fi-profile-became-active-restore-it-through"
            )
            .into(),
        ));
    }
    let snapshots = config_backups::snapshots(id)?;
    let session = begin_inactive(uuid, &snapshots[0].0.path, context.actor_uid, flag)?;
    let outcome = (|| {
        config_backups::restore_files_unrecorded(id)?;
        let desired = snapshots.iter().any(|(before, _)| before.existed);
        if !desired && profile_uuids(flag)?.contains(uuid) {
            run("nmcli", &["connection", "delete", "uuid", uuid], flag)?;
        }
        run("nmcli", &["connection", "reload"], flag)?;
        if profile_uuids(flag)?.contains(uuid) != desired
            || run(
                "nmcli",
                &["--get-values", "UUID", "connection", "show", "--active"],
                flag,
            )? != active_before
        {
            return Err(ManagementError::Failed(
                i18n::tr!("platform-network-history-wi-fi-profile-restoration-did-not-match-the-saved-state").into(),
            ));
        }
        Ok(())
    })();
    finish_inactive(
        &session,
        uuid,
        &if outcome.is_ok() {
            i18n::tr!("platform-network-history-restored-inactive-wi-fi-profile")
        } else {
            i18n::tr!("platform-network-history-inactive-wi-fi-restore-failed")
        },
        &AtomicBool::new(false),
    )?;
    outcome
        .map(|_| i18n::tr!("platform-network-history-inactive-saved-wi-fi-profile-restored").into())
}

#[cfg(test)]
#[path = "../../../tests/unit/management/network_history_tests.rs"]
mod tests;
