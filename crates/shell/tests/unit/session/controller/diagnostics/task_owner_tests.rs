use super::*;
use app::diagnostics::{DiagnosticsSnapshot, DiagnosticsTaskEnvelope, DiagnosticsTaskEvent};
use std::sync::atomic::{AtomicBool, Ordering};

fn state() -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 30),
        ShellHomeMode::User,
    );
    state.enter_screen(ShellScreen::Diagnostics);
    state
}
fn snapshot(label: &str) -> DiagnosticsSnapshot {
    DiagnosticsSnapshot {
        scanned_at: Utc::now(),
        checks: Vec::new(),
        logs: Vec::new(),
        incidents: Vec::new(),
        warnings: vec![label.into()],
    }
}
fn register(state: &mut ShellSession, request: u64, class: TaskClass) -> Arc<AtomicBool> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel = cancelled.clone();
    state.register_page_task(
        TaskDomain::Diagnostics,
        request,
        class,
        TaskControl::new(
            move || {
                cancel.store(true, Ordering::Release);
            },
            || true,
        ),
    );
    cancelled
}
fn deliver_scan(state: &mut ShellSession, request_id: u64, label: &str) {
    state.deliver_diagnostics_event(DiagnosticsTaskEnvelope {
        request_id,
        event: DiagnosticsTaskEvent::ScanCompleted(Ok(snapshot(label))),
    });
}

#[test]
fn scan_result_updates_its_owner_without_switching_visible_page() {
    let mut state = state();
    let first = state.active_page_id();
    register(&mut state, 101, TaskClass::Query);
    state.enter_new_page(ShellScreen::Diagnostics);
    let second = state.active_page_id();
    register(&mut state, 102, TaskClass::Query);
    deliver_scan(&mut state, 101, "first");
    assert_eq!(state.active_page_id(), second);
    assert!(state.app.diagnostics_snapshot().is_none());
    assert_eq!(
        state.with_page_context(first, |state| state
            .app
            .diagnostics_snapshot()
            .unwrap()
            .warnings
            .clone()),
        Some(vec!["first".into()])
    );
    assert!(state.tasks.owner(TaskDomain::Diagnostics, 101).is_none());
    assert_eq!(
        state.tasks.owner(TaskDomain::Diagnostics, 102),
        Some(TaskOwner::Page(second))
    );
    deliver_scan(&mut state, 102, "second");
    assert_eq!(
        state.app.diagnostics_snapshot().unwrap().warnings,
        ["second"]
    );
}

#[test]
fn closing_query_cancels_it_and_discards_late_result_after_reopen() {
    let mut state = state();
    let first = state.active_page_id();
    let cancelled = register(&mut state, 201, TaskClass::Query);
    state.close_page_instance(first);
    assert!(cancelled.load(Ordering::Acquire));
    state.enter_new_page(ShellScreen::Diagnostics);
    let second = state.active_page_id();
    deliver_scan(&mut state, 201, "closed page result");
    assert_eq!(state.active_page_id(), second);
    assert!(state.app.diagnostics_snapshot().is_none());
    assert!(state.pages.diagnostics.diagnostics_feedback.is_none());
    assert!(!state.tasks.has_work());
}

#[test]
fn detached_repair_keeps_restart_requirement_without_retaining_page_or_snapshot() {
    let mut state = state();
    let page = state.active_page_id();
    register(&mut state, 301, TaskClass::Operation);
    state.pages.diagnostics.diagnostics_repair_log_context =
        Some(runtime_log::LogContext::default());
    state.tasks.transfer_operations(page);
    state.detach_diagnostics_page(page);
    assert!(
        state
            .pages
            .diagnostics
            .diagnostics_repair_log_context
            .is_none()
    );
    state.close_page_instance(page);
    assert!(state.pages.diagnostics.get(page).is_none());
    state.enter_new_page(ShellScreen::Diagnostics);
    let reopened = state.active_page_id();
    state.deliver_diagnostics_event(DiagnosticsTaskEnvelope {
        request_id: 301,
        event: DiagnosticsTaskEvent::RepairCompleted {
            results: Vec::new(),
            snapshot: Some(snapshot("closed repair")),
            restart_required: true,
            cancelled: true,
        },
    });
    assert!(state.diagnostics_restart_required);
    assert_eq!(state.active_page_id(), reopened);
    assert!(state.app.diagnostics_snapshot().is_none());
    assert!(!state.tasks.has_work());
}

#[test]
fn cancelled_page_repair_terminal_event_clears_busy_state_and_registry() {
    let mut state = state();
    let page = state.active_page_id();
    register(&mut state, 401, TaskClass::Operation);
    state.pages.diagnostics.diagnostics_scanning = true;
    state.pages.diagnostics.diagnostics_rescan_pending = true;
    state.tasks.cancel_page(page);
    state.deliver_diagnostics_event(DiagnosticsTaskEnvelope {
        request_id: 401,
        event: DiagnosticsTaskEvent::RepairCompleted {
            results: Vec::new(),
            snapshot: None,
            restart_required: false,
            cancelled: true,
        },
    });
    assert!(!state.pages.diagnostics.diagnostics_scanning);
    assert!(!state.pages.diagnostics.diagnostics_rescan_pending);
    assert!(!state.tasks.page_operations(page));
}
