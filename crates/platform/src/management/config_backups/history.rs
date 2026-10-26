use super::index::Summary;
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum SortValue {
    Text(String),
    Time(u64),
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    column: usize,
    descending: bool,
    value: SortValue,
    id: String,
}

fn sorting(query: &ManagementQuery) -> Result<(usize, bool), ManagementError> {
    let column = query
        .options
        .get("sort_column")
        .map(|column| column.parse::<usize>())
        .transpose()
        .map_err(|_| {
            ManagementError::InvalidInput(i18n::tr!(
                "backup-history-invalid-option",
                name = "sort_column"
            ))
        })?
        .unwrap_or(0);
    let descending = match query.options.get("sort_descending").map(String::as_str) {
        None | Some("true") => true,
        Some("false") => false,
        _ => {
            return Err(ManagementError::InvalidInput(i18n::tr!(
                "backup-history-invalid-option",
                name = "sort_descending"
            )));
        }
    };
    if column > 4 {
        return Err(ManagementError::InvalidInput(i18n::tr!(
            "backup-history-invalid-option",
            name = "sort_column"
        )));
    }
    Ok((column, descending))
}

fn sort_value(summary: &Summary, column: usize) -> SortValue {
    match column {
        0 => SortValue::Text(summary.id.clone()),
        1 => SortValue::Time(summary.timestamp),
        2 => SortValue::Text(source_name(&summary.source).to_lowercase()),
        3 => SortValue::Text(summary.subject.to_lowercase()),
        _ => SortValue::Text(result_name(&summary.result).to_lowercase()),
    }
}

fn source_name(source: &str) -> String {
    match source {
        "system-config" => i18n::tr!("platform-backups-source-config"),
        "system-settings" => i18n::tr!("system-settings-title"),
        "users" => i18n::tr!("platform-backups-source-users"),
        "network" => i18n::tr!("platform-backups-source-network"),
        "firewall" => i18n::tr!("platform-backups-source-firewall"),
        "ssh" => i18n::tr!("platform-backups-source-ssh"),
        "scheduled-tasks" => i18n::tr!("platform-backups-source-scheduled"),
        "restore" => i18n::tr!("platform-backups-source-restore"),
        "legacy" => i18n::tr!("platform-backups-source-legacy"),
        _ => source.into(),
    }
}

fn result_name(result: &str) -> String {
    match result {
        "pending" => i18n::tr!("management-value-pending"),
        "restored" => i18n::tr!("management-value-restored"),
        "restore failed" => i18n::tr!("platform-backups-restore-failed"),
        "awaiting SSH connection confirmation" => i18n::tr!("platform-backups-awaiting-ssh"),
        "awaiting network confirmation" => i18n::tr!("platform-backups-awaiting-network"),
        "saved" => i18n::tr!("platform-backups-saved"),
        "rolled back" => i18n::tr!("system-settings-rolled-back"),
        "recovery conflict" => i18n::tr!("system-settings-recovery-conflict"),
        "conflict" => i18n::tr!("platform-management-review-again"),
        "restored absence" => i18n::tr!("platform-backups-restored-absence"),
        "" => i18n::tr!("platform-backups-unknown"),
        _ => result.into(),
    }
}

fn option_time(query: &ManagementQuery, name: &str) -> Result<Option<i64>, ManagementError> {
    query
        .options
        .get(name)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value.parse().map_err(|_| {
                ManagementError::InvalidInput(i18n::tr!(
                    "backup-history-invalid-option",
                    name = name
                ))
            })
        })
        .transpose()
}

fn matches_query(summary: &Summary, query: &ManagementQuery) -> Result<bool, ManagementError> {
    let from = option_time(query, "from_timestamp")?;
    let to = option_time(query, "to_timestamp")?;
    if from.zip(to).is_some_and(|(from, to)| from > to) {
        return Err(ManagementError::InvalidInput(i18n::tr!(
            "backup-history-invalid-range"
        )));
    }
    Ok(query
        .target
        .as_ref()
        .is_none_or(|target| target == &summary.id)
        && query
            .options
            .get("source")
            .is_none_or(|source| source.is_empty() || source == "all" || source == &summary.source)
        && from.is_none_or(|from| i128::from(summary.timestamp) >= i128::from(from))
        && to.is_none_or(|to| i128::from(summary.timestamp) <= i128::from(to))
        && (query.filter.is_empty()
            || format!(
                "{} {} {} {} {} {}",
                summary.id,
                summary.source,
                source_name(&summary.source),
                summary.subject,
                summary.result,
                result_name(&summary.result)
            )
            .to_lowercase()
            .contains(&query.filter.to_lowercase())))
}

