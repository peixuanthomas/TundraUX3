use crate::session::overlays::ResolvedExplorerOverlay;
use crate::session::*;

const BUTTON_MAX_PRESS: Duration = Duration::from_millis(500);

impl ShellSession {
    pub(in crate::session) fn button_at(
        &self,
        point: CellPosition,
    ) -> Option<ui::components::ButtonRegion> {
        if self.hit_map.target_at(point) == Some(ShellComponent::BackButton)
            && let Some(area) = self.frame_layout.and_then(|layout| layout.back_button)
        {
            return Some(ui::components::ButtonRegion {
                id: "shell.back".into(),
                area,
                disabled: false,
            });
        }
        if self.auto_admin_visible() {
            return self
                .button_regions
                .iter()
                .rev()
                .find(|button| {
                    button.id.as_str().starts_with("aa.") && rect_contains(button.area, point)
                })
                .cloned();
        }
        if let Some(id) = self.notification_active_modal_id() {
            let prefix = format!("notification.{id}.");
            return self
                .button_regions
                .iter()
                .rev()
                .find(|button| {
                    let notification_action = button.id.as_str().starts_with(&prefix);
                    let modal_back = button.id.as_str() == "shell.back"
                        && self.hit_map.target_at(point) == Some(ShellComponent::BackButton);
                    (notification_action || modal_back) && rect_contains(button.area, point)
                })
                .cloned();
        }
        if self.resolved_overlay_owner() == Some(ShellComponent::Explorer)
            && matches!(
                self.resolved_explorer_overlay(),
                Some(ResolvedExplorerOverlay::Semantic(
                    ExplorerOverlayMode::ContextMenu { .. }
                ))
            )
            && self.explorer_hit_target_at(point).is_none()
        {
            // Covered toolbar buttons must not consume the outside click.
            return None;
        }
        if self.active_screen() == ShellScreen::Logs
            && self.logs_has_active_overlay()
            && self
                .logs_main_area()
                .is_some_and(|main| rect_contains(main, point))
        {
            // A newly opened modal can still have the previous page's button
            // registry until the next frame. Never capture a covered button.
            return self.logs_button_at(point);
        }
        if self.management_overlay_contains(point) {
            return self.management_button_at(point);
        }
        self.management_button_at(point)
            .or_else(|| self.logs_button_at(point))
            .or_else(|| self.table_sort_button_at(point))
            .or_else(|| {
                self.button_regions
                    .iter()
                    .rev()
                    .find(|button| rect_contains(button.area, point))
                    .cloned()
            })
            .or_else(|| {
                if self.hit_map.target_at(point) == Some(ShellComponent::StatusBar) {
                    return self.frame_layout.and_then(|layout| {
                        layout
                            .status_message
                            .filter(|area| rect_contains(*area, point))
                            .map(|area| ui::components::ButtonRegion {
                                id: "shell.status".into(),
                                area,
                                disabled: false,
                            })
                    });
                }
                // Details rows are drawn as a table, but launch on release just
                // like icon buttons. Use the same layout as their hit testing.
                if self.active_screen() != ShellScreen::Launcher
                    || self.hit_map.target_at(point) != Some(ShellComponent::Launcher)
                {
                    return None;
                }
                let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
                let ui::ShellLayout::Full { main, .. } = self.shell_layout_for(area) else {
                    return None;
                };
                let model = self.to_launcher_view_model();
                let layout = ui::launcher_layout(main, &model);
                let Some(ui::LauncherHitTarget::Item(index)) = layout.hit_test(point.0, point.1)
                else {
                    return None;
                };
                let item = model.items.get(index)?;
                Some(ui::components::ButtonRegion {
                    id: format!("launcher.item.{}", item.id).into(),
                    area: layout.items.iter().find(|item| item.index == index)?.area,
                    disabled: item.status != ui::LauncherItemStatus::Ready,
                })
            })
    }

