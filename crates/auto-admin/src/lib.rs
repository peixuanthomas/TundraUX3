//! Approval, operation input and supervised task completion for AutoAdmin.
//! Shell owns the modal, focus and navigation; this crate owns each job.
use platform::management::{ManagementError, OperationEvent, OperationInput};
use ratatui::layout::Rect;
use std::fmt;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicU8, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};
#[cfg(target_os = "linux")]
use std::{collections::VecDeque, io};
use terminal_runtime::pty::{TerminalSnapshot, to_ui_snapshot};
use ui::{InputPhase, Key as InputKey, KeyEvent as KeyInput};
use watchdog::{ManagedTaskGroup, ManagedThreadHandle, TaskId, TaskSpec};
use zeroize::Zeroizing;

#[cfg(target_os = "linux")]
mod linux;
mod stop;
mod tasks;
#[cfg(target_os = "linux")]
pub use linux::AutoAdminAuthorization;
pub use tasks::spawn_task;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum JobPhase {
    Waiting,
    Running,
    Denied,
    Finished,
}
use JobPhase::{Denied as DENIED, Finished as FINISHED, Running as RUNNING, Waiting as WAITING};
struct Question {
    id: String,
    choices: Vec<String>,
    secret: bool,
    value: Zeroizing<String>,
    editing: ui::components::TextEditState,
}

struct Display {
    parser: vt100::Parser,
    status: String,
    question: Option<Question>,
    revision: u64,
    terminal_stream: bool,
    stop: Option<stop::StopRequest>,
}

struct Shared {
    description: String,
    phase: AtomicU8,
    display: Mutex<Display>,
    approved: Condvar,
    responses: mpsc::Sender<OperationInput>,
    power_succeeded: std::sync::atomic::AtomicBool,
    worker: Mutex<Option<ManagedThreadHandle<()>>>,
    stop_requested: std::sync::atomic::AtomicBool,
    force_requested: std::sync::atomic::AtomicBool,
    helper_control: std::sync::atomic::AtomicBool,
    helper_connected: std::sync::atomic::AtomicBool,
    history: Mutex<Option<platform::task_history::TaskRecorder>>,
    history_handed_off: std::sync::atomic::AtomicBool,
    #[cfg(target_os = "linux")]
    process: Mutex<Option<platform::management::termination::ProcessTree>>,
    #[cfg(target_os = "linux")]
    terminal: Mutex<Option<linux::AuthTerminal>>,
}

#[derive(Clone)]
pub struct AutoAdminJob(Arc<Shared>);
impl fmt::Debug for AutoAdminJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AutoAdminJob")
    }
}
impl PartialEq for AutoAdminJob {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for AutoAdminJob {}

impl AutoAdminJob {
    pub fn new(
        description: String,
        policy: storage::AutoAdminPolicy,
        responses: mpsc::Sender<OperationInput>,
    ) -> Self {
        let phase = match policy {
            storage::AutoAdminPolicy::Manual => WAITING,
            storage::AutoAdminPolicy::Automatic => RUNNING,
            storage::AutoAdminPolicy::Deny => DENIED,
        };
        Self(Arc::new(Shared {
            description,
            phase: AtomicU8::new(phase as u8),
            display: Mutex::new(Display {
                parser: vt100::Parser::new(24, 100, 2000),
                status: i18n::tr!(if phase == DENIED {
                    "aa-blocked"
                } else if phase == WAITING {
                    "aa-request"
                } else {
                    "aa-running"
                }),
                question: None,
                revision: 0,
                terminal_stream: false,
                stop: None,
            }),
            approved: Condvar::new(),
            responses,
            power_succeeded: std::sync::atomic::AtomicBool::new(false),
            worker: Mutex::new(None),
            stop_requested: std::sync::atomic::AtomicBool::new(false),
            force_requested: std::sync::atomic::AtomicBool::new(false),
            helper_control: std::sync::atomic::AtomicBool::new(false),
            helper_connected: std::sync::atomic::AtomicBool::new(false),
            history: Mutex::new(None),
            history_handed_off: std::sync::atomic::AtomicBool::new(false),
            #[cfg(target_os = "linux")]
            process: Mutex::new(None),
            #[cfg(target_os = "linux")]
            terminal: Mutex::new(None),
        }))
    }
    pub fn record_history(&self, history: platform::task_history::TaskRecorder, origin: String) {
        use platform::task_history::TaskStatus;
        let _ = history.store().update(history.id(), |summary| {
            summary.name = self.history_name();
            summary.origin = origin;
            summary.status = match self.phase() {
                WAITING => TaskStatus::WaitingApproval,
                DENIED => TaskStatus::Cancelled,
                _ => TaskStatus::Running,
            };
            if self.phase() == DENIED {
                summary.finished_at = Some(platform::task_history::now());
            }
        });
        *self.0.history.lock().unwrap_or_else(|e| e.into_inner()) = Some(history);
    }

