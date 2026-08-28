use super::super::*;
use system_services::{MetricState, NetworkState, StoragePressure, StorageState};

pub(in crate::session) const SYSTEM_STATUS_WIDGET_KINDS: [storage::SystemStatusWidgetKind; 11] = [
    storage::SystemStatusWidgetKind::SystemOverview,
    storage::SystemStatusWidgetKind::Cpu,
    storage::SystemStatusWidgetKind::Memory,
    storage::SystemStatusWidgetKind::Storage,
    storage::SystemStatusWidgetKind::Network,
    storage::SystemStatusWidgetKind::Temperature,
    storage::SystemStatusWidgetKind::Battery,
    storage::SystemStatusWidgetKind::UptimeLoad,
    storage::SystemStatusWidgetKind::TopProcesses,
    storage::SystemStatusWidgetKind::Diagnostics,
    storage::SystemStatusWidgetKind::Activity,
];

impl ShellSession {
    pub(in crate::session) fn open_system_status(&mut self) {
        let Some(session) = self.app.auth_session() else {
            return;
        };
        if session.role == UserRole::Guest {
            return;
        }
        if self
            .settings_task_runtime
            .set_system_status_active(true)
            .is_err()
        {
            self.notify_status("System Status service unavailable");
            return;
        }
        if self.active_screen() != ShellScreen::SystemStatus {
            self.screen_stack.push(ShellScreen::SystemStatus);
        }
        self.focused_component = ShellComponent::SystemStatus;
        self.system_status_route = ui::SystemStatusRoute::Dashboard;
        self.system_status_tab = ui::SystemStatusTab::Overview;
        self.system_status_dashboard_scroll_row = 0;
        self.system_status_dashboard_draft = None;
        self.system_status_add_picker = None;
        self.system_status_discard_dialog = false;
        self.system_status_discard_confirm_selected = true;
        self.system_status_dashboard_feedback = None;
        self.system_status_widget_drag = None;
        self.ensure_system_status_widget_selection();
        self.system_status_selected_row = 0;
        self.system_status_scroll_offset = 0;
        self.diagnostics_tab = ui::DiagnosticsTab::Health;
        self.diagnostics_list_window_start = 0;
        self.diagnostics_list_window_is_explicit = false;
        self.diagnostics_feedback = None;
        self.diagnostics_restart_required = self.diagnostics_restart_is_required();
        if self.diagnostics_task_runtime.is_some() {
            self.request_diagnostics_scan();
        } else if self.app.diagnostics_snapshot().is_none() {
            self.diagnostics_feedback = Some("Diagnostics runtime is unavailable".to_string());
        }
    }

    pub(in crate::session) fn close_system_status(&mut self) {
        let _ = self.settings_task_runtime.set_system_status_active(false);
        self.clear_system_status_scrollbar_drag();
        self.clear_diagnostics_scrollbar_drag();
        self.diagnostics_repair_preview.clear();
        self.reset_diagnostics_repair_dialog_selection();
        self.system_status_dashboard_draft = None;
        self.system_status_add_picker = None;
        self.system_status_discard_dialog = false;
        self.system_status_discard_confirm_selected = true;
        self.system_status_widget_drag = None;
        if self.active_screen() == ShellScreen::SystemStatus {
            self.screen_stack.pop();
        }
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.focused_component = ShellComponent::Home;
    }

    pub(in crate::session) fn refresh_system_status(&mut self) {
        if self.settings_task_runtime.refresh_system_status().is_ok() {
            self.system_status_refresh_requested_revision = self
                .app
                .system_status_snapshot()
                .map(|s| s.revision)
                .or(Some(0));
        } else {
            self.system_status_refresh_requested_revision = None;
            self.notify_status("System Status service unavailable");
        }
    }

    pub(in crate::session) fn apply_system_status_snapshot(
        &mut self,
        snapshot: app::AppSystemStatusSnapshot,
    ) {
        if self
            .system_status_refresh_requested_revision
            .is_some_and(|revision| snapshot.revision > revision)
        {
            self.system_status_refresh_requested_revision = None;
        }
        self.record_system_status_history(&snapshot);
        self.app.dispatch_at(
            app::AppCommand::SetSystemStatusSnapshot(Some(snapshot.clone())),
            Instant::now(),
        );
        self.evaluate_system_status_alerts(&snapshot);
    }

