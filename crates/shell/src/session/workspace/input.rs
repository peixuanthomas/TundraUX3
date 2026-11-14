use super::*;

impl Workspace {
    pub(in crate::session) fn input(
        &mut self,
        input: &InputEvent,
        state: &mut ShellSession,
    ) -> io::Result<bool> {
        if matches!(input, InputEvent::FocusGained | InputEvent::FocusLost) {
            self.terminal_focused = matches!(input, InputEvent::FocusGained);
            self.cancel_capture();
            self.close_split_menu();
            self.taskbar_menu = None;
            self.prefix.clear();
            self.last_pointer = None;
            self.pointer_hovered = false;
            return Ok(false);
        }
        if !state.interactive_overlays().is_empty() {
            self.cancel_capture();
            self.close_split_menu();
            self.taskbar_menu = None;
            self.prefix.clear();
            return Ok(false);
        }
        if self.taskbar_menu.is_some() {
            self.handle_taskbar_menu(input, state)?;
            return Ok(true);
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
            if self.split_window.is_some() {
                self.split_key(key);
                return Ok(true);
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
            return self.mouse(*mouse, state);
        }
        if matches!(input, InputEvent::Paste(_))
            && !self.has_overlay()
            && let Some(id) = self.input_focus
        {
            self.send_input(id, input)?;
            return Ok(true);
        }
        Ok(self.has_overlay() || self.layout.blocked)
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
                self.close_split_menu();
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
                    self.selection.minimize(id);
                    self.close_split_menu();
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
                    self.close_split_menu();
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
                    self.close_split_menu();
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
        if let Some(capture) = self.capture.take() {
            self.suppress_release = Some(capture.button);
            if let PointerTarget::Pane(id) = capture.target {
                self.enqueue(
                    id,
                    ShellToApp::CancelPointer {
                        layout: capture.generation,
                    },
                );
            }
        }
    }

    fn target(&self, point: CellPosition) -> Option<PointerTarget> {
        if self
            .toast_area
            .is_some_and(|area| area.contains(point.into()))
        {
            return None;
        }
        if self.panel.is_some() || self.wait_for.is_some() {
            return self
                .panel_hits
                .iter()
                .find(|(_, area)| area.contains(point.into()))
                .map(|(action, _)| PointerTarget::Overlay(*action));
        }
        if self.split_window.is_some() {
            return self
                .split_menu
                .as_ref()
                .and_then(|menu| menu.hit_at(point))
                .map(PointerTarget::Overlay);
        }
        if let Some(action) = self
            .taskbar
            .as_ref()
            .and_then(|taskbar| taskbar.hit_at(point))
        {
            return Some(PointerTarget::Taskbar(action));
        }
        if let Some(layout) = self.chrome_layout {
            if !self.layout.blocked && self.hides_home() {
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
        if self.layout.blocked {
            return self
                .blocked_pane()
                .and_then(|pane| self.frame_target(&pane, point));
        }
        self.layout.panes.iter().find_map(|pane| {
            if pane.content.contains(point.into()) {
                Some(PointerTarget::Pane(pane.id))
            } else {
                self.frame_target(pane, point)
            }
        })
    }

    fn frame_target(
        &self,
        pane: &WorkspacePaneLayout,
        point: CellPosition,
    ) -> Option<PointerTarget> {
        if !pane.area.contains(point.into()) || pane.content.contains(point.into()) {
            return None;
        }
        match pane.controls.hit_at(point) {
            Some(WorkspaceWindowAction::Close)
                if self.windows[&pane.id].close != CloseProgress::Idle =>
            {
                None
            }
            Some(action) => Some(PointerTarget::WindowButton(pane.id, action)),
            None => Some(PointerTarget::PaneFrame(pane.id)),
        }
    }

    fn mouse(&mut self, mouse: MouseInput, state: &ShellSession) -> io::Result<bool> {
        let point = mouse.coordinates();
        match mouse.kind {
            ui::MouseEventKind::Moved if self.last_pointer != Some(point) => {
                self.panel_key_focus = false;
                self.pointer_hovered = true;
            }
            ui::MouseEventKind::Down(_)
            | ui::MouseEventKind::Up(_)
            | ui::MouseEventKind::Drag(_) => {
                self.panel_key_focus = false;
                self.pointer_hovered = false;
            }
            _ => {}
        }
        self.last_pointer = Some(point);
        let hit = self.target(point);
        match mouse.kind {
            ui::MouseEventKind::Down(button) => {
                self.prefix.clear();
                self.cancel_capture();
                self.suppress_release = None;
                if button == PointerButton::Right
                    && let Some(PointerTarget::Taskbar(
                        WorkspaceTaskbarAction::Activate(id) | WorkspaceTaskbarAction::Close(id),
                    )) = hit
                {
                    self.open_taskbar_menu(id, point, state);
                    return Ok(true);
                }
                if self.split_window.is_some()
                    && self
                        .split_menu
                        .as_ref()
                        .is_none_or(|menu| !menu.area.contains(point.into()))
                {
                    self.close_split_menu();
                    self.suppress_release = Some(button);
                    return Ok(true);
                }
                if let Some(target) = hit {
                    if let PointerTarget::Pane(id) = target {
                        self.selection.focus(id);
                        if let Some(layout) = self.chrome_layout {
                            let generation = self.generation;
                            self.relayout(layout.bounds, layout.main, false);
                            if generation != self.generation && self.target(point) != Some(target) {
                                self.suppress_release = Some(button);
                                return Ok(true);
                            }
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
                if self.suppress_release == Some(button) {
                    self.suppress_release = None;
                    return Ok(true);
                }
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
                if self.suppress_release == Some(button) {
                    return Ok(true);
                }
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
                    self.cancel_capture();
                    match direction {
                        ScrollDirection::Up => self.apply_action(Action::ScrollUp)?,
                        ScrollDirection::Down => self.apply_action(Action::ScrollDown)?,
                        _ => {}
                    }
                    return Ok(true);
                }
                if self.split_window.is_some() {
                    self.cancel_capture();
                    if self
                        .split_menu
                        .as_ref()
                        .is_some_and(|menu| menu.area.contains(point.into()))
                    {
                        match direction {
                            ScrollDirection::Up => self.apply_split_action(Action::ScrollUp),
                            ScrollDirection::Down => self.apply_split_action(Action::ScrollDown),
                            _ => {}
                        }
                    }
                    return Ok(true);
                }
                if self.wait_for.is_some() {
                    return Ok(true);
                }
                if self
                    .taskbar
                    .as_ref()
                    .is_some_and(|taskbar| taskbar.area.contains(point.into()))
                {
                    self.cancel_capture();
                    let action = match direction {
                        ScrollDirection::Up | ScrollDirection::Left => {
                            WorkspaceTaskbarAction::ScrollLeft
                        }
                        ScrollDirection::Down | ScrollDirection::Right => {
                            WorkspaceTaskbarAction::ScrollRight
                        }
                    };
                    self.apply_taskbar_action(action);
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
            || self.has_overlay()
            || self.layout.blocked
            || (self.hides_home() && self.layout.available.contains(point.into())))
    }

    fn send_mouse(&mut self, id: SessionId, mut mouse: MouseInput) -> io::Result<()> {
        if let Some(pane) = self
            .layout
            .panes
            .iter()
            .find(|pane| pane.id == id && pane.content.contains(mouse.coordinates().into()))
        {
            mouse.position.column -= pane.content.x;
            mouse.position.row -= pane.content.y;
            self.send_input(id, &InputEvent::Mouse(mouse))?;
        }
        Ok(())
    }

    fn activate_pointer(&mut self, target: PointerTarget) -> io::Result<()> {
        match target {
            PointerTarget::Pane(_) => {}
            PointerTarget::PaneFrame(id) => self.selection.focus(id),
            PointerTarget::WindowButton(id, action) => match action {
                WorkspaceWindowAction::Minimize => self.selection.minimize(id),
                WorkspaceWindowAction::ToggleMaximize => self.selection.toggle_maximized(id),
                WorkspaceWindowAction::SplitMenu => {
                    self.selection.focus(id);
                    self.panel_key_focus = true;
                    self.split_window = Some(id);
                    self.split_model = WorkspaceSplitMenuViewModel {
                        focused: Some(Action::SetMode(self.mode)),
                        ..Default::default()
                    };
                }
                WorkspaceWindowAction::Close => self.request_close(id),
            },
            PointerTarget::Taskbar(action) => self.apply_taskbar_action(action),
            PointerTarget::Overlay(action) => {
                if self.split_window.is_some() {
                    self.apply_split_action(action);
                } else {
                    self.apply_action(action)?;
                }
            }
            PointerTarget::Chrome(ChromeAction::Tasks) => self.toggle_tasks_requested = true,
            PointerTarget::Chrome(action) => {
                if let Some(id) = self.input_focus {
                    let layout = self.generation;
                    let message = match action {
                        ChromeAction::Back => ShellToApp::Back { layout },
                        ChromeAction::Help => ShellToApp::Help { layout },
                        ChromeAction::Keyboard => ShellToApp::Keyboard { layout },
                        ChromeAction::Tasks => unreachable!(),
                    };
                    self.enqueue(id, message);
                }
            }
        }
        Ok(())
    }

    fn apply_taskbar_action(&mut self, action: WorkspaceTaskbarAction) {
        match action {
            WorkspaceTaskbarAction::Activate(id) => {
                if self.windows.contains_key(&id) {
                    self.selection.activate(id, self.layout.capacity);
                }
            }
            WorkspaceTaskbarAction::Close(id) => self.request_close(id),
            WorkspaceTaskbarAction::ScrollLeft => {
                self.taskbar_scroll = self.taskbar_scroll.saturating_sub(1);
            }
            WorkspaceTaskbarAction::ScrollRight => {
                if let Some(taskbar) = &self.taskbar {
                    self.taskbar_scroll = (self.taskbar_scroll + 1).min(taskbar.max_scroll);
                }
            }
        }
    }

    fn split_key(&mut self, key: &KeyInput) {
        if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
            return;
        }
        self.panel_key_focus = true;
        match key.key {
            InputKey::Escape => self.close_split_menu(),
            InputKey::Enter | InputKey::Space => {
                if let Some(action) = self.split_model.focused
                    && self.split_menu.as_ref().is_some_and(|menu| {
                        menu.hits.iter().any(|(candidate, _)| *candidate == action)
                    })
                {
                    self.apply_split_action(action);
                }
            }
            InputKey::Up | InputKey::Down | InputKey::Tab | InputKey::BackTab => {
                if let Some(menu) = &self.split_menu {
                    self.split_model.focused =
                        adjacent_action(&menu.hits, self.split_model.focused, key);
                }
            }
            _ => {}
        }
    }

    fn apply_split_action(&mut self, action: Action) {
        match action {
            Action::SetMode(mode) => {
                self.mode = mode;
                self.selection.restore_split();
                self.split_model.focused = Some(action);
            }
            Action::Add => {
                self.selection.add_from_home();
                self.close_split_menu();
            }
            Action::Show(id) => {
                if self.windows.contains_key(&id) {
                    self.selection.show(id);
                    self.close_split_menu();
                }
            }
            Action::ScrollUp => self.split_model.scroll = self.split_model.scroll.saturating_sub(1),
            Action::ScrollDown => {
                if let Some(menu) = &self.split_menu {
                    self.split_model.scroll = (self.split_model.scroll + 1).min(menu.max_scroll);
                }
            }
            Action::Dismiss => self.close_split_menu(),
            _ => {}
        }
    }

    fn overlay_key(&mut self, key: &KeyInput) -> io::Result<()> {
        if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
            return Ok(());
        }
        self.panel_key_focus = true;
        if key.key == InputKey::Escape {
            return self.apply_action(Action::Dismiss);
        }
        let focus = if self.wait_for.is_some() {
            self.modal_focus
        } else {
            self.panel.as_ref().and_then(|panel| panel.action_focus)
        }
        .filter(|action| {
            self.panel_hits
                .iter()
                .any(|(candidate, _)| candidate == action)
        });
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
                if let Some(action) = adjacent_action(&self.panel_hits, focus, key) {
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
                if let Some(id) = self.wait_for {
                    if let Some(window) = self.windows.get_mut(&id) {
                        window.close = if matches!(window.close, CloseProgress::Confirmation { .. })
                        {
                            CloseProgress::Confirmation {
                                size_notice_dismissed: true,
                            }
                        } else {
                            CloseProgress::Waiting
                        };
                    }
                    self.hide_close_wait(id);
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
            Action::Switch(id) if self.windows.contains_key(&id) => {
                self.selection.switch(id);
                self.panel = None;
            }
            Action::Show(id) if self.windows.contains_key(&id) => {
                self.selection.show(id);
                self.panel = None;
            }
            Action::Background(id) => {
                self.selection.minimize(id);
                self.panel = None;
            }
            Action::Maximize(id) if self.windows.contains_key(&id) => {
                self.selection.toggle_maximized(id);
                self.panel = None;
            }
            Action::Close(id) => self.request_close(id),
            Action::ForceKill(id) => self.force(id)?,
            _ => {}
        }
        Ok(())
    }

    pub(in crate::session) fn owns_chrome_target(&self, point: CellPosition) -> bool {
        matches!(self.target(point), Some(PointerTarget::Chrome(_)))
    }
}

fn adjacent_action(
    hits: &[(Action, Rect)],
    focus: Option<Action>,
    key: &KeyInput,
) -> Option<Action> {
    let length = hits.len();
    if length == 0 {
        return None;
    }
    let current = focus.and_then(|action| hits.iter().position(|(hit, _)| *hit == action));
    let backwards =
        matches!(key.key, InputKey::BackTab | InputKey::Left | InputKey::Up) || key.modifiers.shift;
    let next = if backwards {
        current.map_or(length - 1, |index| (index + length - 1) % length)
    } else {
        current.map_or(0, |index| (index + 1) % length)
    };
    Some(hits[next].0)
}

fn center(area: Rect) -> (i32, i32) {
    (
        i32::from(area.x) * 2 + i32::from(area.width),
        i32::from(area.y) * 2 + i32::from(area.height),
    )
}
