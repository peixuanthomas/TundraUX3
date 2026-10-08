use super::*;

fn management_form_is_menu(form: &ManagementEditor) -> bool {
    matches!(&form.purpose, FormPurpose::Menu(_))
        || matches!(&form.purpose, FormPurpose::Configuration(action) if action == "menu")
}

impl ShellSession {
    pub(super) fn handle_management_choice_key(&mut self, key: &KeyInput) -> bool {
        let Some(index) = self.management_state.choice_field else {
            return false;
        };
        if key.has_non_shift_modifier() {
            return true;
        }
        let selected = self.management_state.choice_selected;
        let count = self
            .management_state
            .form
            .as_ref()
            .and_then(|form| form.fields.get(index))
            .map_or(0, |field| field.choices.len());
        let shortcut_key = match key.key {
            InputKey::Char(c) => InputKey::Char(c.to_ascii_lowercase()),
            _ => key.key.clone(),
        };
        let shortcut = self.management_state.form.as_ref().and_then(|form| {
            let FormPurpose::Menu(items) = &form.purpose else {
                return None;
            };
            items.iter().position(|(action, _)| {
                management_action_shortcut(self.management_state.kind, &action.id)
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
                .management_state
                .form
                .as_mut()
                .and_then(|form| form.fields.get_mut(index))
            {
                field.value = field.choices[selected].clone();
            }
            self.management_state.choice_selected = selected;
            self.management_state.choice_field = None;
            self.submit_management_form();
            self.management_state.shortcut_repeat_guard = Some(key.key.clone());
            return true;
        }
        match key.key {
            InputKey::Escape => {
                if self
                    .management_state
                    .form
                    .as_ref()
                    .is_some_and(management_form_is_menu)
                {
                    self.management_state.form = None;
                }
                self.management_state.choice_field = None;
            }
            InputKey::Up => {
                self.management_state.choice_selected =
                    self.management_state.choice_selected.saturating_sub(1)
            }
            InputKey::Down => {
                self.management_state.choice_selected =
                    (self.management_state.choice_selected + 1).min(count.saturating_sub(1))
            }
            InputKey::PageUp => {
                self.management_state.choice_selected =
                    self.management_state.choice_selected.saturating_sub(8)
            }
            InputKey::PageDown => {
                self.management_state.choice_selected =
                    (self.management_state.choice_selected + 8).min(count.saturating_sub(1))
            }
            InputKey::Home => self.management_state.choice_selected = 0,
            InputKey::End => self.management_state.choice_selected = count.saturating_sub(1),
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
                if let Some(form) = &mut self.management_state.form {
                    if let Some(field) = form.fields.get_mut(index) {
                        if let Some(value) = field.choices.get(selected) {
                            field.value = value.clone();
                        }
                    }
                }
                self.management_state.choice_field = None;
                if self
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
        self.management_state.choice_scroll =
            self.management_state.choice_selected.saturating_sub(3);
        true
    }
    pub(super) fn open_management_choice_field(&mut self, index: usize) {
        let Some(field) = self
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
        self.management_state.choice_field = Some(index);
        self.management_state.choice_selected = selected;
        self.management_state.choice_scroll = selected.saturating_sub(3);
        self.management_state.choice_columns = 0;
    }
    pub(in crate::session) fn cancel_management_pointer_gesture(&mut self) {
        self.management_state.scrollbar_grab = None;
    }
    pub(in crate::session) fn management_pointer_drag_active(&self) -> bool {
        (self.active_screen() == ShellScreen::Management || self.config_editor_form_visible())
            && self.management_state.scrollbar_grab.is_some()
    }
    pub(super) fn reset_management_form_view(&mut self) {
        self.management_state.form_field_scroll = None;
        self.management_state.choice_field = None;
        self.management_state.choice_scroll = 0;
        self.management_state.choice_columns = 0;
        self.cancel_management_pointer_gesture();
    }
    pub(in crate::session) fn management_button_at(
        &self,
        point: CellPosition,
    ) -> Option<ui::components::ButtonRegion> {
        if self.notification_has_active_modal()
            || self.time_sync_dialog_visible
            || self.active_popup.is_some()
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
        let filter = self.management_state.filter_input.clone();
        if let Some(query) = &mut self.management_state.query {
            if query.filter != filter {
                query.filter = filter;
                query.target = None;
                self.management_state.list_scroll_explicit = false;
            }
        }
    }
    pub(super) fn apply_management_filter(&mut self) {
        self.sync_management_filter();
        if let Some(query) = &mut self.management_state.query {
            query.target = None;
        }
        self.management_state.filtering = false;
        self.management_state.list_scroll_explicit = false;
        self.refresh_management();
    }
    pub(super) fn management_touch_control(&mut self, control: ui::ManagementControl) {
        match control {
            ui::ManagementControl::Refresh => {
                self.management_state.outcome = None;
                self.sync_management_filter();
                self.refresh_management();
            }
            ui::ManagementControl::Search => self.management_state.filtering = true,
            ui::ManagementControl::ApplySearch => self.apply_management_filter(),
            ui::ManagementControl::ClearSearch => {
                self.management_state.filter_input.clear();
                self.apply_management_filter();
            }
            ui::ManagementControl::Details => {
                if self.management_state.snapshot.rows.is_empty() {
                    return;
                }
                self.management_state.details_only = !self.management_state.details_only;
                self.management_state.details_scroll = 0;
                self.management_state.actions_focused = true;
                if let Some(id) = self
                    .management_state
                    .snapshot
                    .rows
                    .get(self.management_state.selected)
                    .map(|row| row.id.clone())
                {
                    if let Some(query) = &mut self.management_state.query {
                        query.target = Some(id);
                    }
                    self.refresh_management();
                }
            }
            ui::ManagementControl::Terminal => {
                if let Some(job) = self.management_state.auto_admin_job.clone() {
                    self.show_auto_admin_job(job);
                    return;
                }
                self.management_state.terminal_mode = !self.management_state.terminal_mode;
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
        self.management_state.action_scroll = Some(
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
                self.management_state.scroll = offset;
                self.management_state.list_scroll_explicit = true;
            }
            ui::ManagementScrollTarget::Columns => self.management_state.table_scroll = offset,
            ui::ManagementScrollTarget::Details => self.management_state.details_scroll = short,
            ui::ManagementScrollTarget::Actions => {
                self.management_state.action_scroll = Some(offset)
            }
            ui::ManagementScrollTarget::FormMessage => {
                if let Some(form) = &mut self.management_state.form {
                    form.message_scroll = short;
                }
            }
            ui::ManagementScrollTarget::FormFields => {
                self.management_state.form_field_scroll = Some(offset)
            }
            ui::ManagementScrollTarget::Choices => self.management_state.choice_scroll = offset,
            ui::ManagementScrollTarget::ChoiceColumns => {
                self.management_state.choice_columns = offset
            }
            ui::ManagementScrollTarget::Output => {
                if let Some(parser) = &self.management_state.parser {
                    if let Ok(mut parser) = parser.0.lock() {
                        parser.set_scrollback(
                            bar.content_len
                                .saturating_sub(bar.viewport_len)
                                .saturating_sub(offset),
                        );
                    }
                } else {
                    self.management_state.output_scroll = short;
                }
            }
        }
    }
    fn drag_management_scrollbar(&mut self, point: CellPosition) {
        let Some((target, grab)) = self.management_state.scrollbar_grab else {
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
                if self.management_state.choice_field.is_some() {
                    matches!(
                        bar.target,
                        ui::ManagementScrollTarget::Choices
                            | ui::ManagementScrollTarget::ChoiceColumns
                    )
                } else if self.management_state.form.is_some()
                    && !self.management_state.terminal_mode
                {
                    matches!(
                        bar.target,
                        ui::ManagementScrollTarget::FormMessage
                            | ui::ManagementScrollTarget::FormFields
                    )
                } else if self.management_state.terminal_mode {
                    bar.target == ui::ManagementScrollTarget::Output
                } else {
                    matches!(
                        bar.target,
                        ui::ManagementScrollTarget::Rows
                            | ui::ManagementScrollTarget::Columns
                            | ui::ManagementScrollTarget::Details
                            | ui::ManagementScrollTarget::Actions
                    )
                }
            })
            .collect()
    }
    pub(super) fn clamp_management_scroll(&mut self) {
        let count = self.management_state.snapshot.rows.len();
        self.management_state.selected =
            self.management_state.selected.min(count.saturating_sub(1));
        let layout =
            ui::management_layout(self.management_main(), &self.to_management_view_model());
        let page = layout.list_capacity.max(1);
        self.management_state.scroll = self.management_state.scroll.min(count.saturating_sub(page));
        if !self.management_state.list_scroll_explicit {
            if self.management_state.selected < self.management_state.scroll {
                self.management_state.scroll = self.management_state.selected;
            }
            if self.management_state.selected >= self.management_state.scroll + page {
                self.management_state.scroll = self.management_state.selected + 1 - page;
            }
        }
        self.management_state.selected_action = self
            .management_state
            .selected_action
            .min(self.management_actions().len().saturating_sub(1));
        self.management_state.details_scroll = layout
            .scrollbars
            .iter()
            .find(|bar| bar.target == ui::ManagementScrollTarget::Details)
            .map_or(0, |bar| bar.offset.min(u16::MAX as usize) as u16);
        if let Some(form) = &mut self.management_state.form {
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
            let target = if self.management_state.choice_field.is_some() {
                ui::ManagementScrollTarget::Choices
            } else if self.management_state.form.is_some() && !self.management_state.terminal_mode {
                if layout.fields_area.contains(position) {
                    ui::ManagementScrollTarget::FormFields
                } else {
                    ui::ManagementScrollTarget::FormMessage
                }
            } else if self.management_state.terminal_mode {
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
            self.management_state.scrollbar_grab = Some((bar.target, bar.grab_at(point)));
            self.drag_management_scrollbar(point);
            return;
        }
        if self.management_state.choice_field.is_some() {
            if layout.choice_cancel.contains(position) {
                if self
                    .management_state
                    .form
                    .as_ref()
                    .is_some_and(management_form_is_menu)
                {
                    self.management_state.form = None;
                }
                self.management_state.choice_field = None;
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
                let field_index = self.management_state.choice_field;
                if let Some(form) = &mut self.management_state.form {
                    if let Some(field) = field_index.and_then(|field| form.fields.get_mut(field)) {
                        field.value = field.choices[*index].clone();
                    }
                }
                self.management_state.choice_field = None;
                self.management_state.choice_selected = *index;
                if self
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
        if self.management_state.form.is_some() && !self.management_state.terminal_mode {
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
                let choice = if let Some(form) = &mut self.management_state.form {
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
                    self.management_state.choice_field = Some(*index);
                    self.management_state.choice_selected = selected;
                    self.management_state.choice_scroll = selected.saturating_sub(3);
                    self.management_state.choice_columns = 0;
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
            self.management_state.filtering = true;
            return;
        }
        if self.management_state.terminal_mode {
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
            self.management_state.selected = (self.management_state.scroll
                + usize::from(point.1 - layout.list_rows.y))
            .min(self.management_state.snapshot.rows.len().saturating_sub(1));
            self.management_state.actions_focused = false;
            self.management_state.details_scroll = 0;
            self.management_state.action_scroll = None;
        }
        self.clamp_management_scroll();
    }
}