fn empty_snapshot(query: &ManagementQuery) -> ManagementSnapshot {
    ManagementSnapshot {
        details_complete: query.target.is_some(),
        backend: i18n::tr!("platform-backups-backend"),
        columns: [
            "platform-backups-recovery",
            "platform-backups-time",
            "platform-backups-source",
            "platform-backups-file",
            "platform-backups-result",
        ]
        .into_iter()
        .map(|key| i18n::tr!(key))
        .collect(),
        details: vec![
            (
                i18n::tr!("management-backups-how-title"),
                i18n::tr!("management-backups-how"),
            ),
            (
                i18n::tr!("management-backups-scope-title"),
                i18n::tr!("management-backups-scope"),
            ),
        ],
        ..Default::default()
    }
}

fn inspection_action(id: &str, label: &str, query: &ManagementQuery) -> ManagementAction {
    ManagementAction {
        id: id.into(),
        label: i18n::tr!(label),
        values: query.inspection_values(),
        ..Default::default()
    }
}

fn row(
    root: &Path,
    summary: &Summary,
    details: bool,
    preview: bool,
) -> Result<ManagementRow, ManagementError> {
    let time = chrono::DateTime::from_timestamp(summary.timestamp as i64, 0)
        .map(|time| {
            time.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| i18n::tr!("platform-backups-unknown"));
    let mut row = ManagementRow {
        id: summary.id.clone(),
        cells: vec![
            if summary.retained {
                i18n::tr!("backup-history-retained-id", id = summary.id.clone())
            } else {
                summary.id.clone()
            },
            time,
            source_name(&summary.source),
            summary.subject.clone(),
            result_name(&summary.result),
        ],
        identity: BTreeMap::from([
            ("backup_id".into(), summary.id.clone()),
            ("source".into(), summary.source.clone()),
            ("retained".into(), summary.retained.to_string()),
        ]),
        ..Default::default()
    };
    if preview {
        return Ok(row);
    }
    row.actions.push(ManagementAction {
        id: if summary.retained {
            "release_backup"
        } else {
            "retain_backup"
        }
        .into(),
        label: i18n::tr!(if summary.retained {
            "backup-history-release"
        } else {
            "backup-history-retain"
        }),
        privileged: true,
        ..Default::default()
    });
    let fields = vec![ManagementField {
        id: "allow_unvalidated".into(),
        label: i18n::tr!("platform-backups-unvalidated"),
        value: "false".into(),
        choices: vec!["false".into(), "true".into()],
        ..Default::default()
    }];
    if summary.legacy {
        row.identity.insert("legacy".into(), "true".into());
        row.identity.insert("path".into(), summary.subject.clone());
        if details {
            let stored = system_config::read_backup(&root.join(format!("{}.json", summary.id)))?;
            let current = system_config::read(&stored.document.path)?;
            row.detail = vec![
                (i18n::tr!("platform-backups-file"), summary.subject.clone()),
                (
                    i18n::tr!("platform-backups-original"),
                    safe_text(&stored.document.path, &stored.document.content),
                ),
                (
                    i18n::tr!("platform-backups-current"),
                    safe_text(&current.path, &current.content),
                ),
            ];
            row.actions.push(ManagementAction {
                id: "restore_legacy".into(),
                label: i18n::tr!("platform-backups-restore-legacy"),
                privileged: true,
                confirm: true,
                values: BTreeMap::from([
                    ("path".into(), summary.subject.clone()),
                    ("backup_id".into(), summary.id.clone()),
                    ("expected_version".into(), current.version),
                ]),
                fields,
                primary: true,
                ..Default::default()
            });
        }
    } else {
        row.actions.push(ManagementAction {
            id: "preview_restore".into(),
            label: i18n::tr!("platform-backups-view-diff"),
            privileged: true,
            ..Default::default()
        });
        row.actions.push(ManagementAction {
            id: "restore".into(),
            label: i18n::tr!("platform-backups-restore"),
            privileged: true,
            confirm: true,
            primary: true,
            fields,
            disabled_reason: summary.disabled_reason.clone().or_else(|| {
                (!summary.restorable).then(|| i18n::tr!("platform-backups-unfinished"))
            }),
            ..Default::default()
        });
        if details {
            row.detail.push((
                i18n::tr!("platform-backups-difference"),
                differences(&read_record(root, &summary.id)?),
            ));
        }
    }
    Ok(row)
}

fn active_ids(root: &Path) -> Result<BTreeSet<String>, ManagementError> {
    if root != Path::new(ROOT) {
        return Ok(BTreeSet::new());
    }
    let mut ids = super::super::network::active_backup_ids()?;
    ids.extend(super::super::ssh::active_backup_ids()?);
    Ok(ids)
}

fn candidates(
    root: &Path,
    entries: &BTreeMap<String, Summary>,
    query: &ManagementQuery,
) -> Result<Vec<Summary>, ManagementError> {
    let busy = active_ids(root)?;
    let mut selected = BTreeSet::new();
    for summary in entries.values() {
        if matches_query(summary, query)?
            && summary.completed
            && !summary.retained
            && !busy.contains(&summary.id)
            && index::lease(root, &summary.id, false)?.is_some()
        {
            selected.insert(summary.id.clone());
        }
    }
    loop {
        let protect = entries
            .values()
            .filter(|entry| !selected.contains(&entry.id))
            .flat_map(|entry| entry.references.iter().cloned())
            .collect::<BTreeSet<_>>();
        let previous = selected.len();
        selected.retain(|id| !protect.contains(id));
        if selected.len() == previous {
            break;
        }
    }
    Ok(selected
        .into_iter()
        .filter_map(|id| entries.get(&id).cloned())
        .collect())
}

fn token(query: &ManagementQuery, candidates: &[Summary]) -> Result<String, ManagementError> {
    let data = serde_json::to_vec(&(
        &query.filter,
        &query.target,
        query.options.get("source"),
        query.options.get("from_timestamp"),
        query.options.get("to_timestamp"),
        candidates
            .iter()
            .map(|entry| (&entry.id, &entry.files))
            .collect::<Vec<_>>(),
    ))
    .map_err(error)?;
    Ok(format!("{:x}", Sha256::digest(data)))
}

pub(super) fn query_at(
    root: &Path,
    query: &ManagementQuery,
    flag: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    check_cancelled(flag)?;
    let mut snapshot = empty_snapshot(query);
    if option_time(query, "from_timestamp")?
        .zip(option_time(query, "to_timestamp")?)
        .is_some_and(|(from, to)| from > to)
    {
        return Err(ManagementError::InvalidInput(i18n::tr!(
            "backup-history-invalid-range"
        )));
    }
    let missing = match fs::read_dir(root) {
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            snapshot.needs_authorization = true;
            snapshot
                .notices
                .push(i18n::tr!("management-backups-permission"));
            let mut action =
                inspection_action("inspect_backups", "platform-backups-authorize", query);
            action.privileged = true;
            snapshot.actions.push(action);
            return Ok(snapshot);
        }
        Err(failure) => return Err(error(failure)),
    };
    if missing {
        snapshot.notices.push(i18n::tr!(
            "backup-history-overview",
            count = 0,
            bytes = 0,
            retained = 0
        ));
        snapshot.notices.push(i18n::tr!("management-backups-empty"));
        snapshot.actions.push(inspection_action(
            "filter_backups",
            "backup-history-filter",
            query,
        ));
        return Ok(snapshot);
    }
    let _lock = match index::lock(root) {
        Ok(lock) => lock,
        Err(ManagementError::PermissionDenied(message)) => {
            snapshot.needs_authorization = true;
            snapshot
                .notices
                .push(i18n::tr!("management-backups-permission"));
            snapshot
                .details
                .push((i18n::tr!("management-requirements"), message));
            let mut action =
                inspection_action("inspect_backups", "platform-backups-authorize", query);
            action.privileged = true;
            snapshot.actions.push(action);
            return Ok(snapshot);
        }
        Err(error) => return Err(error),
    };
    let entries = index::load(root, flag)?;
    let total_bytes = entries.values().map(Summary::bytes).sum::<u64>();
    snapshot.notices.push(i18n::tr!(
        "backup-history-overview",
        count = entries.len(),
        bytes = total_bytes,
        retained = entries.values().filter(|entry| entry.retained).count()
    ));
    let preview = query.scope == "cleanup-preview";
    let filtered = if preview {
        candidates(root, &entries, query)?
    } else {
        entries
            .values()
            .filter_map(|entry| match matches_query(entry, query) {
                Ok(true) => Some(Ok(entry.clone())),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    if preview {
        let mut ordered = filtered.clone();
        ordered.sort_by(|left, right| left.id.cmp(&right.id));
        snapshot.notices.push(i18n::tr!(
            "backup-history-cleanup-preview",
            count = filtered.len(),
            bytes = filtered.iter().map(Summary::bytes).sum::<u64>()
        ));
        let mut action = inspection_action("cleanup_backups", "backup-history-cleanup", query);
        action
            .values
            .insert("cleanup_token".into(), token(query, &ordered)?);
        action.confirm = true;
        action.privileged = true;
        action.primary = true;
        action.disabled_reason = filtered
            .is_empty()
            .then(|| i18n::tr!("backup-history-nothing-to-clean"));
        snapshot.actions.push(action);
        let mut cancel = query.clone();
        cancel.scope.clear();
        cancel.options.remove("cursor");
        cancel.options.remove("anchor");
        snapshot.actions.push(inspection_action(
            "cancel_cleanup_preview",
            "backup-history-cancel-preview",
            &cancel,
        ));
    } else {
        snapshot.actions.push(inspection_action(
            "filter_backups",
            "backup-history-filter",
            query,
        ));
        let mut preview = query.clone();
        preview.scope = "cleanup-preview".into();
        preview.options.remove("cursor");
        preview.options.remove("anchor");
        snapshot.actions.push(inspection_action(
            "preview_cleanup",
            "backup-history-preview-cleanup",
            &preview,
        ));
    }
    let (column, descending) = sorting(query)?;
    let cursor = query
        .options
        .get("cursor")
        .filter(|_| query.target.is_none())
        .filter(|cursor| !cursor.is_empty())
        .map(|cursor| {
            serde_json::from_str::<Cursor>(cursor).map_err(|_| {
                ManagementError::InvalidInput(i18n::tr!("backup-history-page-changed"))
            })
        })
        .transpose()?;
    if cursor
        .as_ref()
        .is_some_and(|cursor| cursor.column != column || cursor.descending != descending)
    {
        return Err(ManagementError::Conflict(i18n::tr!(
            "backup-history-page-changed"
        )));
    }
    let mut ordered = filtered
        .into_iter()
        .map(|summary| (sort_value(&summary, column), summary))
        .collect::<Vec<_>>();
    ordered.sort_by(|(left_value, left), (right_value, right)| {
        let order = left_value
            .cmp(right_value)
            .then_with(|| left.id.cmp(&right.id));
        if descending { order.reverse() } else { order }
    });
    let limit = query
        .options
        .get("page_size")
        .map(|size| size.parse::<usize>())
        .transpose()
        .map_err(|_| {
            ManagementError::InvalidInput(i18n::tr!(
                "backup-history-invalid-option",
                name = "page_size"
            ))
        })?
        .unwrap_or(50)
        .clamp(1, 200);
    let start = if let Some(cursor) = &cursor {
        ordered
            .iter()
            .position(|(value, entry)| {
                let order = value
                    .cmp(&cursor.value)
                    .then_with(|| entry.id.cmp(&cursor.id));
                if descending {
                    order.is_lt()
                } else {
                    order.is_gt()
                }
            })
            .unwrap_or(ordered.len())
    } else {
        query
            .options
            .get("anchor")
            .and_then(|anchor| ordered.iter().position(|(_, entry)| &entry.id == anchor))
            .map(|position| position / limit * limit)
            .unwrap_or(0)
    };
    for (_, summary) in ordered.iter().skip(start).take(limit) {
        check_cancelled(flag)?;
        snapshot
            .rows
            .push(row(root, summary, query.target.is_some(), preview)?);
    }
    if ordered.len().saturating_sub(start) > limit
        && let Some((value, last)) = ordered.get(start + limit - 1)
    {
        let mut next = query.clone();
        next.options.remove("anchor");
        next.options.insert(
            "cursor".into(),
            serde_json::to_string(&Cursor {
                column,
                descending,
                value: value.clone(),
                id: last.id.clone(),
            })
            .map_err(error)?,
        );
        snapshot.actions.push(inspection_action(
            "backups_next_page",
            "backup-history-next",
            &next,
        ));
    }
    if cursor.is_some() || start > 0 {
        let mut first = query.clone();
        first.options.remove("cursor");
        first.options.remove("anchor");
        snapshot.actions.push(inspection_action(
            "backups_first_page",
            "backup-history-first",
            &first,
        ));
    }
    if snapshot.rows.is_empty() {
        snapshot.notices.push(i18n::tr!("management-backups-empty"));
    }
    Ok(snapshot)
}

pub(super) fn execute_at(
    root: &Path,
    command: &ManagementCommand,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<Option<String>, ManagementError> {
    if !matches!(
        command.action.as_str(),
        "retain_backup" | "release_backup" | "cleanup_backups" | "preview_cleanup"
    ) {
        return Ok(None);
    }
    check_cancelled(flag)?;
    if command.action == "preview_cleanup" {
        let mut query = ManagementQuery::from_inspection(command);
        query.scope = "cleanup-preview".into();
        query.options.remove("cursor");
        query.options.remove("anchor");
        io.emit(OperationEvent::Snapshot {
            snapshot: query_at(root, &query, flag)?.with_authorized_refresh(command),
        });
        return Ok(Some(i18n::tr!("backup-history-preview-ready")));
    }
    let history = index::lock(root)?;
    if matches!(command.action.as_str(), "retain_backup" | "release_backup") {
        let id = command
            .target
            .as_deref()
            .ok_or_else(|| ManagementError::InvalidInput(i18n::tr!("platform-backups-select")))?;
        record_path(root, id)?;
        let retained = command.action == "retain_backup";
        if record_path(root, id)?.exists() {
            let mut record = read_record(root, id)?;
            record.retained = retained;
            write_record_locked(root, &record, history.current)?;
        } else {
            system_config::read_backup(&root.join(format!("{id}.json")))?;
            index::set_legacy_retained(root, id, retained)?;
            let mut entries = index::load(root, flag)?;
            if let Some(entry) = entries.get_mut(id) {
                entry.retained = retained;
            }
            index::rewrite(root, &entries)?;
        }
        return Ok(Some(i18n::tr!(if retained {
            "backup-history-retained"
        } else {
            "backup-history-released"
        })));
    }
    let query = ManagementQuery::from_inspection(command);
    let mut entries = index::load(root, flag)?;
    let candidates = candidates(root, &entries, &query)?;
    let expected = command.values.get("cleanup_token").ok_or_else(|| {
        ManagementError::InvalidInput(i18n::tr!("backup-history-preview-required"))
    })?;
    if token(&query, &candidates)? != *expected {
        return Err(ManagementError::Conflict(i18n::tr!(
            "backup-history-record-changed"
        )));
    }
    let _gate = index::cleanup_lease(root)?
        .ok_or_else(|| ManagementError::Conflict(i18n::tr!("backup-history-record-changed")))?;
    for candidate in &candidates {
        check_cancelled(flag)?;
        if !index::unchanged(root, candidate)? {
            let _ = fs::remove_file(root.join("index/checkpoint.json"));
            return Err(ManagementError::Conflict(i18n::tr!(
                "backup-history-record-changed"
            )));
        }
    }
    for candidate in &candidates {
        check_cancelled(flag)?;
        for path in candidate.paths(root).into_iter().rev() {
            fs::remove_file(path).map_err(error)?;
        }
        entries.remove(&candidate.id);
        match fs::remove_file(root.join("index/leases").join(&candidate.id)) {
            Ok(()) => {}
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => {}
            Err(failure) => return Err(error(failure)),
        }
    }
    for directory in [root.to_owned(), root.join("operations")] {
        if directory.exists() {
            let (directory, _) = system_config::parent(&directory.join("sync"), false)?;
            directory.sync_all().map_err(error)?;
        }
    }
    index::rewrite(root, &entries)?;
    Ok(Some(i18n::tr!(
        "backup-history-cleaned",
        count = candidates.len(),
        bytes = candidates.iter().map(Summary::bytes).sum::<u64>()
    )))
}

#[cfg(test)]
#[path = "../../../tests/unit/management/backup_history_tests.rs"]
mod tests;
