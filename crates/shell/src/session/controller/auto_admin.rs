//! One approval and terminal surface for built-in administrator operations.
use super::super::*;
use platform::management::{ManagementError, OperationEvent, OperationInput};
use std::sync::{
    Condvar,
    atomic::{AtomicU8, Ordering},
};
use zeroize::Zeroizing;

#[cfg(target_os = "linux")]
#[path = "auto_admin_linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
pub(super) use linux::AutoAdminAuthorization;

const WAITING: u8 = 0;
const RUNNING: u8 = 1;
const DENIED: u8 = 2;
const FINISHED: u8 = 3;
// Legacy terminals send held keys as Press events and provide no release event.
// Keep consuming the action key until it is released, another key is pressed,
// or its repeat stream has been quiet long enough for a deliberate new press.
const ACTION_KEY_QUIET_TIME: Duration = Duration::from_millis(750);

pub(super) fn policy_label(policy: storage::AutoAdminPolicy) -> String {
    i18n::tr!(match policy {
        storage::AutoAdminPolicy::Automatic => "aa-policy-automatic",
        storage::AutoAdminPolicy::Manual => "aa-policy-manual",
        storage::AutoAdminPolicy::Deny => "aa-policy-deny",
    })
}

struct Question {
    id: String,
    choices: Vec<String>,
    secret: bool,
    value: Zeroizing<String>,
}

struct Display {
    parser: vt100::Parser,
    status: String,
    question: Option<Question>,
    revision: u64,
    terminal_stream: bool,
}

struct Shared {
    description: String,
    phase: AtomicU8,
    display: Mutex<Display>,
    approved: Condvar,
    responses: mpsc::Sender<OperationInput>,
    power_succeeded: std::sync::atomic::AtomicBool,
    worker: Mutex<Option<ManagedThreadHandle<()>>>,
    #[cfg(target_os = "linux")]
    terminal: Mutex<Option<linux::AuthTerminal>>,
}

#[derive(Clone)]
pub(in crate::session) struct AutoAdminJob(Arc<Shared>);
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::session) struct AutoAdminState {
    job: Option<AutoAdminJob>,
    pub(super) visible: bool,
    approve_selected: bool,
    scroll: u16,
    revision: u64,
    pointer: Option<(usize, u8, Instant)>,
    suppress_repeats: bool,
    action_key: Option<(InputKey, Instant)>,
    button_focus: Option<usize>,
}

impl AutoAdminState {
    fn consume_action_repeat(&mut self, key: &KeyInput, at: Instant) -> bool {
        let Some((action, last_seen)) = self.action_key.as_mut() else {
            return false;
        };
        if &key.key != action {
            if key.phase.is_press_like() {
                self.action_key = None;
            }
            return false;
        }
        if key.phase == InputPhase::Release {
            self.action_key = None;
            return true;
        }
        if key.phase == InputPhase::Repeat
            || at.saturating_duration_since(*last_seen) < ACTION_KEY_QUIET_TIME
        {
            *last_seen = at;
            return true;
        }
        self.action_key = None;
        false
    }
}

