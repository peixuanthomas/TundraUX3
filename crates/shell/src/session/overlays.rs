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
    pub immediate: bool,
}

impl ShellOverlayDescriptor {
    fn dialog(id: String, category: ShellOverlayCategory, component: ShellComponent) -> Self {
        Self {
            kind: ui::MotionOverlayKind::Dialog,
            id,
            category,
            target: Some(RoutedTarget::Modal(component)),
            immediate: false,
        }
    }

    pub(super) fn focus_order(&self) -> Vec<ShellComponent> {
        match self.component() {
            Some(ShellComponent::ClockCreateInput) => vec![
                ShellComponent::ClockCreateInput,
                ShellComponent::ClockCreateAlarmButton,
                ShellComponent::ClockCreateCountdownButton,
            ],
            Some(component) => vec![component],
            None => Vec::new(),
        }
    }

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::session) enum ResolvedExplorerOverlay {
    RestoreConflict,
    OperationConflict,
    Input(ExplorerInputMode),
    Semantic(ExplorerOverlayMode),
    PendingDialog(app::explorer::ExplorerDialogKind),
}

impl ResolvedExplorerOverlay {
    pub(in crate::session) fn descriptor(self) -> ShellOverlayDescriptor {
        let (kind, id, category) = match self {
            Self::RestoreConflict => (
                ui::MotionOverlayKind::Dialog,
                "explorer-restore-conflict",
                ShellOverlayCategory::PageDialog,
            ),
            Self::OperationConflict => (
                ui::MotionOverlayKind::Dialog,
                "explorer-operation-conflict",
                ShellOverlayCategory::PageDialog,
            ),
            Self::Input(mode) => (
                ui::MotionOverlayKind::Dialog,
                match mode {
                    ExplorerInputMode::NewFolder => "explorer-input:new-folder",
                    ExplorerInputMode::NewTextFile => "explorer-input:new-text-file",
                    ExplorerInputMode::Rename => "explorer-input:rename",
                    ExplorerInputMode::RestoreDestination => "explorer-input:restore-destination",
                    _ => unreachable!("only rendered Explorer inputs resolve as overlays"),
                },
                ShellOverlayCategory::PageDialog,
            ),
            Self::Semantic(mode) => (
                ui::MotionOverlayKind::Popover,
                match mode {
                    ExplorerOverlayMode::ContextMenu { .. } => "explorer-popover:context-menu",
                    ExplorerOverlayMode::Sort { .. } => "explorer-popover:sort",
                    ExplorerOverlayMode::Options => "explorer-popover:options",
                    ExplorerOverlayMode::Properties => "explorer-popover:properties",
                },
                ShellOverlayCategory::PagePopover,
            ),
            Self::PendingDialog(kind) => (
                ui::MotionOverlayKind::Dialog,
                match kind {
                    app::explorer::ExplorerDialogKind::DeleteToTrash => {
                        "explorer-dialog:delete-to-trash"
                    }
                    app::explorer::ExplorerDialogKind::DumpTrash => "explorer-dialog:dump-trash",
                },
                ShellOverlayCategory::PageDialog,
            ),
        };
        ShellOverlayDescriptor {
            kind,
            id: id.into(),
            category,
            target: Some(RoutedTarget::Component(ShellComponent::Explorer)),
            immediate: false,
        }
    }
}

