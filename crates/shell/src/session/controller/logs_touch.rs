use super::*;

impl ShellSession {
    pub(in crate::session) fn cancel_logs_pointer_gesture(&mut self) {
        self.logs_state.scrollbar_grab = None;
        self.logs_state.detail_scrollbar_grab = None;
    }
    pub(in crate::session) fn logs_pointer_drag_active(&self) -> bool {
        self.active_screen() == ShellScreen::Logs
            && (self.logs_state.scrollbar_grab.is_some()
                || self.logs_state.detail_scrollbar_grab.is_some())
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
        let region = |id: String, area: Rect, disabled: bool| ui::components::ButtonRegion {
            id: id.into(),
            area,
            disabled,
        };
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
    fn logs_touch_action(&mut self, target: ui::LogsHitTarget) {
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
            ui::LogsHitTarget::ClearFilters => {
                self.handle_logs_key(&KeyInput::new(InputKey::Char('c')))
            }
            ui::LogsHitTarget::RelatedIncident => {
                self.handle_logs_key(&KeyInput::new(InputKey::Char('i')))
            }
            ui::LogsHitTarget::RelatedEvents => {
                self.handle_logs_key(&KeyInput::new(InputKey::Char('e')))
            }
            ui::LogsHitTarget::Back => self.handle_logs_key(&KeyInput::new(InputKey::Escape)),
            _ => {}
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
            let delta = if direction == ScrollDirection::Up {
                -3
            } else if direction == ScrollDirection::Down {
                3
            } else {
                0
            };
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
            return;
        };
        match target {
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
    }
}
