use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn current_vim_install(&self) -> Option<&ui::EditorVimInstallViewModel> {
        self.editor_vim_install
            .as_ref()
            .filter(|(page, _)| *page == self.content_page_id())
            .map(|(_, view)| view)
    }

    pub(in crate::session) fn cancel_vim_install(&mut self) {
        if let Some((_, view)) = &mut self.editor_vim_install {
            view.cancelling = true;
        }
    }

    pub(in crate::session) fn toggle_editor_vim(&mut self) {
        if self.editor_vim_terminal.is_some() {
            self.editor_vim_toggle_requested = true;
        } else {
            self.set_editor_vim_enabled(!self.editor_vim_enabled());
        }
    }

    pub(in crate::session) fn set_editor_vim_enabled(&mut self, enabled: bool) -> bool {
        self.editor_vim_enabled = Some(enabled);
        self.pages.editor.editor_cursor_acceleration = None;
        true
    }

    pub(in crate::session) fn editor_vim_enabled(&self) -> bool {
        self.editor_vim_enabled
            .unwrap_or(self.app.storage_config().editor.vim_mode_enabled)
    }

    pub(super) fn handle_editor_vim_key(
        &mut self,
        _key: &KeyInput,
        _platform: &dyn Platform,
    ) -> bool {
        // The real process owns input. Keep startup input out of the ordinary editor.
        self.editor_vim_enabled()
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/editor/vim_toggle_tests.rs"]
mod tests;
