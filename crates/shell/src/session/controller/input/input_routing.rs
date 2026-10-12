use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn route_key_input(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        if key.key == InputKey::Escape {
            return self.route_back_key(key);
        }
        if let Some(routed) = self.route_overlay_key(key) {
            return routed;
        }
        if self.active_screen() == ShellScreen::CommandLine {
            return (
                RoutedTarget::Component(ShellComponent::CommandLine),
                if key.phase.is_press_like() {
                    ShellCommand::CommandLineKey(key.clone())
                } else {
                    ShellCommand::Noop
                },
            );
        }

        if !key.phase.is_press_like() {
            if self.active_screen() == ShellScreen::Editor
                && matches!(
                    key.key,
                    InputKey::Left | InputKey::Right | InputKey::Up | InputKey::Down
                )
            {
                return (
                    RoutedTarget::Component(ShellComponent::Editor),
                    ShellCommand::EditorKey(key.clone()),
                );
            }
            return (RoutedTarget::Global, ShellCommand::Noop);
        }

        if key.is_ctrl_c() && self.active_screen() == ShellScreen::Editor {
            return (
                RoutedTarget::Component(ShellComponent::Editor),
                ShellCommand::EditorKey(key.clone()),
            );
        }
        if key.is_ctrl_c() && self.active_screen() == ShellScreen::Management {
            return (
                RoutedTarget::Component(ShellComponent::Management),
                ShellCommand::ManagementKey(key.clone()),
            );
        }

        if key.is_ctrl_c() && self.active_screen() != ShellScreen::Explorer {
            return (RoutedTarget::Global, ShellCommand::Shutdown);
        }

        if key.phase == InputPhase::Press
            && key.key == InputKey::F(6)
            && key.modifiers.is_control()
            && !key.modifiers.alt
            && !key.modifiers.super_key
            && !key.modifiers.hyper
            && !key.modifiers.meta
            && let Some(column) = self.next_table_sort_column(key.modifiers.shift)
        {
            return (RoutedTarget::Global, ShellCommand::SortTable(column));
        }

        if self.active_screen() == ShellScreen::Clock {
            return self.route_clock_key(key);
        }

        if self.active_screen() == ShellScreen::Logs {
            return (
                RoutedTarget::Component(ShellComponent::Logs),
                ShellCommand::LogsKey(key.clone()),
            );
        }
        if self.active_screen() == ShellScreen::Management {
            return (
                RoutedTarget::Component(ShellComponent::Management),
                ShellCommand::ManagementKey(key.clone()),
            );
        }
        if self.active_screen() == ShellScreen::Diagnostics {
            return self.route_diagnostics_key(key);
        }
        if self.active_screen() == ShellScreen::SystemStatus {
            return self.route_system_status_key(key);
        }

        if self.active_screen() == ShellScreen::FirstRunSetup {
            return self.route_setup_key(key);
        }

        if self.active_screen() == ShellScreen::Login {
            return self.route_login_key(key);
        }

        if self.active_screen() == ShellScreen::BootstrapAdmin {
            return self.route_auth_key(key);
        }

        if self.active_screen() == ShellScreen::UserManagement {
            return self.route_user_management_key(key);
        }

        if self.active_screen() == ShellScreen::Explorer {
            return self.route_explorer_key(key);
        }

        if self.active_screen() == ShellScreen::Launcher {
            return self.route_launcher_key(key);
        }

        if self.active_screen() == ShellScreen::Editor {
            return (
                RoutedTarget::Component(ShellComponent::Editor),
                ShellCommand::EditorKey(key.clone()),
            );
        }

        if self.active_screen() == ShellScreen::Settings {
            return (
                RoutedTarget::Component(ShellComponent::Settings),
                ShellCommand::SettingsKey(key.clone()),
            );
        }

        if self.active_screen() == ShellScreen::Home
            && key.phase == InputPhase::Press
            && self.identity_backend != identity::IdentityBackend::Linux
            && self.current_home_username().is_some()
            && key.modifiers.is_control()
            && !key.modifiers.alt
            && !key.modifiers.super_key
            && !key.modifiers.hyper
            && !key.modifiers.meta
            && !key.modifiers.shift
            && matches!(key.key, InputKey::Char('l' | 'L'))
        {
            return (RoutedTarget::Global, ShellCommand::Logout);
        }
        if self.active_screen() == ShellScreen::Home
            && key.phase == InputPhase::Press
            && !key.has_non_shift_modifier()
            && let InputKey::Char(character) = key.key
            && let Some(entry) = self.user_home_entries().into_iter().find(|entry| {
                entry
                    .shortcut()
                    .is_some_and(|shortcut| shortcut.eq_ignore_ascii_case(&character))
            })
        {
            let command = match entry.icon_identity() {
                "explorer" => ShellCommand::OpenExplorer,
                "launcher" => ShellCommand::OpenLauncher,
                "settings" => ShellCommand::OpenSettings,
                "system_status" => ShellCommand::OpenSystemStatus,
                "user_management" | "user_profile" => ShellCommand::OpenUserManagement,
                _ => ShellCommand::Noop,
            };
            return (RoutedTarget::Global, command);
        }

        if self.active_screen() == ShellScreen::Home
            && (key.has_non_shift_modifier()
                || (key.phase != InputPhase::Press
                    && matches!(
                        key.key,
                        InputKey::Char(_) | InputKey::Enter | InputKey::Escape
                    )))
        {
            return (RoutedTarget::Global, ShellCommand::RecordInput);
        }

        if matches!(&key.key, InputKey::BackTab)
            || (matches!(&key.key, InputKey::Tab) && key.modifiers.shift)
        {
            return (RoutedTarget::Global, ShellCommand::FocusPrevious);
        }
        if matches!(&key.key, InputKey::Tab) {
            return (RoutedTarget::Global, ShellCommand::FocusNext);
        }

        match self.active_screen() {
            ShellScreen::Home
                if self.pages.common.focused_component == ShellComponent::HomeLogout
                    && matches!(&key.key, InputKey::Enter | InputKey::Char(' ')) =>
            {
                (
                    RoutedTarget::Component(ShellComponent::HomeLogout),
                    ShellCommand::Logout,
                )
            }
            _ if self.pages.common.focused_component == ShellComponent::ClockButton
                && matches!(&key.key, InputKey::Enter | InputKey::Char(' ')) =>
            {
                (
                    RoutedTarget::Component(ShellComponent::ClockButton),
                    self.clock_button_activation_command(),
                )
            }
            _ if self.pages.common.focused_component == ShellComponent::StatusBar
                && key.phase == InputPhase::Press
                && key.is_unmodified_action_key()
                && matches!(&key.key, InputKey::Enter | InputKey::Char(' ')) =>
            {
                (
                    RoutedTarget::Component(ShellComponent::StatusBar),
                    ShellCommand::Activate {
                        target: ShellComponent::StatusBar,
                        coordinates: (0, 0),
                        click: ClickKind::Single,
                    },
                )
            }
            ShellScreen::Home if matches!(&key.key, InputKey::Left) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeEntryLeft,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Right) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeEntryRight,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Up) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeEntryUp,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Down) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeEntryDown,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Home) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeFirstEntry,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::End) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::HomeLastEntry,
            ),
            ShellScreen::Home if matches!(&key.key, InputKey::Enter | InputKey::Char(' ')) => (
                RoutedTarget::Component(ShellComponent::Home),
                ShellCommand::ActivateSelectedHomeEntry,
            ),
            ShellScreen::Home
                if self.identity_backend != identity::IdentityBackend::Linux
                    && self.current_home_username().is_some()
                    && (key.is_character('l') || key.is_character('L')) =>
            {
                (RoutedTarget::Global, ShellCommand::LogoutToLockscreen)
            }
            ShellScreen::Home if key.is_character('q') || matches!(&key.key, InputKey::Escape) => {
                (RoutedTarget::Global, ShellCommand::RequestExit)
            }
            _ => (
                RoutedTarget::Component(self.pages.common.focused_component),
                ShellCommand::RecordInput,
            ),
        }
    }

    pub(in crate::session) fn route_exit_confirm_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Modal(ShellComponent::ExitDialog);

        if matches!(&key.key, InputKey::BackTab)
            || (matches!(&key.key, InputKey::Tab) && key.modifiers.shift)
        {
            return (target, ShellCommand::FocusPrevious);
        }
        if matches!(&key.key, InputKey::Tab) {
            return (target, ShellCommand::FocusNext);
        }

        if key.is_character('y') || key.is_character('Y') || matches!(&key.key, InputKey::Enter) {
            return (target, ShellCommand::ConfirmExit);
        }

        if key.is_character('r') || key.is_character('R') {
            return (target, ShellCommand::Restart);
        }

        if key.is_character('n') || key.is_character('N') || matches!(&key.key, InputKey::Escape) {
            return (target, ShellCommand::CancelExit);
        }

        (target, ShellCommand::CaptureOverlayInput)
    }

    pub(in crate::session) fn route_popup_key(
        &self,
        key: &KeyInput,
    ) -> (RoutedTarget, ShellCommand) {
        let target = RoutedTarget::Popup(ShellComponent::ContextMenu);

        if self.pages.explorer.explorer_overlay_mode.is_some() {
            return self.route_explorer_overlay_key(key);
        }

        if matches!(&key.key, InputKey::Escape) {
            return (target, ShellCommand::ClosePopup);
        }
        if matches!(&key.key, InputKey::BackTab)
            || (matches!(&key.key, InputKey::Tab) && key.modifiers.shift)
        {
            return (target, ShellCommand::FocusPrevious);
        }
        if matches!(&key.key, InputKey::Tab) {
            return (target, ShellCommand::FocusNext);
        }

        (target, ShellCommand::CaptureOverlayInput)
    }

    pub(in crate::session) fn route_mouse_input(
        &mut self,
        mouse: MouseInput,
        received_at: Instant,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();
        let hit_target = self.hit_map.target_at(coordinates);
        let hit_layer = self.hit_map.layer_at(coordinates);

        if self.active_screen() == ShellScreen::CommandLine
            && self.interactive_overlays().is_empty()
            && hit_target == Some(ShellComponent::BackButton)
            && mouse.kind == ui::MouseEventKind::Down(PointerButton::Left)
        {
            return (RoutedTarget::Global, ShellCommand::RequestCommandLineClose);
        }

        if self.notification_has_active_modal()
            && self.notification_scrollbar_drag.is_some()
            && let Some(routed) = self.route_overlay_mouse(mouse, hit_target, received_at)
        {
            return routed;
        }
        if hit_layer == Some(ShellHitLayer::ShellChrome) && !self.interactive_overlays().is_empty()
        {
            return self.route_shell_chrome_mouse(mouse, hit_target);
        }
        if let Some(routed) = self.route_overlay_mouse(mouse, hit_target, received_at) {
            return routed;
        }

        if !self.notification_has_active_modal()
            && !self.time_sync_dialog_visible
            && self.pages.common.active_popup.is_none()
        {
            if matches!(mouse.kind, ui::MouseEventKind::Down(PointerButton::Left))
                && let Some(column) = self.table_sort_header_at(coordinates)
            {
                return (target_route(hit_target), ShellCommand::SortTable(column));
            }
            if let Some(command) = self.route_touch_pages_pointer(mouse) {
                return (target_route(hit_target), command);
            }
            let coordinates = mouse.coordinates();
            if self.pages.system_status.system_status_widget_drag.is_some() {
                match mouse.kind {
                    ui::MouseEventKind::Drag(PointerButton::Left) => {
                        return (
                            RoutedTarget::Component(ShellComponent::SystemStatus),
                            ShellCommand::SystemStatusWidgetDrag(coordinates),
                        );
                    }
                    ui::MouseEventKind::Up(PointerButton::Left) => {
                        return (
                            RoutedTarget::Component(ShellComponent::SystemStatus),
                            ShellCommand::SystemStatusWidgetDrop(coordinates),
                        );
                    }
                    _ => {}
                }
            }
            if self.pages.launcher.launcher_drag.is_some() {
                match mouse.kind {
                    ui::MouseEventKind::Drag(PointerButton::Left) => {
                        return (
                            RoutedTarget::Component(ShellComponent::Launcher),
                            ShellCommand::LauncherDragUpdate(coordinates),
                        );
                    }
                    ui::MouseEventKind::Up(PointerButton::Left) => {
                        return (
                            RoutedTarget::Component(ShellComponent::Launcher),
                            ShellCommand::LauncherDrop(coordinates),
                        );
                    }
                    _ => {}
                }
            }
            match (self.scrollbar_drag, mouse.kind) {
                (
                    Some(ScrollbarDragState::Explorer { .. }),
                    ui::MouseEventKind::Drag(PointerButton::Left),
                ) => {
                    return (
                        RoutedTarget::Component(ShellComponent::Explorer),
                        ShellCommand::ExplorerDragUpdate(coordinates, mouse.modifiers),
                    );
                }
                (
                    Some(ScrollbarDragState::Explorer { .. }),
                    ui::MouseEventKind::Up(PointerButton::Left),
                ) => {
                    return (
                        RoutedTarget::Component(ShellComponent::Explorer),
                        ShellCommand::ExplorerDrop(coordinates, mouse.modifiers),
                    );
                }
                (
                    Some(ScrollbarDragState::Diagnostics { .. }),
                    ui::MouseEventKind::Drag(PointerButton::Left),
                ) => {
                    return (
                        RoutedTarget::Component(
                            if self.active_screen() == ShellScreen::SystemStatus {
                                ShellComponent::SystemStatus
                            } else {
                                ShellComponent::Diagnostics
                            },
                        ),
                        ShellCommand::DiagnosticsScrollbarDrag(coordinates),
                    );
                }
                (
                    Some(ScrollbarDragState::SystemStatus { .. }),
                    ui::MouseEventKind::Drag(PointerButton::Left),
                ) => {
                    return (
                        RoutedTarget::Component(ShellComponent::SystemStatus),
                        ShellCommand::SystemStatusScrollbarDrag(coordinates),
                    );
                }
                (
                    Some(ScrollbarDragState::SystemStatus { .. }),
                    ui::MouseEventKind::Up(PointerButton::Left),
                ) => {
                    return (
                        RoutedTarget::Component(ShellComponent::SystemStatus),
                        ShellCommand::SystemStatusScrollbarPointerUp,
                    );
                }
                (
                    Some(ScrollbarDragState::Diagnostics { .. }),
                    ui::MouseEventKind::Up(PointerButton::Left),
                ) => {
                    return (
                        RoutedTarget::Component(
                            if self.active_screen() == ShellScreen::SystemStatus {
                                ShellComponent::SystemStatus
                            } else {
                                ShellComponent::Diagnostics
                            },
                        ),
                        ShellCommand::DiagnosticsScrollbarPointerUp,
                    );
                }
                (
                    Some(ScrollbarDragState::Editor { .. }),
                    ui::MouseEventKind::Drag(PointerButton::Left)
                    | ui::MouseEventKind::Up(PointerButton::Left),
                ) => {
                    return (
                        RoutedTarget::Component(ShellComponent::Editor),
                        ShellCommand::EditorPointer(mouse),
                    );
                }
                _ => {}
            }
        }

        if hit_layer == Some(ShellHitLayer::ShellChrome) {
            return self.route_shell_chrome_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::FirstRunSetup {
            return self.route_setup_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::Login {
            return self.route_login_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::Clock {
            return self.route_clock_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::Diagnostics {
            return self.route_diagnostics_mouse(mouse, hit_target);
        }
        if self.active_screen() == ShellScreen::SystemStatus {
            return self.route_system_status_mouse(mouse, hit_target, received_at);
        }

        if self.active_screen() == ShellScreen::UserManagement {
            return self.route_user_management_mouse(mouse, hit_target);
        }

        if self.active_screen() == ShellScreen::Explorer {
            return self.route_explorer_mouse(mouse, hit_target, received_at);
        }

        if self.active_screen() == ShellScreen::Launcher {
            return self.route_launcher_mouse(mouse, received_at);
        }

        if self.active_screen() == ShellScreen::CommandLine {
            return (
                RoutedTarget::Component(ShellComponent::CommandLine),
                ShellCommand::CaptureOverlayInput,
            );
        }

        if self.active_screen() == ShellScreen::Editor {
            return (
                RoutedTarget::Component(ShellComponent::Editor),
                ShellCommand::EditorPointer(mouse),
            );
        }

        if self.active_screen() == ShellScreen::Settings {
            return (
                RoutedTarget::Component(ShellComponent::Settings),
                ShellCommand::SettingsPointer(mouse),
            );
        }
        if self.active_screen() == ShellScreen::Management
            && (hit_target == Some(ShellComponent::Management)
                || self.management_pointer_drag_active())
        {
            return (
                RoutedTarget::Component(ShellComponent::Management),
                ShellCommand::ManagementPointer(mouse),
            );
        }
        if self.active_screen() == ShellScreen::Logs
            && (hit_target == Some(ShellComponent::Logs) || self.logs_pointer_drag_active())
        {
            return (
                RoutedTarget::Component(ShellComponent::Logs),
                ShellCommand::LogsPointer(mouse),
            );
        }

        match mouse.kind {
            ui::MouseEventKind::Moved => {
                (target_route(hit_target), ShellCommand::Hover(hit_target))
            }
            ui::MouseEventKind::Down(PointerButton::Right) => {
                self.last_click = None;
                (
                    target_route(hit_target),
                    ShellCommand::OpenContextMenu {
                        target: hit_target,
                        coordinates,
                    },
                )
            }
            ui::MouseEventKind::Down(button @ PointerButton::Left) => {
                if let Some(target) = hit_target {
                    let click = self.register_click(hit_target, coordinates, button, received_at);
                    if target == ShellComponent::HomeLogout && button == PointerButton::Left {
                        return (RoutedTarget::Component(target), ShellCommand::Logout);
                    }
                    if target == ShellComponent::ClockButton {
                        return (
                            RoutedTarget::Component(target),
                            if button == PointerButton::Left {
                                self.clock_button_activation_command()
                            } else {
                                ShellCommand::Activate {
                                    target,
                                    coordinates,
                                    click,
                                }
                            },
                        );
                    }
                    if self.active_screen() == ShellScreen::Home && target == ShellComponent::Home {
                        return (
                            RoutedTarget::Component(target),
                            ShellCommand::ActivateHomeEntryAt(coordinates, click),
                        );
                    }

                    (
                        RoutedTarget::Component(target),
                        ShellCommand::Activate {
                            target,
                            coordinates,
                            click,
                        },
                    )
                } else {
                    (RoutedTarget::None, ShellCommand::RecordInput)
                }
            }
            _ => (target_route(hit_target), ShellCommand::RecordInput),
        }
    }

    pub(in crate::session) fn route_shell_chrome_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
    ) -> (RoutedTarget, ShellCommand) {
        let target = target_route(hit_target);

        if self.active_screen() == ShellScreen::Explorer
            && matches!(
                mouse.kind,
                ui::MouseEventKind::Down(_)
                    | ui::MouseEventKind::Up(_)
                    | ui::MouseEventKind::Drag(_)
            )
        {
            self.clear_explorer_pointer_capture();
        }

        match mouse.kind {
            ui::MouseEventKind::Moved => (target, ShellCommand::Hover(hit_target)),
            ui::MouseEventKind::Down(PointerButton::Left)
            | ui::MouseEventKind::Click(PointerButton::Left)
                if hit_target == Some(ShellComponent::StatusBar) =>
            {
                (
                    target,
                    ShellCommand::Activate {
                        target: ShellComponent::StatusBar,
                        coordinates: mouse.coordinates(),
                        click: ClickKind::Single,
                    },
                )
            }
            ui::MouseEventKind::Down(PointerButton::Left)
                if hit_target == Some(ShellComponent::ClockButton) =>
            {
                self.last_click = None;
                (target, self.clock_button_activation_command())
            }
            ui::MouseEventKind::Down(_)
            | ui::MouseEventKind::Up(_)
            | ui::MouseEventKind::Click(_)
            | ui::MouseEventKind::DoubleClick(_)
            | ui::MouseEventKind::Drag(_)
            | ui::MouseEventKind::Scroll(_) => (target, ShellCommand::CaptureOverlayInput),
        }
    }

    pub(in crate::session) fn route_popup_mouse(
        &mut self,
        mouse: MouseInput,
        hit_target: Option<ShellComponent>,
        received_at: Instant,
    ) -> (RoutedTarget, ShellCommand) {
        let coordinates = mouse.coordinates();

        if hit_target != Some(ShellComponent::ContextMenu) {
            if matches!(mouse.kind, ui::MouseEventKind::Down(_)) {
                return (RoutedTarget::OutsidePopup, ShellCommand::ClosePopup);
            }

            return (
                RoutedTarget::Popup(ShellComponent::ContextMenu),
                ShellCommand::CaptureOverlayInput,
            );
        }

        match mouse.kind {
            ui::MouseEventKind::Moved => (
                RoutedTarget::Popup(ShellComponent::ContextMenu),
                ShellCommand::Hover(Some(ShellComponent::ContextMenu)),
            ),
            ui::MouseEventKind::Down(PointerButton::Left) => {
                let click = self.register_click(
                    Some(ShellComponent::ContextMenu),
                    coordinates,
                    PointerButton::Left,
                    received_at,
                );
                (
                    RoutedTarget::Popup(ShellComponent::ContextMenu),
                    ShellCommand::Activate {
                        target: ShellComponent::ContextMenu,
                        coordinates,
                        click,
                    },
                )
            }
            _ => (
                RoutedTarget::Popup(ShellComponent::ContextMenu),
                ShellCommand::CaptureOverlayInput,
            ),
        }
    }

    pub(in crate::session) fn register_click(
        &mut self,
        target: Option<ShellComponent>,
        coordinates: CellPosition,
        button: PointerButton,
        received_at: Instant,
    ) -> ClickKind {
        if button != PointerButton::Left {
            self.last_click = None;
            return ClickKind::Single;
        }

        let is_double_click = self
            .last_click
            .map(|last_click| {
                last_click.target == target
                    && coordinates_within_tolerance(last_click.coordinates, coordinates)
                    && received_at
                        .checked_duration_since(last_click.at)
                        .map(|elapsed| elapsed <= DOUBLE_CLICK_INTERVAL)
                        .unwrap_or(false)
            })
            .unwrap_or(false);

        if is_double_click {
            self.last_click = None;
            ClickKind::Double
        } else {
            self.last_click = Some(TimedClick {
                target,
                coordinates,
                at: received_at,
            });
            ClickKind::Single
        }
    }
}

// Share the overlay focus bindings so Shift+Tab and both arrow axes behave alike.

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/input_routing/notification_scroll_input_tests.rs"]
mod notification_scroll_input_tests;
