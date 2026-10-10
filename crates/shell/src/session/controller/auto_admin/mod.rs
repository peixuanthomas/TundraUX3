//! Shell modal and input routing for independent AutoAdmin jobs.
use crate::session::*;
#[cfg(target_os = "linux")]
pub(super) use ::auto_admin::AutoAdminAuthorization;
use ::auto_admin::JobPhase::{
    Denied as DENIED, Finished as FINISHED, Running as RUNNING, Waiting as WAITING,
};
pub(in crate::session) use ::auto_admin::{AutoAdminJob, spawn_task};
#[cfg(test)]
use platform::management::OperationEvent;
use platform::management::OperationInput;
mod stop;
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
    pub(in crate::session) fn guard_opening_key(&mut self, key: KeyInput, at: Instant) {
        self.action_key = Some((key.key, at));
    }

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

impl ShellSession {
    pub(in crate::session) fn stop_auto_admin(&self) {
        #[cfg(target_os = "linux")]
        self.privilege_session.revoke();
        if let Some(job) = &self.auto_admin.job {
            job.cancel();
        }
    }
    pub(in crate::session) fn auto_admin_power_succeeded(&self) -> bool {
        self.auto_admin
            .job
            .as_ref()
            .is_some_and(AutoAdminJob::power_succeeded)
    }
    pub(in crate::session) fn start_auto_admin_power(
        &mut self,
        reboot: bool,
        platform: Arc<dyn Platform>,
    ) {
        let Some(group) = self.settings_task_runtime.shared.task_group.clone() else {
            return;
        };
        let (responses, inputs) = mpsc::channel();
        let Some(job) = self.begin_auto_admin(
            i18n::tr!(if reboot { "aa-reboot" } else { "aa-poweroff" }),
            true,
            responses,
        ) else {
            return;
        };
        let worker_job = job.clone();
        #[cfg(target_os = "linux")]
        let authority = self.privilege_session.clone();
        #[cfg(target_os = "linux")]
        let administrator = self
            .app
            .auth_session()
            .is_some_and(|actor| actor.role == UserRole::Admin);
        if let Ok(worker) = spawn_task(
            &group,
            TaskId::from_static("auto-admin-power"),
            self.language.clone(),
            Some(&job),
            move || {
                let result = worker_job.run_approved(
                    |error| error.to_string(),
                    || {
                        #[cfg(target_os = "linux")]
                        {
                            let _ = &platform;
                            let action = if reboot {
                                platform::linux::power::PowerAction::Reboot
                            } else {
                                platform::linux::power::PowerAction::PowerOff
                            };
                            // Preserve ordinary logind access: a currently allowed
                            // shutdown, or a non-admin user's system-agent path,
                            // must not newly require membership in sudoers.
                            if !administrator
                                || platform::linux::power::availability(action)
                                    == Ok(platform::linux::power::PowerAvailability::Allowed)
                            {
                                return platform::linux::power::execute_with_interaction(
                                    action,
                                    Some(Arc::new(AutoAdminAuthorization::new(worker_job.clone()))),
                                )
                                .map_err(|e| e.to_string());
                            }
                            authority
                                .ensure(|| {
                                    worker_job.read_secret(
                                        &inputs,
                                        "sudo-password",
                                        i18n::tr!("management-auth-prompt"),
                                    )
                                })
                                .map_err(|e| e.to_string())?;
                            authority
                                .execute(platform::linux::privilege_session::Request::Power {
                                    reboot,
                                })
                                .map_err(|e| e.to_string())
                        }
                        #[cfg(not(target_os = "linux"))]
                        {
                            let _ = &inputs;
                            if reboot {
                                platform.reboot()
                            } else {
                                platform.poweroff()
                            }
                            .map_err(|e| e.to_string())
                        }
                    },
                );
                if result.is_ok() {
                    worker_job.mark_power_succeeded();
                }
                worker_job.finish_result(&result, |()| i18n::tr!("aa-completed"));
            },
        ) {
            job.retain_worker(worker);
        }
    }
    pub(in crate::session) fn begin_auto_admin(
        &mut self,
        description: String,
        privileged: bool,
        responses: mpsc::Sender<OperationInput>,
    ) -> Option<AutoAdminJob> {
        self.synchronize_overlay_focus();
        if self
            .auto_admin
            .job
            .as_ref()
            .is_some_and(AutoAdminJob::running)
        {
            self.auto_admin.visible = true;
            self.synchronize_overlay_focus();
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
            approve_selected: true,
            suppress_repeats: true,
            ..Default::default()
        };
        self.last_key_event = None;
        self.button_pointer_capture = None;
        self.resize_auto_admin();
        self.synchronize_overlay_focus();
        Some(job)
    }
    pub(in crate::session) fn auto_admin_visible(&self) -> bool {
        self.auto_admin.visible && self.auto_admin.job.is_some()
    }
    pub(in crate::session) fn cancel_auto_admin_pointer(&mut self) {
        self.auto_admin.pointer = None;
    }
    pub(in crate::session) fn auto_admin_running(&self) -> bool {
        self.auto_admin
            .job
            .as_ref()
            .is_some_and(AutoAdminJob::running)
    }
    pub(in crate::session) fn resize_auto_admin(&self) {
        if let Some(job) = &self.auto_admin.job
            && let Some(mut model) = self.auto_admin_model()
        {
            // Size the backing terminal for execution even while the compact
            // confirmation/result is visible, preserving output and scrollback.
            model.confirming = false;
            model.finished = false;
            model.stop = ui::AutoAdminStopState::None;
            job.resize(
                ui::auto_admin_layout(
                    Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1),
                    &model,
                )
                .terminal,
            );
        }
    }
    pub(in crate::session) fn poll_auto_admin(&mut self) -> bool {
        self.poll_auto_admin_stop(Instant::now());
        let Some(job) = &self.auto_admin.job else {
            return false;
        };
        #[cfg(target_os = "linux")]
        job.poll_terminal();
        let revision = job.revision();
        let changed = revision != self.auto_admin.revision;
        self.auto_admin.revision = revision;
        changed
    }
    pub(in crate::session) fn close_auto_admin(&mut self) {
        self.synchronize_overlay_focus();
        if let Some(job) = &self.auto_admin.job {
            // Keep execution and all of its questions in the foreground until
            // the worker reports completion, failure or disconnection.
            if job.phase() == RUNNING {
                return;
            }
            job.decide(false);
        }
        self.auto_admin.visible = false;
        self.auto_admin.pointer = None;
        self.synchronize_overlay_focus();
    }
    pub(in crate::session) fn show_auto_admin_job(&mut self, job: AutoAdminJob) {
        self.synchronize_overlay_focus();
        if self
            .auto_admin
            .job
            .as_ref()
            .is_some_and(|active| active.running() && active != &job)
        {
            self.auto_admin.visible = true;
            self.synchronize_overlay_focus();
            self.notify_status(i18n::msg!("aa-busy"));
            return;
        }
        if self.auto_admin.job.as_ref() != Some(&job) {
            self.auto_admin = AutoAdminState {
                job: Some(job),
                approve_selected: true,
                ..Default::default()
            };
        }
        self.auto_admin.visible = true;
        self.auto_admin.pointer = None;
        self.auto_admin.suppress_repeats = true;
        self.last_key_event = None;
        self.button_pointer_capture = None;
        self.resize_auto_admin();
        self.synchronize_overlay_focus();
    }
    pub(in crate::session) fn auto_admin_view(&self) -> Option<ui::AutoAdminViewModel> {
        if !self.auto_admin_visible() {
            return None;
        }
        self.auto_admin_model()
    }
    pub(in crate::session) fn auto_admin_pressed_button(
        &self,
    ) -> Option<ui::components::ButtonRegion> {
        let (index, phase, _) = self.auto_admin.pointer?;
        if self.auto_admin.job.as_ref()?.interaction_phase() != phase {
            return None;
        }
        self.button_regions
            .iter()
            .filter(|region| region.id.as_str().starts_with("aa."))
            .nth(index)
            .cloned()
    }
    fn auto_admin_model(&self) -> Option<ui::AutoAdminViewModel> {
        let job = self.auto_admin.job.as_ref()?;
        Some(job.view_model(
            self.auto_admin.approve_selected,
            self.auto_admin.button_focus,
            self.auto_admin.scroll,
        ))
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
        self.synchronize_overlay_focus();
        let consumed = self.handle_auto_admin_input_inner(input, received_at);
        self.synchronize_overlay_focus();
        consumed
    }

    fn handle_auto_admin_input_inner(&mut self, input: &InputEvent, received_at: Instant) -> bool {
        // AA consumes input before ordinary page routing. Its Back button still
        // uses the shared release capture and exactly the same policy as Esc.
        let normalized;
        let input = if self.auto_admin_visible()
            && (matches!(input, InputEvent::Mouse(mouse)
                if self.hit_map.target_at(mouse.coordinates()) == Some(ShellComponent::BackButton))
                || self
                    .button_pointer_capture
                    .as_ref()
                    .is_some_and(|capture| capture.region.id.as_str() == "shell.back"))
        {
            let Some(prepared) = self.prepare_button_input(input.clone(), received_at) else {
                return true;
            };
            normalized = self.normalize_shell_navigation_input(prepared);
            &normalized
        } else {
            input
        };
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
                self.update_button_input_mode(input);
                self.auto_admin.action_key = Some((InputKey::F(12), received_at));
                return true;
            }
            return false;
        }
        let job = self.auto_admin.job.as_ref().unwrap().clone();
        self.update_button_input_mode(input);
        if matches!(input, InputEvent::Key(_) | InputEvent::Paste(_)) {
            self.auto_admin.pointer = None;
        }
        if let InputEvent::Key(key) = input {
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
            InputEvent::Key(key) if job.stop_warning() => {
                if key.phase == InputPhase::Press {
                    match key.key {
                        InputKey::Tab
                        | InputKey::BackTab
                        | InputKey::Left
                        | InputKey::Right
                        | InputKey::Up
                        | InputKey::Down => {
                            let index = self.auto_admin.button_focus.unwrap_or(0);
                            self.auto_admin.button_focus =
                                Some(if job.can_kill() { 1 - index.min(1) } else { 0 });
                        }
                        InputKey::Enter | InputKey::Space => {
                            self.activate_auto_admin_button(
                                &job,
                                self.auto_admin.button_focus.unwrap_or(0),
                            );
                            self.auto_admin.action_key = Some((key.key.clone(), received_at));
                        }
                        InputKey::Escape => {
                            self.activate_auto_admin_button(&job, 0);
                            self.auto_admin.action_key = Some((key.key.clone(), received_at));
                        }
                        InputKey::PageDown => {
                            self.auto_admin.scroll = self.auto_admin.scroll.saturating_add(3)
                        }
                        InputKey::PageUp => {
                            self.auto_admin.scroll = self.auto_admin.scroll.saturating_sub(3)
                        }
                        _ => {}
                    }
                }
            }
            InputEvent::Key(key) if key.key == InputKey::F(12) => {
                if key.phase == InputPhase::Press {
                    self.close_auto_admin();
                    self.auto_admin.action_key = Some((key.key.clone(), received_at));
                }
            }
            InputEvent::Key(key) if job.phase() == WAITING && key.phase == InputPhase::Press => {
                match key.key {
                    InputKey::Tab | InputKey::BackTab | InputKey::Left | InputKey::Right => {
                        self.auto_admin.approve_selected = !self.auto_admin.approve_selected
                    }
                    InputKey::Enter | InputKey::Space => {
                        self.activate_auto_admin_button(
                            &job,
                            usize::from(!self.auto_admin.approve_selected),
                        );
                        self.auto_admin.suppress_repeats = true;
                        self.auto_admin.action_key = Some((key.key.clone(), received_at));
                        self.auto_admin.button_focus = None;
                    }
                    InputKey::Escape => {
                        self.close_auto_admin();
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
                let count = if cfg!(target_os = "linux") { 4 } else { 3 };
                if key.phase.is_press_like() {
                    match key.key {
                        InputKey::BackTab | InputKey::Left | InputKey::Up => {
                            self.auto_admin.button_focus = Some((index + count - 1) % count);
                        }
                        InputKey::Tab if key.modifiers.shift => {
                            self.auto_admin.button_focus = Some((index + count - 1) % count);
                        }
                        InputKey::Tab | InputKey::Right | InputKey::Down => {
                            self.auto_admin.button_focus = Some((index + 1) % count);
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
                    job.scroll_terminal(key.key == InputKey::PageUp, 10);
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
                let layout = ui::auto_admin_layout(
                    Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1),
                    &self.auto_admin_model().unwrap(),
                );
                let hit = layout
                    .buttons
                    .iter()
                    .position(|r| r.contains(ratatui::layout::Position::from(mouse.coordinates())));
                let activated = match mouse.kind {
                    ui::MouseEventKind::Down(ui::MouseButton::Left) => {
                        self.auto_admin.button_focus = None;
                        if job.phase() == WAITING
                            && let Some(index) = hit
                        {
                            self.auto_admin.approve_selected = index == 0;
                        }
                        self.auto_admin.pointer =
                            hit.map(|index| (index, job.interaction_phase(), received_at));
                        None
                    }
                    ui::MouseEventKind::Up(ui::MouseButton::Left) => {
                        let previous = self.auto_admin.pointer.take();
                        hit.filter(|h| {
                            previous.is_some_and(|(index, phase, at)| {
                                index == *h
                                    && phase == job.interaction_phase()
                                    && received_at.saturating_duration_since(at)
                                        <= Duration::from_millis(500)
                            })
                        })
                    }
                    ui::MouseEventKind::Click(ui::MouseButton::Left) => hit,
                    ui::MouseEventKind::Drag(_) => {
                        self.auto_admin.pointer = None;
                        None
                    }
                    ui::MouseEventKind::Scroll(direction) => {
                        let up = direction == ScrollDirection::Up;
                        if job.phase() == WAITING || job.stop_warning() {
                            self.auto_admin.scroll = if up {
                                self.auto_admin.scroll.saturating_sub(3)
                            } else {
                                self.auto_admin.scroll.saturating_add(3)
                            };
                        } else {
                            job.scroll_terminal(up, 3);
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
        if job.phase() == WAITING || job.stop_warning() {
            let model = self.auto_admin_model().unwrap();
            let max_scroll = ui::auto_admin_max_scroll(
                Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1),
                &model,
            );
            self.auto_admin.scroll = self.auto_admin.scroll.min(max_scroll);
        }
        true
    }
    fn activate_auto_admin_button(&mut self, job: &AutoAdminJob, index: usize) {
        if job.stop_warning() {
            if index == 0 {
                job.continue_waiting();
                self.auto_admin.button_focus = None;
            } else if index == 1 && job.can_kill() {
                job.request_stop(true);
                self.auto_admin.button_focus = None;
            }
            self.auto_admin.pointer = None;
            return;
        }
        match job.phase() {
            WAITING if index == 0 => job.decide(true),
            WAITING if index == 1 => self.close_auto_admin(),
            DENIED | FINISHED if index == 0 => self.close_auto_admin(),
            RUNNING => match index {
                0 => job.paste("y"),
                1 => job.paste("n"),
                2 => {
                    job.key(&KeyInput::new(InputKey::Enter));
                }
                3 if cfg!(target_os = "linux") => {
                    job.request_stop(false);
                    self.auto_admin.button_focus = None;
                    self.auto_admin.pointer = None;
                }
                _ => {}
            },
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/auto_admin/tests.rs"]
mod tests;

#[cfg(all(test, target_os = "linux"))]
#[path = "../../../../tests/unit/session/controller/auto_admin/linux_tests.rs"]
mod linux_tests;
