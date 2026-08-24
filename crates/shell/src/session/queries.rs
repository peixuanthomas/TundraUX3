use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::session) enum ShellOverlayCategory {
    ShellModal,
    PageDialog,
    ContextPopup,
    PagePopover,
    Toast,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::session) struct ShellOverlayDescriptor {
    pub kind: ui::MotionOverlayKind,
    pub id: String,
    pub category: ShellOverlayCategory,
    pub target: Option<RoutedTarget>,
}

impl ShellOverlayDescriptor {
    pub fn component(&self) -> Option<ShellComponent> {
        match self.target {
            Some(
                RoutedTarget::Component(component)
                | RoutedTarget::Popup(component)
                | RoutedTarget::Modal(component),
            ) => Some(component),
            _ => None,
        }
    }
}

impl ShellSession {
    pub fn active_screen(&self) -> ShellScreen {
        self.screen_stack
            .last()
            .copied()
            .unwrap_or(ShellScreen::Home)
    }

    pub(in crate::session) fn content_screen(&self) -> ShellScreen {
        self.screen_stack
            .iter()
            .rev()
            .copied()
            .find(|screen| *screen != ShellScreen::ExitConfirm)
            .unwrap_or(ShellScreen::Home)
    }

    pub fn home_mode(&self) -> ShellHomeMode {
        self.home_mode
    }

    pub fn screen_stack(&self) -> &[ShellScreen] {
        &self.screen_stack
    }

    pub fn terminal_size(&self) -> (u16, u16) {
        self.terminal_size
    }

    pub fn tick_count(&self) -> u64 {
        self.tick_count
    }

    pub fn last_key_event(&self) -> Option<&str> {
        self.last_key_event.as_deref()
    }

    pub fn last_mouse_event(&self) -> Option<&str> {
        self.last_mouse_event.as_deref()
    }

    pub fn last_resize_event(&self) -> Option<&str> {
        self.last_resize_event.as_deref()
    }

    pub fn mouse_coordinates(&self) -> Option<(u16, u16)> {
        self.mouse_coordinates
    }

    pub fn shutdown_requested(&self) -> bool {
        self.shutdown_requested
    }

    pub fn restart_requested(&self) -> bool {
        self.restart_requested
    }

    pub fn terminal_flags(&self) -> ShellTerminalFlags {
        self.terminal_flags
    }

    pub fn mouse_scroll_direction(&self) -> Option<&str> {
        self.mouse_scroll_direction.as_deref()
    }

    pub fn mouse_drag_direction(&self) -> Option<&str> {
        self.mouse_drag_direction.as_deref()
    }

    pub fn platform_capability_summary(&self) -> &str {
        &self.platform_capability_summary
    }

    pub fn focused_component(&self) -> ShellComponent {
        self.focused_component
    }

    pub fn selected_home_entry_index(&self) -> usize {
        let count = self.user_home_entries().len();
        if count == 0 {
            0
        } else {
            self.selected_home_entry_index.min(count - 1)
        }
    }

    pub fn hovered_component(&self) -> Option<ShellComponent> {
        self.hovered_component
    }

    pub fn active_popup(&self) -> Option<ShellPopup> {
        self.active_popup
    }

