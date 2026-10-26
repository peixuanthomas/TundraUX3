use super::*;

#[test]
fn history_survives_reopen_and_reads_only_requested_output_window() {
    let directory = tempfile::tempdir().unwrap();
    let store = HistoryStore::new(directory.path().join("history"));
    let summary = TaskSummary::new("测试任务".into(), "script".into(), "editor".into());
    let recorder = TaskRecorder::new(store.clone(), summary.clone()).unwrap();
    recorder.output(&vec![b'x'; 600_000]);
    recorder.finish(TaskStatus::Succeeded, "done".into());
    let reopened = HistoryStore::new(store.root());
    let page = reopened.list(&HistoryQuery::default()).unwrap();
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].status, TaskStatus::Succeeded);
    let (bytes, total) = reopened.output(&summary.id, 500_000, 100).unwrap();
    assert_eq!(bytes, vec![b'x'; 100]);
    assert_eq!(total, 600_000);
}

#[test]
fn cleanup_requires_same_finished_records_as_preview() {
    let directory = tempfile::tempdir().unwrap();
    let store = HistoryStore::new(directory.path().join("history"));
    let mut summary = TaskSummary::new("task".into(), "script".into(), "editor".into());
    store.save(&summary).unwrap();
    assert!(store.preview_cleanup(&[summary.id.clone()]).is_err());
    summary.status = TaskStatus::Failed;
    store.save(&summary).unwrap();
    let preview = store.preview_cleanup(&[summary.id.clone()]).unwrap();
    store.append_output(&summary.id, b"late output").unwrap();
    assert!(store.cleanup(&preview).is_err());
    assert!(store.read(&summary.id).is_ok());
    let preview = store.preview_cleanup(&[summary.id.clone()]).unwrap();
    store.cleanup(&preview).unwrap();
    assert!(!store.path(&summary.id, "json").unwrap().exists());
    assert!(!store.path(&summary.id, "output").unwrap().exists());
}

#[test]
fn abandoned_running_task_is_unknown_and_cannot_be_reported_successful() {
    let directory = tempfile::tempdir().unwrap();
    let store = HistoryStore::new(directory.path().join("history"));
    let mut summary = TaskSummary::new("task".into(), "script".into(), "editor".into());
    summary.process_id = 0;
    store.save(&summary).unwrap();
    assert_eq!(
        store.list(&HistoryQuery::default()).unwrap().records[0].status,
        TaskStatus::Unknown
    );
    assert!(store.path("../escape", "json").is_err());
}

#[test]
fn questions_and_documents_are_not_output_history() {
    use crate::management::OperationEvent;
    let directory = tempfile::tempdir().unwrap();
    let store = HistoryStore::new(directory.path().join("history"));
    let recorder = TaskRecorder::new(
        store.clone(),
        TaskSummary::new("task".into(), "services".into(), "management".into()),
    )
    .unwrap();
    recorder.event(&OperationEvent::Question {
        id: "password".into(),
        prompt: "secret prompt".into(),
        choices: Vec::new(),
        secret: true,
    });
    assert!(store.output(recorder.id(), 0, 1024).unwrap().0.is_empty());
    assert_eq!(
        store.read(recorder.id()).unwrap().status,
        TaskStatus::WaitingInput
    );
}

