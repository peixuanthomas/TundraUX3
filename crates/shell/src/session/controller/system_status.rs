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
        if self.active_screen() != ShellScreen::SystemStatus {
            self.screen_stack.push(ShellScreen::SystemStatus);
        }
        self.focused_component = ShellComponent::SystemStatus;
        self.system_status_tab = ui::SystemStatusTab::Overview;
        self.system_status_selected_row = 0;
        self.system_status_scroll_offset = 0;
        self.settings_task_runtime.set_system_status_active(true);
    }

    pub(in crate::session) fn close_system_status(&mut self) {
        self.settings_task_runtime.set_system_status_active(false);
        if self.active_screen() == ShellScreen::SystemStatus {
            self.screen_stack.pop();
        }
        if self.screen_stack.is_empty() {
            self.screen_stack.push(ShellScreen::Home);
        }
        self.focused_component = ShellComponent::Home;
    }

    pub(in crate::session) fn refresh_system_status(&mut self) {
        self.system_status_refresh_requested_revision = self
            .app
            .system_status_snapshot()
            .map(|s| s.revision)
            .or(Some(0));
        self.settings_task_runtime.refresh_system_status();
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