    pub(in crate::session) fn active_overlay_descriptor(&self) -> Option<ShellOverlayDescriptor> {
        let dialog = |id: String, category, component| ShellOverlayDescriptor {
            kind: ui::MotionOverlayKind::Dialog,
            id,
            category,
            target: Some(RoutedTarget::Modal(component)),
        };
        if let Some(notification) = self.to_notification_view_model() {
            return Some(dialog(
                format!("notification:{}", notification.id),
                ShellOverlayCategory::ShellModal,
                self.notification_active_modal_component()?,
            ));
        }
        if self.time_sync_dialog_visible {
            return Some(dialog(
                "time-sync".into(),
                ShellOverlayCategory::ShellModal,
                ShellComponent::TimeSyncDialog,
            ));
        }
        if self.active_screen() == ShellScreen::ExitConfirm {
            return Some(dialog(
                "exit-confirm".into(),
                ShellOverlayCategory::ShellModal,
                ShellComponent::ExitDialog,
            ));
        }
        if let Some(popup) = self.active_popup() {
            return Some(ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Popover,
                id: format!("popup:{popup:?}"),
                category: ShellOverlayCategory::ContextPopup,
                target: Some(RoutedTarget::Popup(ShellComponent::ContextMenu)),
            });
        }
        if let Some(confirmation) = self.launcher_pending_confirmation.as_ref() {
            let id = match confirmation {
                LauncherPendingConfirmation::Launch { id, kind, .. } => {
                    format!("launcher-confirm:launch:{id}:{kind:?}")
                }
                LauncherPendingConfirmation::Remove { ids, .. } => {
                    format!("launcher-confirm:remove:{ids:?}")
                }
            };
            return Some(ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Dialog,
                id,
                category: ShellOverlayCategory::PageDialog,
                target: Some(RoutedTarget::Component(ShellComponent::Launcher)),
            });
        }
        if let Some(menu) = self.editor_open_menu {
            return Some(ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Popover,
                id: format!("editor-open-menu:{menu:?}"),
                category: ShellOverlayCategory::PagePopover,
                target: Some(RoutedTarget::Component(ShellComponent::Editor)),
            });
        }
        if self.editor_quick_menu_anchor.is_some() {
            return Some(ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Popover,
                id: "editor-quick-menu".into(),
                category: ShellOverlayCategory::PagePopover,
                target: Some(RoutedTarget::Component(ShellComponent::Editor)),
            });
        }
        if let Some(mode) = self.explorer_overlay_mode.as_ref() {
            return Some(ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Popover,
                id: format!("explorer:{mode:?}"),
                category: ShellOverlayCategory::PagePopover,
                target: Some(RoutedTarget::Component(ShellComponent::Explorer)),
            });
        }
        if let Some(picker) = self
            .settings_state
            .as_ref()
            .and_then(|settings| settings.picker.as_ref())
        {
            return Some(ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Popover,
                id: format!("settings-picker:{:?}", picker.kind),
                category: ShellOverlayCategory::PagePopover,
                target: Some(RoutedTarget::Component(ShellComponent::Settings)),
            });
        }
        let page_dialog = if self.setup_custom_color_target.is_some() {
            Some(("setup-custom-color", ShellComponent::SetupCustomColorDialog))
        } else if self.clock_create_state.is_some() {
            Some(("clock-create", ShellComponent::ClockCreateInput))
        } else if self.editor_settings_dialog.is_some() {
            Some(("editor-settings", ShellComponent::Editor))
        } else if !self.diagnostics_repair_preview.is_empty() {
            Some((
                "diagnostics-repair",
                ShellComponent::DiagnosticsRepairDialog,
            ))
        } else if self.settings_state.as_ref().is_some_and(|settings| {
            settings.color_editor.is_some()
                || settings.weather_location_editor.is_some()
                || settings.file_extensions_editor.is_some()
                || settings.time_sync_server_editor.is_some()
        }) {
            Some(("settings-editor", ShellComponent::Settings))
        } else {
            None
        };
        if let Some((id, component)) = page_dialog {
            return Some(dialog(
                id.into(),
                ShellOverlayCategory::PageDialog,
                component,
            ));
        }
        let user_management_mode = match &self.user_management_mode {
            UserManagementMode::Browse => None,
            UserManagementMode::Create(_) => Some("user-management:create"),
            UserManagementMode::EditInfo(_) => Some("user-management:edit-info"),
            UserManagementMode::Password(_) => Some("user-management:password"),
        };
        if let Some(id) = user_management_mode {
            return Some(ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Dialog,
                id: id.into(),
                category: ShellOverlayCategory::PageDialog,
                target: Some(RoutedTarget::Component(ShellComponent::UserManagement)),
            });
        }
        let notifications = self.app.notification_center();
        (notifications.alert().is_none())
            .then(|| notifications.toast())
            .flatten()
            .map(|toast| ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Toast,
                id: toast.to_string(),
                category: ShellOverlayCategory::Toast,
                target: None,
            })
    }

    pub fn hit_map(&self) -> &ShellHitMap {
        &self.hit_map
    }

    pub fn hit_map_generation(&self) -> u64 {
        self.hit_map.generation()
    }

    pub fn hit_target_at(&self, coordinates: CellPosition) -> Option<ShellComponent> {
        self.hit_map.target_at(coordinates)
    }

    pub fn last_command(&self) -> Option<&ShellCommand> {
        self.last_command.as_ref()
    }

    pub fn last_routed_target(&self) -> Option<RoutedTarget> {
        self.last_routed_target
    }

    pub(in crate::session) fn home_display_mode(&self) -> ui::HomeDisplayMode {
        if matches!(
            self.content_screen(),
            ShellScreen::FirstRunSetup | ShellScreen::Login | ShellScreen::BootstrapAdmin
        ) {
            return ui::HomeDisplayMode::Auth;
        }

        match self.home_mode {
            ShellHomeMode::Debug => ui::HomeDisplayMode::Debug,
            ShellHomeMode::User => ui::HomeDisplayMode::User,
        }
    }

    pub fn auth_session(&self) -> Option<&AuthSession> {
        self.app.auth_session()
    }

    #[doc(hidden)]
    pub fn login_idle_deadline_for_test(&self) -> Instant {
        self.login_idle_deadline
    }

    #[doc(hidden)]
    pub fn login_password_visible_until_for_test(&self) -> Option<Instant> {
        self.login_password_visible_until
    }

    pub(in crate::session) fn return_to_lockscreen_requested(&self) -> bool {
        self.return_to_lockscreen_requested
    }
}
