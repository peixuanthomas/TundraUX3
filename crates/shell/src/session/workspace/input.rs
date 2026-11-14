use super::*;

impl Workspace {
    pub(in crate::session) fn input(
        &mut self,
        input: &InputEvent,
        global_modal: bool,
    ) -> io::Result<bool> {
        if matches!(input, InputEvent::FocusGained | InputEvent::FocusLost) {
            self.terminal_focused = matches!(input, InputEvent::FocusGained);
            self.cancel_capture();
            self.prefix.clear();
            self.last_pointer = None;
            return Ok(false);
        }
        if global_modal
            && self.input_focus.is_some()
            && let Some(chrome) = self.chrome_layout
        {
            self.relayout(chrome.bounds, chrome.main, true);
        }
        if let InputEvent::Key(key) = input {
            let command = self.prefix.route(key);
            if command != WindowShortcut::Pass {
                self.shortcut(command)?;
                return Ok(true);
            }
            if self.panel.is_some() || self.wait_for.is_some() {
                self.overlay_key(key)?;
                return Ok(true);
            }
            if global_modal {
                return Ok(false);
            }
            if self.layout.blocked {
                return Ok(true);
            }
            if self.selection.focused().is_some()
                && self.layout.panes.is_empty()
                && key.phase == InputPhase::Press
                && key.key == InputKey::Escape
                && self.selection.cancel_add()
            {
                return Ok(true);
            }
            if let Some(id) = self.input_focus {
                if key.key == InputKey::F(1)
                    && !key.has_non_shift_modifier()
                    && !self.has_native_input()
                {
                    if key.phase == InputPhase::Press {
                        self.enqueue(
                            id,
                            ShellToApp::Help {
                                layout: self.generation,
                            },
                        );
                    }
                } else {
                    self.send_input(id, input)?;
                }
                return Ok(true);
            }
        }
        if let InputEvent::Mouse(mouse) = input {
            return self.mouse(*mouse, global_modal);
        }
        if matches!(input, InputEvent::Paste(_))
            && !global_modal
            && self.panel.is_none()
            && self.wait_for.is_none()
            && let Some(id) = self.input_focus
        {
            self.send_input(id, input)?;
            return Ok(true);
        }
        Ok(self.panel.is_some() || self.wait_for.is_some() || self.layout.blocked)
    }

    fn shortcut(&mut self, shortcut: WindowShortcut) -> io::Result<()> {
        let focused = self.selection.focused();
        match shortcut {
            WindowShortcut::Pass | WindowShortcut::Consume => {}
            WindowShortcut::SendPrefix(key) => {
                if let Some(id) = self.input_focus {
                    self.send_input(id, &InputEvent::Key(key))?;
                }
            }
            WindowShortcut::Panel => {
                if self.panel.take().is_none() {
                    self.panel = Some(WorkspacePanelViewModel {
                        selected: focused,
                        ..Default::default()
                    });
                    self.panel_key_focus = true;
                }
                self.cancel_capture();
            }
            WindowShortcut::Background => {
                if let Some(id) = focused {
                    self.selection.background(id);
                    self.cancel_capture();
                }
            }
            WindowShortcut::Layout => {
                self.mode = match self.mode {
                    WorkspaceSplitMode::Grid => WorkspaceSplitMode::LeftRight,
                    WorkspaceSplitMode::LeftRight => WorkspaceSplitMode::TopBottom,
                    WorkspaceSplitMode::TopBottom => WorkspaceSplitMode::Grid,
                };
            }
            WindowShortcut::Maximize => {
                if let Some(id) = focused {
                    self.selection.toggle_maximized(id);
                }
            }
            WindowShortcut::MoveFocus(x, y) => self.move_focus(x, y),
            WindowShortcut::Close => {
                if let Some(id) = focused {
                    self.request_close(id);
                }
            }
            WindowShortcut::Kill => {
                if let Some(id) = focused {
                    self.force(id)?;
                }
            }
        }
        Ok(())
    }

