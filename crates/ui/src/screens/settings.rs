use ratatui::Frame;
use ratatui::layout::{Constraint, HorizontalAlignment, Layout, Rect};
use ratatui::style::{Color, Modifier};
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Wrap};

use crate::components::{
    Button, ComponentTone, DataTable, List as ComponentList, ListItem as ComponentListItem,
    Scrollbar, Surface, TextInput, terminal_width, truncate_to_terminal_width,
};
use crate::screens::shell::{render_status, render_top};
use crate::{
    BorderShape, RenderContext, ShellChromeViewModel, ShellLayout, TimezoneMapWidget, TundraTheme,
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
            Self::Appearance => "Theme, motion, icons, colors and borders",
            Self::RegionTime => "Language, city and timezone",
            Self::FileExplorer => "Display, sorting and safety",
            Self::Editor => "Cursor and file associations",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsField {
    Theme,
    BorderShape,
    BorderColor,
    AccentColor,
    MotionPreference,
    Language,
    Timezone,
    TimeSyncSource,
    TimeSyncServer,
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
    ExplorerOpenExtensions,
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
    Theme,
    DefaultThemeIcons,
    Language,
    Timezone,
    BorderColor,
    AccentColor,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsPickerOptionViewModel {
    pub label: String,
    pub detail: String,
    pub enabled: bool,
    pub timezone_id: Option<String>,
    pub longitude: Option<f64>,
    pub latitude: Option<f64>,
}

impl SettingsPickerOptionViewModel {
    pub fn new(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            detail: detail.into(),
            enabled: true,
            timezone_id: None,
            longitude: None,
            latitude: None,
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFileExtensionsEditorViewModel {
    pub value: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsTimeSyncServerEditorViewModel {
    pub value: String,
    pub error: Option<String>,
    pub validating: bool,
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
    pub file_extensions_editor: Option<SettingsFileExtensionsEditorViewModel>,
    pub time_sync_server_editor: Option<SettingsTimeSyncServerEditorViewModel>,
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
    FileExtensionsEditor,
    TimeSyncServerEditor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsLayout {
    pub main: Rect,
    pub category_cards: Vec<SettingsCategoryLayout>,
    pub fields: Vec<SettingsFieldLayout>,
    pub picker_options: Vec<SettingsPickerOptionLayout>,
    pub color_editor: Option<Rect>,
    pub weather_location_editor: Option<Rect>,
    pub file_extensions_editor: Option<Rect>,
    pub time_sync_server_editor: Option<Rect>,
}

pub fn render_settings(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SettingsViewModel,
    theme: &TundraTheme,
) -> SettingsLayout {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_settings_context(frame, area, chrome, model, &context)
}

pub(crate) fn render_settings_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SettingsViewModel,
    context: &RenderContext,
) -> SettingsLayout {
    let theme = &context.compatibility_theme();
    let main = match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => compact,
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_status(frame, status, chrome, theme);
            main
        }
    };
    let layout = settings_layout(main, model);
    render_settings_content(frame, &layout, model, context);
    layout
}

pub fn settings_layout(area: Rect, model: &SettingsViewModel) -> SettingsLayout {
    let (category_area, detail_area) = settings_content_areas(area);
    let category_cards = SettingsCategory::ALL
        .into_iter()
        .enumerate()
        .filter_map(|(index, category)| {
            let row = Rect::new(
                category_area.x.saturating_add(1),
                category_area
                    .y
                    .saturating_add(1)
                    .saturating_add(index as u16),
                category_area.width.saturating_sub(2),
                1,
            );
            rect_intersection(row, category_area)
                .map(|area| SettingsCategoryLayout { category, area })
        })
        .collect();

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
        file_extensions_editor: model
            .file_extensions_editor
            .as_ref()
            .map(|_| centered(area, area.width.min(72), area.height.min(11))),
        time_sync_server_editor: model
            .time_sync_server_editor
            .as_ref()
            .map(|_| centered(area, area.width.min(76), area.height.min(11))),
    }
}

pub fn settings_hit_test(layout: &SettingsLayout, point: (u16, u16)) -> Option<SettingsHitTarget> {
    if let Some(area) = layout.time_sync_server_editor
        && contains(area, point)
    {
        return Some(SettingsHitTarget::TimeSyncServerEditor);
    }
    if let Some(area) = layout.file_extensions_editor
        && contains(area, point)
    {
        return Some(SettingsHitTarget::FileExtensionsEditor);
    }
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
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    Surface::new()
        .titled(" Settings ")
        .bordered(true)
        .render_frame(frame, layout.main, context);

    let mut categories = ComponentList::new(
        "settings.categories",
        SettingsCategory::ALL
            .into_iter()
            .map(|category| {
                ComponentListItem::new(
                    format!("settings.category.{category:?}").to_ascii_lowercase(),
                    category.label(),
                )
            })
            .collect(),
    )
    .titled(" Sections ");
    categories.set_selected(
        SettingsCategory::ALL
            .iter()
            .position(|category| *category == model.selected_category),
    );
    categories.set_focused(true);
    categories.render_with_context(settings_category_area(layout), frame.buffer_mut(), context);

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
            let preview_context =
                RenderContext::from_theme(&preview_theme, context.motion, context.capabilities);
            let surface = Surface::new()
                .titled(" Preview ")
                .bordered(true)
                .border_shape(preview.border_shape);
            let inner = surface.inner(visible);
            surface.render_frame(frame, visible, &preview_context);
            frame.render_widget(
                Paragraph::new(vec![
                    Line::styled("Live preview", preview_theme.title_style()),
                    Line::styled(
                        "Selected controls use the accent color.",
                        preview_theme.body_style(),
                    ),
                ])
                .alignment(HorizontalAlignment::Left),
                inner,
            );
        }
    }

    render_cards(frame, detail_area, model, context);
    render_settings_footer(frame, detail_area, model, context);

    if let Some(picker) = &model.picker {
        render_picker(frame, layout.main, picker, context);
    }
    if let Some(editor) = &model.color_editor {
        render_color_editor(frame, layout.main, editor, context);
    }
    if let Some(editor) = &model.weather_location_editor {
        render_weather_location_editor(frame, layout.main, editor, context);
    }
    if let Some(editor) = &model.file_extensions_editor {
        render_file_extensions_editor(frame, layout.main, editor, context);
    }
    if let Some(editor) = &model.time_sync_server_editor {
        render_time_sync_server_editor(frame, layout.main, editor, context);
    }
}