    pub fn history_id(&self) -> Option<String> {
        self.0
            .history
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|history| history.id().to_owned())
    }

    pub fn history_name(&self) -> String {
        self.0
            .description
            .lines()
            .take(2)
            .collect::<Vec<_>>()
            .join(" · ")
    }

    pub fn phase(&self) -> JobPhase {
        match self.0.phase.load(Ordering::Acquire) {
            0 => WAITING,
            1 => RUNNING,
            2 => DENIED,
            3 => FINISHED,
            _ => unreachable!("invalid AutoAdmin phase"),
        }
    }
    /// Includes structured questions and secret prompts from background work.
    pub fn needs_input(&self) -> bool {
        self.phase() == WAITING
            || self
                .0
                .display
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .question
                .is_some()
    }
    pub fn task_finished(&self) -> bool {
        !self.running()
            && self
                .0
                .worker
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .is_none_or(|worker| worker.is_finished())
    }

    pub fn running(&self) -> bool {
        matches!(self.phase(), WAITING | RUNNING)
    }
    pub fn wait_for_approval(&self) -> Result<(), ManagementError> {
        let mut display = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        while self.phase() == WAITING {
            let (next, timeout) = self
                .0
                .approved
                .wait_timeout(display, Duration::from_secs(300))
                .unwrap_or_else(|e| e.into_inner());
            display = next;
            if timeout.timed_out() && self.phase() == WAITING {
                self.0.phase.store(DENIED as u8, Ordering::Release);
                display.status = i18n::tr!("aa-expired");
                display.revision += 1;
            }
        }
        if self.accepts_input() {
            Ok(())
        } else {
            Err(ManagementError::Cancelled)
        }
    }
    pub fn decide(&self, approve: bool) {
        let mut display = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        if self.phase() != WAITING {
            return;
        }
        self.0.phase.store(
            if approve { RUNNING } else { DENIED } as u8,
            Ordering::Release,
        );
        display.status = i18n::tr!(if approve { "aa-running" } else { "aa-rejected" });
        display.revision += 1;
        self.0.approved.notify_all();
        if let Some(history) = self
            .0
            .history
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            if approve {
                history.status(platform::task_history::TaskStatus::Running);
            } else {
                history.finish(
                    platform::task_history::TaskStatus::Cancelled,
                    i18n::tr!("aa-rejected"),
                );
            }
        }
    }
    pub fn emit(&self, event: &OperationEvent) {
        if !self.0.history_handed_off.load(Ordering::Acquire)
            && let Some(history) = self
                .0
                .history
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
        {
            if (self.phase() == DENIED || self.0.stop_requested.load(Ordering::Acquire))
                && matches!(
                    event,
                    OperationEvent::Completed { .. } | OperationEvent::Failed { .. }
                )
            {
                let message = match event {
                    OperationEvent::Completed { message } | OperationEvent::Failed { message } => {
                        message.clone()
                    }
                    _ => unreachable!(),
                };
                history.finish(platform::task_history::TaskStatus::Cancelled, message);
            } else {
                history.event(event);
            }
            #[cfg(target_os = "linux")]
            if matches!(event,OperationEvent::Connected{operation_id} if !operation_id.starts_with("/run/tundraux3-management/"))
                && history
                    .store()
                    .read(history.id())
                    .is_ok_and(|record| record.process_id != std::process::id())
            {
                self.0.history_handed_off.store(true, Ordering::Release);
            }
        }
        let mut d = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        d.revision = d.revision.wrapping_add(1);
        match event {
            OperationEvent::Connected { .. } => {
                self.0.helper_connected.store(true, Ordering::Release);
            }
            OperationEvent::TerminalOutput { bytes } => {
                d.terminal_stream = true;
                d.parser.process(bytes);
            }
            OperationEvent::Output { text } => print_line(&mut d.parser, text),
            OperationEvent::TaskOutput {
                offset,
                end,
                total,
                text,
            } => {
                print_line(
                    &mut d.parser,
                    &i18n::tr!(
                        "task-output-range",
                        start = *offset,
                        end = *end,
                        total = *total
                    ),
                );
                print_line(&mut d.parser, text);
            }
            OperationEvent::Progress { message, percent } => {
                d.status = percent.map_or_else(|| message.clone(), |p| format!("{p}% {message}"));
                if !d.terminal_stream {
                    print_line(&mut d.parser, message);
                }
            }
            OperationEvent::Question {
                id,
                prompt,
                choices,
                secret,
            } => {
                if self.0.stop_requested.load(Ordering::Acquire) {
                    return;
                }
                if id == "sudo-password" {
                    print_line(&mut d.parser, &i18n::tr!("management-system-authorization"));
                } else {
                    print_line(&mut d.parser, prompt);
                }
                for (index, choice) in choices.iter().enumerate() {
                    print_line(&mut d.parser, &format!("{}: {choice}", index + 1));
                }
                d.status = i18n::tr!("aa-input-required");
                d.question = Some(Question {
                    id: id.clone(),
                    choices: choices.clone(),
                    secret: *secret,
                    value: Zeroizing::new(String::new()),
                    editing: Default::default(),
                });
            }
            OperationEvent::Completed { message }
            | OperationEvent::Failed { message }
            | OperationEvent::Disconnected { message } => {
                // A denied request remains visibly denied, including when its worker exits.
                if self.phase() != DENIED {
                    self.0.phase.store(FINISHED as u8, Ordering::Release);
                    d.status = message.clone();
                    print_line(&mut d.parser, message);
                }
                d.question = None;
                d.stop = None;
            }
            OperationEvent::Started {
                kind,
                action,
                target,
            } => {
                print_line(
                    &mut d.parser,
                    &format!(
                        "{} · {} · {}",
                        kind.id(),
                        action,
                        target.as_deref().unwrap_or_default()
                    ),
                );
            }
            _ => {}
        }
    }
    pub fn finish(&self, result: Result<String, String>) {
        self.emit(&match result {
            Ok(message) => OperationEvent::Completed { message },
            Err(message) => OperationEvent::Failed { message },
        });
    }
    pub fn read_secret(
        &self,
        inputs: &mpsc::Receiver<OperationInput>,
        id: &str,
        prompt: String,
    ) -> Result<Zeroizing<String>, ManagementError> {
        if !self.accepts_input() {
            return Err(ManagementError::Cancelled);
        }
        self.emit(&OperationEvent::Question {
            id: id.into(),
            prompt,
            choices: vec![],
            secret: true,
        });
        let deadline = Instant::now() + Duration::from_secs(300);
        while self.accepts_input() && Instant::now() < deadline {
            match inputs.recv_timeout(Duration::from_millis(100)) {
                Ok(OperationInput::Answer {
                    id: answer_id,
                    value,
                }) => {
                    let value = Zeroizing::new(value);
                    if answer_id == id {
                        return Ok(value);
                    }
                }
                Ok(OperationInput::Cancel | OperationInput::Terminate | OperationInput::Kill)
                | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(ManagementError::Cancelled);
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        Err(ManagementError::Cancelled)
    }
    fn send_bytes(&self, bytes: Vec<u8>) {
        if !self.accepts_input() {
            return;
        }
        #[cfg(target_os = "linux")]
        if let Some(terminal) = self
            .0
            .terminal
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            terminal.write(&bytes);
            return;
        }
        let _ = self.0.responses.send(OperationInput::Terminal { bytes });
    }
    // True means Enter handled a structured question and must not carry over
    // into the terminal or the next password prompt.
    pub fn key(&self, key: &KeyInput) -> bool {
        if !key.phase.is_press_like() || !self.accepts_input() {
            return false;
        }
        let mut d = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(q) = &mut d.question {
            match key.key {
                InputKey::Enter if key.phase == InputPhase::Press => {
                    let value = if q.choices.is_empty() {
                        Some(q.value.to_string())
                    } else {
                        q.value
                            .trim()
                            .parse::<usize>()
                            .ok()
                            .and_then(|n| n.checked_sub(1))
                            .and_then(|n| q.choices.get(n))
                            .cloned()
                            .or_else(|| {
                                q.choices
                                    .iter()
                                    .find(|c| c.eq_ignore_ascii_case(q.value.trim()))
                                    .cloned()
                            })
                    };
                    if let Some(value) = value {
                        let _ = self.0.responses.send(OperationInput::Answer {
                            id: q.id.clone(),
                            value,
                        });
                        d.question = None;
                        d.status = i18n::tr!("aa-running");
                    } else {
                        d.status = i18n::tr!("aa-choose-number");
                    }
                }
                InputKey::Char('c' | 'C') if key.modifiers.is_control() => {
                    let _ = self.0.responses.send(OperationInput::Cancel);
                    d.question = None;
                }
                _ => {
                    let added = match key.key {
                        InputKey::Char(c) => c.len_utf8(),
                        InputKey::Space => 1,
                        _ => 0,
                    };
                    if q.value.len() + added <= 16 * 1024 {
                        q.editing.handle_event(
                            &mut q.value,
                            ui::InputEvent::Key(key.clone()),
                            Rect::default(),
                        );
                    }
                }
            }
            d.revision += 1;
            return key.key == InputKey::Enter && key.phase == InputPhase::Press;
        }
        let application_cursor = d.parser.screen().application_cursor();
        d.parser.set_scrollback(0);
        drop(d);
        if let Some(bytes) = terminal_runtime::pty::key_event_bytes(key, application_cursor) {
            self.send_bytes(bytes);
        }
        false
    }
    pub fn click_input(&self, point: (u16, u16), mut area: Rect) {
        let mut d = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(q) = &mut d.question {
            let mut value = if q.secret {
                "•".repeat(q.value.chars().count())
            } else {
                q.value.to_string()
            };
            area.x += 2;
            area.width = area.width.saturating_sub(2);
            q.editing.handle_event(
                &mut value,
                ui::InputEvent::mouse_down(ui::MouseButton::Left, point),
                area,
            );
            d.revision += 1;
        }
    }

    pub fn paste(&self, text: &str) {
        if !self.accepts_input() {
            return;
        }
        let mut d = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(q) = &mut d.question {
            let mut length = q.value.len();
            let text: String = text
                .chars()
                .filter(|c| !c.is_control())
                .take_while(|c| {
                    length += c.len_utf8();
                    length <= 16 * 1024
                })
                .collect();
            q.editing
                .handle_event(&mut q.value, ui::InputEvent::Paste(text), Rect::default());
            d.revision += 1;
            return;
        }
        let bracketed = d.parser.screen().bracketed_paste();
        drop(d);
        // Bound each protocol packet without dropping long or multibyte pastes.
        if bracketed {
            self.send_bytes(b"\x1b[200~".to_vec());
        }
        for chunk in text.as_bytes().chunks(8192) {
            self.send_bytes(chunk.to_vec());
        }
        if bracketed {
            self.send_bytes(b"\x1b[201~".to_vec());
        }
    }
    pub fn resize(&self, area: Rect) {
        let size = (area.height.max(1), area.width.max(1));
        let mut d = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        if d.parser.screen().size() == size {
            return;
        }
        d.parser.set_size(size.0, size.1);
        d.revision += 1;
        drop(d);
        let _ = self.0.responses.send(OperationInput::Resize {
            columns: size.1,
            rows: size.0,
        });
        #[cfg(target_os = "linux")]
        if let Some(terminal) = self
            .0
            .terminal
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_mut()
        {
            terminal.resize(size.1, size.0);
        }
    }
}

