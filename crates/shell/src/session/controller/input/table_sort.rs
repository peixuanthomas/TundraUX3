use crate::session::*;

/// Move whole records, never just the displayed cells, and retain selection.
pub(super) fn reorder<T>(
    rows: &mut Vec<T>,
    cells: &[Vec<String>],
    sort: ui::TableSort,
    selected: &mut usize,
) {
    if rows.len() != cells.len() {
        return;
    }
    let mut order = (0..rows.len()).collect::<Vec<_>>();
    order.sort_by(|a, b| {
        sort.compare(
            cells[*a].get(sort.column).map_or("", String::as_str),
            cells[*b].get(sort.column).map_or("", String::as_str),
        )
    });
    *selected = order
        .iter()
        .position(|index| index == selected)
        .unwrap_or(0);
    let mut original = std::mem::take(rows)
        .into_iter()
        .map(Some)
        .collect::<Vec<_>>();
    *rows = order
        .into_iter()
        .map(|index| original[index].take().unwrap())
        .collect();
}

impl ShellSession {
    pub(in crate::session) fn logs_sort_key(&self) -> String {
        format!(
            "logs.{:?}.{:?}",
            self.logs_state.category, self.logs_state.section
        )
    }

    fn active_table_sort(&self) -> Option<(String, usize)> {
        if self.active_overlay_descriptor().is_some() {
            return None;
        }
        match self.active_screen() {
            ShellScreen::Launcher if self.launcher_pending_confirmation.is_none() => {
                Some(("launcher".into(), 4))
            }
            ShellScreen::Clock if self.clock_create_state.is_none() => Some(("clock".into(), 2)),
            ShellScreen::UserManagement if self.to_user_management_view_model().form.is_none() => {
                Some(("users".into(), 4))
            }
            ShellScreen::Diagnostics if self.diagnostics_repair_preview.is_empty() => {
                Some((format!("diagnostics.{:?}", self.diagnostics_tab), 3))
            }
            ShellScreen::Logs if !self.logs_has_active_overlay() => Some((
                self.logs_sort_key(),
                ui::logs_content_model(&self.to_logs_view_model())
                    .table_data()
                    .0
                    .len(),
            )),
            ShellScreen::SystemStatus if self.diagnostics_repair_preview.is_empty() => {
                let ui::SystemStatusRoute::Detail(detail) = self.system_status_route else {
                    return None;
                };
                if let Some(tab) = detail.diagnostics_tab() {
                    Some((format!("diagnostics.{tab:?}"), 3))
                } else if detail == ui::SystemStatusDetail::Processes {
                    Some(("processes".into(), 4))
                } else {
                    Some((
                        format!("system-status.{:?}", self.system_status_route),
                        self.to_system_status_view_model()?.table_data().0.len(),
                    ))
                }
            }
            _ => None,
        }
    }

    pub(in crate::session) fn next_table_sort_column(&self, reverse: bool) -> Option<usize> {
        let (key, count) = self.active_table_sort()?;
        if count == 0 {
            return None;
        }
        let process_sort = ui::TableSort {
            column: match self.system_status_process_sort.column {
                ui::SystemStatusProcessSortColumn::Pid => 0,
                ui::SystemStatusProcessSortColumn::Name => 1,
                ui::SystemStatusProcessSortColumn::Cpu => 2,
                ui::SystemStatusProcessSortColumn::Memory => 3,
            },
            descending: self.system_status_process_sort.descending,
        };
        let sort = if key == "processes" {
            Some(&process_sort)
        } else {
            self.table_sorts.get(&key)
        };
        Some(if reverse {
            sort.map_or(0, |sort| sort.column)
        } else {
            sort.map_or(0, |sort| (sort.column + 1) % count)
        })
    }

    pub(in crate::session) fn table_sort_header_at(&self, point: CellPosition) -> Option<usize> {
        self.table_sort_header_rect_at(point)
            .map(|(column, _)| column)
    }