fn render_cards(
    frame: &mut Frame<'_>,
    detail_area: Rect,
    model: &SettingsViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
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
            Surface::new()
                .titled(format!(" {} ", card.title))
                .bordered(true)
                .raised(true)
                .render_frame(frame, visible, context);
        }
        let rows_area = Rect::new(
            card_area.x.saturating_add(1),
            card_area.y.saturating_add(1),
            card_area.width.saturating_sub(2),
            u16::try_from(card.items.len()).unwrap_or(u16::MAX),
        );
        let Some(visible_rows) = rect_intersection(rows_area, detail_area) else {
            y = y.saturating_add(height).saturating_add(1);
            continue;
        };
        let value_width = card
            .items
            .iter()
            .map(settings_control_width)
            .max()
            .unwrap_or(0)
            .min(rows_area.width / 2);
        let rows = card
            .items
            .iter()
            .map(|item| vec![item.label.clone(), String::new()])
            .collect::<Vec<_>>();
        let first_visible = usize::from(visible_rows.y.saturating_sub(rows_area.y));
        let visible_end = first_visible.saturating_add(usize::from(visible_rows.height));
        let selected = card
            .items
            .iter()
            .position(|item| item.field == model.selected_field)
            .filter(|index| *index >= first_visible && *index < visible_end);
        let tones = card
            .items
            .iter()
            .map(|item| {
                if item.enabled {
                    ComponentTone::Default
                } else {
                    ComponentTone::Muted
                }
            })
            .collect();
        let mut table = DataTable::new(
            format!("settings.card.{}", card.title),
            Vec::<String>::new(),
            rows,
        )
        .with_column_widths(vec![
            visible_rows
                .width
                .saturating_sub(value_width)
                .saturating_sub(1),
            value_width,
        ])
        .with_viewport_start(first_visible)
        .show_header(false)
        .with_row_tones(tones)
        .bordered(false);
        table.selected = selected;
        table.state.focused = selected.is_some();
        table.render_frame(frame, visible_rows, context);

        for (index, item) in card.items.iter().enumerate() {
            let row = Rect::new(
                rows_area.x,
                rows_area.y.saturating_add(index as u16),
                rows_area.width,
                1,
            );
            let Some(row) = rect_intersection(row, detail_area) else {
                continue;
            };
            render_settings_control(
                frame,
                Rect::new(
                    row.right().saturating_sub(value_width),
                    row.y,
                    value_width,
                    row.height,
                ),
                item,
                item.field == model.selected_field,
                theme,
            );
        }
        y = y.saturating_add(height).saturating_add(1);
    }
}

