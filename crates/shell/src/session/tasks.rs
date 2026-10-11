//! Page ownership is checked here before a worker result reaches any page state.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::session) enum TaskDomain {
    Launcher,
    Editor,
    Explorer,
    Diagnostics,
    SettingsTimeSync,
    Update,
    Logs,
    ManagementQuery,
    ManagementDraft,
    ManagementOperation,
    UserManagement,
    AutoAdmin,
    CommandLine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::session) enum TaskOwner {
    Page(PageInstanceId),
    Session,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::session) enum TaskClass {
    Query,
    Operation,
}

/// Keeps cancellation and worker lifetime separate from page/UI lifetime.
#[derive(Clone)]
pub(in crate::session) struct TaskControl {
    cancel: Arc<dyn Fn() + Send + Sync>,
    finished: Arc<dyn Fn() -> bool + Send + Sync>,
}
impl TaskControl {
    pub(in crate::session) fn new(
        cancel: impl Fn() + Send + Sync + 'static,
        finished: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            cancel: Arc::new(cancel),
            finished: Arc::new(finished),
        }
    }
}
impl fmt::Debug for TaskControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TaskControl")
    }
}
impl PartialEq for TaskControl {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.cancel, &other.cancel)
    }
}
impl Eq for TaskControl {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::session) struct PageTask {
    pub generation: PageInstanceId,
    pub owner: TaskOwner,
    pub class: TaskClass,
    epoch: PageInstanceId,
    pub cancelling: bool,
    accepts_result: bool,
    completed: bool,
    control: TaskControl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::session) struct ShellTasks {
    epoch: PageInstanceId,
    records: BTreeMap<(TaskDomain, u64), PageTask>,
}
impl Default for ShellTasks {
    fn default() -> Self {
        Self {
            epoch: PageInstanceId::new(),
            records: BTreeMap::new(),
        }
    }
}
impl ShellTasks {
    pub(in crate::session) fn register(
        &mut self,
        domain: TaskDomain,
        request: u64,
        owner: TaskOwner,
        class: TaskClass,
        control: TaskControl,
    ) -> PageInstanceId {
        // Native request ids must stay unique until their completion is consumed.
        assert!(
            !self.records.contains_key(&(domain, request)),
            "task request id reused"
        );
        let generation = PageInstanceId::new();
        self.records.insert(
            (domain, request),
            PageTask {
                generation,
                owner,
                class,
                epoch: self.epoch,
                cancelling: false,
                accepts_result: true,
                completed: false,
                control,
            },
        );
        generation
    }

    pub(in crate::session) fn owner(&self, domain: TaskDomain, request: u64) -> Option<TaskOwner> {
        self.records
            .get(&(domain, request))
            .filter(|task| task.epoch == self.epoch && task.accepts_result && !task.completed)
            .map(|task| task.owner)
    }

    #[cfg(test)]
    pub(in crate::session) fn generation_matches(
        &self,
        domain: TaskDomain,
        request: u64,
        generation: PageInstanceId,
    ) -> bool {
        self.owner(domain, request).is_some()
            && self
                .records
                .get(&(domain, request))
                .is_some_and(|task| task.generation == generation)
    }

    pub(in crate::session) fn complete(&mut self, domain: TaskDomain, request: u64) {
        if let Some(task) = self.records.get_mut(&(domain, request)) {
            task.completed = true;
        }
        self.reap();
    }

    pub(in crate::session) fn cancel(&mut self, domain: TaskDomain, request: u64) {
        if let Some(task) = self.records.get_mut(&(domain, request)) {
            task.cancelling = true;
            (task.control.cancel)();
        }
    }

    pub(in crate::session) fn close_queries(&mut self, page: PageInstanceId) {
        for task in self
            .records
            .values_mut()
            .filter(|task| task.owner == TaskOwner::Page(page) && task.class == TaskClass::Query)
        {
            task.accepts_result = false;
            task.cancelling = true;
            (task.control.cancel)();
        }
    }

    pub(in crate::session) fn cancel_page(&mut self, page: PageInstanceId) {
        for task in self
            .records
            .values_mut()
            .filter(|task| task.owner == TaskOwner::Page(page))
        {
            task.cancelling = true;
            (task.control.cancel)();
        }
    }

    pub(in crate::session) fn transfer_operations(&mut self, page: PageInstanceId) {
        for task in self.records.values_mut().filter(|task| {
            task.owner == TaskOwner::Page(page) && task.class == TaskClass::Operation
        }) {
            task.owner = TaskOwner::Session;
        }
    }

    pub(in crate::session) fn page_operations(&self, page: PageInstanceId) -> bool {
        self.records.values().any(|task| {
            task.epoch == self.epoch
                && task.owner == TaskOwner::Page(page)
                && task.class == TaskClass::Operation
                && !task.completed
        })
    }

    pub(in crate::session) fn page_settling(&self, page: PageInstanceId) -> bool {
        self.records.values().any(|task| {
            task.owner == TaskOwner::Page(page)
                && task.class == TaskClass::Operation
                && !(task.control.finished)()
        })
    }

    pub(in crate::session) fn page_running(&self, page: PageInstanceId) -> bool {
        self.records
            .values()
            .any(|task| task.owner == TaskOwner::Page(page) && !(task.control.finished)())
    }

    pub(in crate::session) fn reset(&mut self) {
        self.stop_session(false);
    }

    pub(in crate::session) fn stop_session(&mut self, preserve_prepared_update: bool) {
        self.epoch = PageInstanceId::new();
        // A prepared transaction handed to the replacement helper is no longer
        // a Shell task. Cancelling it here would delete the helper's input.
        if preserve_prepared_update {
            self.records
                .retain(|(domain, _), _| *domain != TaskDomain::Update);
        }
        for task in self.records.values_mut() {
            task.accepts_result = false;
            if !task.cancelling {
                task.cancelling = true;
                (task.control.cancel)();
            }
        }
    }

    pub(in crate::session) fn reap(&mut self) {
        self.records.retain(|_, task| {
            !(task.control.finished)()
                || (!task.completed && task.accepts_result && task.epoch == self.epoch)
        });
    }

    pub(in crate::session) fn has_work(&self) -> bool {
        !self.records.is_empty()
    }

    pub(in crate::session) fn has_running(&self) -> bool {
        self.records.values().any(|task| !(task.control.finished)())
    }
}

impl ShellSession {
    pub(in crate::session) fn wait_for_session_exit(&mut self, command: ShellCommand) -> bool {
        if !self.tasks.has_running() {
            return false;
        }
        self.pending_session_exit = Some(command);
        self.show_stopping_auto_admin();
        self.notify_status(i18n::msg!("page-close-waiting"));
        true
    }
    pub(in crate::session) fn stop_session_tasks(
        &mut self,
        preserve_prepared_update: bool,
    ) -> bool {
        let preserve = preserve_prepared_update && self.update_apply_manifest.is_some();
        if !preserve {
            self.cancel_update();
            self.update_apply_manifest = None;
            if self.settings_task_runtime.update_cancel_error().is_some() {
                return false;
            }
        }
        self.tasks.stop_session(preserve);
        true
    }

    pub(in crate::session) fn register_page_task(
        &mut self,
        domain: TaskDomain,
        request: u64,
        class: TaskClass,
        control: TaskControl,
    ) -> PageInstanceId {
        let owner = self
            .pages
            .common
            .selected_id()
            .expect("tasks require a page instance");
        self.tasks
            .register(domain, request, TaskOwner::Page(owner), class, control)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session/tasks.rs"]
mod tests;