fn print_line(parser: &mut vt100::Parser, text: &str) {
    // Backend descriptions are text, never terminal escape instructions.
    let text = text
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .replace('\n', "\r\n");
    parser.process(text.as_bytes());
    parser.process(b"\r\n");
}

impl AutoAdminJob {
    pub fn cancel(&self) {
        let _lock = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        self.0.phase.store(DENIED as u8, Ordering::Release);
        self.0.approved.notify_all();
    }
    pub fn power_succeeded(&self) -> bool {
        self.0.power_succeeded.load(Ordering::Acquire)
    }
    pub fn mark_power_succeeded(&self) {
        self.0.power_succeeded.store(true, Ordering::Release);
    }
    pub fn retain_worker(&self, worker: ManagedThreadHandle<()>) {
        *self.0.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
    }
    pub fn revision(&self) -> u64 {
        self.0
            .display
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .revision
    }
    pub fn scroll_terminal(&self, up: bool, lines: usize) {
        let mut d = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        let old = d.parser.screen().scrollback();
        d.parser.set_scrollback(if up {
            old.saturating_add(lines)
        } else {
            old.saturating_sub(lines)
        });
    }
    pub fn view_model(
        &self,
        approve_selected: bool,
        button_focus: Option<usize>,
        scroll: u16,
    ) -> ui::AutoAdminViewModel {
        let mut d = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        let input = d.question.as_ref().map(|q| {
            format!(
                "> {}",
                if q.secret {
                    "•".repeat(q.value.chars().count())
                } else {
                    q.value.to_string()
                }
            )
        });
        let snapshot = TerminalSnapshot::from_parser(&mut d.parser);
        let finished = matches!(self.phase(), DENIED | FINISHED);
        let mut terminal = to_ui_snapshot(&snapshot);
        if finished || d.question.is_some() || button_focus.is_some() {
            for cell in &mut terminal.cells {
                cell.cursor = false;
            }
        }
        ui::AutoAdminViewModel {
            description: self.0.description.clone(),
            status: stop::status(&d),
            confirming: self.phase() == WAITING,
            finished,
            approve_selected,
            button_focus,
            scroll,
            input,
            input_sensitive: d.question.as_ref().is_some_and(|question| question.secret),
            input_editing: d
                .question
                .as_ref()
                .map(|question| question.editing.clone())
                .unwrap_or_default(),
            terminal: Arc::new(terminal),
            stop: d
                .stop
                .as_ref()
                .map_or(ui::AutoAdminStopState::None, |s| s.state),
            can_kill: self.can_kill(),
            can_stop: cfg!(target_os = "linux"),
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/job.rs"]
mod tests;