#[test]
fn status_filter_and_page_marker_do_not_repeat_records() {
    let directory = tempfile::tempdir().unwrap();
    let store = HistoryStore::new(directory.path().join("history"));
    for n in 0..7 {
        let mut summary = TaskSummary::new(format!("task {n}"), "script".into(), "editor".into());
        summary.started_at = n;
        summary.status = TaskStatus::Succeeded;
        store.save(&summary).unwrap();
    }
    let first = store
        .list(&HistoryQuery {
            limit: 3,
            status: "finished".into(),
            ..Default::default()
        })
        .unwrap();
    let second = store
        .list(&HistoryQuery {
            limit: 3,
            before: first.next,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(first.records.len(), 3);
    assert_eq!(second.records.len(), 3);
    assert!(
        first
            .records
            .iter()
            .all(|a| second.records.iter().all(|b| a.id != b.id))
    );
}

#[test]
fn launching_an_external_timer_is_only_a_submission_result() {
    let directory = tempfile::tempdir().unwrap();
    let store = HistoryStore::new(directory.path().join("history"));
    let summary = TaskSummary::new(
        "Run timer".into(),
        "scheduled-tasks".into(),
        "scheduled-tasks".into(),
    );
    let recorder = TaskRecorder::new(store.clone(), summary).unwrap();
    recorder.event(&crate::management::OperationEvent::Started {
        kind: crate::management::ManagementKind::ScheduledTasks,
        action: "run_now".into(),
        target: Some("test.timer".into()),
    });
    recorder.event(&crate::management::OperationEvent::Completed {
        message: "Service start requested".into(),
    });
    assert_eq!(
        store.read(recorder.id()).unwrap().status,
        TaskStatus::Submitted
    );
}

#[test]
fn output_pages_keep_complete_chinese_characters_and_record_output_failures() {
    let directory = tempfile::tempdir().unwrap();
    let store = HistoryStore::new(directory.path().join("history"));
    let summary = TaskSummary::new("中文任务".into(), "script".into(), "script".into());
    let recorder = TaskRecorder::new(store.clone(), summary).unwrap();
    recorder.output("中文输出测试".as_bytes());
    let first = store.output_text(recorder.id(), 0, 7).unwrap();
    let second = store.output_text(recorder.id(), first.end, 7).unwrap();
    let third = store.output_text(recorder.id(), second.end, 7).unwrap();
    assert_eq!(
        format!("{}{}{}", first.text, second.text, third.text),
        "中文输出测试"
    );
    assert_eq!(third.end, third.total);
    std::fs::remove_file(store.path(recorder.id(), "output").unwrap()).unwrap();
    std::fs::create_dir(store.path(recorder.id(), "output").unwrap()).unwrap();
    recorder.output(b"cannot write");
    assert!(!store.read(recorder.id()).unwrap().output_complete);
    assert!(!store.list(&HistoryQuery::default()).unwrap().records[0].output_complete);
}

#[test]
fn task_page_merges_correlated_archives_and_exposes_source_usage() {
    use crate::management::task_history::{
        append_store, cleanup_message, empty_snapshot, finish_snapshot,
    };
    use crate::management::{ManagementKind, ManagementQuery};
    let directory = tempfile::tempdir().unwrap();
    let local = HistoryStore::new(directory.path().join("local"));
    let system = HistoryStore::new(directory.path().join("system"));
    let mut summary = TaskSummary::new("重启服务".into(), "services".into(), "services".into());
    summary.status = TaskStatus::Succeeded;
    local.save(&summary).unwrap();
    system.save(&summary).unwrap();
    let query = ManagementQuery::new(ManagementKind::Tasks);
    let mut snapshot = empty_snapshot(&query);
    append_store(&mut snapshot, &query, &local, "local", false).unwrap();
    append_store(&mut snapshot, &query, &system, "system", true).unwrap();
    finish_snapshot(&mut snapshot);
    assert_eq!(snapshot.rows.len(), 1);
    assert_eq!(snapshot.rows[0].identity["bucket"], "system");
    assert_eq!(snapshot.task_history_usage.len(), 2);
    assert!(!snapshot.notices.is_empty());
    let preview = system.preview_cleanup(&[summary.id.clone()]).unwrap();
    let message = cleanup_message(&preview);
    assert!(message.contains("重启服务"));
    assert!(message.contains(&summary.id));
    assert!(
        !snapshot
            .actions
            .iter()
            .find(|action| action.id == "task_filter")
            .unwrap()
            .fields[0]
            .choices
            .contains(&"all".to_owned())
    );
}
