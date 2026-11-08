use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn load_home_order(&mut self) {
        let Some(session) = self.app.auth_session() else {
            return;
        };
        if self.pages.home.order_user_id.as_deref() == Some(&session.user_id) {
            return;
        }
        let Some(storage) = self.storage_manager.as_ref() else {
            return;
        };
        let user_id = session.user_id.clone();
        let key = format!("home.order.{}", session.user_id);
        self.pages.home.order = storage
            .load_state()
            .ok()
            .and_then(|state| {
                state
                    .values
                    .get(&key)
                    .and_then(|value| serde_json::from_str(value).ok())
            })
            .unwrap_or_default();
        self.pages.home.order_user_id = Some(user_id);
        self.pages.home.editing = false;
        self.pages.home.drag = None;
    }

    pub(in crate::session) fn toggle_home_editing(&mut self) {
        if self.user_home_entries().is_empty() {
            return;
        }
        self.pages.home.editing = !self.pages.home.editing;
        self.pages.home.drag = None;
        self.interaction.button_capture = None;
    }

    pub(in crate::session) fn move_home_entry(&mut self, delta: isize) {
        if !self.pages.home.editing {
            return;
        }
        let source = self.selected_home_entry_index();
        let destination = source
            .saturating_add_signed(delta)
            .min(self.user_home_entries().len().saturating_sub(1));
        self.reorder_home_entry(source, destination);
    }

    fn reorder_home_entry(&mut self, source: usize, destination: usize) {
        if source == destination {
            return;
        }
        let mut order = self
            .user_home_entries()
            .iter()
            .map(|entry| entry.icon_identity().to_owned())
            .collect::<Vec<_>>();
        let entry = order.remove(source);
        order.insert(destination, entry);
        if let Some(storage) = self.storage_manager.as_ref()
            && let Some(session) = self.app.auth_session()
        {
            let result = storage.load_state().and_then(|mut state| {
                state.values.insert(
                    format!("home.order.{}", session.user_id),
                    serde_json::to_string(&order).expect("home icon IDs serialize"),
                );
                storage.save_state(&state)
            });
            if let Err(error) = result {
                self.notify_alert_with_tone(
                    i18n::msg!("shell-home-order-save-failed", error = error.to_string()),
                    ui::NotificationTone::Error,
                );
                return;
            }
        }
        self.pages.home.order = order;
        self.select_home_entry(destination);
    }

    pub(in crate::session) fn begin_home_drag(&mut self, index: usize) {
        if let Some(capture) = &mut self.interaction.button_capture {
            capture.activate_on_release = false;
        }
        self.pages.home.drag = self
            .user_home_entries()
            .get(index)
            .map(|entry| HomeDragState {
                entry_id: entry.icon_identity().to_owned(),
                target: None,
            });
    }

    pub(in crate::session) fn update_home_drag(&mut self, coordinates: CellPosition) {
        let target = self
            .home_content_layout()
            .and_then(|layout| layout.drop_target(coordinates));
        if let Some(drag) = self.pages.home.drag.as_mut() {
            drag.target = target;
        }
    }

    pub(in crate::session) fn drop_home_drag(&mut self, coordinates: CellPosition) {
        let Some(drag) = self.pages.home.drag.take() else {
            return;
        };
        if !self.pages.home.editing || drag.target.is_none() {
            return;
        }
        let Some(target) = self
            .home_content_layout()
            .and_then(|layout| layout.drop_target(coordinates))
        else {
            return;
        };
        let entries = self.user_home_entries();
        let Some(source) = entries
            .iter()
            .position(|entry| entry.icon_identity() == drag.entry_id)
        else {
            return;
        };
        let boundary = target.insertion_index();
        let destination = boundary - usize::from(source < boundary);
        self.reorder_home_entry(source, destination);
    }
}
