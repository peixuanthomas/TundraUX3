use super::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SettingsApplication {
    #[default]
    FileExplorer,
    Editor,
}

impl SettingsApplication {
    pub const ALL: [Self; 2] = [Self::FileExplorer, Self::Editor];

    pub fn label(self) -> String {
        match self {
            Self::FileExplorer => i18n::tr!("ui-settings-file-explorer"),
            Self::Editor => i18n::tr!("ui-settings-editor"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsApplicationsLayout {
    pub list: Rect,
    pub detail: Rect,
    pub options: Vec<(SettingsApplication, Rect)>,
}

impl SettingsApplicationsLayout {
    pub(super) fn new(area: Rect) -> Self {
        let width = SettingsApplication::ALL
            .iter()
            .map(|application| terminal_width(&application.label()) as u16 + 4)
            .max()
            .unwrap_or(0)
            .min(area.width / 3);
        let [list, _, detail] = Layout::horizontal([
            Constraint::Length(width),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(area);
        let inner = inset(list, 1, 1);
        let height = if inner.height >= 6 { 3 } else { 1 };
        let options = SettingsApplication::ALL
            .into_iter()
            .enumerate()
            .map(|(index, application)| {
                (
                    application,
                    Rect::new(
                        inner.x,
                        inner.y.saturating_add(index as u16 * height),
                        inner.width,
                        height,
                    )
                    .intersection(inner),
                )
            })
            .collect();
        Self {
            list,
            detail,
            options,
        }
    }

    pub(super) fn render(
        &self,
        frame: &mut Frame<'_>,
        model: &SettingsViewModel,
        context: &RenderContext,
    ) {
        let focused = model.selected_field == SettingsField::ApplicationList;
        Surface::new()
            .titled(i18n::tr!("ui-settings-application-list"))
            .bordered(true)
            .focused(focused)
            .render_frame(frame, self.list, context);
        for (application, area) in &self.options {
            let active = *application == model.selected_application;
            let mut button = Button::new(
                format!("settings.application.{application:?}").to_ascii_lowercase(),
                application.label(),
            )
            .with_current(active)
            .with_bracketed_label(false);
            button.set_focused(focused && active);
            button.render_borderless_frame(frame, *area, &context.compatibility_theme());
        }
    }
}