fn render_settings_footer(
    frame: &mut Frame<'_>,
    detail: Rect,
    model: &SettingsViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
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
        ))
        .alignment(HorizontalAlignment::Left),
        area,
    );
}

fn render_picker(
    frame: &mut Frame<'_>,
    area: Rect,
    picker: &SettingsPickerViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    let dialog = centered(area, area.width.min(78), area.height.min(24));
    frame.render_widget(Clear, dialog);
    Surface::new()
        .titled(format!(" {} ", picker.title))
        .bordered(true)
        .raised(true)
        .render_frame(frame, dialog, context);
    let query_area = Rect::new(
        dialog.x.saturating_add(2),
        dialog.y.saturating_add(1),
        dialog.width.saturating_sub(4),
        1,
    );
    if picker.searchable {
        render_controlled_text_input(
            frame,
            query_area,
            "settings.picker-search",
            &picker.query,
            "Search: ",
            theme,
            theme.muted,
        );
    } else {
        let help = if picker.kind == SettingsPickerKind::DefaultThemeIcons {
            "Arrows: choose    Enter: apply    Esc: back"
        } else {
            "Arrows: choose    Enter: apply    Esc: cancel"
        };
        frame.render_widget(
            Paragraph::new(Line::styled(help, theme.muted_style()))
                .alignment(HorizontalAlignment::Left),
            query_area,
        );
    }
    let list = picker_list_area(dialog, picker.kind == SettingsPickerKind::Timezone);
    let visible = usize::from(list.height);
    let start = picker.window_start.min(picker.options.len());
    let items = picker
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            let detail = if option.detail.is_empty() {
                String::new()
            } else {
                format!("  {}", option.detail)
            };
            ComponentListItem::new(
                format!("settings.picker.{index}"),
                truncate(
                    &format!("{}{detail}", option.label),
                    usize::from(list.width.saturating_sub(2)),
                ),
            )
            .disabled(!option.enabled)
            .tone(if option.enabled {
                ComponentTone::Default
            } else {
                ComponentTone::Muted
            })
        })
        .collect::<Vec<_>>();
    let mut component_list =
        ComponentList::new("settings.picker", items).with_viewport_start(start);
    component_list.set_focused(true);
    component_list.set_selected(Some(picker.selected_index));
    component_list.render_borderless_frame(frame, list, theme);
    if picker.options.len() > visible && list.width > 0 && list.height > 0 {
        Scrollbar::new(picker.options.len(), visible, start).render_frame(frame, list, context);
    }

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
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    let dialog = centered(area, area.width.min(56), area.height.min(9));
    frame.render_widget(Clear, dialog);
    let lines = vec![
        Line::from("Enter a color as #RRGGBB."),
        Line::from(""),
        Line::styled(
            editor.error.clone().unwrap_or_default(),
            theme.error_style(),
        ),
        Line::styled("Enter: apply    Esc: cancel", theme.muted_style()),
    ];
    let surface = Surface::new()
        .titled(format!(" {} ", editor.title))
        .bordered(true)
        .raised(true);
    let inner = surface.inner(dialog);
    surface.render_frame(frame, dialog, context);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(HorizontalAlignment::Left)
            .wrap(Wrap { trim: true }),
        inner,
    );
    render_editor_text_input(frame, dialog, "settings.color-editor", &editor.value, theme);
}