    /// Capture shared buttons before page routing. Existing page actions receive
    /// their press only after a matching release, while text selection and
    /// scrollbars retain their original down/drag/up stream.
    pub(in crate::session) fn prepare_button_input(
        &mut self,
        input: InputEvent,
        received_at: Instant,
    ) -> Option<InputEvent> {
        self.synchronize_overlay_focus();
        // Holding the key that opens Search must not type into the newly focused input.
        match &input {
            InputEvent::Key(key)
                if key.phase == InputPhase::Repeat
                    && self.explorer_search_shortcut_held.as_ref() == Some(&key.key) =>
            {
                return None;
            }
            InputEvent::Key(key) if key.phase == InputPhase::Press => {
                self.explorer_search_shortcut_held = None;
            }
            InputEvent::Key(key) if key.phase == InputPhase::Release => {
                self.explorer_search_shortcut_held = None;
            }
            InputEvent::Mouse(_) | InputEvent::FocusLost | InputEvent::Paste(_) => {
                self.explorer_search_shortcut_held = None;
            }
            _ => {}
        }
        self.update_button_input_mode(&input);
        let InputEvent::Mouse(mouse) = input else {
            if matches!(
                input,
                InputEvent::FocusLost
                    | InputEvent::Resize { .. }
                    | InputEvent::Key(_)
                    | InputEvent::Paste(_)
            ) {
                self.cancel_pointer_gestures_for_modal();
            }
            if matches!(input, InputEvent::FocusLost) {
                self.mouse_coordinates = None;
                self.launcher_drag = None;
                self.editor_cursor_acceleration = None;
            }
            if matches!(input, InputEvent::Resize { .. }) {
                self.button_regions.clear();
            }
            return Some(input);
        };
        self.mouse_coordinates = Some(mouse.coordinates());
        if self.notification_has_active_modal() && self.handle_notification_scrollbar(mouse) {
            self.button_pointer_capture = None;
            return None;
        }
        if self.interactive_overlays().is_empty()
            && (self.handle_home_pointer_scrollbar(&mouse)
                || self.handle_launcher_pointer_scrollbar(&mouse)
                || self.handle_diagnostics_detail_pointer(mouse)
                || self.handle_explorer_locations_pointer(mouse))
        {
            self.button_pointer_capture = None;
            return None;
        }
        match mouse.kind {
            ui::MouseEventKind::Down(PointerButton::Left) => {
                self.button_pointer_capture = None;
                if let Some(region) = self.button_at(mouse.coordinates()) {
                    if region.disabled {
                        return None;
                    }
                    // Notifications already validate releases; launcher cards
                    // need the original stream to support drag and drop.
                    let native_release = region.id.as_str().starts_with("notification.")
                        || region.id.as_str().starts_with("launcher.item.");
                    self.button_pointer_capture = Some(ButtonPointerCapture {
                        region,
                        screen: self.active_screen(),
                        overlay: self
                            .active_overlay_descriptor()
                            .filter(|overlay| overlay.kind != ui::MotionOverlayKind::Toast)
                            .map(|overlay| overlay.id),
                        input: mouse,
                        pressed_at: received_at,
                        native_release,
                        activate_on_release: false,
                    });
                    if !native_release {
                        return None;
                    }
                }
            }
            ui::MouseEventKind::Up(PointerButton::Left) => {
                if let Some(capture) = self.button_pointer_capture.take() {
                    let held = received_at.saturating_duration_since(capture.pressed_at);
                    let matches_button = held <= BUTTON_MAX_PRESS
                        && capture.screen == self.active_screen()
                        && capture.overlay
                            == self
                                .active_overlay_descriptor()
                                .filter(|overlay| overlay.kind != ui::MotionOverlayKind::Toast)
                                .map(|overlay| overlay.id)
                        && self.button_at(mouse.coordinates()).as_ref() == Some(&capture.region);
                    if capture.native_release {
                        if capture.activate_on_release && matches_button {
                            return Some(InputEvent::Mouse(MouseInput {
                                kind: ui::MouseEventKind::Click(PointerButton::Left),
                                ..mouse
                            }));
                        }
                        if !matches_button {
                            self.notification_pointer_capture = None;
                        }
                        return Some(input);
                    }
                    if matches_button {
                        return Some(InputEvent::Mouse(MouseInput {
                            position: mouse.position,
                            ..capture.input
                        }));
                    }
                    return None;
                }
            }
            ui::MouseEventKind::Down(_) => {
                self.button_pointer_capture = None;
                self.notification_pointer_capture = None;
            }
            ui::MouseEventKind::Drag(PointerButton::Left) => {
                if self
                    .button_pointer_capture
                    .as_ref()
                    .is_some_and(|capture| capture.region.id.as_str().starts_with("notification."))
                {
                    self.button_pointer_capture = None;
                }
                if let Some(capture) = &mut self.button_pointer_capture {
                    capture.activate_on_release = false;
                }
                if let Some(capture) = &self.button_pointer_capture
                    && !capture.native_release
                {
                    self.button_pointer_capture = None;
                    return None;
                }
            }
            _ => {}
        }
        Some(input)
    }

