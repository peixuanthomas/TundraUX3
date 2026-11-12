use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn route_launcher_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::Launcher);
        if self.pages.launcher.context_menu.is_some() || self.pages.launcher.entry_editor.is_some()
        {
            return (
                target,
                ShellCommand::LauncherEntryInput(InputEvent::Key(key.clone())),
            );
        }
        if self.pages.launcher.launcher_run_settings.is_some() {
            return (target, ShellCommand::LauncherRunSettingsKey(key.clone()));
        }
        if self.pages.launcher.launcher_pending_confirmation.is_some() {
            if key.phase != InputPhase::Press || key.has_non_shift_modifier() {
                return (target, ShellCommand::CaptureOverlayInput);
            }
            return match key.key {
                InputKey::Tab
                | InputKey::BackTab
                | InputKey::Left
                | InputKey::Right
                | InputKey::Up
                | InputKey::Down => (target, ShellCommand::LauncherToggleConfirmationAction),
                InputKey::Enter | InputKey::Char(' ') if key.is_unmodified_action_key() => (
                    target,
                    if self.pages.launcher.launcher_confirm_selected {
                        ShellCommand::LauncherConfirm
                    } else {
                        ShellCommand::LauncherCancelConfirmation
                    },
                ),
                InputKey::Char('y' | 'Y') => (target, ShellCommand::LauncherConfirm),
                InputKey::Escape | InputKey::Char('n' | 'N') => {
                    (target, ShellCommand::LauncherCancelConfirmation)
                }
                _ => (target, ShellCommand::CaptureOverlayInput),
            };
        }
        if self.pages.launcher.search.state.focused
            || matches!(key.key, InputKey::Tab | InputKey::BackTab)
            || matches!(key.key, InputKey::Char('f' | 'F'))
                && (key.modifiers.is_control() || key.modifiers.super_key)
                && !key.modifiers.alt
        {
            return (
                target,
                ShellCommand::LauncherSearchInput(InputEvent::Key(key.clone())),
            );
        }
        if self.pages.launcher.launcher_drag.is_some() && matches!(key.key, InputKey::Escape) {
            return (target, ShellCommand::LauncherCancelDrag);
        }
        if key.has_non_shift_modifier() {
            return (target, ShellCommand::RecordInput);
        }
        match key.key {
            InputKey::F(2) if key.phase == InputPhase::Press => {
                (target, ShellCommand::LauncherOrganize)
            }
            InputKey::Insert if key.phase == InputPhase::Press => {
                (target, ShellCommand::LauncherAdd)
            }
            InputKey::Escape if self.pages.launcher.organizing => {
                (target, ShellCommand::LauncherOrganize)
            }
            InputKey::Escape => (RoutedTarget::Global, ShellCommand::CloseLauncher),
            InputKey::Left if self.pages.launcher.organizing && key.modifiers.shift => {
                (target, ShellCommand::LauncherMove(-1))
            }
            InputKey::Right if self.pages.launcher.organizing && key.modifiers.shift => {
                (target, ShellCommand::LauncherMove(1))
            }
            InputKey::Left => (target, ShellCommand::LauncherPrevious),
            InputKey::Right => (target, ShellCommand::LauncherNext),
            InputKey::Up => (target, ShellCommand::LauncherUp),
            InputKey::Down => (target, ShellCommand::LauncherDown),
            InputKey::PageUp => (target, ShellCommand::LauncherPageUp),
            InputKey::PageDown => (target, ShellCommand::LauncherPageDown),
            InputKey::Home => (target, ShellCommand::LauncherFirst),
            InputKey::End => (target, ShellCommand::LauncherLast),
            InputKey::Enter | InputKey::Char(' ') if key.phase == InputPhase::Press => (
                target,
                if self.to_launcher_view_model().items.is_empty()
                    && self.pages.launcher.search.value().is_empty()
                {
                    ShellCommand::OpenExplorer
                } else {
                    ShellCommand::LauncherActivate
                },
            ),
            InputKey::Delete if key.phase == InputPhase::Press => {
                (target, ShellCommand::LauncherRemove)
            }
            InputKey::F(4) if key.phase == InputPhase::Press => {
                (target, ShellCommand::LauncherRunSettings)
            }
            InputKey::Char('v' | 'V') if key.phase == InputPhase::Press => {
                (target, ShellCommand::LauncherToggleView)
            }
            InputKey::Char('r' | 'R') | InputKey::F(5) if key.phase == InputPhase::Press => {
                (target, ShellCommand::LauncherRefresh)
            }
            _ => (target, ShellCommand::RecordInput),
        }
    }

    pub(in crate::session) fn route_launcher_mouse(
        &mut self,
        mouse: MouseInput,
        received_at: Instant,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Component(ShellComponent::Launcher);
        let coordinates = mouse.coordinates();
        if self.pages.launcher.context_menu.is_some() || self.pages.launcher.entry_editor.is_some()
        {
            return (
                target,
                ShellCommand::LauncherEntryInput(InputEvent::Mouse(mouse)),
            );
        }
        if self.pages.launcher.launcher_run_settings.is_some() {
            return (target, ShellCommand::LauncherRunSettingsPointer(mouse));
        }
        let confirming = self.pages.launcher.launcher_pending_confirmation.is_some();
        if confirming
            && !matches!(
                mouse.kind,
                ui::MouseEventKind::Down(PointerButton::Left) | ui::MouseEventKind::Moved
            )
        {
            return (target, ShellCommand::CaptureOverlayInput);
        }
        if !confirming
            && matches!(
                mouse.kind,
                ui::MouseEventKind::Down(PointerButton::Left)
                    | ui::MouseEventKind::Drag(PointerButton::Left)
                    | ui::MouseEventKind::Up(PointerButton::Left)
            )
            && self
                .launcher_content_layout()
                .is_some_and(|layout| rect_contains(layout.search, coordinates))
        {
            return (
                target,
                ShellCommand::LauncherSearchInput(InputEvent::Mouse(mouse)),
            );
        }
        match mouse.kind {
            ui::MouseEventKind::Down(PointerButton::Right) => {
                (target, ShellCommand::LauncherContextMenu(coordinates))
            }
            ui::MouseEventKind::Scroll(direction) => {
                let delta = match direction {
                    ScrollDirection::Up => -1,
                    ScrollDirection::Down => 1,
                    _ => 0,
                };
                (target, ShellCommand::LauncherScroll(delta))
            }
            ui::MouseEventKind::Down(PointerButton::Left) => {
                let click = self.register_click(
                    Some(ShellComponent::Launcher),
                    coordinates,
                    PointerButton::Left,
                    received_at,
                );
                (target, ShellCommand::LauncherPointer(coordinates, click))
            }
            ui::MouseEventKind::Click(PointerButton::Left)
            | ui::MouseEventKind::DoubleClick(PointerButton::Left) => {
                (target, ShellCommand::LauncherActivate)
            }
            ui::MouseEventKind::Drag(PointerButton::Left) => {
                (target, ShellCommand::LauncherDragUpdate(coordinates))
            }
            ui::MouseEventKind::Up(PointerButton::Left) => {
                (target, ShellCommand::LauncherDrop(coordinates))
            }
            ui::MouseEventKind::Moved => {
                (target, ShellCommand::Hover(Some(ShellComponent::Launcher)))
            }
            _ => (target, ShellCommand::CaptureOverlayInput),
        }
    }
}