impl ShellSession {
    /// Bottom to top, shared by focus, routing, hit testing and animation.
    /// Only the current content page contributes page-owned overlays.
    pub(super) fn interactive_overlays(&self) -> Vec<ShellOverlayDescriptor> {
        let mut overlays = Vec::new();
        if let Some(page) = self.page_overlay_descriptor() {
            overlays.push(page);
        }
        let dialog = ShellOverlayDescriptor::dialog;
        if let Some(notification) = self.to_notification_view_model() {
            if let Some(component) = self.notification_active_modal_component() {
                let mut descriptor = dialog(
                    format!("notification:{}", notification.id),
                    ShellOverlayCategory::ShellModal,
                    component,
                );
                let key = self
                    .app
                    .notification_center()
                    .active_modal()
                    .and_then(|modal| modal.key.as_deref());
                descriptor.immediate = notification.tone == ui::NotificationTone::Critical
                    && key != Some(EXIT_CONFIRM_NOTIFICATION_KEY);
                overlays.push(descriptor);
            }
        } else if self.time_sync_dialog_visible {
            overlays.push(dialog(
                "time-sync".into(),
                ShellOverlayCategory::ShellModal,
                ShellComponent::TimeSyncDialog,
            ));
        } else if self.active_screen() == ShellScreen::ExitConfirm {
            overlays.push(dialog(
                "exit-confirm".into(),
                ShellOverlayCategory::ShellModal,
                ShellComponent::ExitDialog,
            ));
        }
        if self.auto_admin_visible() {
            overlays.push(ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Dialog,
                id: "auto-admin".into(),
                category: ShellOverlayCategory::ShellModal,
                target: Some(RoutedTarget::Global),
                immediate: false,
            });
        }
        overlays
    }

    pub(in crate::session) fn active_overlay_descriptor(&self) -> Option<ShellOverlayDescriptor> {
        self.interactive_overlays().pop().or_else(|| {
            let notifications = self.app.notification_center();
            (notifications.alert().is_none())
                .then(|| notifications.toast_expires_at())
                .flatten()
                .map(|deadline| ShellOverlayDescriptor {
                    kind: ui::MotionOverlayKind::Toast,
                    id: format!("toast:{deadline:?}"),
                    category: ShellOverlayCategory::Toast,
                    target: None,
                    immediate: false,
                })
        })
    }

    pub(in crate::session) fn resolved_overlay_owner(&self) -> Option<ShellComponent> {
        self.active_overlay_descriptor()
            .and_then(|overlay| overlay.component())
    }

    pub(in crate::session) fn resolved_explorer_overlay(&self) -> Option<ResolvedExplorerOverlay> {
        if self.content_screen() != ShellScreen::Explorer {
            return None;
        }
        let explorer = self.app.explorer_state()?;
        if explorer.pending_restore.is_some() {
            return Some(ResolvedExplorerOverlay::RestoreConflict);
        }
        if explorer.pending_conflict.is_some() {
            return Some(ResolvedExplorerOverlay::OperationConflict);
        }
        if matches!(
            self.explorer_input_mode,
            ExplorerInputMode::NewFolder
                | ExplorerInputMode::NewTextFile
                | ExplorerInputMode::Rename
                | ExplorerInputMode::RestoreDestination
        ) {
            return Some(ResolvedExplorerOverlay::Input(self.explorer_input_mode));
        }
        if let Some(mode) = self.explorer_overlay_mode {
            return Some(ResolvedExplorerOverlay::Semantic(mode));
        }
        explorer
            .pending_dialog
            .as_ref()
            .map(|dialog| ResolvedExplorerOverlay::PendingDialog(dialog.kind))
    }
}

