use crate::{LanguageSnapshot, LocalizedMessage};
use std::{
    cell::RefCell,
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, OnceLock},
};

thread_local! {
    static ACTIVE: RefCell<Vec<(u64, Arc<LanguageSnapshot>)>> = const { RefCell::new(Vec::new()) };
    static NEXT_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Thread-bound scope token. Dropping restores the surrounding scope, including during unwinding.
/// Out-of-order drops remove only their own frame and never reinstate an expired scope.
#[must_use = "retain this guard for the duration of the localization scope"]
pub struct SnapshotGuard {
    id: u64,
    _thread_bound: PhantomData<Rc<()>>,
}

pub fn enter_snapshot(snapshot: Arc<LanguageSnapshot>) -> SnapshotGuard {
    let id = NEXT_ID.with(|next| {
        let id = next.get();
        next.set(id.wrapping_add(1));
        id
    });
    ACTIVE.with(|active| active.borrow_mut().push((id, snapshot)));
    SnapshotGuard {
        id,
        _thread_bound: PhantomData,
    }
}
impl Drop for SnapshotGuard {
    fn drop(&mut self) {
        let _ = ACTIVE.try_with(|active| active.borrow_mut().retain(|(id, _)| *id != self.id));
    }
}

pub fn with_snapshot<R>(snapshot: &Arc<LanguageSnapshot>, f: impl FnOnce() -> R) -> R {
    let _guard = enter_snapshot(Arc::clone(snapshot));
    f()
}

pub fn render_current(message: &LocalizedMessage) -> String {
    static DEFAULT: OnceLock<Arc<LanguageSnapshot>> = OnceLock::new();
    let snapshot = ACTIVE
        .with(|active| {
            active
                .borrow()
                .last()
                .map(|(_, snapshot)| Arc::clone(snapshot))
        })
        .unwrap_or_else(|| {
            Arc::clone(DEFAULT.get_or_init(|| Arc::new(LanguageSnapshot::embedded(0))))
        });
    snapshot.render(message)
}