impl ShellSession {
    pub(in crate::session) fn handle_launcher_search_input(
        &mut self,
        input: InputEvent,
        platform: &dyn Platform,
    ) {
        let Some(layout) = self.launcher_content_layout() else {
            return;
        };
        if let InputEvent::Key(key) = &input {
            if !key.phase.is_press_like() {
                return;
            }
            let shortcut =
                (key.modifiers.is_control() || key.modifiers.super_key) && !key.modifiers.alt;
            if shortcut && matches!(key.key, InputKey::Char('f' | 'F')) {
                self.pages.launcher.search.set_focused(true);
                return;
            }
            if !key.has_non_shift_modifier() {
                match key.key {
                    InputKey::Tab | InputKey::BackTab => {
                        let focused = !self.pages.launcher.search.state.focused;
                        self.pages.launcher.search.set_focused(focused);
                        return;
                    }
                    InputKey::Enter | InputKey::Down => {
                        self.pages.launcher.search.set_focused(false);
                        if let Some(index) = self.to_launcher_view_model().selected_index {
                            self.select_launcher_index(index);
                            if key.key == InputKey::Enter && key.phase == InputPhase::Press {
                                self.request_launcher_launch(platform);
                            }
                        }
                        return;
                    }
                    InputKey::Escape => {
                        if self.pages.launcher.search.value().is_empty() {
                            self.pages.launcher.search.set_focused(false);
                            return;
                        }
                        self.pages.launcher.search.set_value("");
                        self.pages.launcher.launcher_viewport_offset = 0;
                        return;
                    }
                    _ => {}
                }
            }
            if shortcut && matches!(key.key, InputKey::Char('c' | 'C' | 'x' | 'X' | 'v' | 'V')) {
                if key.phase != InputPhase::Press {
                    return;
                }
                if matches!(key.key, InputKey::Char('v' | 'V')) {
                    match platform.read_clipboard_text() {
                        Ok(text) => {
                            self.handle_launcher_search_input(InputEvent::Paste(text), platform)
                        }
                        Err(error) => self.notify_status(error.to_string()),
                    }
                    return;
                }
                if let Some(text) = self.pages.launcher.search.selected_text() {
                    match platform.write_clipboard_text(&text) {
                        Ok(()) if matches!(key.key, InputKey::Char('x' | 'X')) => {
                            self.handle_launcher_search_input(
                                InputEvent::key(InputKey::Delete),
                                platform,
                            );
                        }
                        Err(error) => self.notify_status(error.to_string()),
                        _ => {}
                    }
                }
                return;
            }
        }
        let before = self.pages.launcher.search.value().to_owned();
        self.pages.launcher.search.set_focused(true);
        let prefix = ratatui::text::Line::from(format!("{} ", i18n::tr!("ui-launcher-search")))
            .width() as u16;
        let area = Rect::new(
            layout
                .search
                .x
                .saturating_add(prefix.min(layout.search.width)),
            layout.search.y,
            layout.search.width.saturating_sub(prefix),
            layout.search.height,
        );
        self.pages
            .launcher
            .search
            .handle_event_borderless(input, area);
        if before != self.pages.launcher.search.value() {
            self.pages.launcher.launcher_viewport_offset = 0;
            self.pages.launcher.launcher_drag = None;
            if let Some(index) = self.to_launcher_view_model().selected_index {
                self.select_launcher_index(index);
            }
        }
    }
}