fn render_weather_location_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    editor: &SettingsWeatherLocationEditorViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    let dialog = centered(area, area.width.min(68), area.height.min(11));
    frame.render_widget(Clear, dialog);
    let lines = vec![
        Line::from("Enter a detailed city or address using English characters."),
        Line::from(""),
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
    let surface = Surface::new()
        .titled(" Weather location ")
        .bordered(true)
        .raised(true);
    let inner = surface.inner(dialog);
    surface.render_frame(frame, dialog, context);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(HorizontalAlignment::Left)
            .wrap(Wrap { trim: true }),
        inner,
    );
    render_editor_text_input(
        frame,
        dialog,
        "settings.weather-location-editor",
        &editor.value,
        theme,
    );
}

fn render_file_extensions_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    editor: &SettingsFileExtensionsEditorViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    let dialog = centered(area, area.width.min(72), area.height.min(11));
    frame.render_widget(Clear, dialog);
    let lines = vec![
        Line::from("Enter comma-separated filename suffixes Explorer should open here."),
        Line::from(""),
        Line::styled(
            editor.error.clone().unwrap_or_default(),
            theme.error_style(),
        ),
        Line::styled(
            "Examples: .md, .txt, .rs, .d.ts (matching is case-insensitive)",
            theme.muted_style(),
        ),
        Line::styled(
            "Leave empty to always use the system default.  Enter: save  Esc: cancel",
            theme.muted_style(),
        ),
    ];
    let surface = Surface::new()
        .titled(" Explorer files opened in Editor ")
        .bordered(true)
        .raised(true);
    let inner = surface.inner(dialog);
    surface.render_frame(frame, dialog, context);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(HorizontalAlignment::Left)
            .wrap(Wrap { trim: true }),
        inner,
    );
    render_editor_text_input(
        frame,
        dialog,
        "settings.file-extensions-editor",
        &editor.value,
        theme,
    );
}

fn render_time_sync_server_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    editor: &SettingsTimeSyncServerEditorViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    let dialog = centered(area, area.width.min(76), area.height.min(11));
    frame.render_widget(Clear, dialog);
    let status = if editor.validating {
        "Synchronizing with this server…"
    } else {
        editor.error.as_deref().unwrap_or_default()
    };
    let status_style = if editor.validating {
        theme.muted_style()
    } else {
        theme.error_style()
    };
    let lines = vec![
        Line::from("Enter an HTTP(S) endpoint that returns a valid Date response header."),
        Line::from(""),
        Line::styled(status, status_style),
        Line::styled(
            "The address is saved only after a successful synchronization test.",
            theme.muted_style(),
        ),
        Line::styled("Enter: test and save    Esc: cancel", theme.muted_style()),
    ];
    let surface = Surface::new()
        .titled(" Time synchronization server ")
        .bordered(true)
        .raised(true);
    let inner = surface.inner(dialog);
    surface.render_frame(frame, dialog, context);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(HorizontalAlignment::Left)
            .wrap(Wrap { trim: true }),
        inner,
    );
    render_editor_text_input(
        frame,
        dialog,
        "settings.time-sync-server-editor",
        &editor.value,
        theme,
    );
}