impl ShellSession {
    fn page_overlay_descriptor(&self) -> Option<ShellOverlayDescriptor> {
        let dialog = ShellOverlayDescriptor::dialog;
        if let Some(overlay) = self.resolved_explorer_overlay() {
            return Some(overlay.descriptor());
        }
        if let Some(popup) = self.active_popup() {
            return Some(ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Popover,
                id: format!("popup:{:?}", popup.owner),
                category: ShellOverlayCategory::ContextPopup,
                target: Some(RoutedTarget::Popup(ShellComponent::ContextMenu)),
                immediate: false,
            });
        }
        let additional = match self.content_screen() {
            ShellScreen::Logs => self.logs_overlay_id().map(|id| (id, ShellComponent::Logs)),
            ShellScreen::Management => self
                .management_overlay_id()
                .map(|id| (id, ShellComponent::Management)),
            ShellScreen::Editor if self.config_editor_form_visible() => self
                .management_overlay_id()
                .map(|id| (id, ShellComponent::Editor)),
            ShellScreen::SystemStatus if self.system_status_discard_dialog => {
                Some(("system-status-discard", ShellComponent::SystemStatus))
            }
            ShellScreen::SystemStatus if self.system_status_add_picker.is_some() => {
                Some(("system-status-add", ShellComponent::SystemStatus))
            }
            ShellScreen::SystemStatus if self.system_status_size_picker.is_some() => {
                Some(("system-status-size", ShellComponent::SystemStatus))
            }
            _ => None,
        };
        if let Some((id, component)) = additional {
            return Some(dialog(
                id.into(),
                ShellOverlayCategory::PageDialog,
                component,
            ));
        }
        if self.content_screen() == ShellScreen::Launcher
            && let Some(confirmation) = self.launcher_pending_confirmation.as_ref()
        {
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
                immediate: false,
            });
        }
        if self.content_screen() == ShellScreen::Editor
            && let Some(menu) = self.editor_open_menu
        {
            return Some(ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Popover,
                id: format!("editor-open-menu:{menu:?}"),
                category: ShellOverlayCategory::PagePopover,
                target: Some(RoutedTarget::Component(ShellComponent::Editor)),
                immediate: false,
            });
        }
        if self.content_screen() == ShellScreen::Editor && self.editor_quick_menu_anchor.is_some() {
            return Some(ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Popover,
                id: "editor-quick-menu".into(),
                category: ShellOverlayCategory::PagePopover,
                target: Some(RoutedTarget::Component(ShellComponent::Editor)),
                immediate: false,
            });
        }
        if self.content_screen() == ShellScreen::Settings
            && let Some(picker) = self
                .settings_state
                .as_ref()
                .and_then(|settings| settings.picker.as_ref())
        {
            return Some(ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Popover,
                id: format!("settings-picker:{:?}", picker.kind),
                category: ShellOverlayCategory::PagePopover,
                target: Some(RoutedTarget::Component(ShellComponent::Settings)),
                immediate: false,
            });
        }
        let page_dialog = if self.content_screen() == ShellScreen::FirstRunSetup
            && self.setup_custom_color_target.is_some()
        {
            Some(("setup-custom-color", ShellComponent::SetupCustomColorDialog))
        } else if self.content_screen() == ShellScreen::Clock && self.clock_create_state.is_some() {
            Some(("clock-create", ShellComponent::ClockCreateInput))
        } else if self.content_screen() == ShellScreen::Editor
            && self.editor_settings_dialog.is_some()
        {
            Some(("editor-settings", ShellComponent::Editor))
        } else if matches!(
            self.content_screen(),
            ShellScreen::Diagnostics | ShellScreen::SystemStatus
        ) && !self.diagnostics_repair_preview.is_empty()
        {
            Some((
                "diagnostics-repair",
                ShellComponent::DiagnosticsRepairDialog,
            ))
        } else if self.content_screen() == ShellScreen::Settings
            && let Some(settings) = self.settings_state.as_ref()
        {
            if let Some(editor) = settings.color_editor.as_ref() {
                Some((
                    match editor.kind {
                        ui::SettingsPickerKind::BorderColor => "settings-editor:color:border",
                        ui::SettingsPickerKind::AccentColor => "settings-editor:color:accent",
                        _ => "settings-editor:color:other",
                    },
                    ShellComponent::Settings,
                ))
            } else if settings.weather_location_editor.is_some() {
                Some(("settings-editor:weather-location", ShellComponent::Settings))
            } else if settings.file_extensions_editor.is_some() {
                Some(("settings-editor:file-extensions", ShellComponent::Settings))
            } else if settings.time_sync_server_editor.is_some() {
                Some(("settings-editor:time-sync-server", ShellComponent::Settings))
            } else {
                None
            }
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
        };
        if self.content_screen() == ShellScreen::UserManagement
            && let Some(id) = user_management_mode
        {
            return Some(ShellOverlayDescriptor {
                kind: ui::MotionOverlayKind::Dialog,
                id: id.into(),
                category: ShellOverlayCategory::PageDialog,
                target: Some(RoutedTarget::Component(ShellComponent::UserManagement)),
                immediate: false,
            });
        }
        None
    }
}
