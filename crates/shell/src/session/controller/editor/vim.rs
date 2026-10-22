use crate::session::*;

impl ShellSession {
    pub(in crate::session) fn toggle_editor_vim(&mut self) {
        if self.editor_vim_terminal.is_some() {
            self.editor_vim_toggle_requested = true;
        } else {
            self.set_editor_vim_enabled(!self.app.storage_config().editor.vim_mode_enabled);
        }
    }

    pub(in crate::session) fn set_editor_vim_enabled(&mut self, enabled: bool) -> bool {
        let config = (|| {
            let mut config = if let Some(storage) = &self.storage_manager {
                storage.load_config()?
            } else {
                self.app.storage_config().clone()
            };
            config.editor.vim_mode_enabled = enabled;
            if let Some(storage) = &self.storage_manager {
                storage.save_config(&config)?;
            }
            Ok::<_, storage::StorageError>(config)
        })();
        match config {
            Ok(config) => {
                self.replace_storage_config(config);
                self.pages.editor.editor_cursor_acceleration = None;
                true
            }
            Err(error) => {
                self.report_editor_error(i18n::msg!(
                    "shell-could-not-save-editor-settings-error",
                    error = error.to_string()
                ));
                false
            }
        }
    }

    pub(super) fn handle_editor_vim_key(
        &mut self,
        _key: &KeyInput,
        _platform: &dyn Platform,
    ) -> bool {
        // The real process owns input. Keep startup input out of the ordinary editor.
        self.app.storage_config().editor.vim_mode_enabled
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/editor/vim_toggle_tests.rs"]
mod tests;
