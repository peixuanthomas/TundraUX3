use super::super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TouchScroll {
    Settings,
    SettingsPicker,
    SettingsCategories,
    Users,
    UserForm,
    Clock,
    LoginPage,
    LoginUsers,
    BootstrapPage,
    SetupPage,
    SetupZones,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(screen: ShellScreen) -> ShellSession {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (30, 10),
            ShellHomeMode::User,
        );
        state.screen_stack = vec![screen];
        state
    }

    #[test]
    fn setup_scrollbar_drag_reaches_hidden_fields_without_activating_them() {
        let mut state = state(ShellScreen::FirstRunSetup);
        state.setup_step = ui::SetupStep::Admin;
        let region = state
            .touch_regions()
            .into_iter()
            .find(|region| region.kind == TouchScroll::SetupPage)
            .unwrap();
        state.handle_touch_pages_pointer(MouseInput::new(
            region.track.x,
            region.track.y + 1,
            ui::MouseEventKind::Down(PointerButton::Left),
        ));
        state.handle_touch_pages_pointer(MouseInput::new(
            region.track.x,
            region.track.bottom() - 1,
            ui::MouseEventKind::Drag(PointerButton::Left),
        ));
        assert!(state.page_touch.setup_scroll > 0);
        assert_eq!(state.setup_step, ui::SetupStep::Admin);
        state.handle_touch_pages_pointer(MouseInput::new(
            0,
            0,
            ui::MouseEventKind::Up(PointerButton::Left),
        ));
        assert!(state.page_touch.drag.is_none());
    }

    #[test]
    fn compact_page_routes_leave_the_shared_header_and_keep_setup_previous_step() {
        let mut clock = state(ShellScreen::Clock);
        let model = clock.to_clock_view_model_at(&clock.app.snapshot().clock, Instant::now());
        let main = clock.touch_main();
        let new = ui::clock_page_layout(main, &model).new_button;
        assert!(matches!(
            clock.route_touch_pages_pointer(MouseInput::new(
                new.x,
                new.y,
                ui::MouseEventKind::Down(PointerButton::Left)
            )),
            Some(ShellCommand::ClockOpenCreate)
        ));
        assert!(
            clock
                .route_touch_pages_pointer(MouseInput::new(
                    main.right() - 1,
                    main.y.saturating_sub(1),
                    ui::MouseEventKind::Down(PointerButton::Left)
                ))
                .is_none()
        );
        clock.cancel_touch_pages_pointer();
        let mut setup = state(ShellScreen::FirstRunSetup);
        setup.setup_step = ui::SetupStep::Timezone;
        setup.page_touch.setup_scroll = u16::MAX;
        let page = setup.touch_setup_viewport();
        let previous = page
            .project(ui::setup_navigation_areas(page.content)[0])
            .unwrap();
        setup.handle_touch_pages_pointer(MouseInput::new(
            previous.x,
            previous.y,
            ui::MouseEventKind::Down(PointerButton::Left),
        ));
        assert_eq!(setup.active_screen(), ShellScreen::FirstRunSetup);
        assert_eq!(setup.setup_step, ui::SetupStep::Language);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TouchDrag {
    kind: TouchScroll,
    grab: u16,
    screen: ShellScreen,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(in crate::session) struct PageTouchState {
    pub(in crate::session) setup_scroll: u16,
    pub(in crate::session) login_scroll: u16,
    pub(in crate::session) bootstrap_scroll: u16,
    drag: Option<TouchDrag>,
}

#[derive(Debug, Clone, Copy)]
struct TouchScrollRegion {
    kind: TouchScroll,
    track: Rect,
    content: usize,
    viewport: usize,
    offset: usize,
}

impl ShellSession {
    pub(in crate::session) fn cancel_touch_pages_pointer(&mut self) {
        self.page_touch.drag = None;
    }

    fn touch_main(&self) -> Rect {
        let terminal = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        match self.shell_layout_for(terminal) {
            ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
        }
    }

    pub(in crate::session) fn touch_setup_viewport(&self) -> ui::AuthViewport {
        ui::setup_viewport(self.touch_main(), &self.to_setup_view_model())
    }

    fn touch_regions(&self) -> Vec<TouchScrollRegion> {
        let mut regions = Vec::new();
        let mut add = |kind, track: Option<Rect>, content, viewport, offset| {
            if let Some(track) = track {
                regions.push(TouchScrollRegion {
                    kind,
                    track,
                    content,
                    viewport,
                    offset,
                });
            }
        };
        match self.active_screen() {
            ShellScreen::Settings => {
                if let Some(model) = self.to_settings_view_model() {
                    let layout = ui::settings_layout(self.touch_main(), &model);
                    if let Some(picker) = model.picker.as_ref() {
                        let list = layout.picker_list.unwrap_or_default();
                        add(
                            TouchScroll::SettingsPicker,
                            (picker.options.len() > usize::from(list.height) && !list.is_empty())
                                .then(|| Rect::new(list.right() - 1, list.y, 1, list.height)),
                            picker.options.len(),
                            usize::from(list.height),
                            picker.window_start,
                        );
                    } else if layout.overlay_apply.is_none() && layout.update_confirmation.is_none()
                    {
                        add(
                            TouchScroll::Settings,
                            layout.scrollbar,
                            layout.content_height,
                            usize::from(layout.detail.height),
                            usize::from(layout.scroll_offset),
                        );
                        add(
                            TouchScroll::SettingsCategories,
                            layout.category_scrollbar,
                            ui::SettingsCategory::ALL.len(),
                            layout.category_capacity,
                            layout.category_window_start,
                        );
                    }
                }
            }
            ShellScreen::UserManagement => {
                let model = self.to_user_management_view_model();
                let layout = ui::user_management_layout(self.touch_main(), &model);
                if let Some(form) = layout.form {
                    add(
                        TouchScroll::UserForm,
                        form.scrollbar,
                        form.field_count,
                        form.field_capacity,
                        form.field_window_start,
                    );
                } else {
                    add(
                        TouchScroll::Users,
                        layout.scrollbar,
                        model.users.len(),
                        layout.visible_capacity,
                        layout.visible_start,
                    );
                }
            }
            ShellScreen::Clock => {
                let model = self.to_clock_view_model_at(&self.app.snapshot().clock, Instant::now());
                let layout = ui::clock_page_layout(self.touch_main(), &model);
                if layout.create_dialog.is_none() {
                    add(
                        TouchScroll::Clock,
                        layout.scrollbar,
                        model.alarms.len() + model.countdowns.len(),
                        layout.entry_capacity,
                        layout.entry_window_start,
                    );
                }
            }
            ShellScreen::Login => {
                let model = self.to_login_view_model();
                let page = ui::login_viewport(self.touch_main(), &model);
                add(
                    TouchScroll::LoginPage,
                    page.scrollbar,
                    usize::from(page.content.height),
                    usize::from(page.area.height),
                    usize::from(page.offset),
                );
                let list = ui::login_layout(page.content).user_list;
                add(
                    TouchScroll::LoginUsers,
                    ui::login_list_scrollbar(page.content, &model)
                        .and_then(|track| page.project(track)),
                    model.users.len(),
                    usize::from(list.height.saturating_sub(2)),
                    model.user_window_start,
                );
            }
            ShellScreen::BootstrapAdmin => {
                let model = self.to_bootstrap_admin_view_model();
                let page = ui::bootstrap_viewport(self.touch_main(), &model);
                add(
                    TouchScroll::BootstrapPage,
                    page.scrollbar,
                    usize::from(page.content.height),
                    usize::from(page.area.height),
                    usize::from(page.offset),
                );
            }
            ShellScreen::FirstRunSetup => {
                let model = self.to_setup_view_model();
                let page = if self.setup_custom_color_target.is_some() {
                    ui::setup_color_viewport(self.touch_main(), &model)
                } else {
                    ui::setup_viewport(self.touch_main(), &model)
                };
                add(
                    TouchScroll::SetupPage,
                    page.scrollbar,
                    usize::from(page.content.height),
                    usize::from(page.area.height),
                    usize::from(page.offset),
                );
                if self.setup_custom_color_target.is_none() && model.step == ui::SetupStep::Timezone
                {
                    let list = ui::setup_timezone_list_area(page.content);
                    add(
                        TouchScroll::SetupZones,
                        ui::setup_timezone_scrollbar(page.content, &model)
                            .and_then(|track| page.project(track)),
                        model.timezones.len(),
                        usize::from(list.height),
                        model.timezone_window_start,
                    );
                }
            }
            _ => {}
        }
        regions
    }

    pub(in crate::session) fn route_touch_pages_pointer(
        &self,
        mouse: MouseInput,
    ) -> Option<ShellCommand> {
        let point = mouse.coordinates();
        if self
            .page_touch
            .drag
            .is_some_and(|drag| drag.screen == self.active_screen())
            && matches!(
                mouse.kind,
                ui::MouseEventKind::Drag(PointerButton::Left)
                    | ui::MouseEventKind::Up(PointerButton::Left)
            )
        {
            return Some(ShellCommand::TouchPagesPointer(mouse));
        }
        if matches!(mouse.kind, ui::MouseEventKind::Down(PointerButton::Left))
            && self
                .touch_regions()
                .iter()
                .any(|region| rect_contains(region.track, point))
        {
            return Some(ShellCommand::TouchPagesPointer(mouse));
        }
        if !rect_contains(self.touch_main(), point) {
            return None;
        }
        if self.active_screen() == ShellScreen::BootstrapAdmin
            && matches!(mouse.kind, ui::MouseEventKind::Down(PointerButton::Left))
        {
            let page =
                ui::bootstrap_viewport(self.touch_main(), &self.to_bootstrap_admin_view_model());
            let submit = ui::bootstrap_submit_area(page.content);
            if page
                .project(submit)
                .is_some_and(|area| rect_contains(area, point))
            {
                return Some(ShellCommand::SubmitBootstrapAdmin);
            }
        }
        if self.active_screen() == ShellScreen::Login
            && matches!(mouse.kind, ui::MouseEventKind::Down(PointerButton::Left))
        {
            let model = self.to_login_view_model();
            let page = ui::login_viewport(self.touch_main(), &model);
            let layout = ui::login_layout(page.content);
            if page
                .project(layout.submit)
                .is_some_and(|area| rect_contains(area, point))
            {
                return Some(ShellCommand::SubmitLogin);
            }
            if page
                .project(layout.password_visibility)
                .is_some_and(|area| rect_contains(area, point))
            {
                return Some(ShellCommand::ToggleLoginPasswordVisibility);
            }
        }
        if matches!(
            self.active_screen(),
            ShellScreen::Login | ShellScreen::FirstRunSetup | ShellScreen::BootstrapAdmin
        ) && !matches!(mouse.kind, ui::MouseEventKind::Moved)
        {
            return Some(ShellCommand::TouchPagesPointer(mouse));
        }
        if self.active_screen() == ShellScreen::Clock
            && matches!(mouse.kind, ui::MouseEventKind::Down(PointerButton::Left))
        {
            let model = self.to_clock_view_model_at(&self.app.snapshot().clock, Instant::now());
            let layout = ui::clock_page_layout(self.touch_main(), &model);
            if let Some(dialog) = layout.create_dialog {
                if rect_contains(dialog.cancel, point) {
                    return Some(ShellCommand::ClockCloseCreate);
                }
                if rect_contains(dialog.create_alarm, point) {
                    return Some(ShellCommand::ClockCreateAlarm);
                }
                if rect_contains(dialog.create_countdown, point) {
                    return Some(ShellCommand::ClockCreateCountdown);
                }
                for field in 0..3 {
                    if rect_contains(dialog.increments[field], point) {
                        return Some(ShellCommand::ClockCreateAdjust(field, 1));
                    }
                    if rect_contains(dialog.decrements[field], point) {
                        return Some(ShellCommand::ClockCreateAdjust(field, -1));
                    }
                    if rect_contains(dialog.values[field], point)
                        || rect_contains(dialog.labels[field], point)
                    {
                        return Some(ShellCommand::ClockCreateSelectField(field));
                    }
                }
                return Some(ShellCommand::CaptureOverlayInput);
            } else {
                if rect_contains(layout.new_button, point) {
                    return Some(ShellCommand::ClockOpenCreate);
                }
                if rect_contains(layout.manage_button, point) {
                    return self
                        .clock_selected_entry_id
                        .map(ShellCommand::ClockManageEntry);
                }
                if let Some(row) = layout
                    .entry_rows
                    .iter()
                    .find(|row| rect_contains(row.area, point))
                {
                    return Some(ShellCommand::ClockManageEntry(row.id));
                }
            }
        }
        if self.active_screen() == ShellScreen::UserManagement
            && matches!(mouse.kind, ui::MouseEventKind::Down(PointerButton::Left))
        {
            let layout = ui::user_management_layout(
                self.touch_main(),
                &self.to_user_management_view_model(),
            );
            if layout.form.is_some() {
                return Some(
                    layout
                        .form_control_at(point.0, point.1)
                        .map(|field| {
                            if matches!(
                                field,
                                ui::UserManagementField::Role
                                    | ui::UserManagementField::Submit
                                    | ui::UserManagementField::Cancel
                            ) {
                                ShellCommand::UserManagementActivateFormControl(field)
                            } else {
                                ShellCommand::UserManagementSetFormFocus(field)
                            }
                        })
                        .unwrap_or(ShellCommand::CaptureOverlayInput),
                );
            }
            if let Some(index) = layout.row_index_at(point.0, point.1) {
                return Some(ShellCommand::UserManagementSelectRow(index));
            }
            if let Some(action) = layout.action_at(point.0, point.1) {
                return Some(ShellCommand::UserManagementActivateAction(action));
            }
        }
        None
    }

    pub(in crate::session) fn handle_touch_pages_pointer(&mut self, mouse: MouseInput) {
        let point = mouse.coordinates();
        match mouse.kind {
            ui::MouseEventKind::Up(PointerButton::Left) => {
                self.page_touch.drag = None;
                return;
            }
            ui::MouseEventKind::Drag(PointerButton::Left) => {
                if let Some(drag) = self.page_touch.drag {
                    if drag.screen != self.active_screen() {
                        self.page_touch.drag = None;
                        return;
                    }
                    if let Some(region) = self
                        .touch_regions()
                        .into_iter()
                        .find(|region| region.kind == drag.kind)
                    {
                        let (_, thumb) = ui::components::Scrollbar::new(
                            region.content,
                            region.viewport,
                            region.offset,
                        )
                        .thumb_range(region.track);
                        let offset = scrollbar_window_start(
                            point.1,
                            drag.grab,
                            region.track.y,
                            region.track.height,
                            thumb,
                            region.content,
                            region.viewport,
                        );
                        self.set_touch_offset(region.kind, offset);
                    }
                }
            }
            ui::MouseEventKind::Down(PointerButton::Left) => {
                if let Some(region) = self
                    .touch_regions()
                    .into_iter()
                    .find(|region| rect_contains(region.track, point))
                {
                    let (start, thumb) = ui::components::Scrollbar::new(
                        region.content,
                        region.viewport,
                        region.offset,
                    )
                    .thumb_range(region.track);
                    let thumb_y = region.track.y + start;
                    let grab = if point.1 >= thumb_y && point.1 < thumb_y + thumb {
                        point.1 - thumb_y
                    } else {
                        thumb / 2
                    };
                    self.page_touch.drag = Some(TouchDrag {
                        kind: region.kind,
                        grab,
                        screen: self.active_screen(),
                    });
                    let offset = scrollbar_window_start(
                        point.1,
                        grab,
                        region.track.y,
                        region.track.height,
                        thumb,
                        region.content,
                        region.viewport,
                    );
                    self.set_touch_offset(region.kind, offset);
                } else {
                    self.page_touch.drag = None;
                    if self.active_screen() == ShellScreen::Login {
                        self.touch_login(point);
                    } else if self.active_screen() == ShellScreen::FirstRunSetup {
                        self.touch_setup(point);
                    } else if self.active_screen() == ShellScreen::BootstrapAdmin {
                        let page = ui::bootstrap_viewport(
                            self.touch_main(),
                            &self.to_bootstrap_admin_view_model(),
                        );
                        if let Some(point) = page.content_point(point) {
                            if point.1 == 3 {
                                self.focused_component = ShellComponent::BootstrapUsername;
                            } else if point.1 == 4 {
                                self.focused_component = ShellComponent::BootstrapPassword;
                            }
                        }
                    }
                }
            }
            ui::MouseEventKind::Scroll(direction) => {
                let delta = match direction {
                    ScrollDirection::Up => -3isize,
                    ScrollDirection::Down => 3,
                    _ => return,
                };
                let kind = match self.active_screen() {
                    ShellScreen::Login => TouchScroll::LoginPage,
                    ShellScreen::FirstRunSetup => TouchScroll::SetupPage,
                    ShellScreen::BootstrapAdmin => TouchScroll::BootstrapPage,
                    _ => return,
                };
                let regions = self.touch_regions();
                if let Some(region) = regions
                    .iter()
                    .find(|region| region.kind == kind)
                    .or_else(|| regions.first())
                {
                    let offset = region
                        .offset
                        .saturating_add_signed(delta)
                        .min(region.content.saturating_sub(region.viewport));
                    self.set_touch_offset(region.kind, offset);
                }
            }
            _ => {}
        }
        self.refresh_hit_map();
    }

    fn set_touch_offset(&mut self, kind: TouchScroll, offset: usize) {
        match kind {
            TouchScroll::Settings => {
                if let Some(state) = self.settings_state.as_mut() {
                    state.scroll_offset = offset.min(usize::from(u16::MAX)) as u16;
                }
            }
            TouchScroll::SettingsPicker => {
                if let Some(picker) = self
                    .settings_state
                    .as_mut()
                    .and_then(|state| state.picker.as_mut())
                {
                    picker.window_start = offset;
                }
            }
            TouchScroll::SettingsCategories => {
                let capacity = self
                    .touch_regions()
                    .into_iter()
                    .find(|region| region.kind == kind)
                    .map_or(1, |region| region.viewport);
                self.select_settings_category(
                    ui::SettingsCategory::ALL[(offset + capacity.saturating_sub(1))
                        .min(ui::SettingsCategory::ALL.len() - 1)],
                );
            }
            TouchScroll::Users => self.user_management_window_start = offset,
            TouchScroll::UserForm => {
                let model = self.to_user_management_view_model();
                if let Some(form) = model.form.as_ref() {
                    let layout = ui::user_management_layout(self.touch_main(), &model)
                        .form
                        .unwrap();
                    let fields = form
                        .field_order()
                        .iter()
                        .copied()
                        .filter(|field| {
                            !matches!(
                                field,
                                ui::UserManagementField::Submit | ui::UserManagementField::Cancel
                            )
                        })
                        .collect::<Vec<_>>();
                    if let Some(field) =
                        fields.get(offset + layout.field_capacity.saturating_sub(1))
                    {
                        self.set_user_management_form_focus(*field);
                    }
                }
            }
            TouchScroll::Clock => self.clock_entry_window_start = offset,
            TouchScroll::LoginPage => {
                self.page_touch.login_scroll = offset.min(usize::from(u16::MAX)) as u16
            }
            TouchScroll::LoginUsers => self.login_user_window_start = offset,
            TouchScroll::BootstrapPage => {
                self.page_touch.bootstrap_scroll = offset.min(usize::from(u16::MAX)) as u16
            }
            TouchScroll::SetupPage => {
                self.page_touch.setup_scroll = offset.min(usize::from(u16::MAX)) as u16
            }
            TouchScroll::SetupZones => self.setup_timezone_window_start = offset,
        }
    }

    fn touch_login(&mut self, point: CellPosition) {
        let model = self.to_login_view_model();
        let page = ui::login_viewport(self.touch_main(), &model);
        let Some(point) = page.content_point(point) else {
            return;
        };
        let layout = ui::login_layout(page.content);
        if rect_contains(layout.password, point) {
            self.focused_component = ShellComponent::LoginPassword;
        } else if rect_contains(layout.user_list, point) {
            self.focused_component = ShellComponent::LoginUserList;
            let row = point.1.saturating_sub(layout.user_list.y + 1);
            if point.1 > layout.user_list.y && point.1 < layout.user_list.bottom().saturating_sub(1)
            {
                let index = self.login_user_window_start + usize::from(row);
                if index < self.login_users.len() {
                    self.select_login_user_at(index);
                    self.focused_component = ShellComponent::LoginPassword;
                    self.ensure_touch_auth_focus_visible();
                }
            }
        }
    }

    fn touch_setup(&mut self, point: CellPosition) {
        if self.setup_custom_color_target.is_some() {
            let page = ui::setup_color_viewport(self.touch_main(), &self.to_setup_view_model());
            let [apply, cancel] = ui::setup_custom_color_actions(page.content);
            if page
                .project(apply)
                .is_some_and(|area| rect_contains(area, point))
            {
                self.apply_setup_custom_color();
                self.ensure_touch_auth_focus_visible();
            } else if page
                .project(cancel)
                .is_some_and(|area| rect_contains(area, point))
            {
                self.cancel_setup_custom_color();
                self.ensure_touch_auth_focus_visible();
            }
            return;
        }
        let model = self.to_setup_view_model();
        let page = ui::setup_viewport(self.touch_main(), &model);
        let Some(point) = page.content_point(point) else {
            return;
        };
        let main = page.content;
        match model.step {
            ui::SetupStep::Language | ui::SetupStep::Timezone => {
                let list = if model.step == ui::SetupStep::Language {
                    ui::setup_language_list_area(main, model.languages.len())
                } else {
                    ui::setup_timezone_list_area(main)
                };
                let controls = if model.step == ui::SetupStep::Timezone
                    && main.width >= 90
                    && main.height >= 14
                {
                    Rect::new(main.x, main.y, 48, main.height)
                } else {
                    main
                };
                let [back, next] = ui::setup_navigation_areas(controls);
                if page.project(next).is_some() && rect_contains(next, point) {
                    self.page_touch.setup_scroll = 0;
                    self.setup_continue();
                } else if page.project(back).is_some()
                    && rect_contains(back, point)
                    && model.step == ui::SetupStep::Timezone
                {
                    self.setup_step = ui::SetupStep::Language;
                    self.setup_focused_field = ui::SetupField::LanguageList;
                    self.focused_component = ShellComponent::SetupLanguage;
                    self.page_touch.setup_scroll = 0;
                } else if rect_contains(list, point) {
                    let index = usize::from(point.1 - list.y);
                    if model.step == ui::SetupStep::Language {
                        if index < model.languages.len() {
                            self.setup_selected_language_index = index;
                            self.focused_component = ShellComponent::SetupLanguage;
                        }
                    } else if model.timezone_window_start + index < model.timezones.len() {
                        self.setup_selected_timezone_index = model.timezone_window_start + index;
                        self.focused_component = ShellComponent::SetupTimezone;
                    }
                    self.error_message = None;
                }
            }
            ui::SetupStep::Admin => {
                for (field, component) in [
                    (
                        ui::SetupField::AdminUsername,
                        ShellComponent::SetupAdminUsername,
                    ),
                    (
                        ui::SetupField::AdminPassword,
                        ShellComponent::SetupAdminPassword,
                    ),
                    (
                        ui::SetupField::AdminPasswordConfirm,
                        ShellComponent::SetupAdminPasswordConfirm,
                    ),
                    (ui::SetupField::PasswordHint, ShellComponent::SetupAdminHint),
                    (ui::SetupField::Submit, ShellComponent::SetupSubmit),
                ] {
                    if rect_contains(ui::setup_admin_field_area(main, field), point) {
                        self.focus_setup_component(component);
                        if field == ui::SetupField::Submit
                            && page
                                .project(ui::setup_admin_field_area(main, field))
                                .is_some()
                        {
                            self.submit_first_run_setup();
                        }
                        break;
                    }
                }
            }
            ui::SetupStep::Appearance => {
                for (field, component) in [
                    (
                        ui::SetupField::AppearanceShape,
                        ShellComponent::SetupAppearanceShape,
                    ),
                    (
                        ui::SetupField::AppearanceThemeColor,
                        ShellComponent::SetupAppearanceThemeColor,
                    ),
                    (
                        ui::SetupField::AppearanceThemeCustom,
                        ShellComponent::SetupAppearanceThemeCustom,
                    ),
                    (
                        ui::SetupField::AppearanceAccentColor,
                        ShellComponent::SetupAppearanceAccentColor,
                    ),
                    (
                        ui::SetupField::AppearanceAccentCustom,
                        ShellComponent::SetupAppearanceAccentCustom,
                    ),
                    (
                        ui::SetupField::AppearanceSubmit,
                        ShellComponent::SetupAppearanceSubmit,
                    ),
                ] {
                    if rect_contains(ui::setup_appearance_field_area(main, field), point) {
                        self.activate_setup_appearance(component, point);
                        break;
                    }
                }
            }
        }
    }

    pub(in crate::session) fn ensure_touch_auth_focus_visible(&mut self) {
        let (page, field) = match self.active_screen() {
            ShellScreen::Login => {
                let model = self.to_login_view_model();
                let page = ui::login_viewport(self.touch_main(), &model);
                let layout = ui::login_layout(page.content);
                let field = match self.focused_component {
                    ShellComponent::LoginPassword => layout.password,
                    ShellComponent::LoginPasswordVisibility => layout.password_visibility,
                    _ => Rect::new(
                        layout.user_list.x,
                        layout.user_list.y
                            + 1
                            + self
                                .login_selected_user
                                .saturating_sub(self.login_user_window_start)
                                as u16,
                        layout.user_list.width,
                        1,
                    ),
                };
                (page, field)
            }
            ShellScreen::BootstrapAdmin => {
                let page = ui::bootstrap_viewport(
                    self.touch_main(),
                    &self.to_bootstrap_admin_view_model(),
                );
                (
                    page,
                    Rect::new(
                        1,
                        if self.focused_component == ShellComponent::BootstrapPassword {
                            4
                        } else {
                            3
                        },
                        page.content.width.saturating_sub(2),
                        1,
                    ),
                )
            }
            ShellScreen::FirstRunSetup if self.setup_custom_color_target.is_none() => {
                let model = self.to_setup_view_model();
                let page = ui::setup_viewport(self.touch_main(), &model);
                let field = match self.setup_step {
                    ui::SetupStep::Admin => {
                        ui::setup_admin_field_area(page.content, self.setup_focused_field)
                    }
                    ui::SetupStep::Appearance => {
                        ui::setup_appearance_field_area(page.content, self.setup_focused_field)
                    }
                    ui::SetupStep::Language => {
                        let list =
                            ui::setup_language_list_area(page.content, model.languages.len());
                        Rect::new(
                            list.x,
                            list.y
                                .saturating_add(self.setup_selected_language_index as u16),
                            list.width,
                            1,
                        )
                    }
                    ui::SetupStep::Timezone => {
                        let list = ui::setup_timezone_list_area(page.content);
                        Rect::new(
                            list.x,
                            list.y.saturating_add(
                                self.setup_selected_timezone_index
                                    .saturating_sub(self.setup_timezone_window_start)
                                    as u16,
                            ),
                            list.width,
                            1,
                        )
                    }
                };
                (page, field)
            }
            _ => return,
        };
        if field.is_empty() {
            return;
        }
        let offset = if field.y < page.offset {
            field.y
        } else if field.bottom() > page.offset + page.area.height {
            field.bottom().saturating_sub(page.area.height)
        } else {
            page.offset
        }
        .min(page.content.height.saturating_sub(page.area.height));
        match self.active_screen() {
            ShellScreen::Login => self.page_touch.login_scroll = offset,
            ShellScreen::BootstrapAdmin => self.page_touch.bootstrap_scroll = offset,
            ShellScreen::FirstRunSetup => self.page_touch.setup_scroll = offset,
            _ => {}
        }
    }
}
