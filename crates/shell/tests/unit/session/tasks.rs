use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn only_operations_are_saved_and_cancellation_waits_for_worker_completion() {
    let root =
        platform::create_temp_dir(&std::env::temp_dir().join("TundraUX3"), "task-history-test")
            .unwrap();
    let mut tasks = ShellTasks::with_history(Some(root.clone()));
    let page = PageInstanceId::new();
    let finished = Arc::new(AtomicBool::new(false));
    tasks.register(
        TaskDomain::ManagementQuery,
        1,
        TaskOwner::Page(page),
        TaskClass::Query,
        TaskControl::new(|| {}, || true),
    );
    let done = finished.clone();
    tasks.register(
        TaskDomain::Explorer,
        2,
        TaskOwner::Page(page),
        TaskClass::Operation,
        TaskControl::new(|| {}, move || done.load(Ordering::Acquire)),
    );
    let history = tasks.recorder(TaskDomain::Explorer, 2).unwrap();
    let store = HistoryStore::new(&root);
    assert_eq!(
        store
            .list(&platform::task_history::HistoryQuery::default())
            .unwrap()
            .records
            .len(),
        1
    );
    tasks.cancel(TaskDomain::Explorer, 2);
    assert_eq!(
        store.read(history.id()).unwrap().status,
        TaskStatus::Cancelling
    );
    assert!(tasks.page_settling(page));
    finished.store(true, Ordering::Release);
    tasks.complete(TaskDomain::Explorer, 2);
    assert_eq!(
        store.read(history.id()).unwrap().status,
        TaskStatus::Cancelled
    );
    assert_eq!(tasks.history_task(history.id()).unwrap().2, Some(page));
    platform::cleanup_temp_path(&root).unwrap();
}

#[test]
fn unbound_sessions_never_create_history_or_mark_unknown_completion_successful() {
    let mut tasks = ShellTasks::default();
    tasks.register(
        TaskDomain::Script,
        1,
        TaskOwner::Session,
        TaskClass::Operation,
        TaskControl::new(|| {}, || true),
    );
    assert!(tasks.recorder(TaskDomain::Script, 1).is_none());
    assert!(
        tasks
            .new_recorder("script", "test".into(), "script")
            .is_none()
    );
    tasks.complete(TaskDomain::Script, 1);
    assert!(!tasks.has_work());
}

#[test]
fn closing_invalidates_results_but_retains_cleanup_until_the_worker_stops() {
    let mut tasks = ShellTasks::default();
    let page = PageInstanceId::new();
    let stopped = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    let completion = stopped.clone();
    let generation = tasks.register(
        TaskDomain::Launcher,
        7,
        TaskOwner::Page(page),
        TaskClass::Query,
        TaskControl::new(
            move || {
                signal.store(true, Ordering::Release);
            },
            move || completion.load(Ordering::Acquire),
        ),
    );
    assert!(tasks.generation_matches(TaskDomain::Launcher, 7, generation));
    tasks.close_queries(page);
    assert!(cancelled.load(Ordering::Acquire));
    assert_eq!(tasks.owner(TaskDomain::Launcher, 7), None);
    tasks.complete(TaskDomain::Launcher, 7);
    assert!(tasks.has_work());
    stopped.store(true, Ordering::Release);
    tasks.reap();
    assert!(!tasks.has_work());
}

#[test]
fn session_reset_rejects_old_completion_and_generations_never_match_reused_requests() {
    let mut tasks = ShellTasks::default();
    let page = PageInstanceId::new();
    let old = tasks.register(
        TaskDomain::Editor,
        4,
        TaskOwner::Page(page),
        TaskClass::Operation,
        TaskControl::new(|| {}, || true),
    );
    tasks.transfer_operations(page);
    assert_eq!(tasks.owner(TaskDomain::Editor, 4), Some(TaskOwner::Session));
    tasks.reset();
    assert_eq!(tasks.owner(TaskDomain::Editor, 4), None);
    tasks.reap();
    let new = tasks.register(
        TaskDomain::Editor,
        4,
        TaskOwner::Page(PageInstanceId::new()),
        TaskClass::Operation,
        TaskControl::new(|| {}, || true),
    );
    assert_ne!(old, new);
    assert!(!tasks.generation_matches(TaskDomain::Editor, 4, old));
    tasks.complete(TaskDomain::Editor, 4);
    assert_eq!(tasks.owner(TaskDomain::Editor, 4), None);
    tasks.complete(TaskDomain::Editor, 4);
    assert!(!tasks.has_work());
}
