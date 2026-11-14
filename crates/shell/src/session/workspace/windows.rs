use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

impl Workspace {
    pub(in crate::session) fn poll(&mut self, state: &mut ShellSession) -> io::Result<bool> {
        let mut redraw = false;
        let connection = WorkerConnection::current().expect("managed workspace");
        connection.progress()?;
        while let Some(message) = connection.try_recv()? {
            match message {
                SupervisorToWorker::AppStarted {
                    worker,
                    pid,
                    endpoint,
                } => {
                    let WorkerRole::App(id) = worker.role else {
                        continue;
                    };
                    if let Some(window) = self.windows.get_mut(&id)
                        && window.worker.is_none_or(|current| current == worker)
                    {
                        window.worker = Some(worker);
                        match connection.connect_app(worker, pid, &endpoint) {
                            Ok(stream) => window.stream = Some(stream),
                            Err(error) => {
                                report_launch_failure(state, error.to_string());
                                self.force(id)?;
                            }
                        }
                    }
                    redraw = true;
                }
                SupervisorToWorker::AppState {
                    worker,
                    state: status,
                } => {
                    let WorkerRole::App(id) = worker.role else {
                        continue;
                    };
                    if let Some(window) = self.windows.get_mut(&id)
                        && window.worker.is_none_or(|current| current == worker)
                    {
                        window.worker = Some(worker);
                        window.state = status;
                        if status == ProcessState::Unresponsive {
                            self.input_fences.remove(&id);
                        }
                        if matches!(status, ProcessState::Exited { .. }) {
                            let launching = window.frame.is_none()
                                && matches!(window.close, CloseProgress::Idle)
                                && !window.force_when_started
                                && !window.launch_error;
                            self.remove(id);
                            if launching {
                                report_launch_failure(
                                    state,
                                    i18n::tr!("ui-components-unavailable"),
                                );
                            }
                        } else if window.force_when_started {
                            window.force_when_started = false;
                            self.force(id)?;
                        }
                    }
                    redraw = true;
                }
                SupervisorToWorker::Close | SupervisorToWorker::Exit => {
                    self.shutdown = true;
                }
                SupervisorToWorker::CleanupError { worker, error } => {
                    if let WorkerRole::App(id) = worker.role
                        && let Some(window) = self.windows.get(&id)
                        && window.worker == Some(worker)
                    {
                        state.notify_alert_with_tone(
                            i18n::msg!(
                                "workspace-stop-failed",
                                window = window.launch.title(),
                                reason = error
                            ),
                            ui::NotificationTone::Error,
                        );
                        redraw = true;
                    }
                }
                _ => {}
            }
        }

        let ids = self.windows.keys().copied().collect::<Vec<_>>();
        let mut received = Vec::new();
        for id in ids {
            let window = self.windows.get_mut(&id).expect("live window");
            let Some(stream) = &mut window.stream else {
                continue;
            };
            while let Some(message) = window.queued.front() {
                match stream.queue(message) {
                    Ok(()) => {
                        window.queued.pop_front();
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(_) => break,
                }
            }
            if window.queued.is_empty()
                && let Some(message) = &window.view_requested
            {
                match stream.queue(message) {
                    Ok(()) => window.view_requested = None,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(_) => window.view_requested = None,
                }
            }
            match stream.poll::<AppToShell>() {
                Ok(messages) => received.extend(messages.into_iter().map(|message| (id, message))),
                Err(_) => {
                    window.stream = None;
                    window.queued.clear();
                    self.input_fences.remove(&id);
                    if !matches!(
                        window.close,
                        CloseProgress::Cleaning(_)
                            | CloseProgress::Waiting
                            | CloseProgress::Exiting
                    ) {
                        window.state = ProcessState::Unresponsive;
                    }
                    redraw = true;
                }
            }
        }
        for (id, message) in received {
            redraw |= self.receive(id, message, state)?;
        }
        self.sync_task_catalogs();
        // A modal prompt is user time. Start the timer when work starts, or
        // while a close request has yet to get any response from the app.
        if self.wait_for.is_none()
            && let Some((&id, _)) = self.windows.iter().find(|(_, window)| {
                matches!(
                    window.close,
                    CloseProgress::Requested(at) | CloseProgress::Cleaning(at)
                        if at.elapsed() >= CLOSE_WAIT
                )
            })
        {
            self.show_close_wait(id);
            redraw = true;
        }

        if let Some(command) = state.window_global_request.take() {
            self.pending_global = Some(command);
        }
        if self.pending_global.is_some() || self.pending_update.is_some() || self.reset {
            if let Some((&id, window)) = self.windows.iter().next() {
                if window.close == CloseProgress::Idle && !window.force_when_started {
                    self.request_close(id);
                    redraw = true;
                }
            } else if let Some(command) = self.pending_global.take() {
                state.windows_active = false;
                let platform = platform::native_platform();
                let action = state.apply_routed_event(
                    RoutedEvent {
                        input: InputEvent::Tick,
                        target: RoutedTarget::Global,
                        command,
                    },
                    platform.as_ref(),
                    Instant::now(),
                );
                match action {
                    ShellAction::PowerOff | ShellAction::Reboot => {
                        state.start_auto_admin_power(
                            action == ShellAction::Reboot,
                            Arc::from(platform),
                        );
                    }
                    ShellAction::Exit => self.shutdown = true,
                    ShellAction::Redraw => {}
                }
                redraw = true;
            }
        }
        state.windows_active = self.active();
        Ok(redraw)
    }

    pub(in crate::session) fn launch_pending(
        &mut self,
        state: &mut ShellSession,
    ) -> io::Result<bool> {
        let mut launches = std::mem::take(&mut state.window_launches);
        let toggle_tasks = std::mem::take(&mut state.window_tasks_requested)
            | std::mem::take(&mut self.toggle_tasks_requested);
        let redraw = !launches.is_empty() || toggle_tasks;
        if toggle_tasks && self.pending_global.is_none() {
            self.toggle_tasks(state)?;
        }
        for launch in launches.drain(..) {
            if self.pending_global.is_some() {
                break;
            }
            self.open(launch, state.auth_session())?;
        }
        state.windows_active = self.active();
        Ok(redraw)
    }

    fn open(&mut self, launch: WindowLaunch, auth: Option<&AuthSession>) -> io::Result<SessionId> {
        // The watchdog can receive an older window ID after logout and login.
        static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
        let id = SessionId(NEXT_SESSION.fetch_add(1, Ordering::Relaxed));
        let init = WindowInit {
            launch: serde_json::to_value(&launch)?,
            profile: auth.map(serde_json::to_value).transpose()?,
            size: workspace_protocol::MIN_CONTENT_SIZE,
            layout: self.generation,
            focused: false,
            image_cell_size: self.image_cell_size,
            text_sizing: self.text_sizing,
        };
        WorkerConnection::current()
            .expect("managed workspace")
            .send(WorkerToSupervisor::SpawnApp {
                session: id,
                application: launch.id().into(),
            })?;
        self.cancel_capture();
        self.selection.open(id);
        self.windows.insert(
            id,
            Window {
                launch,
                worker: None,
                stream: None,
                init: Some(init),
                frame: None,
                image_assets: BTreeMap::new(),
                status: WindowStatus::default(),
                state: ProcessState::Starting,
                queued: VecDeque::new(),
                viewport: None,
                close: CloseProgress::Idle,
                force_when_started: false,
                view_requested: None,
                launch_error: false,
            },
        );
        Ok(id)
    }

    pub(super) fn remove(&mut self, id: SessionId) {
        let focused = self.selection.focused() == Some(id);
        self.input_fences.remove(&id);
        self.input_recipients.remove(&id);
        self.task_catalogs.remove(&id);
        self.windows.remove(&id);
        self.layout.panes.retain(|pane| pane.id != id);
        if let Some(taskbar) = &mut self.taskbar {
            taskbar
                .hits
                .retain(|(action, _)| *action != WorkspaceTaskbarAction::Activate(id));
        }
        self.panel_hits.retain(|(action, _)| {
            !matches!(
                action,
                Action::Select(window)
                    | Action::Switch(window)
                    | Action::Show(window)
                    | Action::Background(window)
                    | Action::Maximize(window)
                    | Action::Close(window)
                    | Action::ForceKill(window)
                    if *window == id
            )
        });
        if let Some(menu) = &mut self.split_menu {
            menu.hits.retain(|(action, _)| *action != Action::Show(id));
        }
        self.selection.remove(id);
        if self.split_window == Some(id) {
            self.close_split_menu();
        }
        if let Some((task, previous)) = self.tasks_return
            && task == id
        {
            self.tasks_return = None;
            if focused && self.windows.contains_key(&previous) {
                self.selection.switch(previous);
            }
        }
        self.hide_close_wait(id);
        if let Some(panel) = &mut self.panel
            && panel.selected == Some(id)
        {
            panel.selected = self
                .selection
                .focused()
                .or(self.windows.keys().next().copied());
        }
        self.cancel_capture();
    }

    fn toggle_tasks(&mut self, state: &ShellSession) -> io::Result<()> {
        let focused = self.selection.focused();
        if let Some((&id, _)) = self.windows.iter().find(|(_, window)| {
            window.launch.application()
                == super::window_launch::WindowApplication::Management(ManagementKind::Tasks)
        }) {
            if focused == Some(id) {
                self.request_close(id);
            } else {
                self.selection.switch(id);
                self.tasks_return = focused.map(|previous| (id, previous));
            }
        } else {
            let id = self.open(
                WindowLaunch::Management(state.tasks_query()),
                state.auth_session(),
            )?;
            self.tasks_return = focused.map(|previous| (id, previous));
        }
        Ok(())
    }

    fn receive(
        &mut self,
        id: SessionId,
        message: AppToShell,
        state: &mut ShellSession,
    ) -> io::Result<bool> {
        let Some(window) = self.windows.get_mut(&id) else {
            return Ok(false);
        };
        match message {
            AppToShell::Welcome { worker } if window.worker == Some(worker) => {
                if let Some(mut init) = window.init.take() {
                    if let Some(ShellToApp::Viewport { size, .. }) = window.viewport {
                        init.size = size.unwrap_or(init.size);
                    }
                    window.queued.push_front(ShellToApp::Init(init));
                    if let Some(viewport) = window.viewport.clone() {
                        window.queued.insert(1, viewport);
                    }
                }
            }
            AppToShell::Welcome { .. } => self.force(id)?,
            AppToShell::Frame(mut frame) => {
                if frame.validate().is_err() {
                    self.force(id)?;
                    return Ok(true);
                }
                let mut assets = BTreeMap::<u64, Arc<[u8]>>::new();
                let mut asset_bytes = 0;
                frame.images.retain_mut(|image| {
                    if image.rgba.is_empty() {
                        let Some(pixels) = assets
                            .get(&image.image_id)
                            .or_else(|| window.image_assets.get(&image.image_id))
                        else {
                            return false;
                        };
                        if pixels.len() as u64
                            != u64::from(image.width) * u64::from(image.height) * 4
                        {
                            return false;
                        }
                        image.rgba = pixels.clone();
                    }
                    if !assets.contains_key(&image.image_id) {
                        asset_bytes += image.rgba.len();
                        if asset_bytes > workspace_protocol::MAX_IMAGE_BYTES {
                            return false;
                        }
                        assets.insert(image.image_id, image.rgba.clone());
                    }
                    true
                });
                window.image_assets = assets;
                if frame.layout != self.generation
                    || !self.layout.panes.iter().any(|pane| {
                        pane.id == id
                            && frame.size == Size::new(pane.content.width, pane.content.height)
                    })
                {
                    return Ok(false);
                }
                if window.frame.as_ref() == Some(&frame) {
                    return Ok(false);
                }
                let modal_changed = window
                    .frame
                    .as_ref()
                    .is_some_and(|old| old.modal != frame.modal);
                window.frame = Some(frame);
                if modal_changed
                    && self.capture.is_some_and(|capture| match capture.target {
                        PointerTarget::Pane(target)
                        | PointerTarget::PaneFrame(target)
                        | PointerTarget::WindowButton(target, _) => target == id,
                        _ => false,
                    })
                {
                    self.cancel_capture();
                }
            }
            AppToShell::InputFence { serial } => {
                if self
                    .input_fences
                    .get(&id)
                    .is_some_and(|latest| serial >= *latest)
                {
                    self.input_fences.remove(&id);
                }
            }
            AppToShell::TaskAction { id: task, cancel } => {
                if window.launch.application()
                    == super::window_launch::WindowApplication::Management(ManagementKind::Tasks)
                    && let Some((&owner, _)) = self
                        .windows
                        .iter()
                        .find(|(_, window)| window.status.task_ids.contains(&task))
                    && self.enqueue(owner, ShellToApp::TaskAction { id: task, cancel })
                    && !cancel
                {
                    self.selection.switch(owner);
                    self.cancel_capture();
                }
            }
            AppToShell::ToggleTasks => {
                if self.selection.focused() == Some(id) {
                    self.toggle_tasks(state)?;
                }
            }
            AppToShell::Status(status) => window.update_status(status),
            AppToShell::OpenApp(launch) => {
                if self.selection.focused() == Some(id) && self.pending_global.is_none() {
                    self.open(serde_json::from_value(launch)?, state.auth_session())?;
                }
            }
            AppToShell::BackgroundRequested => {
                self.selection.minimize(id);
                self.cancel_capture();
            }
            AppToShell::CloseRequested => {
                self.request_close(id);
            }
            AppToShell::CloseDeferred => {
                window.close = CloseProgress::Confirmation {
                    size_notice_dismissed: false,
                };
                if self.pending_global.is_some() && self.selection.focused() != Some(id) {
                    self.selection.switch(id);
                }
                self.hide_close_wait(id);
            }
            AppToShell::CloseStarted => {
                window.close = CloseProgress::Cleaning(Instant::now());
            }
            AppToShell::CloseCancelled => {
                window.close = CloseProgress::Idle;
                self.hide_close_wait(id);
                self.pending_global = None;
                self.pending_update = None;
                self.reset = false;
            }
            AppToShell::Closed => {
                window.finish_cleanup();
                self.input_fences.remove(&id);
                self.hide_close_wait(id);
            }
            AppToShell::LaunchFailed(reason) => {
                window.launch_error = true;
                window.close = CloseProgress::Cleaning(Instant::now());
                self.input_fences.remove(&id);
                report_launch_failure(state, reason);
            }
            AppToShell::ShellAction(HostAction::Logout) => {
                self.pending_global = Some(ShellCommand::LogoutToLockscreen);
            }
            AppToShell::ShellAction(HostAction::Restart) => {
                self.pending_global = Some(ShellCommand::Restart);
            }
            AppToShell::ShellAction(HostAction::Exit) => {
                if self.selection.focused() == Some(id) {
                    state.apply_routed_event(
                        RoutedEvent {
                            input: InputEvent::Tick,
                            target: RoutedTarget::Global,
                            command: ShellCommand::RequestExit,
                        },
                        platform::native_platform().as_ref(),
                        Instant::now(),
                    );
                }
            }
            AppToShell::ShellAction(HostAction::PowerOff) => {
                self.pending_global = Some(ShellCommand::PowerOff);
            }
            AppToShell::ShellAction(HostAction::Reboot) => {
                self.pending_global = Some(ShellCommand::Reboot);
            }
            AppToShell::ShellAction(HostAction::Reset) => self.reset = true,
            AppToShell::ShellAction(HostAction::UpdatePrepared(path)) => {
                self.pending_update = Some(path);
                window.close = CloseProgress::Cleaning(Instant::now());
            }
            AppToShell::ShellAction(HostAction::SettingsChanged) => {}
            AppToShell::ShellAction(HostAction::ClockChanged) => {
                if let Some(auth) = state.auth_session().cloned() {
                    state.load_clock_for_session(&auth);
                }
            }
        }
        Ok(true)
    }

    fn sync_task_catalogs(&mut self) {
        let tasks = self
            .windows
            .iter()
            .filter(|(_, window)| {
                window.launch.application()
                    == super::window_launch::WindowApplication::Management(ManagementKind::Tasks)
            })
            .map(|(&id, _)| id)
            .collect::<Vec<_>>();
        if tasks.is_empty() {
            return;
        }
        let ids = self
            .windows
            .values()
            .flat_map(|window| window.status.task_ids.iter().cloned())
            .collect::<Vec<_>>();
        for id in tasks {
            if self.task_catalogs.get(&id) != Some(&ids)
                && self.enqueue(id, ShellToApp::TaskCatalog { ids: ids.clone() })
            {
                self.task_catalogs.insert(id, ids.clone());
            }
        }
    }

    pub(super) fn request_close(&mut self, id: SessionId) {
        let Some(window) = self.windows.get_mut(&id) else {
            return;
        };
        if window.close != CloseProgress::Idle {
            return;
        }
        window.close = CloseProgress::Requested(Instant::now());
        window.queued.push_back(ShellToApp::RequestClose);
        if self.selection.focused() != Some(id) {
            self.selection.activate(id, self.layout.capacity);
        }
        self.panel = None;
        self.close_split_menu();
        self.cancel_capture();
    }

    pub(super) fn force(&mut self, id: SessionId) -> io::Result<()> {
        let Some(window) = self.windows.get_mut(&id) else {
            return Ok(());
        };
        if let Some(worker) = window.worker {
            WorkerConnection::current()
                .expect("managed workspace")
                .send(WorkerToSupervisor::ForceKill { worker })?;
            window.state = ProcessState::ForceStopping;
        } else {
            window.force_when_started = true;
        }
        window.close = CloseProgress::Waiting;
        self.hide_close_wait(id);
        self.cancel_capture();
        Ok(())
    }

    pub(super) fn enqueue(&mut self, id: SessionId, message: ShellToApp) -> bool {
        if let Some(window) = self.windows.get_mut(&id)
            && window.init.is_none()
            && window.stream.is_some()
            && !matches!(
                window.state,
                ProcessState::Unresponsive | ProcessState::ForceStopping
            )
            && !matches!(
                window.close,
                CloseProgress::Cleaning(_) | CloseProgress::Waiting | CloseProgress::Exiting
            )
        {
            if matches!(
                message,
                ShellToApp::Input { .. }
                    | ShellToApp::Back { .. }
                    | ShellToApp::Help { .. }
                    | ShellToApp::Keyboard { .. }
            ) {
                self.input_recipients.insert(id);
            }
            if let Some(viewport) = window.view_requested.take() {
                window.queued.push_back(viewport);
            }
            if let (
                Some(ShellToApp::Input {
                    layout: old_layout,
                    event: old,
                }),
                ShellToApp::Input { layout, event },
            ) = (window.queued.back_mut(), &message)
                && old_layout == layout
                && same_mouse_motion(old, event)
            {
                *old = event.clone();
            } else {
                window.queued.push_back(message);
            }
            true
        } else {
            false
        }
    }
}

fn report_launch_failure(state: &mut ShellSession, reason: String) {
    state.notify_alert_with_tone(
        i18n::msg!("workspace-launch-failed", reason = reason),
        ui::NotificationTone::Error,
    );
}

fn same_mouse_motion(previous: &serde_json::Value, next: &serde_json::Value) -> bool {
    let (Some(previous), Some(next)) = (previous.get("Mouse"), next.get("Mouse")) else {
        return false;
    };
    let (Some(kind), Some(next_kind)) = (previous.get("kind"), next.get("kind")) else {
        return false;
    };
    kind == next_kind
        && (kind.as_str() == Some("Moved") || kind.get("Drag").is_some())
        && previous.get("modifiers") == next.get("modifiers")
}
