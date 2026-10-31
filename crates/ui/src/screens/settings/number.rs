use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsNumberEditorViewModel {
    pub field: SettingsField,
    pub title: String,
    pub input: TextInput,
    pub minimum: u32,
    pub maximum: u32,
    pub error: Option<String>,
    pub selected_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsNumberLayout {
    pub field: SettingsField,
    pub dialog: Rect,
    pub input: Rect,
    pub decrease: Rect,
    pub increase: Rect,
}

impl SettingsNumberLayout {
    pub(super) fn new(area: Rect, model: &SettingsNumberEditorViewModel) -> Self {
        let dialog = model.dialog();
        let layout = dialog.layout(area);
        let [decrease, increase] = crate::management_number_buttons(&layout, 0, &dialog.fields[0]);
        Self {
            field: model.field,
            dialog: layout.form,
            input: crate::management_field_input_area(&layout, 0, &dialog.fields[0]),
            decrease,
            increase,
        }
    }
}

impl SettingsNumberEditorViewModel {
    pub fn dialog(&self) -> crate::components::FormDialog {
        use crate::components::{FormControl, FormDialog, FormField};
        let mut field = FormField::text("number", &self.title, self.input.value());
        field.input = Some(self.input.clone());
        field.control = FormControl::Number {
            minimum: self.minimum,
            maximum: self.maximum,
            step: 1,
        };
        let mut form = FormDialog::new("settings.number", &self.title, vec![field]);
        form.submit_label = Some(i18n::tr!("settings-save"));
        form.cancel_label = Some(i18n::tr!("settings-cancel"));
        form.message = self.error.clone().unwrap_or_else(|| {
            i18n::tr!(
                "settings-number-range",
                min = self.minimum,
                max = self.maximum
            )
        });
        form.selected = self.selected_index;
        form
    }
}