    fn move_focus(&mut self, x: i16, y: i16) {
        let Some(current) = self
            .layout
            .panes
            .iter()
            .find(|pane| Some(pane.id) == self.selection.focused())
        else {
            return;
        };
        let (cx, cy) = center(current.content);
        if let Some(next) = self
            .layout
            .panes
            .iter()
            .filter_map(|pane| {
                let (px, py) = center(pane.content);
                let primary = (px - cx) * i32::from(x) + (py - cy) * i32::from(y);
                let cross = ((px - cx) * i32::from(y) + (py - cy) * i32::from(x)).abs();
                (primary > 0).then_some(((cross, primary), pane.id))
            })
            .min_by_key(|(distance, _)| *distance)
        {
            self.selection.focus(next.1);
            self.cancel_capture();
        }
    }

    fn send_input(&mut self, id: SessionId, input: &InputEvent) -> io::Result<()> {
        self.enqueue(
            id,
            ShellToApp::Input {
                layout: self.generation,
                event: serde_json::to_value(input)?,
            },
        );
        Ok(())
    }

    pub(super) fn cancel_capture(&mut self) {
        if let Some(PointerCapture {
            target: PointerTarget::Pane(id),
            generation,
            ..
        }) = self.capture.take()
        {
            self.enqueue(id, ShellToApp::CancelPointer { layout: generation });
        }
    }

    fn target(&self, point: CellPosition, global_modal: bool) -> Option<PointerTarget> {
        if self.panel.is_some() || self.wait_for.is_some() {
            return self
                .panel_hits
                .iter()
                .find(|(_, area)| area.contains(point.into()))
                .map(|(action, _)| PointerTarget::Overlay(*action));
        }
        if let Some(layout) = self.chrome_layout {
            if layout
                .windows_button
                .is_some_and(|area| area.contains(point.into()))
            {
                return Some(PointerTarget::Chrome(ChromeAction::Windows));
            }
            if !global_modal && !self.layout.blocked && self.hides_home() {
                for (area, action) in [
                    (layout.back_button, ChromeAction::Back),
                    (layout.help_button, ChromeAction::Help),
                    (layout.tasks_button, ChromeAction::Tasks),
                    (layout.keyboard_launcher, ChromeAction::Keyboard),
                ] {
                    if area.is_some_and(|area| area.contains(point.into())) {
                        return Some(PointerTarget::Chrome(action));
                    }
                }
            }
        }
        if global_modal {
            return None;
        }
        if self
            .toast_area
            .is_some_and(|area| area.contains(point.into()))
        {
            return None;
        }
        self.layout.panes.iter().find_map(|pane| {
            if pane.content.contains(point.into()) {
                Some(PointerTarget::Pane(pane.id))
            } else if pane.title.is_some_and(|area| area.contains(point.into())) {
                Some(PointerTarget::PaneTitle(pane.id))
            } else {
                None
            }
        })
    }