impl AutoAdminJob {
    fn new(
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
            phase: AtomicU8::new(phase),
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
            }),
            approved: Condvar::new(),
            responses,
            power_succeeded: std::sync::atomic::AtomicBool::new(false),
            worker: Mutex::new(None),
            #[cfg(target_os = "linux")]
            terminal: Mutex::new(None),
        }))
    }
    fn phase(&self) -> u8 {
        self.0.phase.load(Ordering::Acquire)
    }
    pub(super) fn running(&self) -> bool {
        matches!(self.phase(), WAITING | RUNNING)
    }
    pub(super) fn wait_for_approval(&self) -> Result<(), ManagementError> {
        let mut display = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        while self.phase() == WAITING {
            let (next, timeout) = self
                .0
                .approved
                .wait_timeout(display, Duration::from_secs(300))
                .unwrap_or_else(|e| e.into_inner());
            display = next;
            if timeout.timed_out() && self.phase() == WAITING {
                self.0.phase.store(DENIED, Ordering::Release);
                display.status = i18n::tr!("aa-expired");
                display.revision += 1;
            }
        }
        if self.phase() == RUNNING {
            Ok(())
        } else {
            Err(ManagementError::Cancelled)
        }
    }
    fn decide(&self, approve: bool) {
        let mut display = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        if self.phase() != WAITING {
            return;
        }
        self.0
            .phase
            .store(if approve { RUNNING } else { DENIED }, Ordering::Release);
        display.status = i18n::tr!(if approve { "aa-running" } else { "aa-rejected" });
        display.revision += 1;
        self.0.approved.notify_all();
    }
    pub(super) fn emit(&self, event: &OperationEvent) {
        let mut d = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        d.revision = d.revision.wrapping_add(1);
        match event {
            OperationEvent::TerminalOutput { bytes } => {
                d.terminal_stream = true;
                d.parser.process(bytes);
            }
            OperationEvent::Output { text } => print_line(&mut d.parser, text),
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
                print_line(&mut d.parser, prompt);
                for (index, choice) in choices.iter().enumerate() {
                    print_line(&mut d.parser, &format!("{}: {choice}", index + 1));
                }
                d.status = i18n::tr!("aa-input-required");
                d.question = Some(Question {
                    id: id.clone(),
                    choices: choices.clone(),
                    secret: *secret,
                    value: Zeroizing::new(String::new()),
                });
            }
            OperationEvent::Completed { message }
            | OperationEvent::Failed { message }
            | OperationEvent::Disconnected { message } => {
                // A denied request remains visibly denied, including when its worker exits.
                if self.phase() != DENIED {
                    self.0.phase.store(FINISHED, Ordering::Release);
                    d.status = message.clone();
                    print_line(&mut d.parser, message);
                }
                d.question = None;
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
    pub(super) fn finish(&self, result: Result<String, String>) {
        self.emit(&match result {
            Ok(message) => OperationEvent::Completed { message },
            Err(message) => OperationEvent::Failed { message },
        });
    }
    pub(super) fn read_secret(
        &self,
        inputs: &mpsc::Receiver<OperationInput>,
        id: &str,
        prompt: String,
    ) -> Result<Zeroizing<String>, ManagementError> {
        if self.phase() != RUNNING {
            return Err(ManagementError::Cancelled);
        }
        self.emit(&OperationEvent::Question {
            id: id.into(),
            prompt,
            choices: vec![],
            secret: true,
        });
        let deadline = Instant::now() + Duration::from_secs(300);
        while self.phase() == RUNNING && Instant::now() < deadline {
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
                Ok(OperationInput::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(ManagementError::Cancelled);
                }
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
        Err(ManagementError::Cancelled)
    }
    fn send_bytes(&self, bytes: Vec<u8>) {
        if self.phase() != RUNNING {
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
    fn key(&self, key: &KeyInput) -> bool {
        if !key.phase.is_press_like() || self.phase() != RUNNING {
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
                InputKey::Backspace => {
                    q.value.pop();
                }
                InputKey::Char(c)
                    if !c.is_control()
                        && !key.modifiers.is_control()
                        && q.value.len() < 16 * 1024 =>
                {
                    q.value.push(c)
                }
                InputKey::Space if q.value.len() < 16 * 1024 => q.value.push(' '),
                _ => {}
            }
            d.revision += 1;
            return key.key == InputKey::Enter && key.phase == InputPhase::Press;
        }
        let application_cursor = d.parser.screen().application_cursor();
        d.parser.set_scrollback(0);
        drop(d);
        if let Some(bytes) =
            super::super::command_line_runtime::key_event_bytes(key, application_cursor)
        {
            self.send_bytes(bytes);
        }
        false
    }
    fn paste(&self, text: &str) {
        if self.phase() != RUNNING {
            return;
        }
        let mut d = self.0.display.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(q) = &mut d.question {
            for c in text.chars().filter(|c| !c.is_control()) {
                if q.value.len() + c.len_utf8() > 16 * 1024 {
                    break;
                }
                q.value.push(c);
            }
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
    fn resize(&self, area: Rect) {
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

impl ShellSession {
    pub(in crate::session) fn stop_auto_admin(&self) {
        if let Some(job) = &self.auto_admin.job {
            let _lock = job.0.display.lock().unwrap_or_else(|e| e.into_inner());
            job.0.phase.store(DENIED, Ordering::Release);
            job.0.approved.notify_all();
        }
    }
    pub(in crate::session) fn auto_admin_power_succeeded(&self) -> bool {
        self.auto_admin
            .job
            .as_ref()
            .is_some_and(|job| job.0.power_succeeded.load(Ordering::Acquire))
    }
    pub(in crate::session) fn start_auto_admin_power(
        &mut self,
        reboot: bool,
        platform: Arc<dyn Platform>,
    ) {
        let Some(group) = self.settings_task_runtime.shared.task_group.clone() else {
            return;
        };
        let (responses, _inputs) = mpsc::channel();
        let Some(job) = self.begin_auto_admin(
            i18n::tr!(if reboot { "aa-reboot" } else { "aa-poweroff" }),
            true,
            responses,
        ) else {
            return;
        };
        let worker_job = job.clone();
        let language = self.language.clone();
        match group.spawn_thread(
            TaskSpec::one_shot(TaskId::from_static("auto-admin-power")),
            move || {
                let _language = i18n::enter_snapshot(language.clone());
                let result = worker_job
                    .wait_for_approval()
                    .map_err(|e| e.to_string())
                    .and_then(|()| {
                        #[cfg(target_os = "linux")]
                        {
                            let _ = &platform;
                            platform::linux::power::execute_with_interaction(
                                if reboot {
                                    platform::linux::power::PowerAction::Reboot
                                } else {
                                    platform::linux::power::PowerAction::PowerOff
                                },
                                Some(Arc::new(AutoAdminAuthorization::new(worker_job.clone()))),
                            )
                            .map_err(|e| e.to_string())
                        }
                        #[cfg(not(target_os = "linux"))]
                        {
                            if reboot {
                                platform.reboot()
                            } else {
                                platform.poweroff()
                            }
                            .map_err(|e| e.to_string())
                        }
                    });
                if result.is_ok() {
                    worker_job.0.power_succeeded.store(true, Ordering::Release);
                }
                worker_job.finish(result.map(|()| i18n::tr!("aa-completed")));
            },
        ) {
            Ok(worker) => *job.0.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker),
            Err(error) => job.finish(Err(error.to_string())),
        }
    }
    pub(in crate::session) fn begin_auto_admin(
        &mut self,
        description: String,
        privileged: bool,
        responses: mpsc::Sender<OperationInput>,
    ) -> Option<AutoAdminJob> {
        if self
            .auto_admin
            .job
            .as_ref()
            .is_some_and(AutoAdminJob::running)
        {
            self.auto_admin.visible = true;
            self.notify_status(i18n::msg!("aa-busy"));
            return None;
        }
        let policy = if privileged {
            // A config read failure must never silently grant automatic approval.
            match self.storage_manager.as_ref().map(|s| s.load_config()) {
                Some(Ok(config)) => config.auto_admin,
                Some(Err(_)) => storage::AutoAdminPolicy::Deny,
                None => storage::AutoAdminPolicy::Manual,
            }
        } else {
            storage::AutoAdminPolicy::Automatic
        };
        let job = AutoAdminJob::new(description, policy, responses);
        self.auto_admin = AutoAdminState {
            job: Some(job.clone()),
            visible: true,
            suppress_repeats: true,
            ..Default::default()
        };
        self.last_key_event = None;
        self.keyboard_focus_visible = true;
        self.button_pointer_capture = None;
        self.resize_auto_admin();
        Some(job)
    }
    pub(in crate::session) fn auto_admin_visible(&self) -> bool {
        self.auto_admin.visible && self.auto_admin.job.is_some()
    }
    pub(in crate::session) fn auto_admin_running(&self) -> bool {
        self.auto_admin
            .job
            .as_ref()
            .is_some_and(AutoAdminJob::running)
    }
    pub(in crate::session) fn resize_auto_admin(&self) {
        if let Some(job) = &self.auto_admin.job {
            job.resize(
                ui::auto_admin_layout(
                    Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1),
                    false,
                )
                .terminal,
            );
        }
    }
    pub(in crate::session) fn poll_auto_admin(&mut self) -> bool {
        let Some(job) = &self.auto_admin.job else {
            return false;
        };
        #[cfg(target_os = "linux")]
        job.poll_terminal();
        let revision = job
            .0
            .display
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .revision;
        let changed = revision != self.auto_admin.revision;
        self.auto_admin.revision = revision;
        changed
    }
    pub(in crate::session) fn close_auto_admin(&mut self) {
        if let Some(job) = &self.auto_admin.job {
            job.decide(false);
        }
        self.auto_admin.visible = false;
        self.auto_admin.pointer = None;
    }
    pub(in crate::session) fn show_auto_admin_job(&mut self, job: AutoAdminJob) {
        if self
            .auto_admin
            .job
            .as_ref()
            .is_some_and(|active| active.running() && active != &job)
        {
            self.auto_admin.visible = true;
            self.notify_status(i18n::msg!("aa-busy"));
            return;
        }
        if self.auto_admin.job.as_ref() != Some(&job) {
            self.auto_admin = AutoAdminState {
                job: Some(job),
                ..Default::default()
            };
        }
        self.auto_admin.visible = true;
        self.auto_admin.pointer = None;
        self.auto_admin.suppress_repeats = true;
        self.last_key_event = None;
        self.keyboard_focus_visible = true;
        self.button_pointer_capture = None;
        self.resize_auto_admin();
    }
    pub(in crate::session) fn auto_admin_view(&self) -> Option<ui::AutoAdminViewModel> {
        if !self.auto_admin_visible() {
            return None;
        }
        let job = self.auto_admin.job.as_ref()?;
        let mut d = job.0.display.lock().unwrap_or_else(|e| e.into_inner());
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
        Some(ui::AutoAdminViewModel {
            description: job.0.description.clone(),
            status: d.status.clone(),
            confirming: job.phase() == WAITING,
            finished: matches!(job.phase(), DENIED | FINISHED),
            approve_selected: self.auto_admin.approve_selected,
            button_focus: self.auto_admin.button_focus,
            scroll: self.auto_admin.scroll,
            input,
            terminal: Arc::new(super::super::command_line_runtime::to_ui_snapshot(
                &snapshot,
            )),
        })
    }
    #[cfg(test)]
    pub(in crate::session) fn handle_auto_admin_input(&mut self, input: &InputEvent) -> bool {
        self.handle_auto_admin_input_at(input, Instant::now())
    }
    pub(in crate::session) fn handle_auto_admin_input_at(
        &mut self,
        input: &InputEvent,
        received_at: Instant,
    ) -> bool {
        // A key used to hide the modal must not activate the page behind it.
        if let InputEvent::Key(key) = input
            && self.auto_admin.consume_action_repeat(key, received_at)
        {
            return true;
        }
        if !self.auto_admin_visible() {
            if matches!(
                input,
                InputEvent::Key(KeyInput {
                    key: InputKey::F(12),
                    phase: InputPhase::Press,
                    ..
                })
            ) && self.auto_admin.job.is_some()
            {
                self.auto_admin.visible = true;
                self.auto_admin.action_key = Some((InputKey::F(12), received_at));
                return true;
            }
            return false;
        }
        let job = self.auto_admin.job.as_ref().unwrap().clone();
        if matches!(input, InputEvent::Key(_) | InputEvent::Paste(_)) {
            self.auto_admin.pointer = None;
        }
        if let InputEvent::Key(key) = input {
            if key.phase.is_press_like() {
                self.keyboard_focus_visible = true;
            }
            if key.phase == InputPhase::Release {
                self.auto_admin.suppress_repeats = false;
                return true;
            }
            if key.phase == InputPhase::Repeat && self.auto_admin.suppress_repeats {
                return true;
            }
            if key.phase == InputPhase::Press {
                self.auto_admin.suppress_repeats = false;
            }
        }
        match input {
            InputEvent::Tick | InputEvent::Shutdown => return false,
            InputEvent::Resize { .. } => {
                self.auto_admin.pointer = None;
                return false;
            }
            InputEvent::Key(key)
                if key.phase == InputPhase::Press && key.key == InputKey::F(12) =>
            {
                self.close_auto_admin();
                self.auto_admin.action_key = Some((key.key.clone(), received_at));
            }
            InputEvent::Key(key) if job.phase() == WAITING && key.phase == InputPhase::Press => {
                match key.key {
                    InputKey::Tab | InputKey::BackTab | InputKey::Left | InputKey::Right => {
                        self.auto_admin.approve_selected = !self.auto_admin.approve_selected
                    }
                    InputKey::Enter | InputKey::Space => {
                        job.decide(self.auto_admin.approve_selected);
                        self.auto_admin.suppress_repeats = true;
                        self.auto_admin.action_key = Some((key.key.clone(), received_at));
                        self.auto_admin.button_focus = None;
                    }
                    InputKey::Escape => {
                        job.decide(false);
                        self.auto_admin.action_key = Some((key.key.clone(), received_at));
                    }
                    InputKey::PageDown | InputKey::Down => {
                        self.auto_admin.scroll = self.auto_admin.scroll.saturating_add(3)
                    }
                    InputKey::PageUp | InputKey::Up => {
                        self.auto_admin.scroll = self.auto_admin.scroll.saturating_sub(3)
                    }
                    _ => {}
                }
            }
            InputEvent::Key(key)
                if matches!(job.phase(), DENIED | FINISHED)
                    && matches!(
                        key.key,
                        InputKey::Enter | InputKey::Space | InputKey::Escape
                    ) =>
            {
                if key.phase == InputPhase::Press {
                    self.close_auto_admin();
                    self.auto_admin.action_key = Some((key.key.clone(), received_at));
                }
            }
            InputEvent::Key(key) if job.phase() == RUNNING && key.key == InputKey::F(6) => {
                if key.phase == InputPhase::Press {
                    self.auto_admin.button_focus = if self.auto_admin.button_focus.is_some() {
                        None
                    } else {
                        Some(0)
                    };
                    self.auto_admin.action_key = Some((key.key.clone(), received_at));
                }
            }
            InputEvent::Key(key)
                if job.phase() == RUNNING && self.auto_admin.button_focus.is_some() =>
            {
                let index = self.auto_admin.button_focus.unwrap();
                if key.phase.is_press_like() {
                    match key.key {
                        InputKey::BackTab | InputKey::Left | InputKey::Up => {
                            self.auto_admin.button_focus = Some((index + 3) % 4);
                        }
                        InputKey::Tab if key.modifiers.shift => {
                            self.auto_admin.button_focus = Some((index + 3) % 4);
                        }
                        InputKey::Tab | InputKey::Right | InputKey::Down => {
                            self.auto_admin.button_focus = Some((index + 1) % 4);
                        }
                        InputKey::Escape => {
                            self.auto_admin.button_focus = None;
                            self.auto_admin.action_key = Some((key.key.clone(), received_at));
                        }
                        InputKey::Enter | InputKey::Space if key.phase == InputPhase::Press => {
                            self.activate_auto_admin_button(&job, index);
                            self.auto_admin.action_key = Some((key.key.clone(), received_at));
                        }
                        _ => {}
                    }
                }
            }
            InputEvent::Key(key)
                if key.modifiers.shift
                    && matches!(key.key, InputKey::PageUp | InputKey::PageDown) =>
            {
                if key.phase.is_press_like() {
                    let mut d = job.0.display.lock().unwrap_or_else(|e| e.into_inner());
                    let old = d.parser.screen().scrollback();
                    d.parser.set_scrollback(if key.key == InputKey::PageUp {
                        old.saturating_add(10)
                    } else {
                        old.saturating_sub(10)
                    });
                }
            }
            InputEvent::Key(key) => {
                if job.key(key) {
                    self.auto_admin.action_key = Some((key.key.clone(), received_at));
                }
            }
            InputEvent::Paste(text) => {
                if self.auto_admin.button_focus.is_none() {
                    self.auto_admin.action_key = None;
                    job.paste(text);
                }
            }
            InputEvent::Mouse(mouse) => {
                self.mouse_coordinates = Some(mouse.coordinates());
                self.keyboard_focus_visible = false;
                let layout = ui::auto_admin_layout(
                    Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1),
                    job.phase() == WAITING,
                );
                let hit = layout
                    .buttons
                    .iter()
                    .position(|r| r.contains(ratatui::layout::Position::from(mouse.coordinates())));
                let activated = match mouse.kind {
                    ui::MouseEventKind::Down(ui::MouseButton::Left) => {
                        self.auto_admin.button_focus = None;
                        self.auto_admin.pointer =
                            hit.map(|index| (index, job.phase(), received_at));
                        None
                    }
                    ui::MouseEventKind::Up(ui::MouseButton::Left) => {
                        let previous = self.auto_admin.pointer.take();
                        hit.filter(|h| {
                            previous.is_some_and(|(index, phase, at)| {
                                index == *h
                                    && phase == job.phase()
                                    && received_at.saturating_duration_since(at)
                                        <= Duration::from_millis(500)
                            })
                        })
                    }
                    ui::MouseEventKind::Click(ui::MouseButton::Left) => hit,
                    ui::MouseEventKind::Scroll(direction) => {
                        let up = direction == ScrollDirection::Up;
                        if job.phase() == WAITING {
                            self.auto_admin.scroll = if up {
                                self.auto_admin.scroll.saturating_sub(3)
                            } else {
                                self.auto_admin.scroll.saturating_add(3)
                            };
                        } else {
                            let mut d = job.0.display.lock().unwrap_or_else(|e| e.into_inner());
                            let old = d.parser.screen().scrollback();
                            d.parser.set_scrollback(if up {
                                old.saturating_add(3)
                            } else {
                                old.saturating_sub(3)
                            });
                        }
                        None
                    }
                    _ => None,
                };
                if let Some(index) = activated {
                    self.auto_admin.button_focus = None;
                    self.activate_auto_admin_button(&job, index);
                }
            }
            InputEvent::FocusLost => self.auto_admin.pointer = None,
            InputEvent::FocusGained => {}
        }
        true
    }
    fn activate_auto_admin_button(&mut self, job: &AutoAdminJob, index: usize) {
        match job.phase() {
            WAITING if index < 2 => job.decide(index == 0),
            DENIED | FINISHED if index == 0 => self.close_auto_admin(),
            RUNNING => match index {
                0 => job.paste("y"),
                1 => job.paste("n"),
                2 => {
                    job.key(&KeyInput::new(InputKey::Enter));
                }
                3 => self.close_auto_admin(),
                _ => {}
            },
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/session/controller/auto_admin/tests.rs"]
mod tests;
