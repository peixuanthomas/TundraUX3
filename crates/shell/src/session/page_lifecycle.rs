use super::*;

impl ShellSession {
    pub(in crate::session) fn persist_all_editor_recovery_now(&mut self, now: Instant) -> bool {
        let ids = self.pages.editor.ids();
        let mut saved = true;
        for id in ids {
            saved &= self
                .with_page_context(id, |session| session.persist_editor_recovery_now(now))
                .unwrap_or(true);
        }
        saved
    }

    pub(in crate::session) fn persist_all_editor_recovery_if_due(&mut self, now: Instant) {
        for id in self.pages.editor.ids() {
            self.with_page_context(id, |session| session.persist_editor_recovery_if_due(now));
        }
    }

    /// Every page return reaches this gate before its state is destroyed.
    pub(in crate::session) fn request_page_close(&mut self, id: PageInstanceId) {
        if self.page_screen(id).is_none() {
            return;
        }
        if self.pending_page_closes.contains(&id) {
            return;
        }
        if self.page_screen(id) == Some(ShellScreen::Management)
            && self
                .with_page_context(id, |session| session.confirm_management_discard(true))
                .unwrap_or(false)
        {
            return;
        }
        if self.page_screen(id) == Some(ShellScreen::Settings)
            && self.settings_task_runtime.update_cancel_error().is_some()
        {
            self.cancel_update();
            if self.settings_task_runtime.update_cancel_error().is_some() {
                return;
            }
            self.pending_page_closes.insert(id);
            return;
        }
        if self.page_screen(id) == Some(ShellScreen::Editor) {
            let dirty_revision = self
                .with_page_context(id, |session| {
                    session
                        .app
                        .editor_state()
                        .filter(|state| state.is_dirty())
                        .map(EditorState::revision)
                })
                .flatten();
            if let Some(current) = dirty_revision {
                if self
                    .approved_editor_closes
                    .get(&id)
                    .is_some_and(|(revision, discard)| !discard || *revision != Some(current))
                {
                    self.approved_editor_closes.remove(&id);
                }
            }
            if dirty_revision.is_some() && !self.approved_editor_closes.contains_key(&id) {
                self.with_page_context(id, |session| session.confirm_editor_close());
                return;
            }
        }
        if self.tasks.page_operations(id) {
            self.with_page_context(id, |session| {
                session.notify_modal_with_options(
                    ShellNotification::modal(
                        i18n::msg!("page-close-task-title"),
                        i18n::msg!("page-close-task-message"),
                        ui::NotificationTone::Warning,
                        vec![
                            ShellNotificationAction::new(
                                "background",
                                i18n::msg!("page-close-background"),
                            )
                            .with_follow_up(ShellCommand::PageCloseBackground(id)),
                            ShellNotificationAction::new(
                                "cancel-task",
                                i18n::msg!("page-close-cancel-task"),
                            )
                            .with_follow_up(ShellCommand::PageCloseCancelTasks(id)),
                            ShellNotificationAction::new("stay", i18n::msg!("page-close-stay"))
                                .cancel()
                                .with_shortcut(InputKey::Escape)
                                .with_follow_up(ShellCommand::PageCloseStay(id)),
                        ],
                    )
                    .with_key(format!("page.close.{}", id.get())),
                );
            });
        } else if self.tasks.page_settling(id) {
            self.tasks.close_queries(id);
            self.pending_page_closes.insert(id);
        } else {
            self.finish_managed_page_close(id);
        }
    }

    pub(in crate::session) fn request_page_close_after_editor_check(
        &mut self,
        id: PageInstanceId,
        discard: bool,
    ) {
        let revision = self
            .with_page_context(id, |session| {
                session.app.editor_state().map(EditorState::revision)
            })
            .flatten();
        self.approved_editor_closes.insert(id, (revision, discard));
        self.request_page_close(id);
    }

    pub(in crate::session) fn continue_page_tasks_in_background(&mut self, id: PageInstanceId) {
        if self.page_screen(id).is_none() {
            return;
        }
        if self.page_screen(id) == Some(ShellScreen::Editor)
            && self.with_page_context(id, |session| session.prepare_editor_background_close())
                != Some(true)
        {
            self.finish_auto_admin_close_choice(id, true);
            return;
        }
        self.tasks.transfer_operations(id);
        self.finish_auto_admin_close_choice(id, false);
        self.tasks.close_queries(id);
        self.detach_management_page(id);
        self.detach_user_management_page(id);
        self.detach_diagnostics_page(id);
        self.hide_background_auto_admin();
        self.approved_editor_closes.remove(&id);
        self.finish_page_close_side_effects(id);
        self.close_page_instance(id);
    }

    pub(in crate::session) fn cancel_page_tasks_and_close(&mut self, id: PageInstanceId) {
        if self.page_screen(id).is_none() {
            return;
        }
        self.finish_auto_admin_close_choice(id, true);
        if let Some(request) = self.settings_update_request
            && self.tasks.owner(TaskDomain::Update, request) == Some(TaskOwner::Page(id))
        {
            self.cancel_update();
            if self.settings_task_runtime.update_cancel_error().is_some() {
                return;
            }
        }
        self.tasks.cancel_page(id);
        self.tasks.close_queries(id);
        self.pending_page_closes.insert(id);
        self.notify_status(i18n::msg!("page-close-waiting"));
        self.poll_page_closes();
    }

    pub(in crate::session) fn stay_on_page(&mut self, id: PageInstanceId) {
        self.finish_auto_admin_close_choice(id, true);
        self.pending_page_closes.remove(&id);
        self.approved_editor_closes.remove(&id);
    }

    pub(in crate::session) fn poll_page_closes(&mut self) {
        self.tasks.reap();
        let waiting = self.pending_page_closes.iter().copied().collect::<Vec<_>>();
        for id in waiting {
            if self.page_screen(id) == Some(ShellScreen::Settings)
                && self.settings_task_runtime.update_cleanup_running()
            {
                continue;
            }
            if self.tasks.page_running(id) {
                continue;
            }
            self.pending_page_closes.remove(&id);
            if let Some((revision, discard)) = self.approved_editor_closes.get(&id).copied() {
                let changed = self
                    .with_page_context(id, |session| {
                        session.app.editor_state().is_some_and(|state| {
                            state.is_dirty() && (!discard || Some(state.revision()) != revision)
                        })
                    })
                    .unwrap_or(false);
                if changed {
                    self.approved_editor_closes.remove(&id);
                    self.request_page_close(id);
                    continue;
                }
            }
            self.finish_managed_page_close(id);
        }
    }

    fn finish_managed_page_close(&mut self, id: PageInstanceId) {
        if self.page_screen(id) == Some(ShellScreen::Editor) {
            if self.with_page_context(id, |session| session.clear_editor_recovery()) == Some(false)
            {
                self.approved_editor_closes.remove(&id);
                self.finish_auto_admin_close_choice(id, true);
                return;
            }
        }
        self.finish_page_close_side_effects(id);
        self.close_page_instance(id);
    }

    fn finish_page_close_side_effects(&mut self, id: PageInstanceId) {
        match self.page_screen(id) {
            Some(ShellScreen::Explorer) => self.finish_explorer_page_close(id),
            Some(ShellScreen::Diagnostics)
                if !self.pages.screens.iter().any(|(other, screen)| {
                    *other != id
                        && matches!(screen, ShellScreen::SystemStatus | ShellScreen::Diagnostics)
                }) =>
            {
                let _ = self.settings_task_runtime.set_system_status_active(false);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session/page_lifecycle.rs"]
mod tests;
