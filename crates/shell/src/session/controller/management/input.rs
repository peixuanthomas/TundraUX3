use super::*;
use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn handle_management_paste(&mut self, value: &str) {
        if self
            .pages
            .management
            .management_state
            .choice_field
            .is_some()
        {
            self.search_management_choices(value, false);
            return;
        }
        if self.handle_management_form_event(InputEvent::Paste(value.to_owned())) {
            return;
        }
        if self.pages.management.management_state.filtering {
            if self.pages.management.management_state.filter_input.len() + value.len() <= 16 * 1024
            {
                self.pages
                    .management
                    .management_state
                    .filter_input
                    .extend(value.chars().filter(|c| !c.is_control()));
            }
        } else if self.pages.management.management_state.terminal_mode {
            let bracketed = self
                .pages
                .management
                .management_state
                .parser
                .as_ref()
                .and_then(|p| p.0.lock().ok().map(|p| p.screen().bracketed_paste()))
                .unwrap_or(false);
            self.management_send(OperationInput::Terminal {
                bytes: if bracketed {
                    format!("\x1b[200~{value}\x1b[201~").into_bytes()
                } else {
                    value.as_bytes().to_vec()
                },
            });
        }
    }

    pub(in crate::session) fn handle_management_key(&mut self, key: &KeyInput) {
        if key.phase == InputPhase::Release {
            if self
                .pages
                .management
                .management_state
                .shortcut_repeat_guard
                .as_ref()
                == Some(&key.key)
            {
                self.pages.management.management_state.shortcut_repeat_guard = None;
            }
            return;
        }
        if self.notification_has_active_modal()
            || self.time_sync_dialog_visible
            || self.pages.common.active_popup.is_some()
        {
            return;
        }
        if key.phase == InputPhase::Repeat
            && self
                .pages
                .management
                .management_state
                .shortcut_repeat_guard
                .as_ref()
                == Some(&key.key)
        {
            return;
        }
        if key.phase == InputPhase::Press {
            self.pages.management.management_state.shortcut_repeat_guard = None;
        }
        if key.phase == InputPhase::Repeat && matches!(key.key, InputKey::Enter | InputKey::Escape)
        {
            return;
        }
        if self.handle_management_choice_key(key) {
            return;
        }
        if matches!(
            key.key,
            InputKey::Tab
                | InputKey::BackTab
                | InputKey::Up
                | InputKey::Down
                | InputKey::Home
                | InputKey::End
                | InputKey::PageUp
                | InputKey::PageDown
        ) {
            self.pages.management.management_state.list_scroll_explicit = false;
            self.pages.management.management_state.action_scroll = None;
            if !matches!(key.key, InputKey::PageUp | InputKey::PageDown) {
                self.pages.management.management_state.form_field_scroll = None;
            }
        }
        if key.key == InputKey::Escape {
            self.cancel_management_pointer_gesture();
        }
        if key.is_ctrl_c() && !self.pages.management.management_state.terminal_mode {
            return;
        }
        if management_control_modifier(key) && matches!(key.key, InputKey::Char('t' | 'T')) {
            if key.phase == InputPhase::Press {
                self.management_touch_control(ui::ManagementControl::Terminal);
            }
            return;
        }
        if self.pages.management.management_state.terminal_mode
            && self.pages.management.management_state.form.is_some()
        {
            if matches!(key.key, InputKey::PageUp | InputKey::PageDown) {
                self.scroll_management_terminal(if key.key == InputKey::PageUp { 10 } else { -10 });
            }
            return;
        }
        if self.handle_management_form_key(key) {
            return;
        }
        if self.handle_management_selection_key(key) || self.handle_package_workspace_key(key) {
            return;
        }
        if self.pages.management.management_state.filtering {
            if management_control_modifier(key) && key.key == InputKey::Enter {
                self.apply_management_filter();
                return;
            }
            if management_control_modifier(key) && matches!(key.key, InputKey::Char('u' | 'U')) {
                if key.phase == InputPhase::Press {
                    self.management_touch_control(ui::ManagementControl::ClearSearch);
                }
                return;
            }
            if key.has_non_shift_modifier() {
                return;
            }
            match key.key {
                InputKey::Escape => {
                    self.pages.management.management_state.filtering = false;
                    if self.pages.management.management_state.kind == Some(ManagementKind::Packages)
                    {
                        self.focus_package_workspace(ui::components::WorkspaceFocus::List);
                    }
                }
                InputKey::Enter => {
                    self.apply_management_filter();
                }
                InputKey::Tab | InputKey::BackTab => {
                    self.apply_management_filter();
                    self.pages.management.management_state.actions_focused = false;
                }
                InputKey::F(5) if key.phase == InputPhase::Press => {
                    self.management_touch_control(ui::ManagementControl::Refresh);
                }
                InputKey::Backspace => {
                    self.pages.management.management_state.filter_input.pop();
                }
                InputKey::Space => self
                    .pages
                    .management
                    .management_state
                    .filter_input
                    .push(' '),
                InputKey::Char(c) => {
                    if !c.is_control() {
                        self.pages.management.management_state.filter_input.push(c);
                    }
                }
                _ => {}
            }
            return;
        }
        if key.modifiers.alt && matches!(key.key, InputKey::PageUp | InputKey::PageDown) {
            self.pages.management.management_state.details_scroll = if key.key == InputKey::PageUp {
                self.pages
                    .management
                    .management_state
                    .details_scroll
                    .saturating_sub(5)
            } else {
                self.pages
                    .management
                    .management_state
                    .details_scroll
                    .saturating_add(5)
            };
            return;
        }
        if self.pages.management.management_state.terminal_mode {
            if self.pages.management.management_state.kind == Some(ManagementKind::Tasks) {
                match key.key {
                    InputKey::Escape => {
                        let state = &mut self.pages.management.management_state;
                        state.terminal_mode = false;
                        if let Some(query) = &mut state.query {
                            query.options.remove("output_view");
                        }
                    }
                    InputKey::Up | InputKey::PageUp => {
                        self.scroll_management_terminal(if key.key == InputKey::PageUp {
                            10
                        } else {
                            1
                        })
                    }
                    InputKey::Down | InputKey::PageDown => {
                        self.scroll_management_terminal(if key.key == InputKey::PageDown {
                            -10
                        } else {
                            -1
                        })
                    }
                    _ => {}
                }
                return;
            }
            if key.modifiers.shift && matches!(key.key, InputKey::PageUp | InputKey::PageDown) {
                self.scroll_management_terminal(if key.key == InputKey::PageUp { 10 } else { -10 });
                return;
            }
            let input = match key.key {
                InputKey::Char(c) if key.modifiers.control && c.is_ascii() => {
                    TerminalInput::Bytes(vec![(c.to_ascii_lowercase() as u8) & 0x1f])
                }
                InputKey::Char(c) => TerminalInput::Text(c.to_string()),
                InputKey::Enter => TerminalInput::Enter,
                InputKey::Backspace => TerminalInput::Backspace,
                InputKey::Space => TerminalInput::Text(" ".into()),
                InputKey::Tab => TerminalInput::Tab,
                InputKey::Escape => TerminalInput::Escape,
                InputKey::Up => TerminalInput::Up,
                InputKey::Down => TerminalInput::Down,
                InputKey::Left => TerminalInput::Left,
                InputKey::Right => TerminalInput::Right,
                InputKey::Home => TerminalInput::Home,
                InputKey::End => TerminalInput::End,
                InputKey::Delete => TerminalInput::Delete,
                InputKey::PageUp => TerminalInput::PageUp,
                InputKey::PageDown => TerminalInput::PageDown,
                _ => return,
            };
            self.management_send(OperationInput::Terminal {
                bytes: encode_terminal_input(
                    &input,
                    self.pages
                        .management
                        .management_state
                        .parser
                        .as_ref()
                        .and_then(|p| p.0.lock().ok().map(|p| p.screen().application_cursor()))
                        .unwrap_or(false),
                ),
            });
            return;
        }
        if key.modifiers.is_control()
            && !key.modifiers.alt
            && !key.modifiers.super_key
            && !key.modifiers.hyper
            && !key.modifiers.meta
            && key.key == InputKey::F(6)
        {
            if key.phase == InputPhase::Press {
                let count = self
                    .pages
                    .management
                    .management_state
                    .snapshot
                    .columns
                    .len();
                if count > 0 {
                    let column = if key.modifiers.shift {
                        self.pages
                            .management
                            .management_state
                            .sort
                            .map_or(0, |sort| sort.column)
                    } else {
                        self.pages
                            .management
                            .management_state
                            .sort
                            .map_or(0, |sort| (sort.column + 1) % count)
                    };
                    self.sort_management(column);
                }
            }
            return;
        }
        if management_control_modifier(key) {
            if key.phase == InputPhase::Press {
                match key.key {
                    InputKey::Enter => {
                        self.management_touch_control(ui::ManagementControl::ApplySearch)
                    }
                    InputKey::Char('u' | 'U') => {
                        self.management_touch_control(ui::ManagementControl::ClearSearch)
                    }
                    _ => {}
                }
            }
            return;
        }
        if key.modifiers.alt
            && !key.modifiers.is_control()
            && !key.modifiers.shift
            && !key.modifiers.super_key
            && !key.modifiers.hyper
            && !key.modifiers.meta
            && matches!(key.key, InputKey::Left | InputKey::Right)
        {
            self.page_management_actions(key.key == InputKey::Right);
            return;
        }
        if key.has_non_shift_modifier() {
            return;
        }
        if key.modifiers.shift && matches!(key.key, InputKey::F(_)) {
            return;
        }
        let action_key = match key.key {
            InputKey::Char(c) => InputKey::Char(c.to_ascii_lowercase()),
            _ => key.key.clone(),
        };
        let shortcut_actions =
            if self.pages.management.management_state.kind == Some(ManagementKind::Packages) {
                self.management_actions()
            } else {
                self.all_management_actions()
            };
        if let Some((action, row)) = shortcut_actions.into_iter().find(|(action, _)| {
            management_action_shortcut(self.pages.management.management_state.kind, &action.id)
                .is_some_and(|(_, shortcut)| shortcut == action_key)
        }) {
            if key.phase == InputPhase::Press {
                self.activate_management_item(action, row);
                self.pages.management.management_state.shortcut_repeat_guard =
                    Some(key.key.clone());
            }
            return;
        }
        if key.phase == InputPhase::Repeat
            && matches!(
                key.key,
                InputKey::Char('0'..='9' | '/' | 'r' | 'R' | 's' | 'S') | InputKey::F(4 | 5)
            )
        {
            return;
        }
        if matches!(
            key.key,
            InputKey::Char('0'..='9' | '/' | 'r' | 'R' | 's' | 'S') | InputKey::F(4 | 5)
        ) {
            self.pages.management.management_state.shortcut_repeat_guard = Some(key.key.clone());
        }
        match key.key {
            InputKey::Escape
                if (self.pages.management.management_state.kind
                    == Some(ManagementKind::Packages)
                    || self
                        .pages
                        .management
                        .management_state
                        .snapshot
                        .details_complete
                    || self.pages.management.management_state.problem.is_some()
                    || self
                        .pages
                        .management
                        .management_state
                        .snapshot
                        .rows
                        .is_empty())
                    && self.pages.management.management_state.details_only =>
            {
                self.toggle_management_details();
            }
            InputKey::Escape => {
                self.return_from_screen(ShellScreen::Management);
            }
            InputKey::Char('/' | 's' | 'S') => {
                self.management_touch_control(ui::ManagementControl::Search)
            }
            InputKey::Char('r' | 'R') | InputKey::F(5) => {
                self.management_touch_control(ui::ManagementControl::Refresh)
            }
            InputKey::F(4) => self.management_touch_control(ui::ManagementControl::Details),
            InputKey::Tab | InputKey::BackTab => {
                self.pages.management.management_state.actions_focused =
                    !self.pages.management.management_state.actions_focused
            }
            InputKey::Enter => {
                if self.pages.management.management_state.actions_focused {
                    self.activate_management_action(
                        self.pages.management.management_state.selected_action,
                    );
                } else {
                    if self.pages.management.management_state.kind == Some(ManagementKind::Packages)
                    {
                        self.toggle_management_details();
                        return;
                    }
                    self.pages.management.management_state.actions_focused = true;
                    if let Some(id) = self
                        .pages
                        .management
                        .management_state
                        .snapshot
                        .rows
                        .get(self.pages.management.management_state.selected)
                        .map(|r| r.id.clone())
                    {
                        if let Some(query) = &mut self.pages.management.management_state.query {
                            query.target = Some(id);
                        }
                        self.refresh_management();
                    }
                }
            }
            InputKey::Char(c) if ('1'..='9').contains(&c) => {
                self.activate_management_action(c as usize - '1' as usize)
            }
            InputKey::Char('0') => self.activate_management_action(9),
            InputKey::Up
            | InputKey::Down
            | InputKey::PageUp
            | InputKey::PageDown
            | InputKey::Home
            | InputKey::End => {
                let actions = self.management_actions();
                let rows_count = self.pages.management.management_state.snapshot.rows.len();
                let (selected, count) = if self.pages.management.management_state.actions_focused {
                    (
                        &mut self.pages.management.management_state.selected_action,
                        actions.len(),
                    )
                } else {
                    (
                        &mut self.pages.management.management_state.selected,
                        rows_count,
                    )
                };
                *selected = match key.key {
                    InputKey::Up => selected.saturating_sub(1),
                    InputKey::Down => (*selected + 1).min(count.saturating_sub(1)),
                    InputKey::PageUp => selected.saturating_sub(10),
                    InputKey::PageDown => (*selected + 10).min(count.saturating_sub(1)),
                    InputKey::Home => 0,
                    InputKey::End => count.saturating_sub(1),
                    _ => *selected,
                };
            }
            _ => {}
        }
        self.clamp_management_scroll();
    }

    pub(super) fn toggle_management_details(&mut self) {
        if !self.pages.management.management_state.can_show_details()
            && !self.pages.management.management_state.details_only
        {
            return;
        }
        self.pages.management.management_state.details_only =
            !self.pages.management.management_state.details_only;
        self.pages.management.management_state.details_scroll = 0;
        self.pages
            .management
            .management_state
            .details_horizontal_scroll = 0;
        let package_view =
            self.pages.management.management_state.kind == Some(ManagementKind::Packages);
        self.pages.management.management_state.actions_focused = !package_view;
        if package_view {
            self.focus_package_workspace(if self.pages.management.management_state.details_only {
                ui::components::WorkspaceFocus::Details
            } else {
                ui::components::WorkspaceFocus::List
            });
        }
        if self.pages.management.management_state.kind == Some(ManagementKind::Backups)
            && !self.pages.management.management_state.details_only
        {
            if let Some(query) = &mut self.pages.management.management_state.query {
                query.target = None;
            }
            self.clamp_management_scroll();
            return;
        }
        if self.pages.management.management_state.problem.is_some()
            || self
                .pages
                .management
                .management_state
                .snapshot
                .rows
                .is_empty()
            || self
                .pages
                .management
                .management_state
                .snapshot
                .details_complete
        {
            self.clamp_management_scroll();
            return;
        }
        let target = self
            .pages
            .management
            .management_state
            .snapshot
            .rows
            .get(self.pages.management.management_state.selected)
            .map(|row| row.id.clone());
        let details_only = self.pages.management.management_state.details_only;
        if let Some(query) = &mut self.pages.management.management_state.query {
            query.target = if package_view
                && (!details_only
                    || !matches!(
                        query.scope.as_str(),
                        "" | "installed" | "search" | "updates"
                    )) {
                None
            } else {
                target
            };
        }
        self.refresh_management();
    }

    pub(in crate::session) fn sort_management(&mut self, column: usize) {
        let state = &mut self.pages.management.management_state;
        if column >= state.snapshot.columns.len() {
            return;
        }
        let selected = state
            .snapshot
            .rows
            .get(state.selected)
            .map(|row| row.id.clone());
        state.sort = Some(ui::TableSort::toggle(state.sort, column));
        state.sort_columns = state.snapshot.columns.clone();
        let sort = state.sort.unwrap();
        if state.kind == Some(ManagementKind::Backups) {
            if let Some(query) = &mut state.query {
                query
                    .options
                    .insert("sort_column".into(), sort.column.to_string());
                query
                    .options
                    .insert("sort_descending".into(), sort.descending.to_string());
                query.options.remove("cursor");
                if let Some(id) = &selected {
                    query.options.insert("anchor".into(), id.clone());
                }
                query.target = None;
            }
            state.details_only = false;
            state.list_scroll_explicit = false;
            state.actions_focused = false;
            self.refresh_management();
            return;
        }
        state.snapshot.rows.sort_by(|a, b| {
            sort.compare(
                a.cells.get(column).map_or("", String::as_str),
                b.cells.get(column).map_or("", String::as_str),
            )
        });
        state.selected = selected
            .and_then(|id| state.snapshot.rows.iter().position(|row| row.id == id))
            .unwrap_or(0);
        state.list_scroll_explicit = false;
        state.actions_focused = false;
        state.revision = state.revision.wrapping_add(1);
        if self.pages.management.management_state.kind == Some(ManagementKind::Packages) {
            self.focus_package_workspace(ui::components::WorkspaceFocus::List);
        }
        self.clamp_management_scroll();
    }

    pub(in crate::session) fn scroll_management_terminal(&mut self, delta: isize) {
        if let Some(parser) = &self.pages.management.management_state.parser {
            if let Ok(mut parser) = parser.0.lock() {
                let position = parser.screen().scrollback().saturating_add_signed(delta);
                parser.set_scrollback(position);
            }
        } else {
            self.pages.management.management_state.output_scroll = self
                .pages
                .management
                .management_state
                .output_scroll
                .saturating_add_signed((-delta).clamp(i16::MIN as isize, i16::MAX as isize) as i16);
        }
    }
}

pub(super) fn toggle_management_check(form: &mut ManagementEditor, index: usize, cell: usize) {
    let field = &mut form.fields[index];
    match field.control {
        FieldControl::Checkbox => field.value = (field.value != "true").to_string(),
        FieldControl::Permissions => {
            // A cell switched back to its original value still applies to recursive children.
            if let FormPurpose::Action(action, _) = &mut form.purpose
                && let Some(initial) = action.values.get_mut(&format!("{}_initial", field.id))
                && initial.len() == 9
                && cell < 9
            {
                initial.replace_range(cell..cell + 1, "m");
            }
            let mut bytes = field.value.as_bytes().to_vec();
            if let Some(value) = bytes.get_mut(cell) {
                *value = if *value == b'1' { b'0' } else { b'1' };
            }
            field.value = String::from_utf8(bytes).expect("permission states are ASCII");
        }
        FieldControl::Text | FieldControl::SearchableChoice => {}
    }
}