fn render_editor_text_input(
    frame: &mut Frame<'_>,
    dialog: Rect,
    id: &'static str,
    value: &str,
    theme: &TundraTheme,
) {
    render_controlled_text_input(
        frame,
        Rect::new(
            dialog.x.saturating_add(1),
            dialog.y.saturating_add(2),
            dialog.width.saturating_sub(2),
            1,
        ),
        id,
        value,
        "> ",
        theme,
        theme.accent_color,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_controlled_text_input(
    frame: &mut Frame<'_>,
    area: Rect,
    id: &'static str,
    value: &str,
    prefix: &str,
    theme: &TundraTheme,
    foreground: Color,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut input = TextInput::new(id).with_cursor_symbol("_");
    input.set_value(value);
    input.set_focused(true);

    let mut input_theme = *theme;
    input_theme.foreground = foreground;
    input.render_borderless_frame_with_prefix(frame, area, &input_theme, prefix);
}

fn settings_detail_area(layout: &SettingsLayout) -> Rect {
    settings_content_areas(layout.main).1
}

fn settings_category_area(layout: &SettingsLayout) -> Rect {
    settings_content_areas(layout.main).0
}

fn settings_content_areas(area: Rect) -> (Rect, Rect) {
    let inner = inset(area, 1, 1);
    let sidebar_width = 18.min(inner.width.saturating_sub(1));
    let gap = u16::from(inner.width > sidebar_width);
    let [category, _, detail] = Layout::horizontal([
        Constraint::Length(sidebar_width),
        Constraint::Length(gap),
        Constraint::Min(0),
    ])
    .areas(inner);
    (category, detail)
}

fn render_settings_control(
    frame: &mut Frame<'_>,
    area: Rect,
    item: &SettingsItemViewModel,
    selected: bool,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if !item.enabled || item.kind == SettingsControlKind::ReadOnly {
        let label = if item.enabled {
            item.value.clone()
        } else {
            format!("{} locked", item.value)
        };
        let style = if item.enabled {
            if selected {
                theme.title_style()
            } else {
                theme.body_style()
            }
        } else {
            theme.muted_style().add_modifier(Modifier::DIM)
        };
        frame.render_widget(
            Paragraph::new(label)
                .alignment(HorizontalAlignment::Right)
                .style(style),
            area,
        );
        return;
    }

    let mut button = Button::new(
        format!("settings.field.{:?}", item.field).to_ascii_lowercase(),
        item.value.clone(),
    );
    button.state.selected = selected;
    button.set_focused(selected);
    button.render_inline_frame(frame, area, theme);
}

fn settings_control_width(item: &SettingsItemViewModel) -> u16 {
    let label_width = u16::try_from(terminal_width(&item.value)).unwrap_or(u16::MAX);
    if !item.enabled {
        return label_width.saturating_add(" locked".len() as u16);
    }

    let component_padding = match item.kind {
        SettingsControlKind::Toggle => 2,
        SettingsControlKind::Cycle
        | SettingsControlKind::Picker
        | SettingsControlKind::Palette
        | SettingsControlKind::Stepper
        | SettingsControlKind::Action => 4,
        SettingsControlKind::ReadOnly => 0,
    };
    label_width.saturating_add(component_padding)
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
    (right > x && bottom > y).then(|| Rect::new(x, y, right - x, bottom - y))
}

fn contains(area: Rect, point: (u16, u16)) -> bool {
    point.0 >= area.x && point.0 < area.right() && point.1 >= area.y && point.1 < area.bottom()
}

fn truncate(value: &str, width: usize) -> String {
    if terminal_width(value) <= width {
        return value.to_string();
    }
    if width == 0 {
        return String::new();
    }

    let content_width = width.saturating_sub(1);
    let mut truncated = truncate_to_terminal_width(value, content_width);
    truncated.push('…');
    truncated
}
