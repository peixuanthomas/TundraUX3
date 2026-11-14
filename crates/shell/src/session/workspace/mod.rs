use super::window_state::{WindowPrefix, WindowSelection, WindowShortcut};
use super::*;
use crate::process_host::{WindowConnection, WorkerConnection};
use platform::management::ManagementKind;
use ratatui::Frame;
use ui::{
    WorkspaceAction as Action, WorkspaceCloseWait, WorkspaceLayout, WorkspacePanePlaceholder,
    WorkspacePanel, WorkspacePanelViewModel, WorkspaceSplitMode, WorkspaceWindowViewModel,
};
use workspace_protocol::{
    AppToShell, CursorShape, LayoutGeneration, ProcessState, SessionId, ShellAction as HostAction,
    ShellToApp, Size, SupervisorToWorker, WindowFrame, WindowInit, WindowStatus, WorkerId,
    WorkerRole, WorkerToSupervisor,
};

mod input;
mod render;
mod windows;

const CLOSE_WAIT: Duration = Duration::from_secs(2);
const APP_POLL: Duration = Duration::from_millis(16);

struct Window {
    launch: WindowLaunch,
    worker: Option<WorkerId>,
    stream: Option<WindowConnection>,
    init: Option<WindowInit>,
    frame: Option<WindowFrame>,
    image_assets: BTreeMap<u64, Arc<[u8]>>,
    status: WindowStatus,
    state: ProcessState,
    queued: VecDeque<ShellToApp>,
    viewport: Option<ShellToApp>,
    close: CloseProgress,
    force_when_started: bool,
    view_requested: Option<ShellToApp>,
    launch_error: bool,
}

impl Window {
    fn finish_cleanup(&mut self) {
        self.close = CloseProgress::Exiting;
    }

