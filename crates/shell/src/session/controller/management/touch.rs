use super::*;

fn management_form_is_menu(form: &ManagementEditor) -> bool {
    matches!(&form.purpose, FormPurpose::Menu(_))
        || matches!(&form.purpose, FormPurpose::Configuration(action) if matches!(action.as_str(), "menu" | "edit-file" | "edit-language" | "edit-newline" | "edit-encoding" | "edit-indent"))
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/management_touch/tests.rs"]
mod tests;

impl ShellSession {
    pub(super) fn handle_management_choice_key(&mut self, key: &KeyInput) -> bool {
        let Some(index) = self.pages.management.management_state.choice_field else {
            return false;
        };
        if key.has_non_shift_modifier() {
            return true;
        }
        let selected = self.pages.management.management_state.choice_selected;
        let page = ui::management_layout(self.management_main(), &self.to_management_view_model())
            .choice_rows
            .len()
            .max(1);
        let count = self
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .and_then(|form| form.fields.get(index))
            .map_or(0, |field| field.choices.len());
        let shortcut_key = match key.key {
            InputKey::Char(c) => InputKey::Char(c.to_ascii_lowercase()),
            _ => key.key.clone(),
        };
        let shortcut = self
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .and_then(|form| {
                let FormPurpose::Menu(items) = &form.purpose else {
                    return None;
                };
                items.iter().position(|(action, _)| {
                    management_action_shortcut(
                        self.pages.management.management_state.kind,
                        &action.id,
                    )
                    .is_some_and(|(_, shortcut)| shortcut == shortcut_key)
                })
            });
        if let Some(selected) = shortcut {
            if key.phase != InputPhase::Press {
                return true;
            }
            let disabled = self
                .to_management_view_model()
                .form
                .as_ref()
                .and_then(|form| form.choice.as_ref())
                .and_then(|choice| choice.disabled.get(selected))
                .copied()
                .unwrap_or(false);
            if disabled {
                return true;
            }
            if let Some(field) = self
                .pages
                .management
                .management_state
                .form
                .as_mut()
                .and_then(|form| form.fields.get_mut(index))
            {
                field.value = field.choices[selected].clone();
            }
            self.pages.management.management_state.choice_selected = selected;
            self.pages.management.management_state.choice_field = None;
            self.submit_management_form();
            self.pages.management.management_state.shortcut_repeat_guard = Some(key.key.clone());
            return true;
        }
        let searchable = self
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .and_then(|form| form.fields.get(index))
            .is_some_and(|field| field.control == FieldControl::SearchableChoice);
        if searchable {
            match key.key {
                InputKey::Char(c) => {
                    self.search_management_choices(&c.to_string(), false);
                    return true;
                }
                InputKey::Backspace => {
                    self.search_management_choices("", true);
                    return true;
                }
                _ => {}
            }
        }
        match key.key {
            InputKey::Escape => {
                if self
                    .pages
                    .management
                    .management_state
                    .form
                    .as_ref()
                    .is_some_and(management_form_is_menu)
                {
                    self.pages.management.management_state.form = None;
                }
                self.pages.management.management_state.choice_field = None;
            }
            InputKey::Up => {
                self.pages.management.management_state.choice_selected = self
                    .pages
                    .management
                    .management_state
                    .choice_selected
                    .saturating_sub(1)
            }
            InputKey::Down => {
                self.pages.management.management_state.choice_selected =
                    (self.pages.management.management_state.choice_selected + 1)
                        .min(count.saturating_sub(1))
            }
            InputKey::PageUp => {
                self.pages.management.management_state.choice_selected = self
                    .pages
                    .management
                    .management_state
                    .choice_selected
                    .saturating_sub(page)
            }
            InputKey::PageDown => {
                self.pages.management.management_state.choice_selected = self
                    .pages
                    .management
                    .management_state
                    .choice_selected
                    .saturating_add(page)
                    .min(count.saturating_sub(1))
            }
            InputKey::Home => self.pages.management.management_state.choice_selected = 0,
            InputKey::End => {
                self.pages.management.management_state.choice_selected = count.saturating_sub(1)
            }
            InputKey::Enter | InputKey::Space if key.phase != InputPhase::Repeat => {
                if self
                    .to_management_view_model()
                    .form
                    .as_ref()
                    .and_then(|form| form.choice.as_ref())
                    .and_then(|choice| choice.disabled.get(selected))
                    .copied()
                    .unwrap_or(false)
                {
                    return true;
                }
                if let Some(form) = &mut self.pages.management.management_state.form {
                    if let Some(field) = form.fields.get_mut(index) {
                        if let Some(value) = field.choices.get(selected) {
                            field.value = value.clone();
                        }
                    }
                }
                self.pages.management.management_state.choice_field = None;
                if self
                    .pages
                    .management
                    .management_state
                    .form
                    .as_ref()
                    .is_some_and(management_form_is_menu)
                {
                    self.submit_management_form();
                }
            }
            _ => {}
        }
        self.keep_management_choice_visible();
        true
    }
    fn keep_management_choice_visible(&mut self) {
        if self
            .pages
            .management
            .management_state
            .choice_field
            .is_none()
        {
            return;
        }
        let model = self.to_management_view_model();
        let Some(choice) = model.form.as_ref().and_then(|form| form.choice.as_ref()) else {
            return;
        };
        let layout = ui::management_layout(self.management_main(), &model);
        let page = layout.choice_rows.len().max(1);
        let start = layout.choice_rows.first().map_or(0, |(index, _)| *index);
        let selected = choice.selected.min(choice.values.len().saturating_sub(1));
        self.pages.management.management_state.choice_scroll = if selected < start {
            selected
        } else if selected >= start.saturating_add(page) {
            selected.saturating_add(1).saturating_sub(page)
        } else {
            start
        };
    }
    pub(super) fn open_management_choice_field(&mut self, index: usize) {
        let Some(field) = self
            .pages
            .management
            .management_state
            .form
            .as_ref()
            .and_then(|form| form.fields.get(index))
        else {
            return;
        };
        if field.choices.is_empty() {
            return;
        }
        let selected = field
            .choices
            .iter()
            .position(|value| value == &field.value)
            .unwrap_or(0);
        self.pages.management.management_state.choice_field = Some(index);
        self.pages.management.management_state.choice_search.clear();
        self.pages.management.management_state.choice_selected = selected;
        self.pages.management.management_state.choice_scroll = 0;
        self.pages.management.management_state.choice_columns = 0;
        self.keep_management_choice_visible();
    }
    pub(super) fn search_management_choices(&mut self, value: &str, backspace: bool) {
        let state = &mut self.pages.management.management_state;
        let Some(field) = state
            .choice_field
            .and_then(|index| state.form.as_ref().and_then(|form| form.fields.get(index)))
        else {
            return;
        };
        if field.control != FieldControl::SearchableChoice {
            return;
        }
        if backspace {
            state.choice_search.pop();
        } else if state.choice_search.len() + value.len() <= 256 {
            state
                .choice_search
                .extend(value.chars().filter(|c| !c.is_control()));
        }
        if let Some(index) = field.choices.iter().position(|choice| {
            choice
                .to_lowercase()
                .contains(&state.choice_search.to_lowercase())
        }) {
            state.choice_selected = index;
        }
        self.keep_management_choice_visible();
    }
    pub(in crate::session) fn cancel_management_pointer_gesture(&mut self) {
        self.pages.management.management_state.scrollbar_grab = None;
    }
    pub(in crate::session) fn management_pointer_drag_active(&self) -> bool {
        (self.active_screen() == ShellScreen::Management || self.config_editor_form_visible())
            && self
                .pages
                .management
                .management_state
                .scrollbar_grab
                .is_some()
    }
    pub(in crate::session) fn management_overlay_contains(&self, point: CellPosition) -> bool {
        (self.active_screen() == ShellScreen::Management || self.config_editor_form_visible())
            && self.pages.management.management_state.form.is_some()
            && !self.pages.management.management_state.terminal_mode
            && rect_contains(self.management_main(), point)
    }
    pub(in crate::session) fn management_overlay_id(&self) -> Option<&'static str> {
        if self.pages.management.management_state.terminal_mode
            || self.pages.management.management_state.form.is_none()
        {
            None
        } else if self
            .pages
            .management
            .management_state
            .choice_field
            .is_some()
        {
            Some("management-choice")
        } else {
            Some("management-form")
        }
    }
    pub(super) fn reset_management_form_view(&mut self) {
        self.pages.management.management_state.choice_search.clear();
        self.pages.management.management_state.permission_cell = 0;
        self.pages.management.management_state.form_field_scroll = None;
        self.pages.management.management_state.choice_field = None;
        self.pages.management.management_state.choice_scroll = 0;
        self.pages.management.management_state.choice_columns = 0;
        self.cancel_management_pointer_gesture();
    }
    pub(in crate::session) fn management_button_at(
        &self,
        point: CellPosition,
    ) -> Option<ui::components::ButtonRegion> {
        if self.notification_has_active_modal()
            || self.time_sync_dialog_visible
            || self.pages.common.active_popup.is_some()
        {
            return None;
        }
        if self.active_screen() != ShellScreen::Management && !self.config_editor_form_visible() {
            return None;
        }
        ui::management_button_regions(self.management_main(), &self.to_management_view_model())
            .into_iter()
            .find(|button| rect_contains(button.area, point))
    }
    fn sync_management_filter(&mut self) {
        let filter = self.pages.management.management_state.filter_input.clone();
        if let Some(query) = &mut self.pages.management.management_state.query {
            if query.filter != filter {
                query.filter = filter;
                query.target = None;
                self.pages.management.management_state.list_scroll_explicit = false;
                self.pages.management.management_state.selection = None;
            }
        }
    }
    pub(super) fn apply_management_filter(&mut self) {
        self.sync_management_filter();
        if let Some(query) = &mut self.pages.management.management_state.query {
            query.target = None;
        }
        self.pages.management.management_state.filtering = false;
        if self.pages.management.management_state.kind == Some(ManagementKind::Packages) {
            self.focus_package_workspace(ui::components::WorkspaceFocus::List);
        }
        self.pages.management.management_state.list_scroll_explicit = false;
        self.refresh_management();
    }
    pub(super) fn management_touch_control(&mut self, control: ui::ManagementControl) {
        match control {
            ui::ManagementControl::Refresh => {
                self.pages.management.management_state.outcome = None;
                self.sync_management_filter();
                self.refresh_management();
            }
            ui::ManagementControl::Search => {
                self.pages.management.management_state.filtering = true;
                if self.pages.management.management_state.kind == Some(ManagementKind::Packages) {
                    self.focus_package_workspace(ui::components::WorkspaceFocus::Search);
                }
            }
            ui::ManagementControl::ApplySearch => self.apply_management_filter(),
            ui::ManagementControl::ClearSearch => {
                self.pages.management.management_state.filter_input.clear();
                self.apply_management_filter();
            }
            ui::ManagementControl::Details => {
                self.toggle_management_details();
            }
            ui::ManagementControl::Terminal => {
                if let Some(job) = self
                    .pages
                    .management
                    .management_state
                    .auto_admin_job
                    .clone()
                {
                    self.show_auto_admin_job(job);
                    return;
                }
                self.pages.management.management_state.terminal_mode =
                    !self.pages.management.management_state.terminal_mode;
                self.resize_management_terminal();
            }
        }
    }
    pub(super) fn page_management_actions(&mut self, next: bool) {
        let model = self.to_management_view_model();
        let layout = ui::management_layout(self.management_main(), &model);
        let button = if next {
            layout.action_next
        } else {
            layout.action_previous
        };
        if button.is_empty()
            || (!next && layout.action_start == 0)
            || (next && layout.action_start + layout.actions.len() >= model.actions.len())
        {
            return;
        }
        let end = model.actions.len().saturating_sub(layout.actions.len());
        self.pages.management.management_state.action_scroll = Some(
            layout
                .action_start
                .saturating_add_signed(if next { 1 } else { -1 })
                .min(end),
        );
    }
    fn set_management_scroll(
        &mut self,
        target: ui::ManagementScrollTarget,
        offset: usize,
        bar: ui::ManagementScrollbar,
    ) {
        let short = offset.min(u16::MAX as usize) as u16;
        match target {
            ui::ManagementScrollTarget::Rows => {
                self.pages.management.management_state.scroll = offset;
                self.pages.management.management_state.list_scroll_explicit = true;
            }
            ui::ManagementScrollTarget::Columns => {
                self.pages.management.management_state.table_scroll = offset
            }
            ui::ManagementScrollTarget::Details => {
                self.pages.management.management_state.details_scroll = short
            }
            ui::ManagementScrollTarget::DetailsColumns => {
                self.pages
                    .management
                    .management_state
                    .details_horizontal_scroll = short
            }
            ui::ManagementScrollTarget::Actions => {
                self.pages.management.management_state.action_scroll = Some(offset)
            }
            ui::ManagementScrollTarget::FormMessage => {
                if let Some(form) = &mut self.pages.management.management_state.form {
                    form.message_scroll = short;
                }
            }
            ui::ManagementScrollTarget::FormFields => {
                self.pages.management.management_state.form_field_scroll = Some(offset)
            }
            ui::ManagementScrollTarget::Choices => {
                self.pages.management.management_state.choice_scroll = offset
            }
            ui::ManagementScrollTarget::ChoiceColumns => {
                self.pages.management.management_state.choice_columns = offset
            }
            ui::ManagementScrollTarget::Output => {
                if let Some(parser) = &self.pages.management.management_state.parser {
                    if let Ok(mut parser) = parser.0.lock() {
                        parser.set_scrollback(
                            bar.content_len
                                .saturating_sub(bar.viewport_len)
                                .saturating_sub(offset),
                        );
                    }
                } else {
                    self.pages.management.management_state.output_scroll = short;
                }
            }
        }
    }
    fn drag_management_scrollbar(&mut self, point: CellPosition) {
        let Some((target, grab)) = self.pages.management.management_state.scrollbar_grab else {
            return;
        };
        let layout =
            ui::management_layout(self.management_main(), &self.to_management_view_model());
        if let Some(bar) = layout
            .scrollbars
            .iter()
            .find(|bar| bar.target == target)
            .copied()
        {
            self.set_management_scroll(target, bar.offset_at(point, grab), bar);
        } else {
            self.cancel_management_pointer_gesture();
        }
    }
    fn visible_management_bars(
        &self,
        layout: &ui::ManagementLayout,
    ) -> Vec<ui::ManagementScrollbar> {
        layout
            .scrollbars
            .iter()
            .copied()
            .filter(|bar| {
                if self
                    .pages
                    .management
                    .management_state
                    .choice_field
                    .is_some()
                {
                    matches!(
                        bar.target,
                        ui::ManagementScrollTarget::Choices
                            | ui::ManagementScrollTarget::ChoiceColumns
                    )
                } else if self.pages.management.management_state.form.is_some()
                    && !self.pages.management.management_state.terminal_mode
                {
                    matches!(
                        bar.target,
                        ui::ManagementScrollTarget::FormMessage
                            | ui::ManagementScrollTarget::FormFields
                    )
                } else if self.pages.management.management_state.terminal_mode {
                    bar.target == ui::ManagementScrollTarget::Output
                } else {
                    matches!(
                        bar.target,
                        ui::ManagementScrollTarget::Rows
                            | ui::ManagementScrollTarget::Columns
                            | ui::ManagementScrollTarget::Details
                            | ui::ManagementScrollTarget::DetailsColumns
                            | ui::ManagementScrollTarget::Actions
                    )
                }
            })
            .collect()
    }
    pub(super) fn clamp_management_scroll(&mut self) {
        let count = self.pages.management.management_state.snapshot.rows.len();
        self.pages.management.management_state.selected = self
            .pages
            .management
            .management_state
            .selected
            .min(count.saturating_sub(1));
        let layout =
            ui::management_layout(self.management_main(), &self.to_management_view_model());
        let page = layout.list_capacity.max(1);
        self.pages.management.management_state.scroll = self
            .pages
            .management
            .management_state
            .scroll
            .min(count.saturating_sub(page));
        if !self.pages.management.management_state.list_scroll_explicit {
            if self.pages.management.management_state.selected
                < self.pages.management.management_state.scroll
            {
                self.pages.management.management_state.scroll =
                    self.pages.management.management_state.selected;
            }
            if self.pages.management.management_state.selected
                >= self.pages.management.management_state.scroll + page
            {
                self.pages.management.management_state.scroll =
                    self.pages.management.management_state.selected + 1 - page;
            }
        }
        self.pages.management.management_state.selected_action = self
            .pages
            .management
            .management_state
            .selected_action
            .min(self.management_actions().len().saturating_sub(1));
        self.pages.management.management_state.details_scroll = layout
            .scrollbars
            .iter()
            .find(|bar| bar.target == ui::ManagementScrollTarget::Details)
            .map_or(0, |bar| bar.offset.min(u16::MAX as usize) as u16);
        self.pages
            .management
            .management_state
            .details_horizontal_scroll = layout
            .scrollbars
            .iter()
            .find(|bar| bar.target == ui::ManagementScrollTarget::DetailsColumns)
            .map_or(0, |bar| bar.offset.min(u16::MAX as usize) as u16);
        if let Some(form) = &mut self.pages.management.management_state.form {
            form.message_scroll = layout
                .scrollbars
                .iter()
                .find(|bar| bar.target == ui::ManagementScrollTarget::FormMessage)
                .map_or(0, |bar| bar.offset.min(u16::MAX as usize) as u16);
        }
    }
    pub(in crate::session) fn handle_management_pointer(&mut self, mouse: MouseInput) {
        let point = mouse.coordinates();
        if matches!(mouse.kind, ui::MouseEventKind::Up(PointerButton::Left)) {
            self.cancel_management_pointer_gesture();
            return;
        }
        if matches!(mouse.kind, ui::MouseEventKind::Drag(PointerButton::Left))
            && self.management_pointer_drag_active()
        {
            self.drag_management_scrollbar(point);
            return;
        }
        let model = self.to_management_view_model();
        let layout = ui::management_layout(self.management_main(), &model);
        let position = ratatui::layout::Position::from(point);
        let bars = self.visible_management_bars(&layout);
        if let ui::MouseEventKind::Scroll(direction) = mouse.kind {
            let delta = if direction == ScrollDirection::Up {
                -3
            } else if direction == ScrollDirection::Down {
                3
            } else {
                0
            };
            let target = if self
                .pages
                .management
                .management_state
                .choice_field
                .is_some()
            {
                ui::ManagementScrollTarget::Choices
            } else if self.pages.management.management_state.form.is_some()
                && !self.pages.management.management_state.terminal_mode
            {
                if layout.fields_area.contains(position) {
                    ui::ManagementScrollTarget::FormFields
                } else {
                    ui::ManagementScrollTarget::FormMessage
                }
            } else if self.pages.management.management_state.terminal_mode {
                ui::ManagementScrollTarget::Output
            } else if layout.details.contains(position) {
                ui::ManagementScrollTarget::Details
            } else if layout.actions_panel.contains(position) {
                ui::ManagementScrollTarget::Actions
            } else {
                ui::ManagementScrollTarget::Rows
            };
            if let Some(bar) = bars.iter().find(|bar| bar.target == target).copied() {
                let offset = bar
                    .offset
                    .saturating_add_signed(delta)
                    .min(bar.content_len.saturating_sub(bar.viewport_len));
                self.set_management_scroll(target, offset, bar);
            }
            return;
        }
        if !matches!(
            mouse.kind,
            ui::MouseEventKind::Down(PointerButton::Left)
                | ui::MouseEventKind::Click(PointerButton::Left)
        ) {
            return;
        }
        if let Some(bar) = bars
            .iter()
            .find(|bar| bar.track.contains(position))
            .copied()
        {
            self.pages.management.management_state.scrollbar_grab =
                Some((bar.target, bar.grab_at(point)));
            self.drag_management_scrollbar(point);
            return;
        }
        if self
            .pages
            .management
            .management_state
            .choice_field
            .is_some()
        {
            if layout.choice_cancel.contains(position) {
                if self
                    .pages
                    .management
                    .management_state
                    .form
                    .as_ref()
                    .is_some_and(management_form_is_menu)
                {
                    self.pages.management.management_state.form = None;
                }
                self.pages.management.management_state.choice_field = None;
                return;
            }
            if let Some((index, _)) = layout
                .choice_rows
                .iter()
                .find(|(_, area)| area.contains(position))
            {
                if model
                    .form
                    .as_ref()
                    .and_then(|form| form.choice.as_ref())
                    .and_then(|choice| choice.disabled.get(*index))
                    .copied()
                    .unwrap_or(false)
                {
                    return;
                }
                let field_index = self.pages.management.management_state.choice_field;
                if let Some(form) = &mut self.pages.management.management_state.form {
                    if let Some(field) = field_index.and_then(|field| form.fields.get_mut(field)) {
                        field.value = field.choices[*index].clone();
                    }
                }
                self.pages.management.management_state.choice_field = None;
                self.pages.management.management_state.choice_selected = *index;
                if self
                    .pages
                    .management
                    .management_state
                    .form
                    .as_ref()
                    .is_some_and(management_form_is_menu)
                {
                    self.submit_management_form();
                }
            }
            return;
        }
        if self.pages.management.management_state.form.is_some()
            && !self.pages.management.management_state.terminal_mode
        {
            if layout.submit.contains(position) {
                if model.form.as_ref().is_some_and(|form| form.submit_disabled) {
                    return;
                }
                self.submit_management_form();
                self.reset_management_form_view();
            } else if layout.cancel.contains(position) {
                self.cancel_management_form();
                self.reset_management_form_view();
            } else if let Some((index, _)) = layout
                .fields
                .iter()
                .find(|(_, area)| area.contains(position))
            {
                let field_view = &model.form.as_ref().unwrap().fields[*index];
                if field_view.control != ui::ManagementFormControl::Text {
                    let area = layout.fields.iter().find(|(i, _)| i == index).unwrap().1;
                    let skip = layout
                        .field_start
                        .saturating_sub(layout.field_ranges[*index].0);
                    let cell = ui::management_permission_cells(field_view, area, skip)
                        .into_iter()
                        .find(|(_, area)| area.contains(position))
                        .map(|(cell, _)| cell);
                    if field_view.control == ui::ManagementFormControl::Permissions
                        && cell.is_none()
                    {
                        return;
                    }
                    let cell = cell.unwrap_or(0);
                    if let Some(form) = &mut self.pages.management.management_state.form {
                        form.selected = *index;
                        super::input::toggle_management_check(form, *index, cell);
                    }
                    self.pages.management.management_state.permission_cell = cell;
                    return;
                }
                let choice = if let Some(form) = &mut self.pages.management.management_state.form {
                    form.selected = *index;
                    if !form.fields[*index].choices.is_empty() {
                        Some(
                            form.fields[*index]
                                .choices
                                .iter()
                                .position(|value| value == &form.fields[*index].value)
                                .unwrap_or(0),
                        )
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(selected) = choice {
                    self.pages.management.management_state.choice_search.clear();
                    self.pages.management.management_state.choice_field = Some(*index);
                    self.pages.management.management_state.choice_selected = selected;
                    self.pages.management.management_state.choice_scroll = 0;
                    self.pages.management.management_state.choice_columns = 0;
                    self.keep_management_choice_visible();
                }
            }
            return;
        }
        if let Some((control, _)) = layout
            .controls
            .iter()
            .find(|(_, area)| area.contains(position))
        {
            self.management_touch_control(*control);
            return;
        }
        if layout.filter.contains(position) {
            self.pages.management.management_state.filtering = true;
            if self.pages.management.management_state.kind == Some(ManagementKind::Packages) {
                self.focus_package_workspace(ui::components::WorkspaceFocus::Search);
            }
            return;
        }
        if self.pages.management.management_state.terminal_mode {
            return;
        }
        if let Some((column, _)) = layout
            .headers
            .iter()
            .find(|(_, area)| area.contains(position))
        {
            self.sort_management(*column);
            return;
        }
        if layout.action_previous.contains(position) || layout.action_next.contains(position) {
            self.page_management_actions(layout.action_next.contains(position));
            return;
        }
        if let Some(index) = layout
            .actions
            .iter()
            .position(|area| area.contains(position))
        {
            self.activate_management_action(layout.action_start + index);
        } else if layout.list_rows.contains(position) {
            let index = (self.pages.management.management_state.scroll
                + usize::from(point.1 - layout.list_rows.y))
            .min(
                self.pages
                    .management
                    .management_state
                    .snapshot
                    .rows
                    .len()
                    .saturating_sub(1),
            );
            if self.management_multiselect_available() {
                if mouse.modifiers.has_non_shift_modifier() {
                    return;
                }
                self.select_management_row(index, mouse.modifiers.shift);
            } else {
                self.pages.management.management_state.selected = index;
            }
            self.pages.management.management_state.actions_focused = false;
            self.pages.management.management_state.details_scroll = 0;
            self.pages
                .management
                .management_state
                .details_horizontal_scroll = 0;
            self.pages.management.management_state.action_scroll = None;
            if self.pages.management.management_state.kind == Some(ManagementKind::Packages) {
                self.focus_package_workspace(ui::components::WorkspaceFocus::List);
            }
        } else if layout.details.contains(position)
            && self.pages.management.management_state.kind == Some(ManagementKind::Packages)
        {
            self.focus_package_workspace(ui::components::WorkspaceFocus::Details);
        }
        self.clamp_management_scroll();
    }
}
