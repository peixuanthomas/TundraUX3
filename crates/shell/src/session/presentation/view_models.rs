use super::super::controller::system_status::format_bytes;
use super::super::*;
impl ShellSession {
    pub fn to_system_status_view_model(&self) -> Option<ui::SystemStatusViewModel> {
        use system_services::*;
        let role = self.app.auth_session()?.role;
        if role == UserRole::Guest {
            return None;
        }
        let mut diagnostics = self.to_diagnostics_view_model();
        match self.system_status_route {
            ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Diagnostics) => {
                diagnostics.tab = ui::DiagnosticsTab::Health;
            }
            ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Activity) => {
                diagnostics.tab = self.diagnostics_tab;
            }
            _ => {}
        }
        let snapshot = self.app.system_status_snapshot();
        let (storage_state, storage) = match snapshot.map(|s| &s.storage) {
            Some(StorageState::Ready(v)) => (ui::SystemStatusSectionState::Ready, Some(v)),
            Some(StorageState::Stale { last_good, error }) => (
                ui::SystemStatusSectionState::Stale {
                    message: error.clone(),
                },
                Some(last_good),
            ),
            Some(StorageState::Unavailable { reason }) => (
                ui::SystemStatusSectionState::Unavailable {
                    message: reason.clone(),
                },
                None,
            ),
            _ => (ui::SystemStatusSectionState::Loading, None),
        };
        let pressure = storage
            .map(|s| s.overall_pressure)
            .unwrap_or(StoragePressure::Unknown);
        let system_volume =
            storage.and_then(|s| s.system_volume_index.and_then(|i| s.volumes.get(i)));
        let usage = system_volume
            .map(volume_usage)
            .unwrap_or_else(|| "Unknown".into());
        let system_volume_used_percentage = system_volume
            .and_then(used_percentage)
            .map(|percentage| percentage.round().clamp(0.0, 100.0) as u8);
        let refreshed = snapshot
            .and_then(successful_system_status_sampled_at)
            .map(format_sample_age)
            .unwrap_or_else(|| "Not yet".into());
        let (network_status, network_tone) = match snapshot.map(|s| &s.network) {
            None | Some(NetworkState::Loading) => {
                ("Loading".into(), ui::components::ComponentTone::Muted)
            }
            Some(NetworkState::Unavailable { .. }) => {
                ("Unavailable".into(), ui::components::ComponentTone::Muted)
            }
            Some(NetworkState::Ready(n)) => (
                if n.has_active_link {
                    "Connected"
                } else {
                    "Disconnected"
                }
                .into(),
                if n.has_active_link {
                    ui::components::ComponentTone::Success
                } else {
                    ui::components::ComponentTone::Warning
                },
            ),
            Some(NetworkState::Stale { last_good, .. }) => (
                format!(
                    "{} (stale)",
                    if last_good.has_active_link {
                        "Connected"
                    } else {
                        "Disconnected"
                    }
                ),
                ui::components::ComponentTone::Warning,
            ),
        };
        let dashboard = self.to_system_status_dashboard_view_model(
            role,
            snapshot,
            storage,
            &diagnostics,
            &refreshed,
        );
        if role == UserRole::User {
            let usage = if storage
                .is_some_and(|s| s.system_volume_source == SystemVolumeSource::FixedVolumeFallback)
            {
                format!("{usage} (source unknown)")
            } else {
                usage
            };
            let (storage_status, storage_tone) = match snapshot.map(|s| &s.storage) {
                None | Some(StorageState::Loading) => {
                    ("Loading".into(), ui::components::ComponentTone::Muted)
                }
                Some(StorageState::Unavailable { .. }) => {
                    ("Unavailable".into(), ui::components::ComponentTone::Muted)
                }
                Some(StorageState::Ready(_)) => {
                    (pressure_label(pressure).into(), pressure_tone(pressure))
                }
                Some(StorageState::Stale { .. }) => (
                    format!("{} (stale)", pressure_label(pressure)),
                    pressure_tone(pressure),
                ),
            };
            return Some(ui::SystemStatusViewModel {
                content: ui::SystemStatusContentViewModel::User(ui::UserSystemStatusViewModel {
                    storage_status,
                    storage_tone,
                    system_volume_usage: usage,
                    system_volume_used_percentage,
                    network_status,
                    network_tone,
                    last_refreshed: refreshed,
                }),
                diagnostics,
                route: self.system_status_route,
                dashboard,
                selected_row: self.system_status_selected_row,
                scroll_offset: self.system_status_scroll_offset,
                refreshing: self.system_status_refresh_requested_revision.is_some(),
                feedback: None,
            });
        }
        let (network_state, network) = match snapshot.map(|s| &s.network) {
            Some(NetworkState::Ready(v)) => (ui::SystemStatusSectionState::Ready, Some(v)),
            Some(NetworkState::Stale { last_good, error }) => (
                ui::SystemStatusSectionState::Stale {
                    message: error.clone(),
                },
                Some(last_good),
            ),
            Some(NetworkState::Unavailable { reason }) => (
                ui::SystemStatusSectionState::Unavailable {
                    message: reason.clone(),
                },
                None,
            ),
            _ => (ui::SystemStatusSectionState::Loading, None),
        };
        let fallback_index = storage
            .filter(|s| s.system_volume_source == SystemVolumeSource::FixedVolumeFallback)
            .and_then(|s| s.system_volume_index);
        let usage = if fallback_index.is_some() {
            format!("{usage} (fixed-volume fallback; source unknown)")
        } else {
            usage
        };
        let storage_rows = storage
            .map(|s| {
                s.volumes
                    .iter()
                    .enumerate()
                    .map(|(index, v)| ui::StorageVolumeRowViewModel {
                        volume: v.identifier.clone(),
                        kind: format!("{:?}", v.kind),
                        system_volume: if Some(index) == fallback_index {
                            "Fallback (source unknown)"
                        } else if v.is_system {
                            "Yes"
                        } else {
                            "No"
                        }
                        .into(),
                        access: format!("{:?}", v.access),
                        usage: volume_usage(v),
                        used_percentage: used_percentage(v)
                            .map(|p| format!("{p:.1}%"))
                            .unwrap_or_else(|| "Unknown".into()),
                        pressure: pressure_label(v.pressure).into(),
                        tone: pressure_tone(v.pressure),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let network_io = snapshot.and_then(|snapshot| metric_value(&snapshot.metrics.network_io));
        let network_rows = network
            .map(|n| {
                n.interfaces
                    .iter()
                    .map(|i| {
                        let rates = network_io.and_then(|io| {
                            io.interfaces
                                .iter()
                                .find(|interface| interface.name == i.name)
                        });
                        ui::NetworkInterfaceRowViewModel {
                            name: i.name.clone(),
                            display_name: i.display_name.clone().unwrap_or_default(),
                            kind: format!("{:?}", i.kind),
                            link_state: format!("{:?}", i.link_state),
                            received_rate: rates
                                .map(|rate| format_rate(rate.received_bytes_per_second))
                                .unwrap_or_else(|| "Unavailable".to_string()),
                            transmitted_rate: rates
                                .map(|rate| format_rate(rate.transmitted_bytes_per_second))
                                .unwrap_or_else(|| "Unavailable".to_string()),
                            addresses: i.addresses.join(", "),
                            tone: if i.link_state == NetworkLinkState::Up {
                                ui::components::ComponentTone::Success
                            } else {
                                ui::components::ComponentTone::Muted
                            },
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(ui::SystemStatusViewModel {
            content: ui::SystemStatusContentViewModel::Admin(ui::AdminSystemStatusViewModel {
                overview: ui::SystemStatusOverviewViewModel {
                    storage_status: pressure_label(pressure).into(),
                    storage_tone: pressure_tone(pressure),
                    system_volume_usage: usage,
                    system_volume_used_percentage,
                    network_status,
                    network_tone,
                    active_link_count: network
                        .map(|n| n.active_link_count.to_string())
                        .unwrap_or_else(|| "Unknown".into()),
                    last_refreshed: refreshed,
                },
                storage_state,
                storage_rows,
                network_state,
                network_rows,
            }),
            diagnostics,
            route: self.system_status_route,
            dashboard,
            selected_row: self.system_status_selected_row,
            scroll_offset: self.system_status_scroll_offset,
            refreshing: self.system_status_refresh_requested_revision.is_some(),
            feedback: None,
        })
    }

    fn to_system_status_dashboard_view_model(
        &self,
        role: UserRole,
        snapshot: Option<&app::AppSystemStatusSnapshot>,
        storage_snapshot: Option<&system_services::StorageSnapshot>,
        diagnostics: &ui::DiagnosticsViewModel,
        refreshed: &str,
    ) -> ui::SystemStatusDashboardViewModel {
        let dashboard = self.system_status_dashboard_config();
        let widgets_for = |profile: storage::DashboardProfile| {
            dashboard
                .layout(profile)
                .placements
                .iter()
                .filter(|placement| self.system_status_widget_allowed(placement.kind))
                .map(|placement| {
                    self.to_system_status_widget_view_model(
                        role,
                        placement,
                        snapshot,
                        storage_snapshot,
                        diagnostics,
                        refreshed,
                    )
                })
                .collect::<Vec<_>>()
        };
        let picker = self.system_status_add_picker.as_ref().map(|picker| {
            let items = super::super::controller::system_status::SYSTEM_STATUS_WIDGET_KINDS
                .iter()
                .copied()
                .map(|kind| {
                    let already_added = dashboard.widgets.contains(&kind);
                    let reason = if already_added {
                        Some("Already added".to_string())
                    } else {
                        self.system_status_widget_unavailable_reason(kind)
                    };
                    ui::SystemStatusPickerItemViewModel {
                        kind: super::super::controller::system_status::ui_widget_kind(kind),
                        label: super::super::controller::system_status::ui_widget_kind(kind)
                            .label()
                            .to_string(),
                        detail: reason.clone().unwrap_or_default(),
                        enabled: !already_added
                            && reason.is_none()
                            && self.system_status_picker_kind_enabled(kind),
                    }
                })
                .collect();
            ui::SystemStatusPickerViewModel {
                title: "Add widget".to_string(),
                items,
                selected: picker.selected,
            }
        });
        let dirty = self.system_status_dashboard_is_dirty();
        ui::SystemStatusDashboardViewModel {
            wide_widgets: widgets_for(storage::DashboardProfile::Wide),
            narrow_widgets: widgets_for(storage::DashboardProfile::Narrow),
            selected: self
                .system_status_selected_widget
                .map(super::super::controller::system_status::ui_widget_kind),
            focus: self.system_status_dashboard_focus,
            scroll_row: self.system_status_dashboard_scroll_row,
            editing: self.system_status_dashboard_draft.is_some(),
            dirty,
            feedback: self.system_status_dashboard_feedback.clone(),
            picker,
            size_picker: self.system_status_size_picker.map(|picker| {
                ui::SystemStatusSizePickerViewModel {
                    selected: picker.selected,
                }
            }),
            dialog: self
                .system_status_discard_dialog
                .then(|| ui::SystemStatusDialogViewModel {
                    title: "Discard dashboard changes?".to_string(),
                    message: "Your unsaved widget layout changes will be lost.".to_string(),
                    confirm_label: "Discard".to_string(),
                    cancel_label: "Continue editing".to_string(),
                    selected_action: usize::from(!self.system_status_discard_confirm_selected),
                }),
            dragging: None,
            actions: ui::SystemStatusActionState {
                refresh_disabled: self.system_status_refresh_requested_revision.is_some(),
                edit_disabled: false,
                add_disabled: !super::super::controller::system_status::SYSTEM_STATUS_WIDGET_KINDS
                    .iter()
                    .copied()
                    .any(|kind| self.system_status_picker_kind_enabled(kind)),
                size_disabled: self.system_status_selected_widget.is_none(),
                remove_disabled: self.system_status_selected_widget.is_none(),
                save_disabled: !dirty,
                cancel_disabled: false,
            },
            updated: refreshed.to_string(),
        }
    }

    fn to_system_status_widget_view_model(
        &self,
        role: UserRole,
        placement: &storage::WidgetPlacement,
        snapshot: Option<&app::AppSystemStatusSnapshot>,
        storage_snapshot: Option<&system_services::StorageSnapshot>,
        diagnostics: &ui::DiagnosticsViewModel,
        refreshed: &str,
    ) -> ui::SystemStatusWidgetViewModel {
        use system_services::MetricState;
        use ui::components::ComponentTone;

        let metrics = snapshot.map(|snapshot| &snapshot.metrics);
        let kind = super::super::controller::system_status::ui_widget_kind(placement.kind);
        let size = super::super::controller::system_status::ui_widget_size(placement.size);
        let mut model = ui::SystemStatusWidgetViewModel {
            kind,
            size,
            column: u16::from(placement.column),
            row: placement.row,
            state: ui::SystemStatusWidgetState::Loading,
            tone: ComponentTone::Accent,
            primary: String::new(),
            secondary: Vec::new(),
            trend: None,
            compact_rows: Vec::new(),
            openable: self.system_status_widget_detail_allowed(placement.kind),
        };

        match placement.kind {
            storage::SystemStatusWidgetKind::SystemOverview => {
                model.state = if snapshot.is_some() {
                    ui::SystemStatusWidgetState::Ready
                } else {
                    ui::SystemStatusWidgetState::Loading
                };
                model.primary = format!("Updated {refreshed}");
                if let Some(metrics) = metrics {
                    if let Some(cpu) = metric_value(&metrics.cpu) {
                        model
                            .secondary
                            .push(format!("CPU {:.0}%", cpu.usage_percent));
                    }
                    if let Some(memory) = metric_value(&metrics.memory) {
                        model.secondary.push(format!(
                            "Memory {} / {}",
                            format_bytes(memory.used_bytes),
                            format_bytes(memory.total_bytes)
                        ));
                    }
                }
                if let Some(storage) = storage_snapshot {
                    model.secondary.push(format!(
                        "Storage {}",
                        pressure_label(storage.overall_pressure)
                    ));
                }
                if let Some(network) = snapshot.and_then(successful_network_snapshot) {
                    model.secondary.push(if network.has_active_link {
                        "Network connected".to_string()
                    } else {
                        "Network disconnected".to_string()
                    });
                }
                if let Some(metrics) = metrics {
                    if let Some(identity) = metric_value(&metrics.identity) {
                        let os = [identity.os_name.as_deref(), identity.os_version.as_deref()]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join(" ");
                        model.compact_rows.push(vec![
                            "System".to_string(),
                            identity
                                .host_name
                                .clone()
                                .unwrap_or_else(|| "Unavailable".to_string()),
                        ]);
                        model.compact_rows.push(vec![
                            "OS".to_string(),
                            if os.is_empty() {
                                "Unavailable".to_string()
                            } else {
                                os
                            },
                        ]);
                        model.compact_rows.push(vec![
                            "Kernel".to_string(),
                            identity
                                .kernel_version
                                .clone()
                                .unwrap_or_else(|| "Unavailable".to_string()),
                        ]);
                    }
                    if let Some(uptime) = metric_value(&metrics.uptime) {
                        model
                            .compact_rows
                            .push(vec!["Uptime".to_string(), format_duration(uptime.seconds)]);
                    }
                    if let Some(cpu) = metric_value(&metrics.cpu) {
                        model.compact_rows.push(vec![
                            "CPU".to_string(),
                            format!("{:.0}% used", cpu.usage_percent),
                        ]);
                    }
                    if let Some(memory) = metric_value(&metrics.memory) {
                        model.compact_rows.push(vec![
                            "Memory".to_string(),
                            format!(
                                "{} / {}",
                                format_bytes(memory.used_bytes),
                                format_bytes(memory.total_bytes)
                            ),
                        ]);
                    }
                }
                if let Some(storage) = storage_snapshot {
                    model.compact_rows.push(vec![
                        "Storage".to_string(),
                        pressure_label(storage.overall_pressure).to_string(),
                    ]);
                }
                if let Some(network) = snapshot.and_then(successful_network_snapshot) {
                    model.compact_rows.push(vec![
                        "Network".to_string(),
                        if network.has_active_link {
                            "Connected"
                        } else {
                            "Disconnected"
                        }
                        .to_string(),
                    ]);
                }
            }
            storage::SystemStatusWidgetKind::Cpu => {
                let state = metrics.map(|metrics| &metrics.cpu);
                let (widget_state, cpu) = metric_widget_state(state, role == UserRole::Admin);
                model.state = widget_state;
                if let Some(cpu) = cpu {
                    model.primary = format!("{:.0}% used", cpu.usage_percent);
                    model.secondary.push(format!(
                        "{} logical cores{}",
                        cpu.logical_core_count,
                        cpu.physical_core_count
                            .map(|count| format!(" · {count} physical"))
                            .unwrap_or_default()
                    ));
                    if let Some(load) = metrics.and_then(|metrics| metric_value(&metrics.load)) {
                        model.secondary.push(format!(
                            "Load {:.2} / {:.2} / {:.2}",
                            load.one, load.five, load.fifteen
                        ));
                    }
                    model.compact_rows = cpu
                        .per_core_percent
                        .iter()
                        .enumerate()
                        .map(|(index, value)| {
                            vec![format!("Core {}", index + 1), format!("{value:.0}%")]
                        })
                        .collect();
                }
                model.trend = Some(self.system_status_history.cpu.iter().copied().collect());
            }
            storage::SystemStatusWidgetKind::Memory => {
                let state = metrics.map(|metrics| &metrics.memory);
                let (widget_state, memory) = metric_widget_state(state, role == UserRole::Admin);
                model.state = widget_state;
                if let Some(memory) = memory {
                    let percentage = if memory.total_bytes == 0 {
                        0
                    } else {
                        memory.used_bytes.saturating_mul(100) / memory.total_bytes
                    };
                    model.primary = format!("{percentage}% used");
                    model.secondary.push(format!(
                        "{} / {}",
                        format_bytes(memory.used_bytes),
                        format_bytes(memory.total_bytes)
                    ));
                    model.secondary.push(format!(
                        "{} available · Swap {} / {}",
                        format_bytes(memory.available_bytes),
                        format_bytes(memory.swap_used_bytes),
                        format_bytes(memory.swap_total_bytes)
                    ));
                    model.compact_rows = vec![
                        vec![
                            "Available".to_string(),
                            format_bytes(memory.available_bytes),
                        ],
                        vec![
                            "Swap used".to_string(),
                            format_bytes(memory.swap_used_bytes),
                        ],
                    ];
                }
                model.trend = Some(self.system_status_history.memory.iter().copied().collect());
            }
            storage::SystemStatusWidgetKind::Storage => {
                let state = snapshot.map(|snapshot| &snapshot.storage);
                model.state = storage_widget_state(state, role == UserRole::Admin);
                if let Some(storage) = storage_snapshot {
                    let system = storage
                        .system_volume_index
                        .and_then(|index| storage.volumes.get(index));
                    model.primary = system
                        .and_then(used_percentage)
                        .map(|value| format!("{value:.0}% used"))
                        .unwrap_or_else(|| pressure_label(storage.overall_pressure).to_string());
                    if let Some(system) = system {
                        model.secondary.push(volume_usage(system));
                        if let Some(available) = system.available_bytes {
                            model
                                .secondary
                                .push(format!("{} available", format_bytes(available)));
                        }
                    }
                    model.compact_rows = storage
                        .volumes
                        .iter()
                        .map(|volume| {
                            vec![
                                if role == UserRole::Admin {
                                    volume.identifier.clone()
                                } else {
                                    "Device storage".to_string()
                                },
                                volume_usage(volume),
                            ]
                        })
                        .collect();
                    model.tone = pressure_tone(storage.overall_pressure);
                }
            }
            storage::SystemStatusWidgetKind::Network => {
                let state = metrics.map(|metrics| &metrics.network_io);
                let (widget_state, io) = metric_widget_state(state, role == UserRole::Admin);
                model.state = widget_state;
                let network = snapshot.and_then(successful_network_snapshot);
                model.primary = network
                    .map(|network| {
                        if network.has_active_link {
                            "Connected".to_string()
                        } else {
                            "Disconnected".to_string()
                        }
                    })
                    .unwrap_or_else(|| "Link unavailable".to_string());
                if let Some(io) = io {
                    model.secondary.push(format!(
                        "Down {} · Up {}",
                        format_rate(io.total_received_bytes_per_second),
                        format_rate(io.total_transmitted_bytes_per_second)
                    ));
                    model.compact_rows = io
                        .interfaces
                        .iter()
                        .map(|interface| {
                            vec![
                                interface.name.clone(),
                                format_rate(interface.received_bytes_per_second),
                                format_rate(interface.transmitted_bytes_per_second),
                            ]
                        })
                        .collect();
                }
                model.trend = Some(
                    self.system_status_history
                        .network_received
                        .iter()
                        .copied()
                        .collect(),
                );
                model.tone = if network.is_some_and(|network| network.has_active_link) {
                    ComponentTone::Success
                } else {
                    ComponentTone::Warning
                };
            }
            storage::SystemStatusWidgetKind::Temperature => {
                let state = metrics.map(|metrics| &metrics.thermal);
                let (widget_state, sensors) = metric_widget_state(state, role == UserRole::Admin);
                model.state = widget_state;
                if let Some(sensors) = sensors {
                    let hottest = sensors
                        .iter()
                        .filter(|sensor| sensor.temperature_celsius.is_finite())
                        .max_by(|left, right| {
                            left.temperature_celsius
                                .total_cmp(&right.temperature_celsius)
                        });
                    if let Some(hottest) = hottest {
                        model.primary = format!("{:.1} °C", hottest.temperature_celsius);
                        model.secondary.push(hottest.label.clone());
                    }
                    model.compact_rows = sensors
                        .iter()
                        .map(|sensor| {
                            vec![
                                sensor.label.clone(),
                                format!("{:.1} °C", sensor.temperature_celsius),
                                sensor
                                    .critical_celsius
                                    .map(|value| format!("critical {value:.1} °C"))
                                    .unwrap_or_default(),
                            ]
                        })
                        .collect();
                }
                model.trend = Some(
                    self.system_status_history
                        .temperature
                        .iter()
                        .copied()
                        .collect(),
                );
            }
            storage::SystemStatusWidgetKind::Battery => {
                let state = metrics.map(|metrics| &metrics.batteries);
                let (widget_state, batteries) = metric_widget_state(state, role == UserRole::Admin);
                model.state = widget_state;
                if let Some(batteries) = batteries {
                    if let Some(battery) = batteries.first() {
                        model.primary =
                            format!("{:.0}% · {:?}", battery.charge_percent, battery.state);
                        let time = battery
                            .time_to_empty_seconds
                            .or(battery.time_to_full_seconds);
                        if let Some(seconds) = time {
                            model.secondary.push(format_duration(seconds));
                        }
                    }
                    model.compact_rows = batteries
                        .iter()
                        .enumerate()
                        .map(|(index, battery)| {
                            vec![
                                battery
                                    .model
                                    .clone()
                                    .unwrap_or_else(|| format!("Battery {}", index + 1)),
                                format!("{:.0}%", battery.charge_percent),
                                format!("{:?}", battery.state),
                            ]
                        })
                        .collect();
                }
            }
            storage::SystemStatusWidgetKind::UptimeLoad => {
                let state = metrics.map(|metrics| &metrics.uptime);
                let (widget_state, uptime) = metric_widget_state(state, role == UserRole::Admin);
                model.state = widget_state;
                if let Some(uptime) = uptime {
                    model.primary = format_duration(uptime.seconds);
                }
                if let Some(metrics) = metrics {
                    match &metrics.load {
                        MetricState::Ready(load)
                        | MetricState::Stale {
                            last_good: load, ..
                        } => {
                            model.secondary.push(format!(
                                "Load {:.2} / {:.2} / {:.2}",
                                load.one, load.five, load.fifteen
                            ));
                            model.compact_rows = vec![
                                vec!["1 minute".to_string(), format!("{:.2}", load.one)],
                                vec!["5 minutes".to_string(), format!("{:.2}", load.five)],
                                vec!["15 minutes".to_string(), format!("{:.2}", load.fifteen)],
                            ];
                        }
                        MetricState::Unavailable { reason } => {
                            model.secondary.push(reason.clone());
                        }
                        MetricState::Loading => {}
                    }
                }
            }
            storage::SystemStatusWidgetKind::TopProcesses => {
                let state = metrics.map(|metrics| &metrics.processes);
                let (widget_state, processes) = metric_widget_state(state, true);
                model.state = widget_state;
                if let Some(processes) = processes {
                    if let Some(process) = processes.top_cpu.first() {
                        model.primary =
                            format!("{} · {:.1}% CPU", process.name, process.cpu_percent);
                    }
                    model.secondary = processes
                        .top_cpu
                        .iter()
                        .take(3)
                        .map(|process| {
                            format!(
                                "{} · {:.1}% · {}",
                                process.name,
                                process.cpu_percent,
                                format_bytes(process.memory_bytes)
                            )
                        })
                        .collect();
                    model.compact_rows = processes
                        .top_cpu
                        .iter()
                        .take(20)
                        .map(|process| {
                            vec![
                                "CPU".to_string(),
                                process.pid.to_string(),
                                process.name.clone(),
                                format!("{:.1}%", process.cpu_percent),
                                format_bytes(process.memory_bytes),
                            ]
                        })
                        .chain(processes.top_memory.iter().take(20).map(|process| {
                            vec![
                                "Memory".to_string(),
                                process.pid.to_string(),
                                process.name.clone(),
                                format!("{:.1}%", process.cpu_percent),
                                format_bytes(process.memory_bytes),
                            ]
                        }))
                        .collect();
                }
            }
            storage::SystemStatusWidgetKind::Diagnostics => {
                model.state = if diagnostics.scanned_at.is_some() {
                    ui::SystemStatusWidgetState::Ready
                } else if diagnostics.scanning {
                    ui::SystemStatusWidgetState::Loading
                } else if let Some(message) = diagnostics.feedback.clone() {
                    ui::SystemStatusWidgetState::Unavailable { message }
                } else {
                    ui::SystemStatusWidgetState::Ready
                };
                let warnings = diagnostics
                    .checks
                    .iter()
                    .filter(|check| check.status == ui::DiagnosticsStatus::Warning)
                    .count();
                let failures = diagnostics
                    .checks
                    .iter()
                    .filter(|check| check.status == ui::DiagnosticsStatus::Fail)
                    .count();
                model.primary = if failures == 0 && warnings == 0 {
                    "No issues".to_string()
                } else {
                    format!("{failures} failures · {warnings} warnings")
                };
                model.secondary = diagnostics
                    .checks
                    .iter()
                    .filter(|check| check.status != ui::DiagnosticsStatus::Pass)
                    .take(3)
                    .map(|check| check.summary.clone())
                    .collect();
                model.compact_rows = diagnostics
                    .checks
                    .iter()
                    .map(|check| vec![check.label.clone(), format!("{:?}", check.status)])
                    .collect();
                model.tone = if failures > 0 {
                    ComponentTone::Danger
                } else if warnings > 0 {
                    ComponentTone::Warning
                } else {
                    ComponentTone::Success
                };
            }
            storage::SystemStatusWidgetKind::Activity => {
                model.state = if diagnostics.scanned_at.is_some() {
                    ui::SystemStatusWidgetState::Ready
                } else if diagnostics.scanning {
                    ui::SystemStatusWidgetState::Loading
                } else {
                    ui::SystemStatusWidgetState::Ready
                };
                model.primary = format!(
                    "{} logs · {} incidents",
                    diagnostics.logs.len(),
                    diagnostics.incidents.len()
                );
                model.secondary = diagnostics
                    .incidents
                    .iter()
                    .take(3)
                    .map(|incident| incident.summary.clone())
                    .collect();
                model.compact_rows = diagnostics
                    .incidents
                    .iter()
                    .map(|incident| {
                        vec![
                            incident.occurred_at.clone(),
                            incident.app.clone(),
                            incident.summary.clone(),
                        ]
                    })
                    .collect();
            }
        }
        model
    }

    pub fn to_home_view_model(&self) -> ui::HomeViewModel {
        let user = self.current_home_username().unwrap_or("Unauthenticated");
        let model = ui::HomeViewModel::user_with_selection_and_icon_assets(
            user,
            self.current_time_label(),
            self.user_home_entries(),
            self.selected_home_entry_index(),
            self.ascii_assets.clone(),
        );
        let model = match self.home_mode {
            ShellHomeMode::Debug => model.with_debug_diagnostics(ui::DebugDiagnosticsViewModel {
                tick_count: self.tick_count,
                last_key_event: self.last_key_event.clone(),
                last_mouse_event: self.last_mouse_event.clone(),
                last_resize_event: self.last_resize_event.clone(),
                mouse_coordinates: self.mouse_coordinates,
                scroll_direction: self.mouse_scroll_direction.clone(),
                drag_direction: self.mouse_drag_direction.clone(),
                terminal_flags: terminal_flag_labels(self.terminal_flags),
                platform_capability_summary: self.platform_capability_summary.clone(),
            }),
            ShellHomeMode::User => model,
        };
        if let Some(username) = self.current_home_username() {
            model.with_account_logout(
                username,
                self.focused_component == ShellComponent::HomeLogout,
            )
        } else {
            model
        }
    }

    pub fn to_diagnostics_view_model(&self) -> ui::DiagnosticsViewModel {
        let can_view_details = self.diagnostics_can_view_details();
        let can_repair = self.diagnostics_can_repair();
        let (checks, logs, incidents, scanned_at) = self
            .app
            .diagnostics_snapshot()
            .map(|snapshot| {
                let checks = snapshot
                    .checks
                    .iter()
                    .map(|check| ui::DiagnosticsCheckViewModel {
                        id: check.id.clone(),
                        label: check.label.clone(),
                        category: check.category.label().to_string(),
                        status: diagnostics_status_to_ui(check.status),
                        summary: if can_view_details {
                            check.summary.clone()
                        } else {
                            diagnostics_public_check_summary(check)
                        },
                        detail: if can_view_details {
                            check.detail.clone()
                        } else {
                            String::new()
                        },
                        remediation: check.remediation.clone().unwrap_or_default(),
                        repairable: check.repair.is_some(),
                    })
                    .collect();
                let incidents = snapshot
                    .incidents
                    .iter()
                    .map(|incident| {
                        let app = incident
                            .app
                            .as_ref()
                            .map(|app| app.display_name.clone())
                            .unwrap_or_else(|| "TundraUX process".to_string());
                        let recovery = if can_view_details {
                            format!("{:?}", incident.recovery)
                        } else {
                            diagnostics_recovery_label(&incident.recovery)
                        };
                        let detail = if can_view_details {
                            format!(
                                "Boundary: {}; Component: {}",
                                incident.boundary,
                                incident.component.as_deref().unwrap_or("none")
                            )
                        } else {
                            String::new()
                        };
                        ui::DiagnosticsIncidentViewModel {
                            id: if can_view_details {
                                incident.incident_id.clone()
                            } else {
                                String::new()
                            },
                            occurred_at: incident
                                .occurred_at
                                .format("%Y-%m-%d %H:%M:%S UTC")
                                .to_string(),
                            app,
                            severity: diagnostics_incident_severity_to_ui(incident.severity),
                            recovery,
                            summary: if can_view_details {
                                incident.summary.clone()
                            } else {
                                String::new()
                            },
                            detail,
                            report_path: if can_view_details {
                                incident
                                    .text_report_path
                                    .as_ref()
                                    .unwrap_or(&incident.json_report_path)
                                    .display()
                                    .to_string()
                            } else {
                                String::new()
                            },
                            restricted: !can_view_details,
                        }
                    })
                    .collect();
                let logs = if can_view_details {
                    snapshot
                        .logs
                        .iter()
                        .map(|log| ui::DiagnosticsLogViewModel {
                            relative_path: log.relative_path.display().to_string(),
                            path: log.path.display().to_string(),
                            modified_at: log
                                .modified_at
                                .format("%Y-%m-%d %H:%M:%S UTC")
                                .to_string(),
                            size_bytes: log.size_bytes,
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                (
                    checks,
                    logs,
                    incidents,
                    Some(
                        snapshot
                            .scanned_at
                            .format("%Y-%m-%d %H:%M:%S UTC")
                            .to_string(),
                    ),
                )
            })
            .unwrap_or_else(|| (Vec::new(), Vec::new(), Vec::new(), None));

        let repair_dialog = (!self.diagnostics_repair_preview.is_empty()).then(|| {
            ui::DiagnosticsRepairDialogViewModel {
                items: self
                    .diagnostics_repair_preview
                    .iter()
                    .enumerate()
                    .map(|(index, action)| ui::DiagnosticsRepairItemViewModel {
                        id: index.to_string(),
                        label: action.label(),
                    })
                    .collect(),
                selected: self.diagnostics_repair_selected,
                confirm_selected: self.diagnostics_repair_confirm_selected,
                scroll_offset: self.diagnostics_repair_scroll_offset,
            }
        });

        ui::DiagnosticsViewModel {
            tab: self.diagnostics_tab,
            checks,
            logs,
            incidents,
            selected_check: self.diagnostics_selected_check,
            selected_log: self.diagnostics_selected_log,
            selected_incident: self.diagnostics_selected_incident,
            list_window_start: self.diagnostics_list_window_start,
            list_window_is_explicit: self.diagnostics_list_window_is_explicit,
            scanning: self.diagnostics_scanning
                || self
                    .diagnostics_task_runtime
                    .as_ref()
                    .is_some_and(ShellDiagnosticsTaskRuntime::is_busy),
            can_view_details,
            can_repair,
            restart_required: self.diagnostics_restart_is_required(),
            repair_dialog,
            feedback: self.diagnostics_feedback.clone(),
            scanned_at,
        }
    }

    pub(in crate::session) fn current_home_username(&self) -> Option<&str> {
        self.app
            .auth_session()
            .map(|session| session.username.as_str())
    }

    pub fn to_clock_view_model(&self) -> ui::ClockViewModel {
        let snapshot = self.app.snapshot().clock;
        self.to_clock_view_model_at(&snapshot, Instant::now())
    }

    pub(in crate::session) fn to_clock_view_model_at(
        &self,
        snapshot: &time::ClockSnapshot,
        now: Instant,
    ) -> ui::ClockViewModel {
        let mut alarms = Vec::new();
        let mut countdowns = Vec::new();
        if let Some(scheduler) = &self.clock_scheduler {
            for entry in scheduler.entries(now) {
                let label = match entry.kind {
                    ScheduledClockEntryKind::DailyAlarm => {
                        if entry.snoozed {
                            format!("{} Daily (snoozed)", entry.display_time)
                        } else {
                            format!("{} Daily", entry.display_time)
                        }
                    }
                    ScheduledClockEntryKind::Countdown => {
                        format!("{} left", entry.display_time)
                    }
                };
                let view = ui::ClockEntryViewModel::new(entry.id, label, entry.strong);
                match entry.kind {
                    ScheduledClockEntryKind::DailyAlarm => alarms.push(view),
                    ScheduledClockEntryKind::Countdown => countdowns.push(view),
                }
            }
        }

        let mut model = ui::ClockViewModel::at(
            snapshot.date.to_string(),
            snapshot.time.format("%H:%M:%S").to_string(),
            snapshot.time.hour() as u8,
            snapshot.time.minute() as u8,
            snapshot.time.second() as u8,
        )
        .with_ascii_assets(self.ascii_assets.clone())
        .with_read_only(self.is_strict_guest());
        model.alarms = alarms;
        model.countdowns = countdowns;
        model.selected_entry_id = (self.focused_component == ShellComponent::ClockEntryList)
            .then_some(self.clock_selected_entry_id)
            .flatten();
        model.entry_window_start = self.clock_entry_window_start;
        model.create_dialog =
            self.clock_create_state
                .as_ref()
                .map(|state| ui::ClockCreateDialogViewModel {
                    input: state.input.clone(),
                    error: state.error.clone(),
                    focus: state.focus,
                });
        model
    }

    pub fn to_time_sync_dialog_view_model(&self) -> Option<ui::TimeSyncDialogViewModel> {
        self.time_sync_dialog_visible
            .then(ui::TimeSyncDialogViewModel::new)
    }

    pub fn to_login_view_model(&self) -> ui::LoginViewModel {
        self.to_login_view_model_at(Instant::now())
    }

    pub fn to_login_view_model_at(&self, now: Instant) -> ui::LoginViewModel {
        let model = ui::LoginViewModel::new(
            self.login_users
                .iter()
                .map(|user| ui::LoginUserOptionViewModel {
                    username: user.username.clone(),
                    display_name: user.display_name.clone(),
                    role: user.role.clone(),
                    enabled: user.enabled,
                    locked: user
                        .locked_until_epoch_ms
                        .map(|locked_until| locked_until > unix_millis())
                        .unwrap_or(false),
                })
                .collect(),
            self.login_selected_user,
            self.login_user_window_start,
            self.login_password.chars().count(),
            match self.focused_component {
                ShellComponent::LoginPassword => ui::LoginField::Password,
                ShellComponent::LoginPasswordVisibility => ui::LoginField::PasswordVisibility,
                _ => ui::LoginField::UserList,
            },
            self.error_message.clone(),
        );
        if self.login_password_is_visible_at(now) {
            model.with_visible_password(self.login_password.clone())
        } else {
            model
        }
    }

    pub fn to_bootstrap_admin_view_model(&self) -> ui::BootstrapAdminViewModel {
        ui::BootstrapAdminViewModel::new(
            self.bootstrap_username.clone(),
            self.bootstrap_password.chars().count(),
            match self.focused_component {
                ShellComponent::BootstrapPassword => ui::AuthField::Password,
                _ => ui::AuthField::Username,
            },
            self.error_message.clone(),
        )
    }

    pub fn to_setup_view_model(&self) -> ui::SetupViewModel {
        let password_requirements = setup_password_requirements(
            &self.setup_admin_username,
            &self.setup_admin_password,
            &self.setup_admin_password_confirm,
        );
        let can_submit = !self.setup_admin_username.trim().is_empty()
            && password_requirements
                .iter()
                .all(|requirement| requirement.met);
        let custom_color = self
            .setup_custom_color_input
            .parse::<storage::BorderColor>()
            .ok();
        let custom_color_conflicts_with_theme = self.setup_custom_color_target
            == Some(ui::SetupCustomColorTarget::Accent)
            && custom_color == Some(self.setup_theme_color);

        ui::SetupViewModel {
            step: self.setup_step,
            languages: app::setup_language_options(),
            timezones: app::setup_timezone_options(),
            selected_language_index: self.setup_selected_language_index,
            selected_timezone_index: self.setup_selected_timezone_index,
            timezone_window_start: self.setup_timezone_window_start,
            admin_username: self.setup_admin_username.clone(),
            admin_password_len: self.setup_admin_password.chars().count(),
            admin_password_confirm_len: self.setup_admin_password_confirm.chars().count(),
            password_requirements,
            password_hint: self.setup_admin_password_hint.clone(),
            focused_field: self.setup_focused_field,
            can_submit,
            border_shape: match self.setup_border_shape {
                storage::BorderShape::Rounded => ui::BorderShape::Rounded,
                storage::BorderShape::Square => ui::BorderShape::Square,
            },
            theme_color: ui_theme_color(self.setup_theme_color),
            theme_color_value: self.setup_theme_color.to_string(),
            accent_color: ui_theme_color(self.setup_accent_color),
            accent_color_value: self.setup_accent_color.to_string(),
            custom_color_target: self.setup_custom_color_target,
            custom_color_input: self.setup_custom_color_input.clone(),
            custom_color_valid: !self.setup_custom_color_input.trim().is_empty()
                && custom_color.is_some()
                && !custom_color_conflicts_with_theme,
            custom_color_conflicts_with_theme,
            custom_color_error: self.setup_custom_color_error.clone(),
            error: self.error_message.clone(),
        }
    }

    pub fn to_user_management_view_model(&self) -> ui::UserManagementViewModel {
        let current_user = self
            .app
            .auth_session()
            .map(|session| session.username.clone())
            .unwrap_or_else(|| "Unauthenticated".to_string());
        let mut model = ui::UserManagementViewModel::new(
            current_user.clone(),
            self.app
                .managed_users()
                .iter()
                .map(|user| ui::UserManagementUserViewModel {
                    username: user.username.clone(),
                    display_name: user.display_name.clone(),
                    role: user.role.as_str().to_string(),
                    enabled: user.enabled,
                    locked: user
                        .locked_until_epoch_ms
                        .map(|locked_until| locked_until > unix_millis())
                        .unwrap_or(false),
                    is_current: user.username.eq_ignore_ascii_case(&current_user),
                })
                .collect(),
            self.user_management_selected,
            self.user_management_message.clone(),
            self.can_manage_all_users(),
            self.user_management_form_view_model(),
        );
        model.user_window_start = self.user_management_window_start;
        model.focus = match self.user_management_focus {
            UserManagementPageFocus::UserList => ui::UserManagementFocus::UserList,
            UserManagementPageFocus::Action(action) => ui::UserManagementFocus::Action(action),
        };
        model.actions = self.user_management_action_view_models();
        model.feedback_tone = match self.user_management_feedback_tone {
            UserManagementFeedbackTone::Info => ui::UserManagementFeedbackTone::Info,
            UserManagementFeedbackTone::Success => ui::UserManagementFeedbackTone::Success,
            UserManagementFeedbackTone::Error => ui::UserManagementFeedbackTone::Error,
        };
        model
    }

    pub fn to_explorer_view_model(&self) -> ui::ExplorerViewModel {
        let app_snapshot = self.app.snapshot();
        let Some(state) = self.app.explorer_state() else {
            return ui::ExplorerViewModel::new("Explorer unavailable", Vec::new(), None);
        };
        let is_trash = state.current_location.is_trash();
        let display_path = if is_trash {
            "Trash".to_string()
        } else {
            state.current_path.display().to_string()
        };

        let entries = state
            .entries
            .iter()
            .map(|entry| ui::ExplorerEntryViewModel {
                name: explorer_display_name(entry, state.show_extensions),
                kind: entry.type_label.clone(),
                size: (entry.kind == app::explorer::ExplorerEntryKind::File)
                    .then(|| explorer_size_label(entry.size, state.size_format)),
                modified: entry.modified.map(|modified| {
                    explorer_system_time_label(
                        modified,
                        state.date_zone,
                        app_snapshot.clock_timezone_id,
                    )
                }),
                attributes: explorer_attribute_labels(&entry.attributes),
                selected: state.is_selected(&entry.path),
            })
            .collect::<Vec<_>>();
        let selected_index = (!entries.is_empty()).then_some(state.selected_index);
        let mut model = ui::ExplorerViewModel::with_ascii_assets(
            display_path.clone(),
            entries,
            selected_index,
            self.ascii_assets.clone(),
        );
        model.is_trash = is_trash;
        model.address_editing = self.explorer_input_mode == ExplorerInputMode::Address;
        model.address_value = if model.address_editing {
            self.explorer_input.clone()
        } else {
            display_path
        };
        model.entry_presentations = state
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let mut presentation = ui::ExplorerEntryPresentationViewModel::new(
                    entry.path.display().to_string(),
                    entry.path.display().to_string(),
                    entry.icon_key.clone(),
                    entry.kind == app::explorer::ExplorerEntryKind::Directory,
                );
                presentation.selected = state.is_selected(&entry.path);
                presentation.focused = index == state.selected_index;
                presentation.cut = state.clipboard.as_ref().is_some_and(|clipboard| {
                    clipboard.mode == app::explorer::ExplorerClipboardMode::Cut
                        && clipboard.paths.contains(&entry.path)
                });
                presentation.drop_target = state
                    .drag
                    .as_ref()
                    .and_then(|drag| drag.target.as_ref())
                    .is_some_and(|target| target == &entry.path);
                presentation.metadata_warning = entry.metadata_warning.clone();
                presentation.original_path = entry
                    .original_path
                    .as_ref()
                    .map(|path| path.display().to_string());
                presentation
            })
            .collect();
        model.quick_locations = state
            .quick_locations
            .iter()
            .map(|location| {
                let mut model = ui::ExplorerQuickLocationViewModel::new(
                    location.id.clone(),
                    location.label.clone(),
                    location.path.display().to_string(),
                    location.icon_key.clone(),
                );
                model.kind = location.kind;
                model.current = if location.is_trash() {
                    is_trash
                } else {
                    !is_trash && location.path == state.current_path
                };
                model.enabled = location.enabled && (location.is_trash() || location.path.is_dir());
                model.drop_target = !location.is_trash()
                    && state
                        .drag
                        .as_ref()
                        .and_then(|drag| drag.target.as_ref())
                        .is_some_and(|target| target == &location.path);
                model
            })
            .collect();
        model.breadcrumbs = if is_trash {
            Vec::new()
        } else {
            explorer_breadcrumb_view_models(&state.current_path, state)
        };
        model.sort_column = match state.sort_field {
            app::explorer::ExplorerSortField::Name => ui::ExplorerSortColumn::Name,
            app::explorer::ExplorerSortField::Type => ui::ExplorerSortColumn::Type,
            app::explorer::ExplorerSortField::Size => ui::ExplorerSortColumn::Size,
            app::explorer::ExplorerSortField::Modified => ui::ExplorerSortColumn::Modified,
        };
        model.sort_direction = state.sort_direction;
        model.viewport_offset = state.viewport_offset;
        model.viewport_follows_focus = state.viewport_follows_focus;
        model.show_sidebar = state.show_sidebar;
        model.selected_count = state.effective_selected_paths().len();
        model.listing_warning_count = state.listing_warning_count;
        model.set_history_availability(
            !state.back_history.is_empty(),
            !state.forward_history.is_empty(),
        );
        let busy = state.operation.is_some();
        if is_trash {
            model.toolbar = ui::ExplorerToolbarViewModel::trash(
                !state.back_history.is_empty(),
                !state.forward_history.is_empty(),
                model.selected_count == 1 && !busy,
                !state.entries.is_empty() && !busy,
            );
        }
        for button in &mut model.toolbar.buttons {
            button.enabled = match button.action {
                ui::ExplorerToolbarAction::Back => !state.back_history.is_empty(),
                ui::ExplorerToolbarAction::Forward => !state.forward_history.is_empty(),
                ui::ExplorerToolbarAction::Up => !is_trash && state.current_path.parent().is_some(),
                ui::ExplorerToolbarAction::New => !is_trash && !busy,
                ui::ExplorerToolbarAction::Cut
                | ui::ExplorerToolbarAction::Copy
                | ui::ExplorerToolbarAction::Delete => model.selected_count > 0 && !busy,
                ui::ExplorerToolbarAction::Paste => state.clipboard.is_some() && !busy,
                ui::ExplorerToolbarAction::Rename => {
                    !is_trash && model.selected_count == 1 && !busy
                }
                ui::ExplorerToolbarAction::Restore => {
                    is_trash && model.selected_count == 1 && !busy
                }
                ui::ExplorerToolbarAction::DumpTrash => {
                    is_trash && !state.entries.is_empty() && !busy
                }
                _ => true,
            };
        }
        model.operation = state.operation.as_ref().map(|operation| {
            let phase = match operation.phase {
                app::explorer::ExplorerOperationPhase::Scanning => {
                    ui::ExplorerProgressStage::Scanning
                }
                app::explorer::ExplorerOperationPhase::WaitingForConflict => {
                    ui::ExplorerProgressStage::CheckingConflicts
                }
                app::explorer::ExplorerOperationPhase::Executing => {
                    if operation.label.to_ascii_lowercase().contains("mov") {
                        ui::ExplorerProgressStage::Moving
                    } else if operation.label.to_ascii_lowercase().contains("trash") {
                        ui::ExplorerProgressStage::Deleting
                    } else {
                        ui::ExplorerProgressStage::Copying
                    }
                }
                app::explorer::ExplorerOperationPhase::Completed
                | app::explorer::ExplorerOperationPhase::Cancelled
                | app::explorer::ExplorerOperationPhase::Failed => {
                    ui::ExplorerProgressStage::Finishing
                }
            };
            ui::ExplorerOperationProgressViewModel {
                phase,
                label: operation.label.clone(),
                completed_items: operation.completed_items as u64,
                total_items: operation.total_items.map(|value| value as u64),
                completed_bytes: operation.completed_bytes,
                total_bytes: operation.total_bytes,
                cancellable: operation.cancellable,
                cancel_label: "Cancel".to_string(),
            }
        });
        model.show_hidden = state.show_hidden;
        model.message = if is_trash && model.selected_count > 0 {
            state.selected_entry().map(|entry| {
                entry.original_path.as_ref().map_or_else(
                    || "Original location unavailable".to_string(),
                    |path| format!("Original location: {}", path.display()),
                )
            })
        } else {
            state.message.clone()
        };
        model.error = state.error.clone();
        model.search = if self.explorer_input_mode == ExplorerInputMode::Search {
            Some(ui::ExplorerSearchViewModel::new(
                self.explorer_input.clone(),
                true,
                Some(state.entries.len()),
            ))
        } else if !state.query.is_empty() {
            Some(ui::ExplorerSearchViewModel::new(
                state.query.clone(),
                false,
                Some(state.entries.len()),
            ))
        } else {
            None
        };
        model.pending_dialog = state.pending_dialog.as_ref().map(|dialog| {
            let (confirm, cancel) = match dialog.kind {
                app::explorer::ExplorerDialogKind::DeleteToTrash => {
                    ("Y / Enter: move", "N / Esc: cancel")
                }
                app::explorer::ExplorerDialogKind::DumpTrash => {
                    ("Y / Enter: empty permanently", "N / Esc: cancel")
                }
            };
            ui::ExplorerDialogViewModel::new(
                dialog.title.clone(),
                dialog.message.clone(),
                confirm,
                cancel,
            )
        });

        model.overlay = if let Some(conflict) = state.pending_restore.as_ref() {
            Some(ui::ExplorerOverlayViewModel::Conflict(
                ui::ExplorerConflictViewModel {
                    title: "Restore conflict".to_string(),
                    source: format!("Trash: {}", conflict.display_name),
                    destination: conflict.target.display().to_string(),
                    choices: vec![
                        ui::ExplorerConflictChoice::KeepBoth,
                        ui::ExplorerConflictChoice::Replace,
                        ui::ExplorerConflictChoice::Cancel,
                    ],
                    selected_choice: ui::ExplorerConflictChoice::KeepBoth,
                    apply_to_remaining: false,
                    allow_apply_to_remaining: false,
                },
            ))
        } else if let Some(conflict) = state.pending_conflict.as_ref() {
            Some(ui::ExplorerOverlayViewModel::Conflict(
                ui::ExplorerConflictViewModel {
                    title: "Name conflict".to_string(),
                    source: conflict.source.display().to_string(),
                    destination: conflict.target.display().to_string(),
                    choices: vec![
                        ui::ExplorerConflictChoice::KeepBoth,
                        ui::ExplorerConflictChoice::Replace,
                        ui::ExplorerConflictChoice::Skip,
                        ui::ExplorerConflictChoice::Cancel,
                    ],
                    selected_choice: ui::ExplorerConflictChoice::KeepBoth,
                    apply_to_remaining: self.explorer_conflict_apply_to_remaining,
                    allow_apply_to_remaining: true,
                },
            ))
        } else if matches!(
            self.explorer_input_mode,
            ExplorerInputMode::NewFolder
                | ExplorerInputMode::NewTextFile
                | ExplorerInputMode::Rename
                | ExplorerInputMode::RestoreDestination
        ) {
            let (kind, title, prompt, confirm_label) = match self.explorer_input_mode {
                ExplorerInputMode::NewFolder => (
                    ui::ExplorerNameDialogKind::NewFolder,
                    "New folder",
                    "Folder name",
                    "Create",
                ),
                ExplorerInputMode::NewTextFile => (
                    ui::ExplorerNameDialogKind::NewTextFile,
                    "New text file",
                    "File name",
                    "Create",
                ),
                ExplorerInputMode::Rename => (
                    ui::ExplorerNameDialogKind::Rename,
                    "Rename",
                    "New name",
                    "Rename",
                ),
                ExplorerInputMode::RestoreDestination => (
                    ui::ExplorerNameDialogKind::RestoreDestination,
                    "Restore item",
                    "Absolute destination directory",
                    "Restore",
                ),
                ExplorerInputMode::Browse
                | ExplorerInputMode::Address
                | ExplorerInputMode::Search => unreachable!(),
            };
            Some(ui::ExplorerOverlayViewModel::Name(
                ui::ExplorerNameDialogViewModel {
                    kind,
                    title: title.to_string(),
                    prompt: prompt.to_string(),
                    value: self.explorer_input.clone(),
                    error: state.error.clone(),
                    confirm_label: confirm_label.to_string(),
                    cancel_label: "Cancel".to_string(),
                },
            ))
        } else if let Some(overlay_mode) = self.explorer_overlay_mode {
            Some(match overlay_mode {
                ExplorerOverlayMode::ContextMenu { anchor } => {
                    explorer_context_menu_view_model(ExplorerContextMenuInput {
                        anchor,
                        selected_count: model.selected_count,
                        clipboard_available: state.clipboard.is_some(),
                        is_trash,
                        trash_has_items: !state.entries.is_empty(),
                        focused_index: self.explorer_overlay_selection,
                        can_manage_launcher: self.can_manage_launcher(),
                        launcher_eligible_count: state
                            .effective_selected_paths()
                            .iter()
                            .filter(|path| {
                                state.entries.iter().any(|entry| {
                                    entry.path == **path && entry.open_policy.requires_launcher()
                                })
                            })
                            .count(),
                    })
                }
                ExplorerOverlayMode::Sort { anchor } => explorer_sort_menu_view_model(
                    anchor,
                    model.sort_column,
                    self.explorer_overlay_selection,
                ),
                ExplorerOverlayMode::Options => explorer_options_view_model(
                    state,
                    self.explorer_overlay_selection,
                    self.can_change_explorer_settings(),
                ),
                ExplorerOverlayMode::Properties => {
                    explorer_properties_view_model(state, app_snapshot.clock_timezone_id)
                }
            })
        } else {
            None
        };

        model
    }

    pub(in crate::session) fn can_manage_all_users(&self) -> bool {
        matches!(
            self.app.auth_session().map(|session| session.role),
            Some(UserRole::Admin)
        )
    }

    pub(in crate::session) fn user_management_action_view_models(
        &self,
    ) -> Vec<ui::UserManagementActionViewModel> {
        use ui::UserManagementAction;

        let selected = self.app.managed_users().get(self.user_management_selected);
        let last_enabled_admin = self.selected_is_last_enabled_admin();
        let no_selection_reason = selected.is_none().then(|| "No user selected".to_string());
        let protected_reason =
            last_enabled_admin.then(|| "At least one enabled admin is required".to_string());
        let mut actions = Vec::new();

        if self.can_manage_all_users() {
            actions.push(user_management_action_model(
                UserManagementAction::NewUser,
                "New user",
                Some('N'),
                true,
                None,
                false,
            ));
        }

        actions.push(user_management_action_model(
            UserManagementAction::EditInfo,
            if self.can_manage_all_users() {
                "Edit"
            } else {
                "Edit profile"
            },
            Some('E'),
            selected.is_some(),
            no_selection_reason.clone(),
            false,
        ));
        actions.push(user_management_action_model(
            UserManagementAction::SetPassword,
            if self.can_manage_all_users() {
                "Password"
            } else {
                "Change password"
            },
            Some('R'),
            selected.is_some(),
            no_selection_reason.clone(),
            false,
        ));

        if self.can_manage_all_users() {
            let locked = selected.is_some_and(user_is_locked);
            let enabled = selected.is_some_and(|user| user.enabled);
            let (toggle_label, toggle_shortcut, disabling) = if !enabled {
                ("Enable", Some('U'), false)
            } else if locked {
                ("Unlock", Some('U'), false)
            } else {
                ("Disable", Some('D'), true)
            };
            actions.push(user_management_action_model(
                UserManagementAction::ToggleEnabled,
                toggle_label,
                toggle_shortcut,
                selected.is_some() && !(disabling && last_enabled_admin),
                no_selection_reason.clone().or_else(|| {
                    (disabling && last_enabled_admin)
                        .then(|| protected_reason.clone())
                        .flatten()
                }),
                disabling,
            ));

            let demoting = selected.is_some_and(|user| user.role == UserRole::Admin);
            actions.push(user_management_action_model(
                UserManagementAction::ToggleRole,
                if demoting { "Make user" } else { "Make admin" },
                Some('C'),
                selected.is_some() && !(demoting && last_enabled_admin),
                no_selection_reason.clone().or_else(|| {
                    (demoting && last_enabled_admin)
                        .then(|| protected_reason.clone())
                        .flatten()
                }),
                demoting,
            ));
        }

        actions.push(user_management_action_model(
            UserManagementAction::Delete,
            if self.can_manage_all_users() {
                "Delete"
            } else {
                "Delete account"
            },
            Some('X'),
            selected.is_some() && !last_enabled_admin,
            no_selection_reason.or(protected_reason),
            true,
        ));
        actions.push(user_management_action_model(
            UserManagementAction::Back,
            "Back",
            None,
            true,
            None,
            false,
        ));
        actions
    }

    fn user_management_form_view_model(&self) -> Option<ui::UserManagementFormViewModel> {
        match &self.user_management_mode {
            UserManagementMode::Browse => None,
            UserManagementMode::Create(form) => Some(ui::UserManagementFormViewModel {
                kind: ui::UserManagementFormKind::Create,
                title: "Create user".to_string(),
                username: form.username.clone(),
                display_name: form.display_name.clone(),
                role: form.role.as_str().to_string(),
                password_len: form.password.chars().count(),
                focused_field: to_ui_user_management_field(form.focused_field),
                error: self.user_management_form_error(),
            }),
            UserManagementMode::EditInfo(form) => Some(ui::UserManagementFormViewModel {
                kind: ui::UserManagementFormKind::EditInfo,
                title: "Edit user info".to_string(),
                username: form.username.clone(),
                display_name: form.display_name.clone(),
                role: String::new(),
                password_len: 0,
                focused_field: to_ui_user_management_field(form.focused_field),
                error: self.user_management_form_error(),
            }),
            UserManagementMode::Password(form) => Some(ui::UserManagementFormViewModel {
                kind: ui::UserManagementFormKind::Password,
                title: "Set password".to_string(),
                username: form.username.clone(),
                display_name: String::new(),
                role: String::new(),
                password_len: form.password.chars().count(),
                focused_field: to_ui_user_management_field(form.focused_field),
                error: self.user_management_form_error(),
            }),
        }
    }

    fn user_management_form_error(&self) -> Option<String> {
        (self.user_management_feedback_tone == UserManagementFeedbackTone::Error)
            .then(|| self.user_management_message.clone())
            .flatten()
    }

    pub fn to_shell_chrome_view_model(&self) -> ui::ShellChromeViewModel {
        let status = if self.home_mode == ShellHomeMode::Debug {
            let mouse_position = self
                .mouse_coordinates
                .map(|(x, y)| format!("{x},{y}"))
                .unwrap_or_else(|| "none".to_string());
            format!(
                "{} | Last Key: {} | Mouse position: {} | Size: {}x{} | Scroll: {} | Drag: {}",
                self.status(),
                self.last_key_event.as_deref().unwrap_or("none"),
                mouse_position,
                self.terminal_size.0,
                self.terminal_size.1,
                self.mouse_scroll_direction.as_deref().unwrap_or("none"),
                self.mouse_drag_direction.as_deref().unwrap_or("none")
            )
        } else {
            self.status().to_string()
        };
        ui::ShellChromeViewModel {
            app_name: "TundraUX 3".to_string(),
            build_mode: build_mode_label().to_string(),
            display_mode: self.home_display_mode(),
            terminal_size: self.terminal_size,
            screen_stack: self
                .screen_stack
                .iter()
                .map(|screen| format!("{screen:?}"))
                .collect(),
            status: ui::StatusViewModel {
                status,
                toast: self.app.notification_center().toast().map(str::to_owned),
                error: self.app.notification_center().alert().map(str::to_owned),
                alert_tone: self
                    .app
                    .notification_center()
                    .alert_tone()
                    .unwrap_or(ui::NotificationTone::Info),
                time_button_label: self.status_time_button_label(),
                time_button_selected: self.time_button_selected(),
            },
        }
    }
}

fn metric_value<T>(state: &system_services::MetricState<T>) -> Option<&T> {
    match state {
        system_services::MetricState::Ready(value)
        | system_services::MetricState::Stale {
            last_good: value, ..
        } => Some(value),
        system_services::MetricState::Loading
        | system_services::MetricState::Unavailable { .. } => None,
    }
}

fn metric_widget_state<'a, T>(
    state: Option<&'a system_services::MetricState<T>>,
    expose_error: bool,
) -> (ui::SystemStatusWidgetState, Option<&'a T>) {
    match state {
        None | Some(system_services::MetricState::Loading) => {
            (ui::SystemStatusWidgetState::Loading, None)
        }
        Some(system_services::MetricState::Ready(value)) => {
            (ui::SystemStatusWidgetState::Ready, Some(value))
        }
        Some(system_services::MetricState::Stale { last_good, error }) => (
            ui::SystemStatusWidgetState::Stale {
                message: if expose_error {
                    error.clone()
                } else {
                    "Metric data is stale".to_string()
                },
            },
            Some(last_good),
        ),
        Some(system_services::MetricState::Unavailable { reason }) => (
            ui::SystemStatusWidgetState::Unavailable {
                message: if expose_error {
                    reason.clone()
                } else {
                    "Metric is unavailable".to_string()
                },
            },
            None,
        ),
    }
}

fn storage_widget_state(
    state: Option<&system_services::StorageState>,
    expose_error: bool,
) -> ui::SystemStatusWidgetState {
    match state {
        None | Some(system_services::StorageState::Loading) => ui::SystemStatusWidgetState::Loading,
        Some(system_services::StorageState::Ready(_)) => ui::SystemStatusWidgetState::Ready,
        Some(system_services::StorageState::Stale { error, .. }) => {
            ui::SystemStatusWidgetState::Stale {
                message: if expose_error {
                    error.clone()
                } else {
                    "Storage data is stale".to_string()
                },
            }
        }
        Some(system_services::StorageState::Unavailable { reason }) => {
            ui::SystemStatusWidgetState::Unavailable {
                message: if expose_error {
                    reason.clone()
                } else {
                    "Storage is unavailable".to_string()
                },
            }
        }
    }
}

fn successful_network_snapshot(
    snapshot: &app::AppSystemStatusSnapshot,
) -> Option<&system_services::NetworkSnapshot> {
    match &snapshot.network {
        system_services::NetworkState::Ready(value)
        | system_services::NetworkState::Stale {
            last_good: value, ..
        } => Some(value),
        system_services::NetworkState::Loading
        | system_services::NetworkState::Unavailable { .. } => None,
    }
}

fn format_rate(bytes_per_second: f64) -> String {
    let rate = bytes_per_second.max(0.0).round() as u64;
    format!(
        "{}/s",
        super::super::controller::system_status::format_bytes(rate)
    )
}

fn format_duration(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = seconds % 86_400 / 3_600;
    let minutes = seconds % 3_600 / 60;
    if days > 0 {
        format!("{days}d {hours}h {minutes}m")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn used_percentage(v: &system_services::StorageVolumeSnapshot) -> Option<f64> {
    let (Some(total), Some(avail)) = (v.total_bytes, v.available_bytes) else {
        return None;
    };
    (total > 0 && avail <= total).then(|| (total - avail) as f64 * 100.0 / total as f64)
}
fn successful_system_status_sampled_at(
    snapshot: &app::AppSystemStatusSnapshot,
) -> Option<chrono::DateTime<Utc>> {
    let storage = match &snapshot.storage {
        system_services::StorageState::Ready(value) => Some(value.sampled_at),
        system_services::StorageState::Stale { last_good, .. } => Some(last_good.sampled_at),
        _ => None,
    };
    let network = match &snapshot.network {
        system_services::NetworkState::Ready(value) => Some(value.sampled_at),
        system_services::NetworkState::Stale { last_good, .. } => Some(last_good.sampled_at),
        _ => None,
    };
    let metrics = (metric_has_sample(&snapshot.metrics.cpu)
        || metric_has_sample(&snapshot.metrics.identity)
        || metric_has_sample(&snapshot.metrics.memory)
        || metric_has_sample(&snapshot.metrics.load)
        || metric_has_sample(&snapshot.metrics.uptime)
        || metric_has_sample(&snapshot.metrics.network_io)
        || metric_has_sample(&snapshot.metrics.thermal)
        || metric_has_sample(&snapshot.metrics.batteries)
        || metric_has_sample(&snapshot.metrics.processes))
    .then_some(snapshot.metrics.sampled_at);
    storage.into_iter().chain(network).chain(metrics).max()
}
fn metric_has_sample<T>(state: &system_services::MetricState<T>) -> bool {
    matches!(
        state,
        system_services::MetricState::Ready(_) | system_services::MetricState::Stale { .. }
    )
}
fn format_sample_age(sampled_at: chrono::DateTime<Utc>) -> String {
    let seconds = Utc::now()
        .signed_duration_since(sampled_at)
        .num_seconds()
        .max(0) as u64;
    match seconds {
        0..=1 => "just now".to_string(),
        2..=59 => format!("{seconds}s ago"),
        60..=3_599 => format!("{}m ago", seconds / 60),
        3_600..=86_399 => format!("{}h ago", seconds / 3_600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}
fn volume_usage(v: &system_services::StorageVolumeSnapshot) -> String {
    match (v.total_bytes, v.available_bytes, used_percentage(v)) {
        (Some(total), Some(avail), Some(_)) => format!(
            "{} / {}",
            super::super::controller::system_status::format_bytes(total - avail),
            super::super::controller::system_status::format_bytes(total)
        ),
        _ => "Unknown".into(),
    }
}
fn pressure_label(v: system_services::StoragePressure) -> &'static str {
    match v {
        system_services::StoragePressure::Unknown => "Unknown",
        system_services::StoragePressure::Normal => "Normal",
        system_services::StoragePressure::Low => "Low",
        system_services::StoragePressure::Critical => "Critical",
    }
}
fn pressure_tone(v: system_services::StoragePressure) -> ui::components::ComponentTone {
    match v {
        system_services::StoragePressure::Normal => ui::components::ComponentTone::Success,
        system_services::StoragePressure::Low => ui::components::ComponentTone::Warning,
        system_services::StoragePressure::Critical => ui::components::ComponentTone::Danger,
        _ => ui::components::ComponentTone::Muted,
    }
}
