use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

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