    pub(in crate::session) fn table_sort_button_at(
        &self,
        point: CellPosition,
    ) -> Option<ui::components::ButtonRegion> {
        let (column, area) = self.table_sort_header_rect_at(point)?;
        let id = match self.active_screen() {
            ShellScreen::UserManagement => format!("user-management.sort.{column}"),
            ShellScreen::Launcher => format!("launcher.sort.{column}"),
            ShellScreen::Clock => format!(
                "clock.sort.{}",
                if column == 0 { "alarm" } else { "countdown" }
            ),
            ShellScreen::SystemStatus
                if self.system_status_route
                    == ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Processes) =>
            {
                format!("system-status.process.sort.{}", [0, 3, 1, 2][column])
            }
            ShellScreen::SystemStatus if matches!(self.system_status_route, ui::SystemStatusRoute::Detail(d) if d.diagnostics_tab().is_none()) =>
            {
                format!("system-status.{:?}.sort.{column}", self.system_status_route)
            }
            _ => format!("diagnostics.table.sort.{column}"),
        };
        Some(ui::components::ButtonRegion {
            id: id.into(),
            area,
            disabled: false,
        })
    }

    fn table_sort_header_rect_at(&self, point: CellPosition) -> Option<(usize, Rect)> {
        self.active_table_sort()?;
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let main = match self.shell_layout_for(area) {
            ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
        };
        let headers = match self.active_screen() {
            ShellScreen::Launcher => ui::launcher_sort_headers(
                &ui::launcher_layout(main, &self.to_launcher_view_model()),
                &self.to_launcher_view_model(),
            ),
            ShellScreen::Clock => {
                let layout = ui::clock_page_layout(main, &self.to_clock_view_model());
                vec![(0, layout.alarms_heading), (1, layout.countdowns_heading)]
            }
            ShellScreen::UserManagement => ui::user_management_sort_headers(
                &ui::user_management_layout(main, &self.to_user_management_view_model()),
            ),
            ShellScreen::Diagnostics => {
                ui::diagnostics_layout(main, &self.to_diagnostics_view_model()).headers
            }
            ShellScreen::Logs => {
                ui::logs_layout(main, &self.to_logs_view_model())
                    .content
                    .headers
            }
            ShellScreen::SystemStatus => {
                let layout = ui::system_status_layout(main, &self.to_system_status_view_model()?);
                if let Some(content) = layout.diagnostics_content {
                    content.headers
                } else if !layout.process_sort_headers.is_empty() {
                    layout
                        .process_sort_headers
                        .into_iter()
                        .map(|(c, r)| {
                            (
                                match c {
                                    ui::SystemStatusProcessSortColumn::Pid => 0,
                                    ui::SystemStatusProcessSortColumn::Name => 1,
                                    ui::SystemStatusProcessSortColumn::Cpu => 2,
                                    ui::SystemStatusProcessSortColumn::Memory => 3,
                                },
                                r,
                            )
                        })
                        .collect()
                } else {
                    layout.table_headers
                }
            }
            _ => return None,
        };
        headers
            .into_iter()
            .find_map(|(column, area)| rect_contains(area, point).then_some((column, area)))
    }

    pub(in crate::session) fn sort_active_table(&mut self, column: usize) {
        let Some((key, count)) = self.active_table_sort() else {
            return;
        };
        if column >= count {
            return;
        }
        if key == "processes" {
            self.sort_system_status_processes(match column {
                0 => ui::SystemStatusProcessSortColumn::Pid,
                1 => ui::SystemStatusProcessSortColumn::Name,
                2 => ui::SystemStatusProcessSortColumn::Cpu,
                _ => ui::SystemStatusProcessSortColumn::Memory,
            });
            return;
        }
        if key == "clock" {
            let group = format!("clock.{column}");
            let sort = ui::TableSort::toggle(self.table_sorts.get(&group).copied(), 0);
            self.table_sorts.insert(group, sort);
            self.table_sorts.insert(
                key,
                ui::TableSort {
                    column,
                    descending: sort.descending,
                },
            );
            self.sync_clock_window_at(Instant::now());
            return;
        }
        let previous_system_row = self
            .to_system_status_view_model()
            .and_then(|m| {
                m.table_data()
                    .1
                    .get(self.system_status_selected_row)
                    .cloned()
            })
            .and_then(|row| row.first().cloned());
        let sort = ui::TableSort::toggle(self.table_sorts.get(&key).copied(), column);
        self.table_sorts.insert(key, sort);
        match self.active_screen() {
            ShellScreen::Launcher => {
                self.launcher_viewport_offset = 0;
                self.launcher_drag = None;
            }
            ShellScreen::UserManagement => self.apply_user_table_sort(),
            ShellScreen::Diagnostics => self.apply_diagnostics_table_sorts(),
            ShellScreen::SystemStatus => {
                self.apply_diagnostics_table_sorts();
                if let Some(id) = previous_system_row {
                    self.system_status_selected_row = self
                        .to_system_status_view_model()
                        .and_then(|m| {
                            m.table_data()
                                .1
                                .iter()
                                .position(|row| row.first() == Some(&id))
                        })
                        .unwrap_or(0);
                }
                self.system_status_scroll_offset = 0;
            }
            ShellScreen::Logs => {
                self.logs_state.paused = true;
                self.apply_logs_sort();
                self.logs_state.scroll = self.logs_state.selected;
                self.logs_state.explicit_scroll = false;
            }
            _ => {}
        }
    }

    pub(in crate::session) fn sort_system_status_processes(
        &mut self,
        column: ui::SystemStatusProcessSortColumn,
    ) {
        let selected = self
            .to_system_status_view_model()
            .and_then(|m| {
                m.table_data()
                    .1
                    .get(self.system_status_selected_row)
                    .cloned()
            })
            .and_then(|row| row.first().cloned());
        self.system_status_process_sort.toggle(column);
        self.system_status_selected_row = selected
            .and_then(|id| {
                self.to_system_status_view_model()
                    .and_then(|m| m.table_data().1.iter().position(|r| r.first() == Some(&id)))
            })
            .unwrap_or(0);
        self.system_status_scroll_offset = 0;
    }

    pub(in crate::session) fn sort_system_status_view_model(
        &self,
        model: &mut ui::SystemStatusViewModel,
    ) {
        let Some(sort) = model.table_sort else {
            return;
        };
        let ui::SystemStatusRoute::Detail(detail) = model.route else {
            return;
        };
        let cells = model.table_data().1;
        let mut unused = 0;
        if let ui::SystemStatusContentViewModel::Admin(a) = &mut model.content {
            match detail {
                ui::SystemStatusDetail::Storage => {
                    reorder(&mut a.storage_rows, &cells, sort, &mut unused)
                }
                ui::SystemStatusDetail::Network => {
                    reorder(&mut a.network_rows, &cells, sort, &mut unused)
                }
                _ => {}
            }
        }
        for widget in model
            .dashboard
            .wide_widgets
            .iter_mut()
            .chain(&mut model.dashboard.narrow_widgets)
        {
            if widget.kind.detail() == detail {
                widget.compact_rows.sort_by(|a, b| {
                    sort.compare(
                        a.get(sort.column).map_or("", String::as_str),
                        b.get(sort.column).map_or("", String::as_str),
                    )
                });
            }
        }
    }

    pub(in crate::session) fn apply_user_table_sort(&mut self) {
        let Some(sort) = self.table_sorts.get("users").copied() else {
            return;
        };
        let mut users = self.app.managed_users().to_vec();
        let cells = users
            .iter()
            .map(|u| {
                vec![
                    u.username.clone(),
                    u.display_name.clone(),
                    u.role.as_str().into(),
                    format!(
                        "{} {}",
                        u.enabled,
                        u.locked_until_epoch_ms
                            .is_some_and(|until| until > unix_millis())
                    ),
                ]
            })
            .collect::<Vec<_>>();
        let mut selected = self.user_management_selected;
        reorder(&mut users, &cells, sort, &mut selected);
        self.app
            .dispatch_at(app::AppCommand::SetManagedUsers(users), Instant::now());
        self.user_management_selected = selected;
        self.user_management_window_start = selected;
    }

    pub(in crate::session) fn apply_logs_sort(&mut self) {
        let Some(sort) = self.table_sorts.get(&self.logs_sort_key()).copied() else {
            return;
        };
        let cells = ui::logs_content_model(&self.to_logs_view_model())
            .table_data()
            .1;
        let state = &mut self.logs_state;
        if state.category == ui::LogsCategory::Linux || state.section == ui::LogsSection::Events {
            reorder(
                &mut state.snapshot.result.events,
                &cells,
                sort,
                &mut state.selected,
            );
        } else if state.section == ui::LogsSection::Files {
            reorder(&mut state.snapshot.files, &cells, sort, &mut state.selected);
        } else {
            reorder(
                &mut state.snapshot.incidents,
                &cells,
                sort,
                &mut state.selected,
            );
        }
        state.revision = state.revision.wrapping_add(1);
    }

    pub(in crate::session) fn apply_diagnostics_table_sorts(&mut self) {
        let Some(mut snapshot) = self.app.diagnostics_snapshot().cloned() else {
            return;
        };
        let mut model = self.to_diagnostics_view_model();
        for tab in ui::DiagnosticsTab::ALL {
            let Some(sort) = self
                .table_sorts
                .get(&format!("diagnostics.{tab:?}"))
                .copied()
            else {
                continue;
            };
            model.tab = tab;
            let cells = model.table_data().1;
            let mut selected = match tab {
                ui::DiagnosticsTab::Health => self.diagnostics_selected_check,
                ui::DiagnosticsTab::Logs => self.diagnostics_selected_log,
                ui::DiagnosticsTab::Incidents => self.diagnostics_selected_incident,
            };
            match tab {
                ui::DiagnosticsTab::Health => {
                    reorder(&mut snapshot.checks, &cells, sort, &mut selected);
                    self.diagnostics_selected_check = selected;
                }
                ui::DiagnosticsTab::Logs => {
                    reorder(&mut snapshot.logs, &cells, sort, &mut selected);
                    self.diagnostics_selected_log = selected;
                }
                ui::DiagnosticsTab::Incidents => {
                    reorder(&mut snapshot.incidents, &cells, sort, &mut selected);
                    self.diagnostics_selected_incident = selected;
                }
            }
        }
        self.app.dispatch_at(
            app::AppCommand::SetDiagnosticsSnapshot(Some(snapshot)),
            Instant::now(),
        );
        self.diagnostics_list_window_is_explicit = false;
    }
}
