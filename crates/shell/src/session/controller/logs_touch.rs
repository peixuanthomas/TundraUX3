use super::*;

impl ShellSession {
    pub(in crate::session) fn logs_has_active_overlay(&self) -> bool {
        self.logs_state.more_selected.is_some() || self.logs_state.filter_form.is_some()
    }
    pub(in crate::session) fn cancel_logs_pointer_gesture(&mut self) {
        self.logs_state.scrollbar_grab = None;
        self.logs_state.detail_scrollbar_grab = None;
        self.logs_state.more_scrollbar_grab = None;
    }
    pub(in crate::session) fn logs_pointer_drag_active(&self) -> bool {
        self.active_screen() == ShellScreen::Logs
            && (self.logs_state.scrollbar_grab.is_some()
                || self.logs_state.detail_scrollbar_grab.is_some()
                || self.logs_state.more_scrollbar_grab.is_some())
    }
    pub(in crate::session) fn logs_button_at(
        &self,
        point: CellPosition,
    ) -> Option<ui::components::ButtonRegion> {
        if self.notification_has_active_modal()
            || self.time_sync_dialog_visible
            || self.active_popup.is_some()
        {
            return None;
        }
        if self.active_screen() != ShellScreen::Logs {
            return None;
        }
        let main = self.logs_main_area()?;
        let model = self.to_logs_view_model();
        let layout = ui::logs_layout(main, &model);
        if let Some(form) = &model.filter_form {
            let form_model = ui::ManagementViewModel {
                form: Some(form.clone()),
                ..Default::default()
            };
            return ui::management_button_regions(main, &form_model)
                .into_iter()
                .find(|region| rect_contains(region.area, point));
        }
        let region = |id: String, area: Rect, disabled: bool| ui::components::ButtonRegion {
            id: id.into(),
            area,
            disabled,
        };
        if model.more_selected.is_some() {
            return layout
                .menu_controls
                .iter()
                .find(|control| rect_contains(control.area, point))
                .map(|control| {
                    region(
                        ui::logs_control_id(&model, control.target),
                        control.area,
                        !ui::logs_control_enabled(&model, control.target),
                    )
                });
        }
        for tab in &layout.category_tabs {
            if rect_contains(tab.area, point) {
                return Some(region(
                    format!("logs.category.{:?}", tab.category),
                    tab.area,
                    false,
                ));
            }
        }
        for tab in &layout.section_tabs {
            if rect_contains(tab.area, point) {
                return Some(region(
                    format!("logs.section.{:?}", tab.section),
                    tab.area,
                    false,
                ));
            }
        }
        for control in &layout.controls {
            if rect_contains(control.area, point) {
                return Some(region(
                    ui::logs_control_id(&model, control.target),
                    control.area,
                    ui::logs_hit_test(main, &model, point) != Some(control.target),
                ));
            }
        }
        None
    }
    pub(super) fn logs_touch_action(&mut self, target: ui::LogsHitTarget) {
        match target {
            ui::LogsHitTarget::Category(category) => {
                self.logs_state.detail_scroll = 0;
                self.logs_set_category(category);
            }
            ui::LogsHitTarget::Section(section) => {
                self.logs_state.detail_scroll = 0;
                self.logs_set_section(section);
            }
            ui::LogsHitTarget::Refresh => self.request_logs_job(None),
            ui::LogsHitTarget::Open => self.logs_open_selected(),
            ui::LogsHitTarget::FilterLevel => self.logs_filter_level(),
            ui::LogsHitTarget::FilterModule => self.logs_filter_module(),
            ui::LogsHitTarget::FilterTime => self.logs_filter_time(),
            ui::LogsHitTarget::Follow => self.logs_follow_control(),
            ui::LogsHitTarget::More => {
                self.cancel_logs_pointer_gesture();
                self.logs_state.more_selected = Some(0);
            }
            ui::LogsHitTarget::CloseMore => self.logs_state.more_selected = None,
            ui::LogsHitTarget::Filters => self.logs_open_filter_form(),
            ui::LogsHitTarget::FormApply => self.logs_apply_filter_form(),
            ui::LogsHitTarget::FormCancel => self.logs_state.filter_form = None,
            ui::LogsHitTarget::FormField(index) => {
                if let Some(form) = self.logs_state.filter_form.as_mut() {
                    form.selected = index;
                    if let Some(field) = form.fields.get_mut(index)
                        && !field.choices.is_empty()
                    {
                        let index = field
                            .choices
                            .iter()
                            .position(|value| value == &field.value)
                            .unwrap_or(0);
                        field.value = field.choices[(index + 1) % field.choices.len()].clone();
                    }
                }
            }
            ui::LogsHitTarget::ClearFilters => {
                self.handle_logs_key(&KeyInput::new(InputKey::Char('c')))
            }
            ui::LogsHitTarget::RelatedIncident => {
                self.handle_logs_key(&KeyInput::new(InputKey::Char('i')))
            }
            ui::LogsHitTarget::RelatedEvents => {
                self.handle_logs_key(&KeyInput::new(InputKey::Char('e')))
            }
            _ => {}
        }
    }
    pub(super) fn logs_move_more_selection(&mut self, direction: isize, wrap: bool) {
        let Some(selected) = self.logs_state.more_selected else {
            return;
        };
        let options = ui::logs_more_controls();
        let model = self.to_logs_view_model();
        for distance in 1..=options.len() {
            let index = selected as isize + direction * distance as isize;
            if !wrap && !(0..options.len() as isize).contains(&index) {
                break;
            }
            let index = index.rem_euclid(options.len() as isize) as usize;
            if ui::logs_control_enabled(&model, options[index].0) {
                self.logs_state.more_selected = Some(index);
                break;
            }
        }
    }
    pub(in crate::session) fn handle_logs_pointer(&mut self, mouse: MouseInput) {
        let Some(main) = self.logs_main_area() else {
            return;
        };
        let model = self.to_logs_view_model();
        let layout = ui::logs_layout(main, &model);
        let point = mouse.coordinates();
        if matches!(mouse.kind, ui::MouseEventKind::Up(PointerButton::Left)) {
            self.cancel_logs_pointer_gesture();
            return;
        }
        if matches!(mouse.kind, ui::MouseEventKind::Drag(PointerButton::Left))
            && self.logs_pointer_drag_active()
        {
            self.drag_logs_scrollbar(point);
            return;
        }
        if let ui::MouseEventKind::Scroll(direction) = mouse.kind {
            let delta: isize = if direction == ScrollDirection::Up {
                -3
            } else if direction == ScrollDirection::Down {
                3
            } else {
                0
            };
            if self.logs_state.filter_form.is_some() {
                return;
            }
            if self.logs_state.more_selected.is_some() {
                if layout.menu.contains(point.into()) && delta != 0 {
                    self.logs_move_more_selection(delta.signum(), false);
                }
                return;
            }
            if layout.content.detail_panel.contains(point.into()) {
                if let Some(bar) = layout.detail_scrollbar {
                    self.logs_state.detail_scroll = bar
                        .offset
                        .saturating_add_signed(delta)
                        .min(bar.content_len.saturating_sub(bar.viewport_len));
                }
            } else {
                self.logs_state.scroll = layout
                    .visible_start
                    .saturating_add_signed(delta)
                    .min(self.logs_count().saturating_sub(layout.visible_capacity));
                self.logs_state.explicit_scroll = true;
                self.logs_state.paused = true;
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
        let Some(target) = ui::logs_hit_test(main, &model, point) else {
            if self.logs_state.more_selected.is_some() && !layout.menu.contains(point.into()) {
                self.logs_state.more_selected = None;
            }
            return;
        };
        if self.logs_state.more_selected.is_some() && target != ui::LogsHitTarget::MoreScrollbar {
            self.logs_state.more_selected = None;
        }
        match target {
            ui::LogsHitTarget::MoreScrollbar => {
                self.logs_state.scrollbar_grab = None;
                self.logs_state.detail_scrollbar_grab = None;
                self.logs_state.more_scrollbar_grab =
                    layout.menu_scrollbar.map(|bar| bar.grab_at(point));
                self.drag_logs_scrollbar(point);
            }
            ui::LogsHitTarget::Scrollbar => {
                self.logs_state.detail_scrollbar_grab = None;
                self.logs_state.scrollbar_grab = Some(
                    layout
                        .content
                        .list_scrollbar
                        .filter(|bar| bar.thumb.contains(point.into()))
                        .map_or(0, |bar| point.1.saturating_sub(bar.thumb.y)),
                );
                self.drag_logs_scrollbar(point);
            }
            ui::LogsHitTarget::DetailScrollbar => {
                self.logs_state.scrollbar_grab = None;
                self.logs_state.detail_scrollbar_grab =
                    layout.detail_scrollbar.map(|bar| bar.grab_at(point));
                self.drag_logs_scrollbar(point);
            }
            ui::LogsHitTarget::Event(index)
            | ui::LogsHitTarget::File(index)
            | ui::LogsHitTarget::Incident(index) => {
                let click = self.register_click(
                    Some(ShellComponent::Logs),
                    point,
                    PointerButton::Left,
                    Instant::now(),
                );
                self.logs_state.selected = index;
                self.logs_state.paused = true;
                self.logs_state.detail_scroll = 0;
                if click == ClickKind::Double {
                    self.logs_open_selected();
                }
            }
            _ => {
                self.cancel_logs_pointer_gesture();
                self.logs_touch_action(target);
            }
        }
    }
    fn drag_logs_scrollbar(&mut self, point: CellPosition) {
        let Some(main) = self.logs_main_area() else {
            return;
        };
        let layout = ui::logs_layout(main, &self.to_logs_view_model());
        if let Some(grab) = self.logs_state.more_scrollbar_grab {
            if let Some(bar) = layout.menu_scrollbar {
                let selected = bar.offset_at(point, grab) + bar.viewport_len.saturating_sub(1);
                self.logs_state.more_selected = Some(selected);
                if !ui::logs_control_enabled(
                    &self.to_logs_view_model(),
                    ui::logs_more_controls()[selected].0,
                ) {
                    self.logs_move_more_selection(1, false);
                }
            } else {
                self.cancel_logs_pointer_gesture();
            }
            return;
        }
        if let Some(grab) = self.logs_state.detail_scrollbar_grab {
            if let Some(bar) = layout.detail_scrollbar {
                self.logs_state.detail_scroll = bar.offset_at(point, grab);
            } else {
                self.cancel_logs_pointer_gesture();
            }
            return;
        }
        let Some(bar) = layout.content.list_scrollbar else {
            self.cancel_logs_pointer_gesture();
            return;
        };
        let count = self.logs_count();
        let scrollbar =
            ui::components::Scrollbar::new(count, layout.visible_capacity, layout.visible_start);
        let (_, thumb) = scrollbar.thumb_range(bar.track);
        self.logs_state.scroll = scrollbar_window_start(
            point.1,
            self.logs_state.scrollbar_grab.unwrap_or(0),
            bar.track.y,
            bar.track.height,
            thumb,
            count,
            layout.visible_capacity,
        );
        self.logs_state.explicit_scroll = true;
        self.logs_state.paused = true;
    }
}