    fn mouse(&mut self, mouse: MouseInput, global_modal: bool) -> io::Result<bool> {
        let point = mouse.coordinates();
        if self.last_pointer != Some(point) || mouse.kind != ui::MouseEventKind::Moved {
            self.last_pointer = Some(point);
            self.panel_key_focus = false;
        }
        let hit = self.target(point, global_modal);
        match mouse.kind {
            ui::MouseEventKind::Down(button) => {
                self.prefix.clear();
                self.cancel_capture();
                if let Some(target) = hit {
                    if let PointerTarget::Pane(id) = target {
                        self.selection.focus(id);
                        if let Some(layout) = self.chrome_layout {
                            self.relayout(layout.bounds, layout.main, global_modal);
                        }
                        self.send_mouse(id, mouse)?;
                    }
                    self.capture = Some(PointerCapture {
                        target,
                        generation: self.generation,
                        button,
                    });
                    return Ok(true);
                }
            }
            ui::MouseEventKind::Up(button) => {
                if let Some(capture) = self.capture.take() {
                    if capture.generation == self.generation
                        && capture.button == button
                        && hit == Some(capture.target)
                    {
                        if let PointerTarget::Pane(id) = capture.target {
                            self.send_mouse(id, mouse)?;
                        } else if button == PointerButton::Left {
                            self.activate_pointer(capture.target)?;
                        }
                    } else if let PointerTarget::Pane(id) = capture.target {
                        self.enqueue(
                            id,
                            ShellToApp::CancelPointer {
                                layout: capture.generation,
                            },
                        );
                    }
                    return Ok(true);
                }
            }
            ui::MouseEventKind::Drag(button) => {
                if let Some(capture) = self.capture {
                    if capture.generation == self.generation
                        && capture.button == button
                        && hit == Some(capture.target)
                    {
                        if let PointerTarget::Pane(id) = capture.target {
                            self.send_mouse(id, mouse)?;
                        }
                    } else {
                        self.cancel_capture();
                    }
                    return Ok(true);
                }
            }
            ui::MouseEventKind::Scroll(direction) => {
                if self.panel.is_some() && self.wait_for.is_none() {
                    match direction {
                        ScrollDirection::Up => self.apply_action(Action::ScrollUp)?,
                        ScrollDirection::Down => self.apply_action(Action::ScrollDown)?,
                        _ => {}
                    }
                    return Ok(true);
                }
                if let Some(PointerTarget::Pane(id)) = hit {
                    self.send_mouse(id, mouse)?;
                    return Ok(true);
                }
            }
            ui::MouseEventKind::Moved => {
                if let Some(PointerTarget::Pane(id)) = hit
                    && Some(id) == self.input_focus
                {
                    self.send_mouse(id, mouse)?;
                }
            }
            _ => {}
        }
        Ok(hit.is_some()
            || self.panel.is_some()
            || self.wait_for.is_some()
            || (!global_modal && self.layout.blocked)
            || (!global_modal && self.hides_home() && self.layout.available.contains(point.into())))
    }

    fn send_mouse(&mut self, id: SessionId, mut mouse: MouseInput) -> io::Result<()> {
        if let Some(pane) = self.layout.panes.iter().find(|pane| pane.id == id) {
            mouse.position.column -= pane.content.x;
            mouse.position.row -= pane.content.y;
            self.send_input(id, &InputEvent::Mouse(mouse))?;
        }
        Ok(())
    }

    fn activate_pointer(&mut self, target: PointerTarget) -> io::Result<()> {
        match target {
            PointerTarget::Pane(_) => {}
            PointerTarget::PaneTitle(id) => self.selection.focus(id),
            PointerTarget::Overlay(action) => self.apply_action(action)?,
            PointerTarget::Chrome(ChromeAction::Windows) => self.shortcut(WindowShortcut::Panel)?,
            PointerTarget::Chrome(ChromeAction::Tasks) => self.toggle_tasks_requested = true,
            PointerTarget::Chrome(action) => {
                if let Some(id) = self.input_focus {
                    let layout = self.generation;
                    let message = match action {
                        ChromeAction::Back => ShellToApp::Back { layout },
                        ChromeAction::Help => ShellToApp::Help { layout },
                        ChromeAction::Keyboard => ShellToApp::Keyboard { layout },
                        ChromeAction::Windows | ChromeAction::Tasks => unreachable!(),
                    };
                    self.enqueue(id, message);
                }
            }
        }
        Ok(())
    }

