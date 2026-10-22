use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn toggle_user_management_more(&mut self) {
        let model = self.to_user_management_view_model();
        if !model
            .page_actions()
            .iter()
            .any(|action| action.action == ui::UserManagementAction::More)
        {
            return;
        }
        self.capture_modal_focus_context();
        self.pages.user_management.user_management_more_open = !model.more_open;
        let focus = if model.more_open {
            ui::UserManagementAction::More
        } else {
            let mut menu = model;
            menu.more_open = true;
            menu.visible_actions()
                .into_iter()
                .find(|action| action.enabled)
                .unwrap()
                .action
        };
        self.pages.user_management.user_management_focus = UserManagementPageFocus::Action(focus);
        self.synchronize_overlay_focus();
        self.refresh_hit_map();
    }

    pub(in crate::session) fn user_management_button_at(
        &self,
        point: CellPosition,
    ) -> Option<ui::components::ButtonRegion> {
        if self.active_screen() != ShellScreen::UserManagement {
            return None;
        }
        let model = self.to_user_management_view_model();
        if model.form.is_some() {
            return None;
        }
        let layout = self.user_management_layout()?;
        layout
            .actions
            .iter()
            .find(|action| rect_contains(action.area, point))
            .map(|action| ui::components::ButtonRegion {
                id: format!("user-management.action.{:?}", action.action).into(),
                area: action.area,
                disabled: !action.enabled,
            })
    }
}
