//! Instance-owned application state. Selecting an instance never copies its data.
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};

/// Identifies one opening of a page, including while it is covered by another page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PageInstanceId(u64);

impl PageInstanceId {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT.try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
                .expect("page instance identifiers exhausted"),
        )
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl Default for PageInstanceId {
    fn default() -> Self {
        Self::new()
    }
}

/// Stores every live instance of one page type. The selected instance is a
/// controller context, not a claim that the page is visible. Background delivery
/// temporarily selects its owner without changing navigation or another model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageInstances<T> {
    instances: BTreeMap<PageInstanceId, T>,
    selected: Option<PageInstanceId>,
    empty: T,
}

impl<T: Default> Default for PageInstances<T> {
    fn default() -> Self {
        Self {
            instances: BTreeMap::new(),
            selected: None,
            empty: T::default(),
        }
    }
}

impl<T> PageInstances<T> {
    pub fn selected_id(&self) -> Option<PageInstanceId> {
        self.selected
    }
    pub fn insert(&mut self, id: PageInstanceId, value: T) {
        assert!(
            !self.instances.contains_key(&id),
            "page instance already exists"
        );
        self.instances.insert(id, value);
    }
    pub fn select(&mut self, id: Option<PageInstanceId>) {
        assert!(
            id.is_none_or(|id| self.instances.contains_key(&id)),
            "unknown page instance"
        );
        self.selected = id;
    }
    pub fn get(&self, id: PageInstanceId) -> Option<&T> {
        self.instances.get(&id)
    }
    pub fn get_mut(&mut self, id: PageInstanceId) -> Option<&mut T> {
        self.instances.get_mut(&id)
    }
    pub fn remove(&mut self, id: PageInstanceId) -> Option<T> {
        if self.selected == Some(id) {
            self.selected = None;
        }
        self.instances.remove(&id)
    }
    pub fn iter(&self) -> impl Iterator<Item = (PageInstanceId, &T)> {
        self.instances.iter().map(|(&id, state)| (id, state))
    }
    pub fn ids(&self) -> Vec<PageInstanceId> {
        self.instances.keys().copied().collect()
    }
}

impl<T: Default> PageInstances<T> {
    pub fn create(&mut self, id: PageInstanceId) {
        self.insert(id, T::default());
    }
    pub fn clear(&mut self) {
        self.instances.clear();
        self.selected = None;
        self.empty = T::default();
    }
}

impl<T> Deref for PageInstances<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.selected
            .and_then(|id| self.instances.get(&id))
            .unwrap_or(&self.empty)
    }
}

impl<T> DerefMut for PageInstances<T> {
    fn deref_mut(&mut self) -> &mut T {
        match self.selected {
            Some(id) => self.instances.get_mut(&id).expect("selected page instance"),
            None => &mut self.empty,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPageKind {
    Explorer,
    Launcher,
    Editor,
    Diagnostics,
    UserManagement,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct EditorPageModel {
    pub state: Option<crate::editor::EditorState>,
    pub effects: std::collections::VecDeque<crate::editor::EditorEffect>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct AppPageModels {
    pub managed_users: PageInstances<Vec<identity::UserAccount>>,
    pub explorer: PageInstances<Option<crate::explorer::ExplorerState>>,
    pub launcher: PageInstances<Option<crate::launcher::LauncherState>>,
    pub editor: PageInstances<EditorPageModel>,
    pub diagnostics: PageInstances<Option<crate::diagnostics::DiagnosticsSnapshot>>,
}

impl AppPageModels {
    pub fn register(&mut self, id: PageInstanceId, kind: AppPageKind) {
        match kind {
            AppPageKind::UserManagement => self.managed_users.create(id),
            AppPageKind::Explorer => self.explorer.create(id),
            AppPageKind::Launcher => self.launcher.create(id),
            AppPageKind::Editor => self.editor.create(id),
            AppPageKind::Diagnostics => self.diagnostics.create(id),
        }
        self.select(Some(id), kind);
    }
    pub fn select(&mut self, id: Option<PageInstanceId>, kind: AppPageKind) {
        match kind {
            AppPageKind::UserManagement => self.managed_users.select(id),
            AppPageKind::Explorer => self.explorer.select(id),
            AppPageKind::Launcher => self.launcher.select(id),
            AppPageKind::Editor => self.editor.select(id),
            AppPageKind::Diagnostics => self.diagnostics.select(id),
        }
    }
    pub fn remove(&mut self, id: PageInstanceId) {
        self.managed_users.remove(id);
        self.explorer.remove(id);
        self.launcher.remove(id);
        self.editor.remove(id);
        self.diagnostics.remove(id);
    }
}