    fn overlay_key(&mut self, key: &KeyInput) -> io::Result<()> {
        if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
            return Ok(());
        }
        self.panel_key_focus = true;
        if key.key == InputKey::Escape {
            return self.apply_action(Action::Dismiss);
        }
        let focus = self
            .modal_focus
            .or_else(|| self.panel.as_ref().and_then(|panel| panel.action_focus));
        if key.key == InputKey::Enter || key.key == InputKey::Space {
            if let Some(action) = focus.or_else(|| {
                self.panel
                    .as_ref()
                    .and_then(|panel| panel.selected.map(Action::Switch))
            }) {
                return self.apply_action(action);
            }
        }
        if matches!(key.key, InputKey::Tab | InputKey::BackTab) || self.wait_for.is_some() {
            if matches!(
                key.key,
                InputKey::Tab | InputKey::BackTab | InputKey::Left | InputKey::Right
            ) {
                let current = focus
                    .and_then(|action| self.panel_hits.iter().position(|(hit, _)| *hit == action));
                let length = self.panel_hits.len();
                if length > 0 {
                    let next = if key.key == InputKey::BackTab
                        || key.key == InputKey::Left
                        || key.modifiers.shift
                    {
                        current.map_or(length - 1, |index| (index + length - 1) % length)
                    } else {
                        current.map_or(0, |index| (index + 1) % length)
                    };
                    let action = self.panel_hits[next].0;
                    if self.wait_for.is_some() {
                        self.modal_focus = Some(action);
                    } else if let Some(panel) = self.panel.as_mut() {
                        panel.action_focus = Some(action);
                    }
                }
            }
            return Ok(());
        }
        if matches!(key.key, InputKey::Up | InputKey::Down) && !self.windows.is_empty() {
            let ids = self.windows.keys().copied().collect::<Vec<_>>();
            let panel = self.panel.as_mut().expect("window panel");
            let current = panel
                .selected
                .and_then(|id| ids.iter().position(|&candidate| candidate == id));
            let next = current.map_or(0, |index| {
                if key.key == InputKey::Up {
                    index.saturating_sub(1)
                } else {
                    (index + 1).min(ids.len() - 1)
                }
            });
            panel.action_focus = None;
            if next < panel.scroll {
                panel.scroll = next;
            } else if self.panel_scroll_rows > 0 && next >= panel.scroll + self.panel_scroll_rows {
                panel.scroll = next + 1 - self.panel_scroll_rows;
            }
            self.apply_action(Action::Select(ids[next]))?;
        }
        Ok(())
    }

    fn apply_action(&mut self, action: Action) -> io::Result<()> {
        match action {
            Action::Dismiss => {
                if let Some(id) = self.wait_for.take() {
                    if let Some(window) = self.windows.get_mut(&id) {
                        window.close = CloseProgress::Waiting;
                    }
                    self.modal_focus = None;
                } else {
                    self.panel = None;
                }
            }
            Action::Select(id) => {
                if let Some(panel) = self.panel.as_mut() {
                    panel.selected = Some(id);
                }
            }
            Action::ScrollUp | Action::ScrollDown => {
                if let Some(panel) = self.panel.as_mut() {
                    panel.scroll = if action == Action::ScrollUp {
                        panel.scroll.saturating_sub(1)
                    } else {
                        (panel.scroll + 1).min(self.panel_max_scroll)
                    };
                }
            }
            Action::Add => {
                self.panel = None;
                self.selection.add_from_home();
            }
            Action::SetMode(mode) => {
                self.panel = None;
                self.mode = mode;
            }
            Action::Switch(id) => {
                self.selection.switch(id);
                self.panel = None;
            }
            Action::Show(id) => {
                self.selection.show(id);
                self.panel = None;
            }
            Action::Background(id) => {
                self.selection.background(id);
                self.panel = None;
            }
            Action::Maximize(id) => {
                self.selection.toggle_maximized(id);
                self.panel = None;
            }
            Action::Close(id) => self.request_close(id),
            Action::ForceKill(id) => self.force(id)?,
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::session) fn owns_chrome_target(&self, point: CellPosition) -> bool {
        matches!(self.target(point, false), Some(PointerTarget::Chrome(_)))
    }
}

fn center(area: Rect) -> (i32, i32) {
    (
        i32::from(area.x) * 2 + i32::from(area.width),
        i32::from(area.y) * 2 + i32::from(area.height),
    )
}
