use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsAnimationEditorViewModel {
    pub items: Vec<SettingsItemViewModel>,
    /// The index after the final item selects the close button.
    pub selected_index: usize,
    pub scroll_offset: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsAnimationLayout {
    pub dialog: Rect,
    pub viewport: Rect,
    pub fields: Vec<SettingsFieldLayout>,
    pub close: Rect,
    pub scrollbar: Option<Rect>,
    pub scroll_offset: u16,
    pub max_scroll_offset: u16,
    value_width: u16,
}

impl SettingsAnimationLayout {
    pub(super) fn new(area: Rect, model: &SettingsAnimationEditorViewModel) -> Self {
        let dialog = centered(
            area,
            area.width.min(76),
            area.height.min(model.items.len() as u16 + 7),
        );
        let mut viewport = Rect::new(
            dialog.x.saturating_add(2),
            dialog.y.saturating_add(4),
            dialog.width.saturating_sub(4),
            dialog.height.saturating_sub(7),
        )
        .intersection(dialog);
        let max_scroll_offset = (model.items.len() as u16).saturating_sub(viewport.height);
        let scroll_offset = model.scroll_offset.min(max_scroll_offset);
        let scrollbar = (max_scroll_offset > 0 && !viewport.is_empty()).then(|| {
            viewport.width = viewport.width.saturating_sub(1);
            Rect::new(viewport.right(), viewport.y, 1, viewport.height)
        });
        let value_width = model
            .items
            .iter()
            .map(settings_control_width)
            .max()
            .unwrap_or(0)
            .min(viewport.width / 2);
        let mut fields = Vec::new();
        for item in model
            .items
            .iter()
            .skip(usize::from(scroll_offset))
            .take(usize::from(viewport.height))
        {
            let row = Rect::new(
                viewport.x,
                viewport.y + fields.len() as u16,
                viewport.width,
                1,
            );
            fields.push(SettingsFieldLayout {
                field: item.field,
                area: row,
            });
        }
        let close = Rect::new(
            dialog.x.saturating_add(2),
            dialog.bottom().saturating_sub(2),
            dialog.width.saturating_sub(4),
            u16::from(dialog.height >= 2),
        )
        .intersection(dialog);
        Self {
            dialog,
            viewport,
            fields,
            close,
            scrollbar,
            scroll_offset,
            max_scroll_offset,
            value_width,
        }
    }
}

pub(super) fn render(
    frame: &mut Frame<'_>,
    layout: &SettingsAnimationLayout,
    model: &SettingsAnimationEditorViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    frame.render_widget(Clear, layout.dialog);
    Surface::new()
        .titled(i18n::tr!("settings-animation-advanced"))
        .bordered(true)
        .raised(true)
        .render_frame(frame, layout.dialog, context);
    frame.render_widget(
        Paragraph::new(i18n::tr!("settings-animation-phase-help"))
            .style(theme.muted_style())
            .wrap(Wrap { trim: true }),
        Rect::new(
            layout.dialog.x.saturating_add(2),
            layout.dialog.y.saturating_add(1),
            layout.dialog.width.saturating_sub(4),
            layout.dialog.height.saturating_sub(2).min(2),
        ),
    );
    for (row, item_layout) in layout.fields.iter().enumerate() {
        let index = row + usize::from(layout.scroll_offset);
        let item = &model.items[index];
        let selected = index == model.selected_index;
        frame.render_widget(
            Paragraph::new(truncate_to_terminal_width(
                &item.label,
                usize::from(
                    item_layout
                        .area
                        .width
                        .saturating_sub(layout.value_width + 1),
                ),
            ))
            .style(if !item.enabled {
                theme.disabled_style()
            } else if selected {
                theme.title_style()
            } else {
                theme.body_style()
            }),
            item_layout.area,
        );
        render_settings_control(
            frame,
            Rect::new(
                item_layout.area.right().saturating_sub(layout.value_width),
                item_layout.area.y,
                layout.value_width,
                1,
            ),
            item,
            selected,
            theme,
        );
    }
    if let Some(track) = layout.scrollbar {
        Scrollbar::new(
            model.items.len(),
            usize::from(layout.viewport.height),
            usize::from(layout.scroll_offset),
        )
        .render_frame(frame, track, context);
    }
    let mut close = Button::new(
        "settings.animation.close",
        i18n::tr!("settings-animation-close"),
    );
    close.set_focused(model.selected_index == model.items.len());
    close.render_inline_frame(frame, layout.close, theme);
}
