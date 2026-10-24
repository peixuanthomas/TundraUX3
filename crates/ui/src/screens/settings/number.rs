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
    pub(super) fn new(area: Rect, field: SettingsField) -> Self {
        let dialog = centered(area, area.width.min(68), area.height.min(11));
        let row = Rect::new(
            dialog.x.saturating_add(2),
            dialog.y.saturating_add(3),
            dialog.width.saturating_sub(4),
            1,
        )
        .intersection(dialog);
        let [decrease, input, increase] = Layout::horizontal([
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(3),
        ])
        .areas(row);
        Self {
            field,
            dialog,
            input,
            decrease,
            increase,
        }
    }
}

pub(super) fn render(
    frame: &mut Frame<'_>,
    layout: &SettingsNumberLayout,
    model: &SettingsNumberEditorViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    frame.render_widget(Clear, layout.dialog);
    Surface::new()
        .titled(model.title.clone())
        .bordered(true)
        .raised(true)
        .render_frame(frame, layout.dialog, context);
    frame.render_widget(
        Paragraph::new(i18n::tr!(
            "settings-number-range",
            min = model.minimum,
            max = model.maximum
        ))
        .style(theme.muted_style())
        .wrap(Wrap { trim: true }),
        Rect::new(
            layout.dialog.x + 2,
            layout.dialog.y + 1,
            layout.dialog.width.saturating_sub(4),
            2,
        ),
    );
    model
        .input
        .render_borderless_frame(frame, layout.input, theme);
    for (id, label, area) in [
        ("settings.number.decrease", "−", layout.decrease),
        ("settings.number.increase", "+", layout.increase),
    ] {
        Button::new(id, label).render_inline_frame(frame, area, theme);
    }
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(model.error.clone().unwrap_or_default(), theme.error_style()),
            Line::styled(i18n::tr!("settings-number-help"), theme.muted_style()),
        ])
        .wrap(Wrap { trim: true }),
        Rect::new(
            layout.dialog.x + 2,
            layout.dialog.y + 5,
            layout.dialog.width.saturating_sub(4),
            layout.dialog.height.saturating_sub(8),
        ),
    );
}