    fn update_status(&mut self, mut status: WindowStatus) {
        if matches!(
            self.close,
            CloseProgress::Cleaning(_) | CloseProgress::Waiting | CloseProgress::Exiting
        ) {
            // Reaping the entire process tree determines when its minimum is released.
            status.minimum = self.status.minimum;
        }
        self.status = status;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum CloseProgress {
    #[default]
    Idle,
    Requested(Instant),
    Confirmation,
    Cleaning(Instant),
    Waiting,
    Exiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChromeAction {
    Windows,
    Back,
    Help,
    Tasks,
    Keyboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerTarget {
    Pane(SessionId),
    PaneTitle(SessionId),
    Overlay(Action),
    Chrome(ChromeAction),
}

#[derive(Debug, Clone, Copy)]
struct PointerCapture {
    target: PointerTarget,
    generation: LayoutGeneration,
    button: PointerButton,
}

pub(super) struct Workspace {
    connection: &'static WorkerConnection,
    image_cell_size: Option<[u16; 2]>,
    text_sizing: bool,
    windows: BTreeMap<SessionId, Window>,
    selection: WindowSelection,
    mode: WorkspaceSplitMode,
    generation: LayoutGeneration,
    layout: WorkspaceLayout,
    width: u16,
    height: u16,
    input_focus: Option<SessionId>,
    terminal_focused: bool,
    last_pointer: Option<CellPosition>,
    capture: Option<PointerCapture>,
    prefix: WindowPrefix,
    panel: Option<WorkspacePanelViewModel>,
    panel_hits: Vec<(Action, Rect)>,
    panel_scroll_rows: usize,
    panel_max_scroll: usize,
    wait_for: Option<SessionId>,
    modal_focus: Option<Action>,
    panel_key_focus: bool,
    chrome_layout: Option<ui::ShellFrameLayout>,
    toast_area: Option<Rect>,
    pending_global: Option<ShellCommand>,
    pending_update: Option<PathBuf>,
    input_serial: u64,
    input_recipients: HashSet<SessionId>,
    input_fences: BTreeMap<SessionId, u64>,
    task_catalogs: BTreeMap<SessionId, Vec<String>>,
    tasks_return: Option<(SessionId, SessionId)>,
    toggle_tasks_requested: bool,
    shutdown: bool,
    reset: bool,
    frozen: bool,
}

impl Workspace {
    pub(super) fn new(
        connection: &'static WorkerConnection,
        image_cell_size: Option<[u16; 2]>,
        text_sizing: bool,
    ) -> Self {
        Self {
            connection,
            image_cell_size,
            text_sizing,
            windows: BTreeMap::new(),
            selection: Default::default(),
            mode: WorkspaceSplitMode::default(),
            generation: LayoutGeneration(0),
            layout: WorkspaceLayout::new(
                Rect::default(),
                Rect::default(),
                &[],
                &[],
                None,
                WorkspaceSplitMode::default(),
            ),
            width: 0,
            height: 0,
            input_focus: None,
            terminal_focused: true,
            last_pointer: None,
            capture: None,
            prefix: WindowPrefix::default(),
            panel: None,
            panel_hits: Vec::new(),
            panel_scroll_rows: 0,
            panel_max_scroll: 0,
            wait_for: None,
            modal_focus: None,
            panel_key_focus: true,
            chrome_layout: None,
            toast_area: None,
            pending_global: None,
            pending_update: None,
            input_serial: 0,
            input_recipients: HashSet::new(),
            input_fences: BTreeMap::new(),
            task_catalogs: BTreeMap::new(),
            tasks_return: None,
            toggle_tasks_requested: false,
            shutdown: false,
            reset: false,
            frozen: false,
        }
    }

    pub(super) fn active(&self) -> bool {
        !self.windows.is_empty()
    }

    pub(super) fn text_sizing(&self) -> bool {
        self.text_sizing
    }

    pub(super) fn shutdown_requested(&self) -> bool {
        self.shutdown
    }

    pub(super) fn reset_requested(&self) -> bool {
        self.reset && !self.active()
    }

    pub(super) fn take_update(&mut self) -> Option<PathBuf> {
        (!self.active())
            .then(|| self.pending_update.take())
            .flatten()
    }

    pub(super) fn poll_timeout(&self) -> Duration {
        if self.active() {
            APP_POLL
        } else {
            Duration::from_millis(250)
        }
    }

    pub(super) fn hides_home(&self) -> bool {
        self.layout.blocked || !self.layout.panes.is_empty()
    }

    pub(super) fn blocked(&self) -> bool {
        self.layout.blocked
    }

    pub(super) fn remember_chrome(
        &mut self,
        layout: ui::ShellFrameLayout,
        toast_area: Option<Rect>,
    ) {
        self.chrome_layout = Some(layout);
        self.toast_area = toast_area;
    }

    pub(super) fn invalidate_layout(&mut self) {
        self.cancel_capture();
        self.chrome_layout = None;
        self.toast_area = None;
        self.input_focus = None;
        self.layout.panes.clear();
        self.generation.0 += 1;
    }

    pub(super) fn has_native_input(&self) -> bool {
        self.input_focus
            .and_then(|id| self.windows.get(&id))
            .and_then(|window| window.frame.as_ref())
            .is_some_and(|frame| {
                frame.layout == self.generation && !frame.modal && !frame.native_escape.is_empty()
            })
    }

    pub(super) fn finish_input_batch(&mut self, external: bool) {
        if !external {
            self.input_recipients.clear();
            return;
        }
        self.input_serial += 1;
        for id in std::mem::take(&mut self.input_recipients) {
            if self.enqueue(
                id,
                ShellToApp::InputFence {
                    serial: self.input_serial,
                },
            ) {
                self.input_fences.insert(id, self.input_serial);
            }
        }
    }

    pub(super) fn external_snapshot_ready(&self) -> bool {
        self.input_fences.is_empty()
            && self.layout.panes.iter().all(|pane| {
                let window = &self.windows[&pane.id];
                window
                    .frame
                    .as_ref()
                    .is_some_and(|frame| frame.layout == self.generation)
                    || window.state == ProcessState::Unresponsive
                    || matches!(window.state, ProcessState::ForceStopping)
                    || matches!(
                        window.close,
                        CloseProgress::Cleaning(_)
                            | CloseProgress::Waiting
                            | CloseProgress::Exiting
                    )
                    || window.status.unresponsive
            })
    }

    pub(super) fn focused_page(&self) -> Option<&str> {
        if self.layout.blocked {
            return Some("terminal_size");
        }
        if self.layout.panes.is_empty() {
            return None;
        }
        let window = self.windows.get(&self.selection.focused()?)?;
        window.frame.as_ref().map(|frame| frame.page.as_str())
    }

    pub(super) fn focused_title(&self) -> Option<String> {
        if self.layout.panes.is_empty() {
            return None;
        }
        self.windows
            .get(&self.selection.focused()?)
            .map(|window| window.launch.title())
    }

    pub(super) fn focused_status(&self) -> Option<&str> {
        if self.layout.blocked || self.layout.panes.is_empty() {
            return None;
        }
        self.windows
            .get(&self.selection.focused()?)?
            .frame
            .as_ref()
            .map(|frame| frame.status.as_str())
            .filter(|status| !status.is_empty())
    }

    pub(super) fn keyboard_available(&self) -> bool {
        !self.layout.blocked
            && !self.layout.panes.is_empty()
            && self
                .selection
                .focused()
                .and_then(|id| self.windows[&id].frame.as_ref())
                .is_some_and(|frame| frame.size.width > 0 && frame.keyboard_available)
    }

    pub(super) fn busy(&self) -> bool {
        self.windows.values().any(|window| window.status.busy)
    }

    pub(super) fn tasks_label(&self) -> Option<String> {
        if self
            .windows
            .values()
            .any(|window| window.status.awaiting_approval)
        {
            Some(i18n::tr!("ui-shell-tasks-waiting"))
        } else if self
            .windows
            .values()
            .any(|window| window.status.running_operations)
        {
            Some(i18n::tr!("ui-shell-tasks-active"))
        } else {
            None
        }
    }

    pub(super) fn animating(&self) -> bool {
        self.layout.panes.iter().any(|pane| {
            self.windows[&pane.id]
                .frame
                .as_ref()
                .is_some_and(|frame| frame.animation_running)
        })
    }

    pub(super) fn cursor_shape(&self) -> Option<CursorShape> {
        if self.panel.is_some() || self.wait_for.is_some() {
            return None;
        }
        let window = self.windows.get(&self.input_focus?)?;
        if matches!(window.state, ProcessState::Unresponsive)
            || window.status.unresponsive
            || matches!(
                window.close,
                CloseProgress::Cleaning(_) | CloseProgress::Waiting | CloseProgress::Exiting
            )
        {
            return None;
        }
        let frame = window.frame.as_ref()?;
        (frame.layout == self.generation).then_some(frame.cursor?.shape)
    }

    pub(super) fn relayout(&mut self, bounds: Rect, available: Rect, modal: bool) -> bool {
        let minimums = self
            .windows
            .iter()
            .map(|(&id, window)| (id, window.status.minimum))
            .collect::<Vec<_>>();
        let capacity = WorkspaceLayout::capacity(bounds, available, &minimums, self.mode);
        let focused = self.selection.focused();
        let visible = self.selection.visible(capacity);
        let layout =
            WorkspaceLayout::new(bounds, available, &minimums, &visible, focused, self.mode);
        let input_focus = focused.filter(|id| {
            !modal
                && self.terminal_focused
                && self.panel.is_none()
                && self.wait_for.is_none()
                && layout.panes.iter().any(|pane| pane.id == *id)
        });
        let frozen = layout.blocked || self.frozen;
        let changed = self.layout != layout
            || self.input_focus != input_focus
            || self.width != bounds.width
            || self.height != bounds.height;
        self.width = bounds.width;
        self.height = bounds.height;
        if changed {
            self.cancel_capture();
            self.generation.0 += 1;
            self.layout = layout;
            self.input_focus = input_focus;
        }
        let mut updated = false;
        for (&id, window) in &mut self.windows {
            let pane = self.layout.panes.iter().find(|pane| pane.id == id);
            let size = (!frozen).then(|| {
                pane.map_or(window.status.minimum, |pane| {
                    Size::new(pane.content.width, pane.content.height)
                })
            });
            let message = ShellToApp::Viewport {
                layout: self.generation,
                size,
                visible: pane.is_some(),
                focused: self.input_focus == Some(id),
            };
            if window.viewport.as_ref() != Some(&message) {
                window.viewport = Some(message.clone());
                if window.init.is_none() {
                    window.view_requested = Some(message);
                }
                updated = true;
            }
        }
        changed || updated
    }

    pub(super) fn set_suspend(&mut self, suspended: bool) {
        self.frozen = suspended;
        if let Some(layout) = self.chrome_layout {
            self.relayout(layout.bounds, layout.main, suspended);
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/session/workspace_close.rs"]
mod close_tests;