    pub(in crate::session) fn evaluate_system_status_alerts(
        &mut self,
        snapshot: &app::AppSystemStatusSnapshot,
    ) {
        let Some(role) = self.app.auth_session().map(|s| s.role) else {
            return;
        };
        if role == UserRole::Guest {
            return;
        }
        let volumes = match &snapshot.storage {
            StorageState::Ready(value) => Some(&value.volumes),
            StorageState::Stale { last_good, .. } => Some(&last_good.volumes),
            _ => None,
        };
        if let Some(volumes) = volumes {
            let present = volumes
                .iter()
                .map(|v| v.identifier.clone())
                .collect::<HashSet<_>>();
            for missing in self
                .system_status_storage_alerts
                .keys()
                .filter(|id| !present.contains(*id))
                .cloned()
                .collect::<Vec<_>>()
            {
                self.resolve_notification_alert(&format!("system-status.storage:{missing}"));
                self.system_status_storage_alerts.remove(&missing);
            }
            for volume in volumes {
                let key = format!("system-status.storage:{}", volume.identifier);
                match volume.pressure {
                    StoragePressure::Normal => {
                        self.system_status_storage_alerts.remove(&volume.identifier);
                        self.resolve_notification_alert(&key);
                    }
                    StoragePressure::Unknown => {}
                    StoragePressure::Low | StoragePressure::Critical => {
                        let next = if volume.pressure == StoragePressure::Critical {
                            SystemStatusAlertLevel::Critical
                        } else {
                            SystemStatusAlertLevel::Low
                        };
                        let notify = match self.system_status_storage_alerts.get(&volume.identifier)
                        {
                            None => true,
                            Some(SystemStatusAlertLevel::Low)
                                if next == SystemStatusAlertLevel::Critical =>
                            {
                                true
                            }
                            _ => false,
                        };
                        if notify {
                            let available = volume
                                .available_bytes
                                .map(format_bytes)
                                .unwrap_or_else(|| "unknown".into());
                            let message = if role == UserRole::Admin {
                                format!("Storage {} has {available} available", volume.identifier)
                            } else {
                                format!("Device storage is running low ({available} available)")
                            };
                            let tone = if next == SystemStatusAlertLevel::Critical {
                                ui::NotificationTone::Critical
                            } else {
                                ui::NotificationTone::Warning
                            };
                            self.notify_alert_with_key(&key, message, tone);
                            self.system_status_storage_alerts
                                .insert(volume.identifier.clone(), next);
                        }
                    }
                }
            }
        }
        let link = match &snapshot.network {
            NetworkState::Ready(v) => Some(v.has_active_link),
            NetworkState::Stale { last_good, .. } => Some(last_good.has_active_link),
            _ => None,
        };
        if let Some(link) = link {
            match self.system_status_network_baseline.replace(link) {
                Some(true) if !link => {
                    if !self.system_status_disconnected_notified {
                        self.notify_alert_with_key(
                            "system-status.network",
                            "Network connection was lost",
                            ui::NotificationTone::Warning,
                        );
                        self.system_status_disconnected_notified = true;
                    }
                }
                Some(false) if link => {
                    self.resolve_notification_alert("system-status.network");
                    self.system_status_disconnected_notified = false;
                }
                _ => {}
            }
        }
    }

    pub(in crate::session) fn reset_system_status_trackers(&mut self) {
        for key in self
            .system_status_storage_alerts
            .keys()
            .map(|id| format!("system-status.storage:{id}"))
            .collect::<Vec<_>>()
        {
            self.resolve_notification_alert(&key);
        }
        self.resolve_notification_alert("system-status.network");
        self.system_status_storage_alerts.clear();
        self.system_status_network_baseline = None;
        self.system_status_disconnected_notified = false;
        self.system_status_refresh_requested_revision = None;
        self.system_status_route = ui::SystemStatusRoute::Dashboard;
        self.system_status_selected_widget = None;
        self.system_status_dashboard_scroll_row = 0;
        self.system_status_dashboard_draft = None;
        self.system_status_add_picker = None;
        self.system_status_discard_dialog = false;
        self.system_status_discard_confirm_selected = true;
        self.system_status_dashboard_feedback = None;
        self.system_status_widget_drag = None;
        self.system_status_history = SystemStatusMetricHistory::default();
        self.clear_system_status_scrollbar_drag();
    }

