use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Borders, Clear, Paragraph, Wrap};

use crate::render::{render_status, render_top};
use crate::{
    BorderShape, ShellChromeViewModel, ShellLayout, TimezoneMapWidget, TundraTheme,
    compute_shell_layout,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SettingsCategory {
    #[default]
    Appearance,
    RegionTime,
    FileExplorer,
    Editor,
}

impl SettingsCategory {
    pub const ALL: [Self; 4] = [
        Self::Appearance,
        Self::RegionTime,
        Self::FileExplorer,
        Self::Editor,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::RegionTime => "Region & Time",
            Self::FileExplorer => "File Explorer",
            Self::Editor => "Editor",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Appearance => "Your colors and borders",
            Self::RegionTime => "Language, city and timezone",
            Self::FileExplorer => "Display, sorting and safety",
            Self::Editor => "Cursor acceleration",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsField {
    BorderShape,
    BorderColor,
    AccentColor,
    Language,
    Timezone,
    WeatherLocation,
    ShowHidden,
    ShowSystem,
    ShowExtensions,
    FoldersFirst,
    ShowSidebar,
    CaseSensitiveSort,
    SizeFormat,
    DateZone,
    SortField,
    SortDirection,
    ConfirmDelete,
    ConfirmNameConflicts,
    CursorAcceleration,
    CursorDelay,
    CursorRamp,
    CursorHorizontalStep,
    CursorVerticalStep,
    RestoreDefaults,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsControlKind {
    Toggle,
    Cycle,
    Picker,
    Palette,
    Stepper,
    ReadOnly,
    Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsControl {
    Field(SettingsField),
    RestoreDefaults,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsItemViewModel {
    pub field: SettingsField,
    pub label: String,
    pub value: String,
    pub description: String,
    pub kind: SettingsControlKind,
    pub enabled: bool,
}

impl SettingsItemViewModel {
    pub fn new(
        field: SettingsField,
        label: impl Into<String>,
        value: impl Into<String>,
        description: impl Into<String>,
        kind: SettingsControlKind,
    ) -> Self {
        Self {
            field,
            label: label.into(),
            value: value.into(),
            description: description.into(),
            kind,
            enabled: true,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        if !enabled {
            self.kind = SettingsControlKind::ReadOnly;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsCardViewModel {
    pub title: String,
    pub items: Vec<SettingsItemViewModel>,
}

impl SettingsCardViewModel {
    pub fn new(title: impl Into<String>, items: Vec<SettingsItemViewModel>) -> Self {
        Self {
            title: title.into(),
            items,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsAppearancePreview {
    pub border_shape: BorderShape,
    pub border_color: Color,
    pub accent_color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsPickerKind {
    Language,
    Timezone,
    BorderColor,
    AccentColor,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsPickerOptionViewModel {
    pub label: String,
    pub detail: String,
    pub timezone_id: Option<String>,
    pub longitude: Option<f64>,
    pub latitude: Option<f64>,
}

impl SettingsPickerOptionViewModel {
    pub fn new(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            detail: detail.into(),
            timezone_id: None,
            longitude: None,
            latitude: None,
        }
    }

    pub fn timezone(
        mut self,
        timezone_id: impl Into<String>,
        longitude: f64,
        latitude: f64,
    ) -> Self {
        self.timezone_id = Some(timezone_id.into());
        self.longitude = Some(longitude);
        self.latitude = Some(latitude);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsPickerViewModel {
    pub kind: SettingsPickerKind,
    pub title: String,
    pub query: String,
    pub options: Vec<SettingsPickerOptionViewModel>,
    pub selected_index: usize,
    pub window_start: usize,
    pub searchable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsColorEditorViewModel {
    pub title: String,
    pub value: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsWeatherLocationEditorViewModel {
    pub value: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsViewModel {
    pub selected_category: SettingsCategory,
    pub selected_field: SettingsField,
    pub cards: Vec<SettingsCardViewModel>,
    pub appearance_preview: Option<SettingsAppearancePreview>,
    pub status: String,
    pub locked_message: Option<String>,
    pub scroll_offset: u16,
    pub picker: Option<SettingsPickerViewModel>,
    pub color_editor: Option<SettingsColorEditorViewModel>,
    pub weather_location_editor: Option<SettingsWeatherLocationEditorViewModel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsCategoryLayout {
    pub category: SettingsCategory,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsFieldLayout {
    pub field: SettingsField,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsPickerOptionLayout {
    pub index: usize,
    pub area: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsHitTarget {
    Category(SettingsCategory),
    Field(SettingsField),
    PickerOption(usize),
    ColorEditor,
    WeatherLocationEditor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsLayout {
    pub main: Rect,
    pub category_cards: Vec<SettingsCategoryLayout>,
    pub fields: Vec<SettingsFieldLayout>,
    pub picker_options: Vec<SettingsPickerOptionLayout>,
    pub color_editor: Option<Rect>,
    pub weather_location_editor: Option<Rect>,
}

pub fn render_settings(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SettingsViewModel,
    theme: &TundraTheme,
) -> SettingsLayout {
    let main = match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => compact,
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_status(frame, status, chrome, theme);
            main
        }
    };
    let layout = settings_layout(main, model);
    render_settings_content(frame, &layout, model, theme);
    layout
}

pub fn settings_layout(area: Rect, model: &SettingsViewModel) -> SettingsLayout {
    let inner = inset(area, 1, 1);
    let wide = inner.width >= 96;
    let (category_area, detail_area) = if wide {
        let category_width = 28.min(inner.width.saturating_sub(40));
        (
            Rect::new(inner.x, inner.y, category_width, inner.height),
            Rect::new(
                inner.x.saturating_add(category_width).saturating_add(1),
                inner.y,
                inner.width.saturating_sub(category_width).saturating_sub(1),
                inner.height,
            ),
        )
    } else {
        let category_height = 5.min(inner.height);
        (
            Rect::new(inner.x, inner.y, inner.width, category_height),
            Rect::new(
                inner.x,
                inner.y.saturating_add(category_height).saturating_add(1),
                inner.width,
                inner
                    .height
                    .saturating_sub(category_height)
                    .saturating_sub(1),
            ),
        )
    };

    let category_cards = if wide {
        SettingsCategory::ALL
            .into_iter()
            .enumerate()
            .map(|(index, category)| SettingsCategoryLayout {
                category,
                area: Rect::new(
                    category_area.x,
                    category_area
                        .y
                        .saturating_add((index as u16).saturating_mul(5)),
                    category_area.width,
                    4.min(category_area.height),
                ),
            })
            .filter(|layout| rect_intersection(layout.area, category_area).is_some())
            .collect()
    } else {
        let count = SettingsCategory::ALL.len() as u16;
        let width = category_area.width.checked_div(count).unwrap_or(0);
        SettingsCategory::ALL
            .into_iter()
            .enumerate()
            .map(|(index, category)| {
                let x = category_area
                    .x
                    .saturating_add((index as u16).saturating_mul(width));
                let final_width = if index + 1 == SettingsCategory::ALL.len() {
                    category_area.right().saturating_sub(x)
                } else {
                    width
                };
                SettingsCategoryLayout {
                    category,
                    area: Rect::new(x, category_area.y, final_width, category_area.height),
                }
            })
            .collect()
    };

    let mut fields = Vec::new();
    let mut y = detail_area.y.saturating_sub(
        model
            .scroll_offset
            .min(detail_area.height.saturating_add(200)),
    );
    if model.appearance_preview.is_some() {
        y = y.saturating_add(5);
    }
    for card in &model.cards {
        let height = (card.items.len() as u16).saturating_add(2).max(3);
        let card_area = Rect::new(detail_area.x, y, detail_area.width, height);
        for (index, item) in card.items.iter().enumerate() {
            let row = Rect::new(
                card_area.x.saturating_add(1),
                card_area.y.saturating_add(1).saturating_add(index as u16),
                card_area.width.saturating_sub(2),
                1,
            );
            if let Some(visible) = rect_intersection(row, detail_area) {
                fields.push(SettingsFieldLayout {
                    field: item.field,
                    area: visible,
                });
            }
        }
        y = y.saturating_add(height).saturating_add(1);
    }

    let mut picker_options = Vec::new();
    if let Some(picker) = &model.picker {
        let dialog = centered(area, area.width.min(78), area.height.min(24));
        let list = picker_list_area(dialog, picker.kind == SettingsPickerKind::Timezone);
        let visible_rows = usize::from(list.height);
        let start = picker.window_start.min(picker.options.len());
        let end = start.saturating_add(visible_rows).min(picker.options.len());
        picker_options.extend((start..end).map(|index| SettingsPickerOptionLayout {
            index,
            area: Rect::new(
                list.x,
                list.y.saturating_add((index - start) as u16),
                list.width,
                1,
            ),
        }));
    }

    SettingsLayout {
        main: area,
        category_cards,
        fields,
        picker_options,
        color_editor: model
            .color_editor
            .as_ref()
            .map(|_| centered(area, area.width.min(56), area.height.min(9))),
        weather_location_editor: model
            .weather_location_editor
            .as_ref()
            .map(|_| centered(area, area.width.min(68), area.height.min(11))),
    }
}

pub fn settings_hit_test(layout: &SettingsLayout, point: (u16, u16)) -> Option<SettingsHitTarget> {
    if let Some(area) = layout.weather_location_editor
        && contains(area, point)
    {
        return Some(SettingsHitTarget::WeatherLocationEditor);
    }
    if let Some(area) = layout.color_editor
        && contains(area, point)
    {
        return Some(SettingsHitTarget::ColorEditor);
    }
    if let Some(option) = layout
        .picker_options
        .iter()
        .find(|option| contains(option.area, point))
    {
        return Some(SettingsHitTarget::PickerOption(option.index));
    }
    if let Some(field) = layout
        .fields
        .iter()
        .find(|field| contains(field.area, point))
    {
        return Some(SettingsHitTarget::Field(field.field));
    }
    layout
        .category_cards
        .iter()
        .find(|category| contains(category.area, point))
        .map(|category| SettingsHitTarget::Category(category.category))
}

fn render_settings_content(
    frame: &mut Frame<'_>,
    layout: &SettingsLayout,
    model: &SettingsViewModel,
    theme: &TundraTheme,
) {
    frame.render_widget(
        theme
            .block()
            .title(" Settings ")
            .borders(Borders::ALL)
            .style(theme.body_style()),
        layout.main,
    );

    for category in &layout.category_cards {
        let selected = category.category == model.selected_category;
        let lines = vec![
            Line::styled(
                category.category.label(),
                if selected {
                    theme.title_style()
                } else {
                    theme.body_style()
                },
            ),
            Line::styled(category.category.description(), theme.muted_style()),
        ];
        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    theme
                        .block()
                        .borders(Borders::ALL)
                        .border_style(theme.selectable_border_style(selected)),
                )
                .wrap(Wrap { trim: true }),
            category.area,
        );
    }

    let detail_area = settings_detail_area(layout);
    if let Some(preview) = model.appearance_preview {
        let preview_area = Rect::new(
            detail_area.x,
            detail_area.y.saturating_sub(
                model
                    .scroll_offset
                    .min(detail_area.height.saturating_add(200)),
            ),
            detail_area.width,
            4,
        );
        if let Some(visible) = rect_intersection(preview_area, detail_area) {
            let preview_theme = TundraTheme {
                border_shape: preview.border_shape,
                border_color: preview.border_color,
                accent_color: preview.accent_color,
                ..*theme
            };
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled("Live preview", preview_theme.title_style()),
                    Line::styled(
                        "Selected controls use the accent color.",
                        preview_theme.body_style(),
                    ),
                ])
                .block(
                    preview_theme
                        .block()
                        .title(" Preview ")
                        .borders(Borders::ALL),
                ),
                visible,
            );
        }
    }

    render_cards(frame, detail_area, model, theme);
    render_settings_footer(frame, detail_area, model, theme);

    if let Some(picker) = &model.picker {
        render_picker(frame, layout.main, picker, theme);
    }
    if let Some(editor) = &model.color_editor {
        render_color_editor(frame, layout.main, editor, theme);
    }
    if let Some(editor) = &model.weather_location_editor {
        render_weather_location_editor(frame, layout.main, editor, theme);
    }
}

fn render_cards(
    frame: &mut Frame<'_>,
    detail_area: Rect,
    model: &SettingsViewModel,
    theme: &TundraTheme,
) {
    let mut y = detail_area.y.saturating_sub(
        model
            .scroll_offset
            .min(detail_area.height.saturating_add(200)),
    );
    if model.appearance_preview.is_some() {
        y = y.saturating_add(5);
    }
    for card in &model.cards {
        let height = (card.items.len() as u16).saturating_add(2).max(3);
        let card_area = Rect::new(detail_area.x, y, detail_area.width, height);
        if let Some(visible) = rect_intersection(card_area, detail_area) {
            frame.render_widget(
                theme
                    .block()
                    .title(format!(" {} ", card.title))
                    .borders(Borders::ALL)
                    .style(theme.body_style()),
                visible,
            );
        }
        for (index, item) in card.items.iter().enumerate() {
            let row = Rect::new(
                card_area.x.saturating_add(1),
                card_area.y.saturating_add(1).saturating_add(index as u16),
                card_area.width.saturating_sub(2),
                1,
            );
            let Some(row) = rect_intersection(row, detail_area) else {
                continue;
            };
            let selected = item.field == model.selected_field;
            let base = if !item.enabled {
                theme.muted_style().add_modifier(Modifier::DIM)
            } else if selected {
                theme.title_style()
            } else {
                theme.body_style()
            };
            let value = control_value(item);
            let value_width = value.chars().count().min(usize::from(row.width / 2));
            let label_width = usize::from(row.width)
                .saturating_sub(value_width)
                .saturating_sub(1);
            let label = truncate(&item.label, label_width);
            let padding = usize::from(row.width)
                .saturating_sub(label.chars().count())
                .saturating_sub(value.chars().count());
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(label, base),
                    Span::raw(" ".repeat(padding)),
                    Span::styled(value, base),
                ])),
                row,
            );
        }
        y = y.saturating_add(height).saturating_add(1);
    }
}

fn render_settings_footer(
    frame: &mut Frame<'_>,
    detail: Rect,
    model: &SettingsViewModel,
    theme: &TundraTheme,
) {
    if detail.height < 2 {
        return;
    }
    let description = model
        .cards
        .iter()
        .flat_map(|card| &card.items)
        .find(|item| item.field == model.selected_field)
        .map(|item| item.description.as_str())
        .unwrap_or("Choose a setting.");
    let lock = model.locked_message.as_deref().unwrap_or("");
    let text = if lock.is_empty() {
        format!("{}  |  {}", model.status, description)
    } else {
        format!("{}  |  {}  |  {}", model.status, lock, description)
    };
    let area = Rect::new(detail.x, detail.bottom().saturating_sub(1), detail.width, 1);
    frame.render_widget(
        Paragraph::new(Line::styled(
            truncate(&text, usize::from(area.width)),
            theme.muted_style(),
        )),
        area,
    );
}

fn render_picker(
    frame: &mut Frame<'_>,
    area: Rect,
    picker: &SettingsPickerViewModel,
    theme: &TundraTheme,
) {
    let dialog = centered(area, area.width.min(78), area.height.min(24));
    frame.render_widget(Clear, dialog);
    frame.render_widget(
        theme
            .block()
            .title(format!(" {} ", picker.title))
            .borders(Borders::ALL)
            .style(theme.body_style()),
        dialog,
    );
    let query = if picker.searchable {
        format!("Search: {}_", picker.query)
    } else {
        "Arrows: choose    Enter: apply    Esc: cancel".to_string()
    };
    frame.render_widget(
        Paragraph::new(Line::styled(query, theme.muted_style())),
        Rect::new(
            dialog.x.saturating_add(2),
            dialog.y.saturating_add(1),
            dialog.width.saturating_sub(4),
            1,
        ),
    );
    let list = picker_list_area(dialog, picker.kind == SettingsPickerKind::Timezone);
    let visible = usize::from(list.height);
    let start = picker.window_start.min(picker.options.len());
    let end = start.saturating_add(visible).min(picker.options.len());
    let lines = picker.options[start..end]
        .iter()
        .enumerate()
        .map(|(offset, option)| {
            let index = start + offset;
            let selected = index == picker.selected_index;
            let marker = if selected { "> " } else { "  " };
            let detail = if option.detail.is_empty() {
                String::new()
            } else {
                format!("  {}", option.detail)
            };
            Line::styled(
                truncate(
                    &format!("{marker}{}{detail}", option.label),
                    usize::from(list.width),
                ),
                if selected {
                    theme.title_style()
                } else {
                    theme.body_style()
                },
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), list);

    if picker.kind == SettingsPickerKind::Timezone && dialog.width >= 70 {
        let map = picker_map_area(dialog);
        if let Some(option) = picker.options.get(picker.selected_index) {
            let mut widget = TimezoneMapWidget::themed(&[], theme)
                .selected_timezone_id(option.timezone_id.as_deref());
            if let (Some(longitude), Some(latitude)) = (option.longitude, option.latitude) {
                widget = widget.city(longitude, latitude);
            }
            frame.render_widget(widget, map);
        }
    }
}

fn render_color_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    editor: &SettingsColorEditorViewModel,
    theme: &TundraTheme,
) {
    let dialog = centered(area, area.width.min(56), area.height.min(9));
    frame.render_widget(Clear, dialog);
    let lines = vec![
        Line::from("Enter a color as #RRGGBB."),
        Line::styled(format!("> {}_", editor.value), theme.title_style()),
        Line::styled(
            editor.error.clone().unwrap_or_default(),
            theme.error_style(),
        ),
        Line::styled("Enter: apply    Esc: cancel", theme.muted_style()),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                theme
                    .block()
                    .title(format!(" {} ", editor.title))
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: true }),
        dialog,
    );
}

fn render_weather_location_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    editor: &SettingsWeatherLocationEditorViewModel,
    theme: &TundraTheme,
) {
    let dialog = centered(area, area.width.min(68), area.height.min(11));
    frame.render_widget(Clear, dialog);
    let lines = vec![
        Line::from("Enter a detailed city or address using English characters."),
        Line::styled(format!("> {}_", editor.value), theme.title_style()),
        Line::styled(
            editor.error.clone().unwrap_or_default(),
            theme.error_style(),
        ),
        Line::styled(
            "Leave empty to use the timezone location.",
            theme.muted_style(),
        ),
        Line::styled("Enter: continue    Esc: cancel", theme.muted_style()),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                theme
                    .block()
                    .title(" Weather location ")
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: true }),
        dialog,
    );
}

fn settings_detail_area(layout: &SettingsLayout) -> Rect {
    let inner = inset(layout.main, 1, 1);
    if inner.width >= 96 {
        let category_width = 28.min(inner.width.saturating_sub(40));
        Rect::new(
            inner.x.saturating_add(category_width).saturating_add(1),
            inner.y,
            inner.width.saturating_sub(category_width).saturating_sub(1),
            inner.height,
        )
    } else {
        let category_height = 5.min(inner.height);
        Rect::new(
            inner.x,
            inner.y.saturating_add(category_height).saturating_add(1),
            inner.width,
            inner
                .height
                .saturating_sub(category_height)
                .saturating_sub(1),
        )
    }
}

fn control_value(item: &SettingsItemViewModel) -> String {
    if !item.enabled {
        return format!("{} [locked]", item.value);
    }
    match item.kind {
        SettingsControlKind::Toggle => format!("[{}]", item.value),
        SettingsControlKind::Cycle | SettingsControlKind::Stepper => {
            format!("< {} >", item.value)
        }
        SettingsControlKind::Picker | SettingsControlKind::Palette => {
            format!("[ {} ]", item.value)
        }
        SettingsControlKind::Action => format!("[ {} ]", item.value),
        SettingsControlKind::ReadOnly => item.value.clone(),
    }
}

fn picker_list_area(dialog: Rect, timezone: bool) -> Rect {
    let content = inset(dialog, 2, 2);
    if timezone && dialog.width >= 70 {
        Rect::new(
            content.x,
            content.y.saturating_add(1),
            content.width / 2,
            content.height.saturating_sub(1),
        )
    } else {
        Rect::new(
            content.x,
            content.y.saturating_add(1),
            content.width,
            content.height.saturating_sub(1),
        )
    }
}

fn picker_map_area(dialog: Rect) -> Rect {
    let content = inset(dialog, 2, 2);
    let left = content.width / 2;
    Rect::new(
        content.x.saturating_add(left).saturating_add(1),
        content.y.saturating_add(1),
        content.width.saturating_sub(left).saturating_sub(1),
        content.height.saturating_sub(1),
    )
}

fn inset(area: Rect, horizontal: u16, vertical: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(horizontal),
        area.y.saturating_add(vertical),
        area.width.saturating_sub(horizontal.saturating_mul(2)),
        area.height.saturating_sub(vertical.saturating_mul(2)),
    )
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

fn rect_intersection(first: Rect, second: Rect) -> Option<Rect> {
    let x = first.x.max(second.x);
    let y = first.y.max(second.y);
    let right = first.right().min(second.right());
    let bottom = first.bottom().min(second.bottom());
    (right > x && bottom > y).then_some(Rect::new(x, y, right - x, bottom - y))
}

fn contains(area: Rect, point: (u16, u16)) -> bool {
    point.0 >= area.x && point.0 < area.right() && point.1 >= area.y && point.1 < area.bottom()
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }
    if width <= 1 {
        return value.chars().take(width).collect();
    }
    let mut truncated = value.chars().take(width - 1).collect::<String>();
    truncated.push('…');
    truncated
}
