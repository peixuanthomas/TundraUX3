//! Shared task-page data; system archives keep the existing authorization path.
use super::*;
use crate::task_history::{CleanupPreview, HistoryQuery, HistoryStore, TaskSummary};
use std::sync::atomic::Ordering;

pub fn local_store() -> Result<HistoryStore, ManagementError> {
    Ok(HistoryStore::new(
        crate::AppPaths::from_environment()
            .map_err(|e| ManagementError::Failed(e.to_string()))?
            .data_path()
            .join("task-history"),
    ))
}

#[cfg(target_os = "linux")]
pub fn system_store(actor: u32) -> HistoryStore {
    HistoryStore::new(PathBuf::from("/var/lib/tundraux3/task-history").join(actor.to_string()))
}

pub fn history_query(query: &ManagementQuery) -> HistoryQuery {
    HistoryQuery {
        filter: query.filter.clone(),
        status: query.options.get("status").cloned().unwrap_or_default(),
        before: query.options.get("before").cloned(),
        limit: 50,
    }
}

pub fn task_row(summary: &TaskSummary, bucket: &str, privileged: bool) -> ManagementRow {
    let row_id = format!("{bucket}:{}", summary.id);
    let timestamp = |seconds| {
        chrono::DateTime::from_timestamp(seconds as i64, 0)
            .map(|date| {
                date.with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_default()
    };
    let mut detail = vec![(i18n::tr!("task-column-result"), summary.result.clone())];
    if let Some(at) = summary.finished_at {
        detail.push((i18n::tr!("task-finished-at"), timestamp(at)));
    }
    if !summary.output_complete {
        detail.push((
            i18n::tr!("task-column-output"),
            i18n::tr!("task-output-incomplete"),
        ));
    }
    let actions = [
        ("task_output", "task-action-output", false),
        ("task_output_previous", "task-output-previous", false),
        ("task_output_next", "task-output-next", false),
        ("task_return", "task-action-return", false),
        (
            "task_cancel",
            "task-action-cancel",
            !summary.status.active(),
        ),
        (
            "task_cleanup",
            "task-action-cleanup",
            summary.status.active(),
        ),
    ]
    .into_iter()
    .map(|(id, label, disabled)| ManagementAction {
        id: id.into(),
        label: i18n::tr!(label),
        privileged: (privileged && matches!(id, "task_output" | "task_cleanup" | "task_cancel"))
            || (bucket.starts_with("scheduled-") && id == "task_cancel"),
        confirm: id == "task_cancel",
        disabled_reason: disabled.then(|| i18n::tr!("task-action-unavailable")),
        primary: id != "task_cleanup",
        ..Default::default()
    })
    .collect();
    let name = if bucket == "local"
        && summary
            .operation_id
            .as_ref()
            .is_some_and(|id| id.starts_with("/run/tundraux3-management/"))
    {
        i18n::tr!("task-personal-copy-name", name = summary.name.clone())
    } else {
        summary.name.clone()
    };
    ManagementRow {
        id: row_id,
        cells: vec![
            name,
            summary.status.label(),
            timestamp(summary.started_at),
            source_label(&summary.source),
        ],
        detail,
        actions,
        identity: BTreeMap::from([
            ("history_id".into(), summary.id.clone()),
            ("bucket".into(), bucket.into()),
            ("origin".into(), summary.origin.clone()),
            ("status".into(), summary.status.id().into()),
            (
                "operation_id".into(),
                summary.operation_id.clone().unwrap_or_default(),
            ),
            ("started_at".into(), summary.started_at.to_string()),
        ]),
    }
}

pub fn source_label(source: &str) -> String {
    let key = source.strip_prefix("management:").unwrap_or(source);
    let translated = i18n::tr!(format!("task-source-{key}"));
    if translated.starts_with('[') {
        source.into()
    } else {
        translated
    }
}

pub fn append_store(
    snapshot: &mut ManagementSnapshot,
    query: &ManagementQuery,
    store: &HistoryStore,
    bucket: &str,
    privileged: bool,
) -> Result<(), ManagementError> {
    let page = store.list(&history_query(query))?;
    snapshot
        .task_history_usage
        .push(crate::task_history::HistoryUsage {
            source: bucket.into(),
            records: page.total,
            bytes: page.bytes,
            active_ids: page.active_ids,
        });
    snapshot.notices.extend(page.notices);
    snapshot.rows.extend(
        page.records
            .iter()
            .map(|summary| task_row(summary, bucket, privileged)),
    );
    if let Some(next) = page.next {
        snapshot.actions.push(ManagementAction {
            id: "task_next_page".into(),
            label: i18n::tr!("task-next-page"),
            values: BTreeMap::from([("before".into(), next)]),
            ..Default::default()
        });
    }
    Ok(())
}

pub fn query(
    query: &ManagementQuery,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    let mut snapshot = empty_snapshot(query);
    if cancelled.load(Ordering::Relaxed) {
        return Err(ManagementError::Cancelled);
    }
    let store = query
        .options
        .get("local_history_root")
        .map(|root| HistoryStore::new(PathBuf::from(root)))
        .map(Ok)
        .unwrap_or_else(local_store)?;
    append_store(&mut snapshot, query, &store, "local", false)?;
    #[cfg(target_os = "linux")]
    if unsafe { libc::geteuid() } == 0 {
        append_system(&mut snapshot, query, unsafe { libc::getuid() })?;
    }
    Ok(snapshot)
}

pub fn finish_snapshot(snapshot: &mut ManagementSnapshot) {
    snapshot
        .task_history_usage
        .sort_by(|a, b| a.source.cmp(&b.source));
    snapshot
        .task_history_usage
        .dedup_by(|a, b| a.source == b.source);
    let total = snapshot
        .task_history_usage
        .iter()
        .map(|usage| usage.bytes)
        .sum::<u64>();
    let sources = snapshot
        .task_history_usage
        .iter()
        .map(|usage| {
            let source = match usage.source.as_str() {
                "local" => i18n::tr!("task-history-personal"),
                "system" => i18n::tr!("task-history-system"),
                source => i18n::tr!(
                    "task-history-scheduled",
                    uid = source.trim_start_matches("scheduled-")
                ),
            };
            i18n::tr!(
                "task-history-source-statistics",
                source = source,
                count = usage.records,
                bytes = usage.bytes
            )
        })
        .collect::<Vec<_>>()
        .join(" · ");
    let statistics = i18n::tr!("task-history-total", bytes = total, sources = sources);
    snapshot
        .notices
        .retain(|notice| !notice.starts_with(&i18n::tr!("task-history-total-label")));
    snapshot.notices.insert(0, statistics);
    let more = snapshot
        .actions
        .iter()
        .any(|action| action.id == "task_next_page");
    snapshot
        .actions
        .retain(|action| action.id != "task_next_page");
    let system_ids = snapshot
        .rows
        .iter()
        .filter(|row| {
            row.identity
                .get("bucket")
                .is_some_and(|bucket| bucket != "local")
                && !row.identity.contains_key("cached_system")
        })
        .filter_map(|row| row.identity.get("history_id").cloned())
        .collect::<std::collections::BTreeSet<_>>();
    snapshot.rows.retain(|row| {
        row.identity
            .get("bucket")
            .is_none_or(|bucket| bucket != "local")
            || row
                .identity
                .get("history_id")
                .is_none_or(|id| !system_ids.contains(id))
    });
    let local_ids = snapshot
        .rows
        .iter()
        .filter(|row| {
            row.identity
                .get("bucket")
                .is_some_and(|bucket| bucket == "local")
        })
        .filter_map(|row| row.identity.get("history_id").cloned())
        .collect::<std::collections::BTreeSet<_>>();
    snapshot.rows.retain(|row| {
        !row.identity.contains_key("cached_system")
            || row
                .identity
                .get("history_id")
                .is_none_or(|id| !local_ids.contains(id))
    });
    snapshot.rows.sort_by(|a, b| {
        b.identity
            .get("started_at")
            .and_then(|s| s.parse::<u64>().ok())
            .cmp(
                &a.identity
                    .get("started_at")
                    .and_then(|s| s.parse::<u64>().ok()),
            )
            .then_with(|| {
                b.identity
                    .get("history_id")
                    .cmp(&a.identity.get("history_id"))
            })
    });
    snapshot.rows.dedup_by(|a, b| a.id == b.id);
    if snapshot.rows.len() > 50 || more {
        snapshot.rows.truncate(50);
        if let Some(row) = snapshot.rows.last() {
            snapshot.actions.push(ManagementAction {
                id: "task_next_page".into(),
                label: i18n::tr!("task-next-page"),
                values: BTreeMap::from([(
                    "before".into(),
                    format!(
                        "{}/{}",
                        row.identity.get("started_at").cloned().unwrap_or_default(),
                        row.identity.get("history_id").cloned().unwrap_or_default()
                    ),
                )]),
                ..Default::default()
            });
        }
    }
    snapshot.actions.push(ManagementAction {
        id: "task_first_page".into(),
        label: i18n::tr!("task-first-page"),
        ..Default::default()
    });
}

pub fn empty_snapshot(query: &ManagementQuery) -> ManagementSnapshot {
    let mut snapshot = ManagementSnapshot {
        backend: i18n::tr!("tasks-title"),
        details_complete: true,
        columns: [
            "task-column-name",
            "task-column-state",
            "task-column-started",
            "task-column-source",
        ]
        .into_iter()
        .map(|key| i18n::tr!(key))
        .collect(),
        actions: vec![ManagementAction {
            id: "task_filter".into(),
            label: i18n::tr!("task-action-filter"),
            fields: vec![ManagementField {
                id: "status".into(),
                label: i18n::tr!("task-column-state"),
                value: i18n::tr!(format!(
                    "task-state-{}",
                    query
                        .options
                        .get("status")
                        .map(String::as_str)
                        .unwrap_or("all")
                )),
                choices: ["all", "active", "attention", "finished"]
                    .into_iter()
                    .map(|status| i18n::tr!(format!("task-state-{status}")))
                    .collect(),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    #[cfg(target_os = "linux")]
    snapshot.actions.push(ManagementAction {
        id: "inspect_tasks".into(),
        label: i18n::tr!("task-action-system"),
        privileged: true,
        values: query.inspection_values(),
        ..Default::default()
    });
    snapshot
}

#[cfg(target_os = "linux")]
fn append_system(
    snapshot: &mut ManagementSnapshot,
    query: &ManagementQuery,
    actor: u32,
) -> Result<(), ManagementError> {
    append_store(snapshot, query, &system_store(actor), "system", true)?;
    let root = std::path::Path::new("/var/lib/tundraux3/scheduled-results");
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries {
            let entry = entry.map_err(|e| ManagementError::Failed(e.to_string()))?;
            let Some(uid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            let bucket = format!("scheduled-{uid}");
            let start = snapshot.rows.len();
            append_store(
                snapshot,
                query,
                &HistoryStore::new(root.join(uid.to_string()).join("history")),
                &bucket,
                uid != actor,
            )?;
            if uid != 0 && uid != actor {
                for row in &mut snapshot.rows[start..] {
                    for action in &mut row.actions {
                        if action.id == "task_cleanup" {
                            action.disabled_reason = Some(i18n::tr!("task-history-other-account"));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn command_store(command: &ManagementCommand, actor: u32) -> Result<HistoryStore, ManagementError> {
    let bucket = command
        .identity
        .get("bucket")
        .map(String::as_str)
        .unwrap_or("local");
    if bucket == "local" {
        return local_store();
    }
    #[cfg(target_os = "linux")]
    if let Some(uid) = bucket
        .strip_prefix("scheduled-")
        .and_then(|s| s.parse::<u32>().ok())
    {
        if uid == unsafe { libc::geteuid() } {
            return Ok(HistoryStore::new(
                PathBuf::from("/var/lib/tundraux3/scheduled-results")
                    .join(uid.to_string())
                    .join("history"),
            ));
        }
    }
    #[cfg(target_os = "linux")]
    if unsafe { libc::geteuid() } == 0 {
        if bucket == "system" {
            return Ok(system_store(actor));
        }
        if let Some(uid) = bucket
            .strip_prefix("scheduled-")
            .and_then(|s| s.parse::<u32>().ok())
        {
            if command.action == "task_cleanup" && uid != 0 {
                return Err(ManagementError::PermissionDenied(i18n::tr!(
                    "task-history-other-account"
                )));
            }
            return Ok(HistoryStore::new(
                PathBuf::from("/var/lib/tundraux3/scheduled-results")
                    .join(uid.to_string())
                    .join("history"),
            ));
        }
    }
    let _ = actor;
    Err(ManagementError::PermissionDenied(i18n::tr!(
        "task-history-system-authorization"
    )))
}

pub fn execute(
    command: &ManagementCommand,
    context: &ExecutionContext,
    io: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    if cancelled.load(Ordering::Relaxed) {
        return Err(ManagementError::Cancelled);
    }
    if command.action == "inspect_tasks" {
        #[cfg(target_os = "linux")]
        {
            if unsafe { libc::geteuid() } != 0 {
                return Err(ManagementError::PermissionDenied(i18n::tr!(
                    "task-history-system-authorization"
                )));
            }
            let query = ManagementQuery::from_inspection(command);
            let mut snapshot = empty_snapshot(&query).with_authorized_refresh(command);
            append_system(&mut snapshot, &query, context.actor_uid)?;
            io.emit(OperationEvent::Snapshot { snapshot });
            return Ok(i18n::tr!("task-history-loaded"));
        }
    }
    let store = command_store(command, context.actor_uid)?;
    let id = command
        .identity
        .get("history_id")
        .ok_or_else(|| ManagementError::InvalidInput(i18n::tr!("task-history-invalid-id")))?;
    if command.action == "task_output" {
        let offset = command
            .values
            .get("offset")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let window = store.output_text(id, offset, 64 * 1024)?;
        io.emit(OperationEvent::TaskOutput {
            offset: window.offset,
            end: window.end,
            total: window.total,
            text: window.text,
        });
        return Ok(i18n::tr!("task-history-loaded"));
    }
    if command.action == "task_cleanup" {
        let preview = store.preview_cleanup(&[id.clone()])?;
        io.emit(OperationEvent::Output {
            text: cleanup_message(&preview),
        });
        let confirm = i18n::tr!("task-action-delete-confirm");
        let answer = io.ask(
            "task-history-cleanup",
            &i18n::tr!("task-cleanup-question"),
            &[confirm.clone(), i18n::tr!("task-action-keep")],
            false,
        )?;
        if answer != confirm {
            return Err(ManagementError::Cancelled);
        }
        let personal_copy = store.read(id)?.has_personal_copy;
        store.cleanup(&preview)?;
        return Ok(if personal_copy {
            i18n::tr!("task-history-system-cleaned")
        } else {
            i18n::tr!("task-history-cleaned")
        });
    }
    #[cfg(target_os = "linux")]
    if command.action == "task_cancel" {
        use std::io::{BufRead, Write};
        let summary = store.read(id)?;
        let socket = summary
            .operation_id
            .ok_or_else(|| ManagementError::InvalidInput(i18n::tr!("task-action-unavailable")))?;
        if let Some(unit) = socket.strip_prefix("systemd:system:") {
            return super::scheduled_tasks::cancel_run(unit, id, context, cancelled);
        }
        let mut stream = super::helper::connect(std::path::Path::new(&socket), context.actor_uid)?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_millis(100)))
            .map_err(|e| ManagementError::Failed(e.to_string()))?;
        let mut bytes = serde_json::to_vec(&OperationInput::Terminate)
            .map_err(|e| ManagementError::Failed(e.to_string()))?;
        bytes.push(b'\n');
        stream
            .write_all(&bytes)
            .map_err(|e| ManagementError::Failed(e.to_string()))?;
        let mut reader = std::io::BufReader::new(stream);
        let mut line = String::new();
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err(ManagementError::Cancelled);
            }
            match reader.read_line(&mut line) {
                Ok(0) => return Err(ManagementError::Failed(i18n::tr!("task-status-unknown"))),
                Ok(_) => {
                    if let Ok(record) = serde_json::from_str::<OperationRecord>(&line) {
                        if matches!(
                            record.event,
                            OperationEvent::Completed { .. } | OperationEvent::Failed { .. }
                        ) {
                            return Ok(i18n::tr!(
                                "task-cancel-result",
                                status = store.read(id)?.status.label()
                            ));
                        }
                    }
                    line.clear();
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) => {}
                Err(e) => return Err(ManagementError::Failed(e.to_string())),
            }
        }
    }
    Err(ManagementError::InvalidInput(i18n::tr!(
        "task-action-unavailable"
    )))
}

pub fn cleanup_message(preview: &CleanupPreview) -> String {
    i18n::tr!(
        "task-cleanup-preview",
        count = preview.entries.len(),
        bytes = preview.bytes,
        names = preview
            .entries
            .iter()
            .map(|entry| {
                let time = chrono::DateTime::from_timestamp(entry.started_at as i64, 0)
                    .map(|date| {
                        date.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_default();
                format!(
                    "{} · {} · {}\n  {}",
                    entry.name,
                    time,
                    entry.status.label(),
                    entry.id
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    )
}
