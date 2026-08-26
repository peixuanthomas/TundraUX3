use super::super::*;
use system_services::{NetworkState, StoragePressure, StorageState};

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
        self.system_status_tab = ui::SystemStatusTab::Overview;
        self.system_status_selected_row = 0;
        self.system_status_scroll_offset = 0;
    }

    pub(in crate::session) fn close_system_status(&mut self) {
        let _ = self.settings_task_runtime.set_system_status_active(false);
        self.clear_system_status_scrollbar_drag();
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
        self.clear_system_status_scrollbar_drag();
    }

    pub(in crate::session) fn set_system_status_tab(&mut self, tab: ui::SystemStatusTab) {
        self.system_status_tab = tab;
        self.system_status_selected_row = 0;
        self.system_status_scroll_offset = 0;
        self.clear_system_status_scrollbar_drag();
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
        let scrollbar = ui::components::Scrollbar::new(
            model.item_count(),
            layout.visible_capacity,
            layout.visible_start,
        );
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
        let scrollbar = ui::components::Scrollbar::new(
            model.item_count(),
            layout.visible_capacity,
            layout.visible_start,
        );
        let (_, thumb_len) = scrollbar.thumb_range(track);
        self.system_status_scroll_offset = scrollbar_window_start(
            coordinates.1,
            grab_offset,
            track.y,
            track.height,
            thumb_len,
            model.item_count(),
            layout.visible_capacity,
        );
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