    /// Track physical input before a released button becomes a synthetic key.
    /// AA uses this too, although its modal consumes input before page routing.
    pub(in crate::session) fn update_button_input_mode(&mut self, input: &InputEvent) {
        match input {
            InputEvent::Mouse(mouse) => {
                // Some terminals report stationary motion after a key press.
                // It must not erase a keyboard user's visible selection.
                if mouse.kind == ui::MouseEventKind::Move
                    && self.mouse_coordinates == Some(mouse.coordinates())
                {
                    return;
                }
                self.keyboard_focus_visible = false;
                self.button_hover_suppressed = matches!(
                    mouse.kind,
                    ui::MouseEventKind::Up(_)
                        | ui::MouseEventKind::Click(_)
                        | ui::MouseEventKind::DoubleClick(_)
                );
                self.mouse_coordinates = Some(mouse.coordinates());
            }
            InputEvent::FocusLost => {
                self.keyboard_focus_visible = false;
                self.button_hover_suppressed = true;
                self.mouse_coordinates = None;
            }
            InputEvent::Key(key) if key.phase.is_press_like() => {
                if self.auto_admin_visible() {
                    self.keyboard_focus_visible = true;
                    return;
                }
                let (_, command) = self.route_key_input(key);
                if key.phase == InputPhase::Press && command == ShellCommand::BeginExplorerSearch {
                    self.explorer_search_shortcut_held = Some(key.key.clone());
                }
                if !matches!(
                    command,
                    ShellCommand::Noop
                        | ShellCommand::RecordInput
                        | ShellCommand::CaptureOverlayInput
                ) {
                    self.keyboard_focus_visible = true;
                }
            }
            _ => {}
        }
    }

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
                if self.focused_component == ShellComponent::HomeLogout
                    && matches!(&key.key, InputKey::Enter | InputKey::Char(' ')) =>
            {
                (
                    RoutedTarget::Component(ShellComponent::HomeLogout),
                    ShellCommand::Logout,
                )
            }
            _ if self.focused_component == ShellComponent::ClockButton
                && matches!(&key.key, InputKey::Enter | InputKey::Char(' ')) =>
            {
                (
                    RoutedTarget::Component(ShellComponent::ClockButton),
                    self.clock_button_activation_command(),
                )
            }
            _ if self.focused_component == ShellComponent::StatusBar
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
                RoutedTarget::Component(self.focused_component),
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

        if self.explorer_overlay_mode.is_some() {
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
            && self.active_popup.is_none()
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
            if self.system_status_widget_drag.is_some() {
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
            if self.launcher_drag.is_some() {
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