    pub(in crate::session) fn set_system_status_tab(&mut self, tab: ui::SystemStatusTab) {
        let admin = self
            .app
            .auth_session()
            .is_some_and(|session| session.role == UserRole::Admin);
        if !admin
            && matches!(
                tab,
                ui::SystemStatusTab::Storage | ui::SystemStatusTab::Network
            )
        {
            return;
        }
        self.system_status_tab = tab;
        self.system_status_route = match tab {
            ui::SystemStatusTab::Overview => ui::SystemStatusRoute::Dashboard,
            ui::SystemStatusTab::Storage => {
                ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Storage)
            }
            ui::SystemStatusTab::Network => {
                ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Network)
            }
            ui::SystemStatusTab::Health => {
                ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Diagnostics)
            }
            ui::SystemStatusTab::Logs | ui::SystemStatusTab::Incidents => {
                ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Activity)
            }
        };
        self.system_status_selected_row = 0;
        self.system_status_scroll_offset = 0;
        self.clear_system_status_scrollbar_drag();
        self.clear_diagnostics_scrollbar_drag();
        if let Some(diagnostics_tab) = tab.diagnostics_tab() {
            self.diagnostics_tab = diagnostics_tab;
            self.diagnostics_list_window_start = 0;
            self.diagnostics_list_window_is_explicit = false;
            self.clamp_diagnostics_selection();
        }
    }

    pub(in crate::session) fn open_system_status_detail(
        &mut self,
        kind: storage::SystemStatusWidgetKind,
    ) {
        if !self.system_status_widget_detail_allowed(kind) {
            self.system_status_dashboard_feedback = Some(
                if matches!(
                    kind,
                    storage::SystemStatusWidgetKind::Storage
                        | storage::SystemStatusWidgetKind::Network
                        | storage::SystemStatusWidgetKind::TopProcesses
                ) {
                    "Administrator permission is required".to_string()
                } else {
                    "Details are unavailable".to_string()
                },
            );
            return;
        }
        self.system_status_selected_widget = Some(kind);
        self.system_status_route = ui::SystemStatusRoute::Detail(ui_widget_kind(kind).detail());
        self.system_status_selected_row = 0;
        self.system_status_scroll_offset = 0;
        self.clear_system_status_scrollbar_drag();
        match kind {
            storage::SystemStatusWidgetKind::Diagnostics => {
                self.system_status_tab = ui::SystemStatusTab::Health;
                self.diagnostics_tab = ui::DiagnosticsTab::Health;
            }
            storage::SystemStatusWidgetKind::Activity => {
                self.system_status_tab = ui::SystemStatusTab::Logs;
                self.diagnostics_tab = ui::DiagnosticsTab::Logs;
            }
            storage::SystemStatusWidgetKind::Storage => {
                self.system_status_tab = ui::SystemStatusTab::Storage;
            }
            storage::SystemStatusWidgetKind::Network => {
                self.system_status_tab = ui::SystemStatusTab::Network;
            }
            _ => self.system_status_tab = ui::SystemStatusTab::Overview,
        }
    }

    pub(in crate::session) fn back_from_system_status_detail(&mut self) {
        self.system_status_route = ui::SystemStatusRoute::Dashboard;
        self.system_status_tab = ui::SystemStatusTab::Overview;
        self.system_status_selected_row = 0;
        self.system_status_scroll_offset = 0;
        self.clear_system_status_scrollbar_drag();
        self.clear_diagnostics_scrollbar_drag();
    }

    pub(in crate::session) fn begin_system_status_dashboard_edit(&mut self) {
        if !matches!(self.system_status_route, ui::SystemStatusRoute::Dashboard) {
            return;
        }
        self.system_status_dashboard_draft = Some(self.system_status_dashboard_config());
        self.system_status_add_picker = None;
        self.system_status_discard_dialog = false;
        self.system_status_discard_confirm_selected = true;
        self.system_status_dashboard_feedback = Some("Editing dashboard".to_string());
        self.ensure_system_status_widget_selection();
    }

    pub(in crate::session) fn request_cancel_system_status_dashboard_edit(&mut self) {
        if self.system_status_dashboard_draft.is_none() {
            return;
        }
        if self.system_status_dashboard_is_dirty() {
            self.system_status_discard_dialog = true;
            self.system_status_discard_confirm_selected = true;
        } else {
            self.finish_cancel_system_status_dashboard_edit();
        }
    }

    pub(in crate::session) fn finish_cancel_system_status_dashboard_edit(&mut self) {
        self.system_status_dashboard_draft = None;
        self.system_status_add_picker = None;
        self.system_status_discard_dialog = false;
        self.system_status_discard_confirm_selected = true;
        self.system_status_widget_drag = None;
        self.system_status_dashboard_feedback = Some("Dashboard changes cancelled".to_string());
        self.ensure_system_status_widget_selection();
    }

    pub(in crate::session) fn continue_system_status_dashboard_edit(&mut self) {
        self.system_status_discard_dialog = false;
        self.system_status_discard_confirm_selected = true;
    }

    pub(in crate::session) fn save_system_status_dashboard(&mut self) {
        let Some(dashboard) = self.system_status_dashboard_draft.clone() else {
            return;
        };
        let Some(storage) = self.storage_manager.clone() else {
            self.system_status_dashboard_feedback = Some("Storage unavailable".to_string());
            return;
        };
        let Some(actor) = self.app.auth_session().cloned() else {
            self.system_status_dashboard_feedback = Some("Login required".to_string());
            return;
        };
        match UserService::with_debug_policy(storage, self.debug_policy)
            .update_user_system_status_dashboard(&actor, &actor.username, dashboard)
        {
            Ok(account) => {
                self.app.dispatch_at(
                    app::AppCommand::SetActiveSystemStatusDashboard(Some(
                        account.system_status_dashboard,
                    )),
                    Instant::now(),
                );
                self.system_status_dashboard_draft = None;
                self.system_status_add_picker = None;
                self.system_status_discard_dialog = false;
                self.system_status_discard_confirm_selected = true;
                self.system_status_widget_drag = None;
                self.system_status_dashboard_feedback = Some("Saved dashboard".to_string());
            }
            Err(error) => {
                self.system_status_dashboard_feedback = Some(format!(
                    "Could not save dashboard: {}",
                    format_core_error(&error)
                ));
            }
        }
    }

    pub(in crate::session) fn open_system_status_add_picker(&mut self) {
        if self.system_status_dashboard_draft.is_none() {
            return;
        }
        let selected = SYSTEM_STATUS_WIDGET_KINDS
            .iter()
            .position(|kind| self.system_status_picker_kind_enabled(*kind))
            .unwrap_or(0);
        self.system_status_add_picker = Some(SystemStatusAddPickerState { selected });
    }

    pub(in crate::session) fn close_system_status_add_picker(&mut self) {
        self.system_status_add_picker = None;
    }

    pub(in crate::session) fn move_system_status_add_picker(&mut self, delta: isize) {
        let Some(current) = self
            .system_status_add_picker
            .as_ref()
            .map(|picker| picker.selected)
        else {
            return;
        };
        let count = SYSTEM_STATUS_WIDGET_KINDS.len();
        for step in 1..=count {
            let next =
                (current as isize + delta * step as isize).rem_euclid(count as isize) as usize;
            if self.system_status_picker_kind_enabled(SYSTEM_STATUS_WIDGET_KINDS[next]) {
                if let Some(picker) = self.system_status_add_picker.as_mut() {
                    picker.selected = next;
                }
                return;
            }
        }
    }

    pub(in crate::session) fn select_system_status_picker_item(&mut self, index: usize) {
        if index < SYSTEM_STATUS_WIDGET_KINDS.len() {
            if let Some(picker) = self.system_status_add_picker.as_mut() {
                picker.selected = index;
            }
        }
    }

    pub(in crate::session) fn add_selected_system_status_widget(&mut self) {
        let Some(index) = self
            .system_status_add_picker
            .as_ref()
            .map(|picker| picker.selected)
        else {
            return;
        };
        let Some(kind) = SYSTEM_STATUS_WIDGET_KINDS.get(index).copied() else {
            return;
        };
        if !self.system_status_picker_kind_enabled(kind) {
            return;
        }
        if self
            .system_status_dashboard_draft
            .as_mut()
            .is_some_and(|dashboard| dashboard.add_widget(kind))
        {
            self.system_status_selected_widget = Some(kind);
            self.system_status_dashboard_feedback =
                Some(format!("Added {}", ui_widget_kind(kind).label()));
        }
        self.system_status_add_picker = None;
    }

    pub(in crate::session) fn remove_selected_system_status_widget(&mut self) {
        let Some(kind) = self.system_status_selected_widget else {
            return;
        };
        if self
            .system_status_dashboard_draft
            .as_mut()
            .is_some_and(|dashboard| dashboard.remove_widget(kind))
        {
            self.system_status_dashboard_feedback =
                Some(format!("Removed {}", ui_widget_kind(kind).label()));
            self.system_status_selected_widget = None;
            self.ensure_system_status_widget_selection();
        }
    }

    pub(in crate::session) fn cycle_selected_system_status_widget_size(&mut self) {
        let Some(kind) = self.system_status_selected_widget else {
            return;
        };
        let Some((_, layout)) = self.system_status_layout() else {
            return;
        };
        let profile = storage_profile(layout.profile);
        let next = self
            .system_status_dashboard_draft
            .as_ref()
            .and_then(|dashboard| {
                dashboard
                    .layout(profile)
                    .placements
                    .iter()
                    .find(|placement| placement.kind == kind)
            })
            .map(|placement| placement.size.cycle());
        if let (Some(dashboard), Some(next)) = (self.system_status_dashboard_draft.as_mut(), next) {
            dashboard.resize_widget(profile, kind, next);
        }
    }

    pub(in crate::session) fn move_selected_system_status_widget(
        &mut self,
        column_delta: i16,
        row_delta: i16,
    ) {
        let Some(kind) = self.system_status_selected_widget else {
            return;
        };
        let Some((_, layout)) = self.system_status_layout() else {
            return;
        };
        let profile = storage_profile(layout.profile);
        let target = self
            .system_status_dashboard_draft
            .as_ref()
            .and_then(|dashboard| {
                dashboard
                    .layout(profile)
                    .placements
                    .iter()
                    .find(|placement| placement.kind == kind)
            })
            .map(|placement| {
                (
                    u8::try_from((i16::from(placement.column) + column_delta).max(0))
                        .unwrap_or(u8::MAX),
                    u16::try_from((i32::from(placement.row) + i32::from(row_delta)).max(0))
                        .unwrap_or(u16::MAX),
                )
            });
        if let (Some(dashboard), Some((column, row))) =
            (self.system_status_dashboard_draft.as_mut(), target)
        {
            dashboard.move_widget(profile, kind, column, row);
        }
    }

    pub(in crate::session) fn select_system_status_widget_direction(
        &mut self,
        column_delta: i8,
        row_delta: i8,
    ) {
        let Some((_, layout)) = self.system_status_layout() else {
            return;
        };
        let widgets = layout
            .widgets
            .iter()
            .filter(|widget| !widget.preview)
            .collect::<Vec<_>>();
        if widgets.is_empty() {
            self.system_status_selected_widget = None;
            return;
        }
        let current = self
            .system_status_selected_widget
            .map(ui_widget_kind)
            .and_then(|kind| widgets.iter().find(|widget| widget.kind == kind).copied());
        let Some(current) = current else {
            self.system_status_selected_widget = Some(storage_widget_kind(widgets[0].kind));
            return;
        };
        let center = |area: Rect| {
            (
                i32::from(area.x) + i32::from(area.width) / 2,
                i32::from(area.y) + i32::from(area.height) / 2,
            )
        };
        let (cx, cy) = center(current.area);
        let candidate = widgets
            .into_iter()
            .filter(|widget| widget.kind != current.kind)
            .filter_map(|widget| {
                let (x, y) = center(widget.area);
                let dx = x - cx;
                let dy = y - cy;
                let in_direction = match (column_delta, row_delta) {
                    (-1, 0) => dx < 0,
                    (1, 0) => dx > 0,
                    (0, -1) => dy < 0,
                    (0, 1) => dy > 0,
                    _ => false,
                };
                in_direction.then(|| {
                    let primary = if column_delta == 0 {
                        dy.abs()
                    } else {
                        dx.abs()
                    };
                    let secondary = if column_delta == 0 {
                        dx.abs()
                    } else {
                        dy.abs()
                    };
                    ((primary, secondary, y, x), widget.kind)
                })
            })
            .min_by_key(|(score, _)| *score)
            .map(|(_, kind)| storage_widget_kind(kind));
        if let Some(candidate) = candidate {
            self.system_status_selected_widget = Some(candidate);
        }
    }

    pub(in crate::session) fn select_system_status_widget(
        &mut self,
        kind: ui::SystemStatusWidgetKind,
    ) {
        self.system_status_selected_widget = Some(storage_widget_kind(kind));
    }

    pub(in crate::session) fn begin_system_status_widget_drag(
        &mut self,
        kind: ui::SystemStatusWidgetKind,
        coordinates: CellPosition,
    ) {
        if self.system_status_dashboard_draft.is_none() {
            return;
        }
        self.select_system_status_widget(kind);
        let Some((_, layout)) = self.system_status_layout() else {
            return;
        };
        let Some(widget) = layout
            .widgets
            .iter()
            .find(|widget| widget.kind == kind && !widget.preview)
        else {
            return;
        };
        let column_step = (widget
            .area
            .width
            .saturating_sub(widget.size.cols().saturating_sub(1))
            / widget.size.cols().max(1))
        .saturating_add(1)
        .max(1);
        let grab_column = coordinates
            .0
            .saturating_sub(widget.area.x)
            .checked_div(column_step)
            .unwrap_or(0)
            .min(widget.size.cols().saturating_sub(1));
        let grab_row = coordinates
            .1
            .saturating_sub(widget.area.y)
            .checked_div(ui::LOGICAL_ROW_HEIGHT + ui::LOGICAL_ROW_GAP)
            .unwrap_or(0)
            .min(widget.size.rows().saturating_sub(1));
        self.system_status_widget_drag = Some(SystemStatusWidgetDragState {
            kind: storage_widget_kind(kind),
            profile: storage_profile(layout.profile),
            origin: self
                .system_status_dashboard_draft
                .clone()
                .expect("dashboard edit draft checked"),
            grab_column,
            grab_row,
        });
    }

    pub(in crate::session) fn update_system_status_widget_drag(
        &mut self,
        coordinates: CellPosition,
    ) {
        let Some(drag) = self.system_status_widget_drag.clone() else {
            return;
        };
        let Some((_, layout)) = self.system_status_layout() else {
            return;
        };
        let grid_width = layout
            .canvas
            .width
            .saturating_sub(if layout.scrollbar.is_some() { 2 } else { 0 });
        let cell_width = grid_width.saturating_sub(layout.column_count.saturating_sub(1))
            / layout.column_count.max(1);
        let column_step = cell_width.saturating_add(1).max(1);
        let pointer_column = coordinates
            .0
            .saturating_sub(layout.canvas.x)
            .checked_div(column_step)
            .unwrap_or(0)
            .min(layout.column_count.saturating_sub(1));
        let pointer_row = layout.visible_row_start.saturating_add(
            coordinates
                .1
                .saturating_sub(layout.canvas.y)
                .checked_div(ui::LOGICAL_ROW_HEIGHT + ui::LOGICAL_ROW_GAP)
                .unwrap_or(0),
        );
        let mut next = drag.origin;
        next.move_widget(
            drag.profile,
            drag.kind,
            u8::try_from(pointer_column.saturating_sub(drag.grab_column)).unwrap_or(u8::MAX),
            pointer_row.saturating_sub(drag.grab_row),
        );
        self.system_status_dashboard_draft = Some(next);
    }

    pub(in crate::session) fn finish_system_status_widget_drag(
        &mut self,
        coordinates: CellPosition,
    ) {
        self.update_system_status_widget_drag(coordinates);
        self.system_status_widget_drag = None;
    }

    pub(in crate::session) fn system_status_dashboard_is_dirty(&self) -> bool {
        self.system_status_dashboard_draft
            .as_ref()
            .is_some_and(|draft| *draft != self.system_status_dashboard_baseline())
    }

    pub(in crate::session) fn system_status_picker_kind_enabled(
        &self,
        kind: storage::SystemStatusWidgetKind,
    ) -> bool {
        self.system_status_widget_allowed(kind)
            && !self
                .system_status_dashboard_config()
                .widgets
                .contains(&kind)
            && self.system_status_widget_unavailable_reason(kind).is_none()
    }

    pub(in crate::session) fn system_status_widget_unavailable_reason(
        &self,
        kind: storage::SystemStatusWidgetKind,
    ) -> Option<String> {
        if !self.system_status_widget_allowed(kind) {
            return Some("Administrator permission is required".to_string());
        }
        let metrics = &self.app.system_status_snapshot()?.metrics;
        match kind {
            storage::SystemStatusWidgetKind::Temperature => match &metrics.thermal {
                MetricState::Unavailable { reason } => Some(reason.clone()),
                _ => None,
            },
            storage::SystemStatusWidgetKind::Battery => match &metrics.batteries {
                MetricState::Unavailable { reason } => Some(reason.clone()),
                _ => None,
            },
            _ => None,
        }
    }

    pub(in crate::session) fn system_status_dashboard_config(
        &self,
    ) -> storage::SystemStatusDashboardConfig {
        self.system_status_dashboard_draft
            .clone()
            .unwrap_or_else(|| self.system_status_dashboard_baseline())
    }

    fn system_status_dashboard_baseline(&self) -> storage::SystemStatusDashboardConfig {
        self.app
            .active_system_status_dashboard()
            .cloned()
            .unwrap_or_else(|| {
                storage::SystemStatusDashboardConfig::for_role(
                    self.app
                        .auth_session()
                        .map(|session| session.role.as_str())
                        .unwrap_or("User"),
                )
            })
    }

    pub(in crate::session) fn system_status_widget_allowed(
        &self,
        kind: storage::SystemStatusWidgetKind,
    ) -> bool {
        let role = self.app.auth_session().map(|session| session.role);
        role.is_some_and(|role| role != UserRole::Guest)
            && (!matches!(kind, storage::SystemStatusWidgetKind::TopProcesses)
                || role == Some(UserRole::Admin))
    }

    pub(in crate::session) fn system_status_widget_detail_allowed(
        &self,
        kind: storage::SystemStatusWidgetKind,
    ) -> bool {
        self.system_status_widget_allowed(kind)
            && (!matches!(
                kind,
                storage::SystemStatusWidgetKind::Storage
                    | storage::SystemStatusWidgetKind::Network
                    | storage::SystemStatusWidgetKind::TopProcesses
            ) || self
                .app
                .auth_session()
                .is_some_and(|session| session.role == UserRole::Admin))
    }

    fn ensure_system_status_widget_selection(&mut self) {
        let dashboard = self.system_status_dashboard_config();
        let selected_valid = self.system_status_selected_widget.is_some_and(|selected| {
            dashboard.widgets.contains(&selected) && self.system_status_widget_allowed(selected)
        });
        if !selected_valid {
            self.system_status_selected_widget = dashboard
                .widgets
                .into_iter()
                .find(|kind| self.system_status_widget_allowed(*kind));
        }
    }

    fn record_system_status_history(&mut self, snapshot: &app::AppSystemStatusSnapshot) {
        let MetricState::Ready(uptime) = &snapshot.metrics.uptime else {
            return;
        };
        if self.system_status_history.last_uptime_seconds == Some(uptime.seconds) {
            return;
        }
        self.system_status_history.last_uptime_seconds = Some(uptime.seconds);
        if let MetricState::Ready(cpu) = &snapshot.metrics.cpu {
            push_system_status_history(
                &mut self.system_status_history.cpu,
                cpu.usage_percent.round().clamp(0.0, 100.0) as u64,
            );
        }
        if let MetricState::Ready(memory) = &snapshot.metrics.memory {
            let percentage = if memory.total_bytes == 0 {
                0
            } else {
                memory.used_bytes.saturating_mul(100) / memory.total_bytes
            };
            push_system_status_history(&mut self.system_status_history.memory, percentage);
        }
        if let MetricState::Ready(network) = &snapshot.metrics.network_io {
            push_system_status_history(
                &mut self.system_status_history.network_received,
                network.total_received_bytes_per_second.max(0.0).round() as u64,
            );
            push_system_status_history(
                &mut self.system_status_history.network_transmitted,
                network.total_transmitted_bytes_per_second.max(0.0).round() as u64,
            );
        }
        if let MetricState::Ready(sensors) = &snapshot.metrics.thermal {
            if let Some(hottest) = sensors
                .iter()
                .map(|sensor| sensor.temperature_celsius)
                .filter(|temperature| temperature.is_finite())
                .max_by(f32::total_cmp)
            {
                push_system_status_history(
                    &mut self.system_status_history.temperature,
                    (hottest.max(0.0) * 10.0).round() as u64,
                );
            }
        }
    }

    fn system_status_layout(&self) -> Option<(ui::SystemStatusViewModel, ui::SystemStatusLayout)> {
        let ui::ShellLayout::Full { main, .. } =
            ui::compute_shell_layout(Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1))
        else {
            return None;
        };
        let model = self.to_system_status_view_model()?;
        let layout = ui::system_status_layout(main, &model);
        Some((model, layout))
    }

    pub(in crate::session) fn scroll_system_status(&mut self, delta: i8) {
        let Some((model, layout)) = self.system_status_layout() else {
            return;
        };
        if matches!(self.system_status_route, ui::SystemStatusRoute::Dashboard) {
            let content_rows = model
                .dashboard
                .widgets(layout.profile)
                .iter()
                .map(|widget| widget.row.saturating_add(widget.size.rows()))
                .max()
                .unwrap_or(0);
            let visible_rows = layout
                .visible_row_end
                .saturating_sub(layout.visible_row_start);
            let maximum = content_rows.saturating_sub(visible_rows);
            self.system_status_dashboard_scroll_row = if delta < 0 {
                layout
                    .visible_row_start
                    .saturating_sub(u16::from(delta.unsigned_abs()))
            } else {
                layout
                    .visible_row_start
                    .saturating_add(u16::from(delta as u8))
                    .min(maximum)
            };
            return;
        }
        let max = model.item_count().saturating_sub(layout.visible_capacity);
        self.system_status_scroll_offset = if delta < 0 {
            layout
                .visible_start
                .saturating_sub(delta.unsigned_abs() as usize)
        } else {
            layout.visible_start.saturating_add(delta as usize).min(max)
        };
        if layout.visible_capacity > 0 {
            self.system_status_selected_row = self.system_status_selected_row.clamp(
                self.system_status_scroll_offset,
                (self.system_status_scroll_offset + layout.visible_capacity - 1)
                    .min(model.item_count().saturating_sub(1)),
            );
        }
    }

    pub(in crate::session) fn begin_system_status_scrollbar_drag(
        &mut self,
        coordinates: CellPosition,
    ) {
        let Some((model, layout)) = self.system_status_layout() else {
            return;
        };
        let Some(track) = layout.scrollbar else {
            return;
        };
        let (content_length, viewport_length, viewport_start) =
            if matches!(self.system_status_route, ui::SystemStatusRoute::Dashboard) {
                (
                    model
                        .dashboard
                        .widgets(layout.profile)
                        .iter()
                        .map(|widget| widget.row.saturating_add(widget.size.rows()))
                        .max()
                        .unwrap_or(0) as usize,
                    layout
                        .visible_row_end
                        .saturating_sub(layout.visible_row_start) as usize,
                    layout.visible_row_start as usize,
                )
            } else {
                (
                    model.item_count(),
                    layout.visible_capacity,
                    layout.visible_start,
                )
            };
        let scrollbar =
            ui::components::Scrollbar::new(content_length, viewport_length, viewport_start);
        let (relative_start, thumb_len) = scrollbar.thumb_range(track);
        let thumb_start = track.y.saturating_add(relative_start);
        let thumb_end = thumb_start.saturating_add(thumb_len);
        let grab_offset = if coordinates.1 >= thumb_start && coordinates.1 < thumb_end {
            coordinates.1 - thumb_start
        } else {
            thumb_len / 2
        };
        self.scrollbar_drag = Some(ScrollbarDragState::SystemStatus { grab_offset });
        self.drag_system_status_scrollbar(coordinates);
    }

    pub(in crate::session) fn drag_system_status_scrollbar(&mut self, coordinates: CellPosition) {
        let Some(ScrollbarDragState::SystemStatus { grab_offset }) = self.scrollbar_drag else {
            return;
        };
        let Some((model, layout)) = self.system_status_layout() else {
            return;
        };
        let Some(track) = layout.scrollbar else {
            self.clear_system_status_scrollbar_drag();
            return;
        };
        let dashboard = matches!(self.system_status_route, ui::SystemStatusRoute::Dashboard);
        let (content_length, viewport_length, viewport_start) = if dashboard {
            (
                model
                    .dashboard
                    .widgets(layout.profile)
                    .iter()
                    .map(|widget| widget.row.saturating_add(widget.size.rows()))
                    .max()
                    .unwrap_or(0) as usize,
                layout
                    .visible_row_end
                    .saturating_sub(layout.visible_row_start) as usize,
                layout.visible_row_start as usize,
            )
        } else {
            (
                model.item_count(),
                layout.visible_capacity,
                layout.visible_start,
            )
        };
        let scrollbar =
            ui::components::Scrollbar::new(content_length, viewport_length, viewport_start);
        let (_, thumb_len) = scrollbar.thumb_range(track);
        let next = scrollbar_window_start(
            coordinates.1,
            grab_offset,
            track.y,
            track.height,
            thumb_len,
            content_length,
            viewport_length,
        );
        if dashboard {
            self.system_status_dashboard_scroll_row = u16::try_from(next).unwrap_or(u16::MAX);
            return;
        }
        self.system_status_scroll_offset = next;
        if layout.visible_capacity > 0 {
            self.system_status_selected_row = self.system_status_selected_row.clamp(
                self.system_status_scroll_offset,
                (self.system_status_scroll_offset + layout.visible_capacity - 1)
                    .min(model.item_count().saturating_sub(1)),
            );
        }
    }

    pub(in crate::session) fn clear_system_status_scrollbar_drag(&mut self) -> bool {
        if matches!(
            self.scrollbar_drag,
            Some(ScrollbarDragState::SystemStatus { .. })
        ) {
            self.scrollbar_drag = None;
            true
        } else {
            false
        }
    }
}

