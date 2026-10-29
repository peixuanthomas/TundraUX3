use super::*;
use crate::components::{FormDialog, FormField};

impl SettingsViewModel {
    pub fn form_dialog(&self) -> Option<FormDialog> {
        let mut form = if let Some(editor) = &self.number_editor {
            editor.dialog()
        } else {
            let (id, title, help, value, editing, error, busy) = if let Some(editor) =
                &self.color_editor
            {
                (
                    "color-editor",
                    editor.title.clone(),
                    i18n::tr!("ui-settings-enter-a-color-as-rrggbb"),
                    &editor.value,
                    &editor.editing,
                    &editor.error,
                    false,
                )
            } else if let Some(editor) = &self.weather_location_editor {
                (
                    "weather-location-editor",
                    i18n::tr!("ui-settings-weather-location-padded"),
                    format!(
                        "{}\n{}",
                        i18n::tr!(
                            "ui-settings-enter-a-detailed-city-or-address-using-english-characters"
                        ),
                        i18n::tr!("ui-settings-leave-empty-to-use-the-timezone-location")
                    ),
                    &editor.value,
                    &editor.editing,
                    &editor.error,
                    false,
                )
            } else if let Some(editor) = &self.file_extensions_editor {
                (
                    "file-extensions-editor",
                    i18n::tr!("ui-settings-explorer-files-opened-in-editor-padded"),
                    format!(
                        "{}\n{}",
                        i18n::tr!(
                            "ui-settings-enter-comma-separated-filename-suffixes-explorer-should-open-here"
                        ),
                        i18n::tr!(
                            "ui-settings-examples-md-txt-rs-d-ts-matching-is-case-insensitive"
                        )
                    ),
                    &editor.value,
                    &editor.editing,
                    &editor.error,
                    false,
                )
            } else if let Some(editor) = &self.time_sync_server_editor {
                (
                    "time-sync-server-editor",
                    editor.system_server_label.clone().unwrap_or_else(|| {
                        i18n::tr!("ui-settings-time-synchronization-server-padded")
                    }),
                    i18n::tr!(if editor.system_server_label.is_some() {
                        "ui-settings-system-time-server-help"
                    } else {
                        "ui-settings-enter-an-http-s-endpoint-that-returns-a-valid-date-response-header"
                    }),
                    &editor.value,
                    &editor.editing,
                    &editor.error,
                    editor.validating,
                )
            } else {
                return None;
            };
            let mut field = FormField::text(id, title.trim(), value);
            field.input = Some(editing.input(id, value));
            field.disabled = busy;
            let mut form = FormDialog::new("settings.text", title.trim(), vec![field]);
            form.message = if busy {
                format!(
                    "{help}\n{}",
                    i18n::tr!(if self
                        .time_sync_server_editor
                        .as_ref()
                        .unwrap()
                        .system_server_label
                        .is_some()
                    {
                        "management-working"
                    } else {
                        "ui-settings-synchronizing-with-this-server"
                    })
                )
            } else {
                format!("{help}\n{}", error.as_deref().unwrap_or_default())
            };
            form.selected = self.form_selected;
            form.submit_disabled = busy;
            form
        };
        form.submit_label = Some(i18n::tr!("settings-save"));
        form.cancel_label = Some(i18n::tr!("settings-cancel"));
        Some(form.with_interaction(self.form_interaction.clone()))
    }
}