fn push_system_status_history(history: &mut VecDeque<u64>, value: u64) {
    const HISTORY_LIMIT: usize = 60;
    if history.len() == HISTORY_LIMIT {
        history.pop_front();
    }
    history.push_back(value);
}

pub(in crate::session) const fn ui_widget_kind(
    kind: storage::SystemStatusWidgetKind,
) -> ui::SystemStatusWidgetKind {
    match kind {
        storage::SystemStatusWidgetKind::SystemOverview => {
            ui::SystemStatusWidgetKind::SystemOverview
        }
        storage::SystemStatusWidgetKind::Cpu => ui::SystemStatusWidgetKind::Cpu,
        storage::SystemStatusWidgetKind::Memory => ui::SystemStatusWidgetKind::Memory,
        storage::SystemStatusWidgetKind::Storage => ui::SystemStatusWidgetKind::Storage,
        storage::SystemStatusWidgetKind::Network => ui::SystemStatusWidgetKind::Network,
        storage::SystemStatusWidgetKind::Temperature => ui::SystemStatusWidgetKind::Temperature,
        storage::SystemStatusWidgetKind::Battery => ui::SystemStatusWidgetKind::Battery,
        storage::SystemStatusWidgetKind::UptimeLoad => ui::SystemStatusWidgetKind::UptimeLoad,
        storage::SystemStatusWidgetKind::TopProcesses => ui::SystemStatusWidgetKind::TopProcesses,
        storage::SystemStatusWidgetKind::Diagnostics => ui::SystemStatusWidgetKind::Diagnostics,
        storage::SystemStatusWidgetKind::Activity => ui::SystemStatusWidgetKind::Activity,
    }
}

pub(in crate::session) const fn storage_widget_kind(
    kind: ui::SystemStatusWidgetKind,
) -> storage::SystemStatusWidgetKind {
    match kind {
        ui::SystemStatusWidgetKind::SystemOverview => {
            storage::SystemStatusWidgetKind::SystemOverview
        }
        ui::SystemStatusWidgetKind::Cpu => storage::SystemStatusWidgetKind::Cpu,
        ui::SystemStatusWidgetKind::Memory => storage::SystemStatusWidgetKind::Memory,
        ui::SystemStatusWidgetKind::Storage => storage::SystemStatusWidgetKind::Storage,
        ui::SystemStatusWidgetKind::Network => storage::SystemStatusWidgetKind::Network,
        ui::SystemStatusWidgetKind::Temperature => storage::SystemStatusWidgetKind::Temperature,
        ui::SystemStatusWidgetKind::Battery => storage::SystemStatusWidgetKind::Battery,
        ui::SystemStatusWidgetKind::UptimeLoad => storage::SystemStatusWidgetKind::UptimeLoad,
        ui::SystemStatusWidgetKind::TopProcesses => storage::SystemStatusWidgetKind::TopProcesses,
        ui::SystemStatusWidgetKind::Diagnostics => storage::SystemStatusWidgetKind::Diagnostics,
        ui::SystemStatusWidgetKind::Activity => storage::SystemStatusWidgetKind::Activity,
    }
}

pub(in crate::session) const fn ui_widget_size(
    size: storage::SystemStatusWidgetSize,
) -> ui::SystemStatusWidgetSize {
    match size {
        storage::SystemStatusWidgetSize::Small => ui::SystemStatusWidgetSize::Small,
        storage::SystemStatusWidgetSize::Wide => ui::SystemStatusWidgetSize::Wide,
        storage::SystemStatusWidgetSize::Large => ui::SystemStatusWidgetSize::Large,
    }
}

pub(in crate::session) const fn storage_profile(
    profile: ui::SystemStatusDashboardProfile,
) -> storage::DashboardProfile {
    match profile {
        ui::SystemStatusDashboardProfile::Wide => storage::DashboardProfile::Wide,
        ui::SystemStatusDashboardProfile::Narrow => storage::DashboardProfile::Narrow,
    }
}

pub(in crate::session) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
