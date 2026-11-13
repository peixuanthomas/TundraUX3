mod animation;
pub use animation::{SettingsAnimationEditorViewModel, SettingsAnimationLayout};
mod applications;
pub use applications::{SettingsApplication, SettingsApplicationsLayout};
mod forms;
mod number;
pub use number::{SettingsNumberEditorViewModel, SettingsNumberLayout};
mod slider;
pub use slider::SettingsSliderLayout;

use ratatui::Frame;
use ratatui::layout::{Constraint, HorizontalAlignment, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Paragraph, Wrap};

use crate::components::{
    Button, ButtonEmphasis, ComponentTone, DataTable, DialogLayout, List as ComponentList,
    ListItem as ComponentListItem, Scrollbar, SidebarWorkspaceLayout, Surface, TextInput,
    UpdateActivity, terminal_width, truncate_to_terminal_width, visible_scrolled_rect,
};
use crate::{RenderContext, TimezoneMapWidget, TundraTheme};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SettingsCategory {
    #[default]
    Appearance,
    RegionTime,
    System,
    Sound,
    Display,
    Wifi,
    Bluetooth,
    Applications,
    Update,
}

impl SettingsCategory {
    pub const ALL: [Self; 5] = [
        Self::Appearance,
        Self::RegionTime,
        Self::System,
        Self::Applications,
        Self::Update,
    ];

    pub fn is_system_device(self) -> bool {
        matches!(
            self,
            Self::Sound | Self::Display | Self::Wifi | Self::Bluetooth
        )
    }

    pub fn label(self) -> String {
        match self {
            Self::Appearance => i18n::tr!("ui-settings-appearance"),
            Self::RegionTime => i18n::tr!("ui-settings-region-time"),
            Self::System => i18n::tr!("ui-settings-system"),
            Self::Sound => i18n::tr!("ui-settings-sound"),
            Self::Display => i18n::tr!("ui-settings-display"),
            Self::Wifi => i18n::tr!("ui-settings-wifi"),
            Self::Bluetooth => i18n::tr!("ui-settings-bluetooth"),
            Self::Applications => i18n::tr!("ui-settings-applications"),
            Self::Update => i18n::tr!("ui-settings-update"),
        }
    }

    pub fn description(self) -> String {
        match self {
            Self::Appearance => i18n::tr!("ui-settings-theme-motion-icons-colors-and-borders"),
            Self::RegionTime => i18n::tr!("ui-settings-language-city-and-timezone"),
            Self::System => i18n::tr!("ui-settings-storage-pressure-warning-thresholds"),
            Self::Sound => i18n::tr!("ui-settings-sound-description"),
            Self::Display => i18n::tr!("ui-settings-display-description"),
            Self::Wifi => i18n::tr!("ui-settings-wifi-description"),
            Self::Bluetooth => i18n::tr!("ui-settings-bluetooth-description"),
            Self::Applications => i18n::tr!("ui-settings-applications-description"),
            Self::Update => i18n::tr!("ui-settings-version-commits-and-source-updates"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SettingsFocus {
    Categories,
    OperatingSystem,
    #[default]
    Content,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsField {
    ApplicationList,
    AutoAdmin,
    AutomationEnabled,
    ScreenTimeout,
    SoundOutputDevice,
    SoundOutputVolume,
    SoundOutputMute,
    SoundInputDevice,
    SoundInputVolume,
    SoundInputMute,
    DisplayDevice,
    DisplayBrightness,
    DisplayAutomaticBrightness,
    WifiEnabled,
    WifiConnection,
    WifiAvailableNetworks,
    WifiSavedNetworks,
    WifiRefresh,
    WifiConnect,
    WifiDisconnect,
    WifiForget,
    BluetoothEnabled,
    BluetoothDiscoverable,
    BluetoothConnectedDevices,
    BluetoothPairedDevices,
    BluetoothNearbyDevices,
    BluetoothSearch,
    BluetoothPair,
    BluetoothConnect,
    BluetoothDisconnect,
    BluetoothUnpair,
    Theme,
    BorderShape,
    BorderColor,
    AccentColor,
    MotionPreference,
    AnimationSpeed,
    AnimationAdvanced,
    PageAcceleration,
    PageCruise,
    PageDeceleration,
    DialogAcceleration,
    DialogCruise,
    DialogDeceleration,
    MenuAcceleration,
    MenuCruise,
    MenuDeceleration,
    ResetAnimationSpeed,
    Language,
    Timezone,
    TimeSyncSource,
    TimeSyncServer,
    RefreshSystemTime,
    WeatherLocation,
    SystemLowAvailable,
    SystemLowPercentage,
    SystemCriticalAvailable,
    SystemCriticalPercentage,
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
    InstalledVersion,
    RemoteVersion,
    CheckUpdates,
    StartUpdate,
    UpdateMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsControlKind {
    Toggle,
    Cycle,
    Slider { selected: usize },
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
    pub unavailable_reason: Option<String>,
    pub checked: Option<bool>,
    pub options: Vec<String>,
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
            unavailable_reason: None,
            checked: None,
            options: Vec::new(),
        }
    }

    pub fn unavailable(mut self, reason: impl Into<String>) -> Self {
        self.enabled = false;
        self.unavailable_reason = Some(reason.into());
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = Some(checked);
        self
    }

    pub fn options(mut self, options: Vec<String>) -> Self {
        self.options = options;
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
pub enum SettingsPickerKind {
    AutoAdmin,
    ScreenTimeout,
    Theme,
    DefaultThemeIcons,
    Language,
    Timezone,
    SystemTimeServers,
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
    pub editing: crate::components::TextEditState,
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
    pub editing: crate::components::TextEditState,
    pub title: String,
    pub value: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsWeatherLocationEditorViewModel {
    pub editing: crate::components::TextEditState,
    pub value: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFileExtensionsEditorViewModel {
    pub editing: crate::components::TextEditState,
    pub value: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsTimeSyncServerEditorViewModel {
    pub editing: crate::components::TextEditState,
    pub system_server_label: Option<String>,
    pub value: String,
    pub error: Option<String>,
    pub validating: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsViewModel {
    pub focus: SettingsFocus,
    pub form_selected: usize,
    pub form_interaction: crate::components::FormInteraction,
    pub selected_application: SettingsApplication,
    pub selected_category: SettingsCategory,
    pub selected_field: SettingsField,
    pub cards: Vec<SettingsCardViewModel>,
    pub status: String,
    pub locked_message: Option<String>,
    pub scroll_offset: u16,
    pub animation_editor: Option<SettingsAnimationEditorViewModel>,
    pub number_editor: Option<SettingsNumberEditorViewModel>,
    pub picker: Option<SettingsPickerViewModel>,
    pub color_editor: Option<SettingsColorEditorViewModel>,
    pub weather_location_editor: Option<SettingsWeatherLocationEditorViewModel>,
    pub file_extensions_editor: Option<SettingsFileExtensionsEditorViewModel>,
    pub time_sync_server_editor: Option<SettingsTimeSyncServerEditorViewModel>,
    pub update: Option<SettingsUpdateViewModel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsUpdateCommitViewModel {
    pub sha: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsUpdateConfirmationViewModel {
    pub title: String,
    pub body: String,
    pub confirm_label: String,
    pub confirm_selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsUpdateViewModel {
    pub summary_title: Option<String>,
    pub activity: Option<crate::components::UpdateActivityViewModel>,
    pub commits: Vec<SettingsUpdateCommitViewModel>,
    pub empty_message: String,
    pub confirmation: Option<SettingsUpdateConfirmationViewModel>,
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
    Application(SettingsApplication),
    OperatingSystem,
    OverlayApply,
    OverlayCancel,
    AdjustField(SettingsField, i8),
    SliderOption(SettingsField, usize),
    Category(SettingsCategory),
    Field(SettingsField),
    PickerOption(usize),
    ColorEditor,
    WeatherLocationEditor,
    FileExtensionsEditor,
    TimeSyncServerEditor,
    NumberEditor,
    UpdateConfirm,
    UpdateCancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsLayout {
    pub applications: Option<SettingsApplicationsLayout>,
    pub workspace: SidebarWorkspaceLayout,
    pub category: Rect,
    pub operating_system: Rect,
    pub main: Rect,
    pub detail: Rect,
    pub scrollbar: Option<Rect>,
    pub content_height: usize,
    pub max_scroll_offset: u16,
    pub scroll_offset: u16,
    pub category_scrollbar: Option<Rect>,
    pub category_window_start: usize,
    pub category_capacity: usize,
    pub animation_editor: Option<SettingsAnimationLayout>,
    pub number_editor: Option<SettingsNumberLayout>,
    pub picker_list: Option<Rect>,
    pub sliders: Vec<SettingsSliderLayout>,
    pub overlay_apply: Option<Rect>,
    pub overlay_cancel: Option<Rect>,
    pub category_cards: Vec<SettingsCategoryLayout>,
    pub fields: Vec<SettingsFieldLayout>,
    /// Absolute content row, including card spacing, even when scrolled out of view.
    pub selected_field_row: Option<u16>,
    pub selected_field_height: u16,
    pub picker_options: Vec<SettingsPickerOptionLayout>,
    pub picker_dialog: Option<Rect>,
    pub color_editor: Option<Rect>,
    pub weather_location_editor: Option<Rect>,
    pub file_extensions_editor: Option<Rect>,
    pub time_sync_server_editor: Option<Rect>,
    pub update_confirmation: Option<Rect>,
    pub update_confirm_button: Option<Rect>,
    pub update_cancel_button: Option<Rect>,
}

pub fn settings_layout(area: Rect, model: &SettingsViewModel) -> SettingsLayout {
    let labels = SettingsCategory::ALL.map(|category| category.label());
    let active = SettingsCategory::ALL
        .iter()
        .position(|category| *category == model.selected_category)
        .unwrap_or(0);
    let inner = inset(area, u16::from(area.width >= 96) * 2, 0);
    let system_height = inner.height.min(1);
    let navigation_height = inner
        .height
        .saturating_sub(system_height + u16::from(inner.height > 1));
    let mut workspace = SidebarWorkspaceLayout::scrollable(
        Rect::new(inner.x, inner.y, inner.width, navigation_height),
        &labels,
        active,
    );
    let (category_area, operating_system) = if workspace.vertical {
        workspace.content.height = inner.height;
        (
            workspace.navigation_area,
            Rect::new(
                inner.x,
                inner.bottom().saturating_sub(system_height),
                workspace.navigation_area.width,
                system_height,
            ),
        )
    } else {
        let mut navigation_labels = labels.to_vec();
        navigation_labels.push(i18n::tr!("system-settings-group-button"));
        workspace = SidebarWorkspaceLayout::new(inner, &navigation_labels);
        let top = workspace.content.y;
        let operating_system = workspace.navigation[labels.len()];
        (
            Rect::new(inner.x, inner.y, inner.width, top.saturating_sub(inner.y)),
            operating_system,
        )
    };
    let raw_detail_area = workspace.content;
    let mut detail_area = Rect::new(
        raw_detail_area.x,
        raw_detail_area.y,
        raw_detail_area.width,
        raw_detail_area.height.saturating_sub(1),
    );
    let applications = (model.selected_category == SettingsCategory::Applications)
        .then(|| SettingsApplicationsLayout::new(detail_area));
    if let Some(applications) = &applications {
        detail_area = applications.detail;
    }
    let mut content_height = settings_content_height(detail_area.width, model);
    let scrollbar = (content_height > usize::from(detail_area.height)
        && detail_area.width > 0
        && detail_area.height > 0)
        .then(|| {
            let track = Rect::new(
                detail_area.right().saturating_sub(1),
                detail_area.y,
                1,
                detail_area.height,
            );
            detail_area.width = detail_area.width.saturating_sub(2);
            content_height = settings_content_height(detail_area.width, model);
            track
        });
    let max_scroll_offset = if scrollbar.is_some() {
        u16::try_from(content_height.saturating_sub(usize::from(detail_area.height)))
            .unwrap_or(u16::MAX)
    } else {
        0
    };
    let scroll_offset = model.scroll_offset.min(max_scroll_offset);
    let category_start = workspace
        .navigation
        .iter()
        .position(|area| !area.is_empty())
        .unwrap_or(0);
    let category_capacity = workspace
        .navigation
        .iter()
        .take(labels.len())
        .filter(|area| !area.is_empty())
        .count();
    let category_scrollbar = (workspace.vertical && category_capacity < labels.len()).then(|| {
        Rect::new(
            category_area.right().saturating_sub(1),
            category_area.y,
            1,
            category_area.height,
        )
    });
    if category_scrollbar.is_some() {
        for row in &mut workspace.navigation {
            row.width = row.width.saturating_sub(2);
        }
    }
    let category_cards = SettingsCategory::ALL
        .into_iter()
        .zip(&workspace.navigation)
        .filter(|(_, area)| !area.is_empty())
        .map(|(category, area)| SettingsCategoryLayout {
            category,
            area: *area,
        })
        .collect();

    let mut fields = Vec::new();
    let mut sliders = Vec::new();
    let mut selected_field_row = None;
    let mut selected_field_height = 1;
    let mut y = i32::from(detail_area.y) - i32::from(scroll_offset);
    for card in &model.cards {
        let height = settings_card_height(card);
        let mut row_y = y + 1;
        for item in &card.items {
            let item_height = settings_item_height(item);
            if item.field == model.selected_field {
                selected_field_row =
                    u16::try_from(row_y - i32::from(detail_area.y) + i32::from(scroll_offset)).ok();
                selected_field_height = item_height;
            }
            if matches!(item.kind, SettingsControlKind::Slider { .. })
                && let Some(layout) = SettingsSliderLayout::new(detail_area, row_y + 1, item)
            {
                sliders.push(layout);
            }
            if row_y >= i32::from(detail_area.y) && row_y < i32::from(detail_area.bottom()) {
                fields.push(SettingsFieldLayout {
                    field: item.field,
                    area: Rect::new(detail_area.x, row_y as u16, detail_area.width, 1),
                });
            }
            row_y += i32::from(item_height);
        }
        y += i32::from(height) + 1;
    }

    let mut picker_options = Vec::new();
    let mut picker_dialog = None;
    let mut picker_list = None;
    if let Some(picker) = &model.picker {
        let dialog = settings_picker_dialog(area, picker);
        picker_dialog = Some(dialog);
        let list = picker_list_area(
            dialog,
            picker.kind == SettingsPickerKind::Timezone,
            picker.searchable,
        );
        picker_list = Some(list);
        let visible_rows = usize::from(list.height);
        let rows = picker_rows_area(list, picker.options.len());
        let start = picker.window_start.min(picker.options.len());
        let end = start.saturating_add(visible_rows).min(picker.options.len());
        picker_options.extend((start..end).map(|index| SettingsPickerOptionLayout {
            index,
            area: Rect::new(
                rows.x,
                rows.y.saturating_add((index - start) as u16),
                rows.width,
                1,
            ),
        }));
    }

    let confirmation = model
        .update
        .as_ref()
        .and_then(|update| update.confirmation.as_ref());
    let update_confirmation = confirmation.map(|confirmation| {
        let width = area.width.min(72);
        let lines = crate::management_wrapped_lines(
            &confirmation.body,
            DialogLayout::content_width(width).max(1),
        )
        .len();
        centered(
            area,
            width,
            (lines.saturating_add(8).min(usize::from(area.height))) as u16,
        )
    });
    let (update_confirm_button, update_cancel_button) = confirmation
        .zip(update_confirmation)
        .map(|(confirmation, dialog)| {
            let [confirm, cancel] = settings_dialog_buttons(
                dialog,
                &confirmation.confirm_label,
                &i18n::tr!("ui-settings-touch-cancel"),
            );
            (Some(confirm), Some(cancel))
        })
        .unwrap_or_default();

    let form_layout = model.form_dialog().map(|form| form.layout(area));
    let active_editor = form_layout
        .as_ref()
        .map(|layout| layout.form)
        .or(picker_dialog);
    let overlay_buttons = form_layout
        .as_ref()
        .map(|layout| [layout.submit, layout.cancel])
        .or_else(|| {
            active_editor.map(|dialog| {
                settings_dialog_buttons(
                    dialog,
                    &i18n::tr!("ui-settings-touch-apply"),
                    &i18n::tr!("ui-settings-touch-cancel"),
                )
            })
        });
    SettingsLayout {
        applications,
        animation_editor: model
            .animation_editor
            .as_ref()
            .map(|editor| editor.layout(area)),
        number_editor: model
            .number_editor
            .as_ref()
            .map(|editor| SettingsNumberLayout::new(area, editor)),
        workspace,
        category: category_area,
        operating_system,
        main: area,
        detail: detail_area,
        scrollbar,
        content_height,
        max_scroll_offset,
        scroll_offset,
        category_scrollbar,
        category_window_start: category_start,
        category_capacity,
        picker_list,
        sliders,
        overlay_apply: overlay_buttons.map(|buttons| buttons[0]),
        overlay_cancel: overlay_buttons.map(|buttons| buttons[1]),
        category_cards,
        fields,
        selected_field_row,
        selected_field_height,
        picker_options,
        picker_dialog,
        color_editor: model
            .color_editor
            .as_ref()
            .and(form_layout.as_ref().map(|layout| layout.form)),
        weather_location_editor: model
            .weather_location_editor
            .as_ref()
            .and(form_layout.as_ref().map(|layout| layout.form)),
        file_extensions_editor: model
            .file_extensions_editor
            .as_ref()
            .and(form_layout.as_ref().map(|layout| layout.form)),
        time_sync_server_editor: model
            .time_sync_server_editor
            .as_ref()
            .and(form_layout.as_ref().map(|layout| layout.form)),
        update_confirmation,
        update_confirm_button,
        update_cancel_button,
    }
}

pub fn settings_hit_test(layout: &SettingsLayout, point: (u16, u16)) -> Option<SettingsHitTarget> {
    if let Some(editor) = &layout.number_editor {
        if layout
            .overlay_apply
            .is_some_and(|area| contains(area, point))
        {
            return Some(SettingsHitTarget::OverlayApply);
        }
        if layout
            .overlay_cancel
            .is_some_and(|area| contains(area, point))
        {
            return Some(SettingsHitTarget::OverlayCancel);
        }
        for (area, delta) in [(editor.decrease, -1), (editor.increase, 1)] {
            if contains(area, point) {
                return Some(SettingsHitTarget::AdjustField(editor.field, delta));
            }
        }
        return contains(editor.input, point).then_some(SettingsHitTarget::NumberEditor);
    }
    if let Some(editor) = &layout.animation_editor {
        return if contains(editor.submit, point) {
            Some(SettingsHitTarget::OverlayApply)
        } else if contains(editor.cancel, point) {
            Some(SettingsHitTarget::OverlayCancel)
        } else {
            None
        };
    }

    if layout.update_confirmation.is_some() {
        if layout
            .update_confirm_button
            .is_some_and(|area| contains(area, point))
        {
            return Some(SettingsHitTarget::UpdateConfirm);
        }
        if layout
            .update_cancel_button
            .is_some_and(|area| contains(area, point))
        {
            return Some(SettingsHitTarget::UpdateCancel);
        }
        return None;
    }
    if layout
        .overlay_apply
        .is_some_and(|area| contains(area, point))
    {
        return Some(SettingsHitTarget::OverlayApply);
    }
    if layout
        .overlay_cancel
        .is_some_and(|area| contains(area, point))
    {
        return Some(SettingsHitTarget::OverlayCancel);
    }
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
    if layout.picker_dialog.is_some()
        || layout.color_editor.is_some()
        || layout.weather_location_editor.is_some()
        || layout.file_extensions_editor.is_some()
        || layout.time_sync_server_editor.is_some()
    {
        return None;
    }
    if contains(layout.operating_system, point) {
        return Some(SettingsHitTarget::OperatingSystem);
    }
    if let Some(applications) = &layout.applications
        && let Some((application, _)) = applications
            .options
            .iter()
            .find(|(_, area)| contains(*area, point))
    {
        return Some(SettingsHitTarget::Application(*application));
    }
    if let Some((slider, index)) = layout
        .sliders
        .iter()
        .find_map(|slider| slider.option_at(point).map(|index| (slider, index)))
    {
        return Some(SettingsHitTarget::SliderOption(slider.field, index));
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

pub fn render_settings_content(
    frame: &mut Frame<'_>,
    layout: &SettingsLayout,
    model: &SettingsViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    Surface::new()
        .background(context.theme.canvas)
        .render_frame(frame, layout.main, context);
    if layout.workspace.vertical {
        Surface::new().raised(true).render_frame(
            frame,
            layout
                .workspace
                .navigation_area
                .union(layout.operating_system),
            context,
        );
    }

    let buttons = SettingsCategory::ALL
        .into_iter()
        .map(|category| {
            let mut button = Button::new(
                format!("settings.category.{category:?}").to_ascii_lowercase(),
                category.label(),
            );
            button.set_focused(
                model.focus == SettingsFocus::Categories && category == model.selected_category,
            );
            button
        })
        .collect::<Vec<_>>();
    let active = SettingsCategory::ALL
        .iter()
        .position(|category| *category == model.selected_category)
        .unwrap_or(0);
    layout
        .workspace
        .render_navigation(frame, &buttons, active, context);
    let mut system_label = i18n::tr!("system-settings-group-button");
    if terminal_width(&system_label) + 2 > layout.operating_system.width as usize {
        system_label = i18n::tr!("system-settings-group-button-short");
    }
    let mut button = Button::new("settings.operating-system", system_label)
        .with_centered_label(true)
        .with_bracketed_label(false);
    button.set_focused(model.focus == SettingsFocus::OperatingSystem);
    button.render_inline_frame(frame, layout.operating_system, theme);
    if let Some(track) = layout.category_scrollbar {
        Scrollbar::new(
            SettingsCategory::ALL.len(),
            layout.category_capacity,
            layout.category_window_start,
        )
        .render_frame(frame, track, context);
    }

    if let Some(applications) = &layout.applications {
        applications.render(frame, model, context);
    }
    let detail_area = layout.detail;
    render_cards(frame, detail_area, layout.scroll_offset, model, context);
    if let Some(scrollbar) = layout.scrollbar {
        Scrollbar::new(
            layout.content_height,
            usize::from(detail_area.height),
            usize::from(layout.scroll_offset),
        )
        .render_frame(frame, scrollbar, context);
    }
    render_settings_footer(frame, settings_raw_detail_area(layout), model, context);
}

pub fn render_settings_overlay(
    frame: &mut Frame<'_>,
    layout: &SettingsLayout,
    model: &SettingsViewModel,
    context: &RenderContext,
) {
    render_settings_overlay_underlay(frame, layout, model, context);
    render_settings_overlay_top(frame, layout, model, context);
}

pub(crate) fn render_settings_overlay_underlay(
    frame: &mut Frame<'_>,
    layout: &SettingsLayout,
    model: &SettingsViewModel,
    context: &RenderContext,
) {
    if model.number_editor.is_some()
        && let (Some(editor), Some(_)) = (&model.animation_editor, &layout.animation_editor)
    {
        editor.render_frame(frame, layout.main, context);
    }
}

pub(crate) fn render_settings_overlay_top(
    frame: &mut Frame<'_>,
    layout: &SettingsLayout,
    model: &SettingsViewModel,
    context: &RenderContext,
) {
    if let Some(picker) = &model.picker {
        render_picker(frame, layout.main, picker, context);
    }
    if let Some(form) = model.form_dialog() {
        form.render_frame(frame, layout.main, context);
    }
    if let Some(confirmation) = model
        .update
        .as_ref()
        .and_then(|update| update.confirmation.as_ref())
    {
        render_update_confirmation(frame, layout, confirmation, context);
    }
    if let (Some(apply), Some(cancel)) = (layout.overlay_apply, layout.overlay_cancel)
        && model.form_dialog().is_none()
    {
        let theme = &DialogLayout::footer_theme(context);
        let mut apply_button = Button::new(
            "settings.overlay.apply",
            i18n::tr!("ui-settings-touch-apply"),
        )
        .with_emphasis(ButtonEmphasis::Primary);
        apply_button.set_focused(
            model
                .number_editor
                .as_ref()
                .is_some_and(|editor| editor.selected_index == 1),
        );
        apply_button.set_disabled(
            model
                .time_sync_server_editor
                .as_ref()
                .is_some_and(|editor| editor.validating),
        );
        apply_button.render_inline_frame(frame, apply, theme);
        let mut cancel_button = Button::new(
            "settings.overlay.cancel",
            i18n::tr!("ui-settings-touch-cancel"),
        );
        cancel_button.set_focused(
            model
                .number_editor
                .as_ref()
                .is_some_and(|editor| editor.selected_index == 2),
        );
        cancel_button.render_inline_frame(frame, cancel, theme);
    }
}

fn render_cards(
    frame: &mut Frame<'_>,
    detail_area: Rect,
    scroll_offset: u16,
    model: &SettingsViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    let mut y = i32::from(detail_area.y) - i32::from(scroll_offset);
    for (card_index, card) in model.cards.iter().enumerate() {
        let height = settings_card_height(card);
        Surface::new().render_scrolled_frame(frame, detail_area, y, height, context);
        Surface::new()
            .titled(format!("  {}", card.title))
            .raised(true)
            .render_scrolled_frame(frame, detail_area, y, 1, context);
        let rows_height = height.saturating_sub(1);
        let Some((visible_rows, skipped_rows)) = visible_scrolled_rect(
            detail_area.x,
            y + 1,
            detail_area.width,
            rows_height,
            detail_area,
        ) else {
            y += i32::from(height) + 1;
            continue;
        };
        let value_width = card
            .items
            .iter()
            .map(settings_control_width)
            .max()
            .unwrap_or(0)
            .min(visible_rows.width / 2);
        let rows = card
            .items
            .iter()
            .flat_map(|item| {
                (0..settings_item_height(item)).map(|row| {
                    vec![
                        if row == 0 {
                            item.label.clone()
                        } else {
                            String::new()
                        },
                        String::new(),
                    ]
                })
            })
            .collect::<Vec<_>>();
        let first_visible = usize::from(skipped_rows);
        let visible_end = first_visible.saturating_add(usize::from(visible_rows.height));
        let selected = card
            .items
            .iter()
            .scan(0usize, |row, item| {
                let start = *row;
                *row += usize::from(settings_item_height(item));
                Some((item, start))
            })
            .find_map(|(item, row)| {
                (model.focus == SettingsFocus::Content && item.field == model.selected_field)
                    .then_some(row)
            })
            .filter(|index| *index >= first_visible && *index < visible_end);
        let tones = card
            .items
            .iter()
            .flat_map(|item| {
                std::iter::repeat_n(
                    if item.enabled {
                        ComponentTone::Default
                    } else {
                        ComponentTone::Muted
                    },
                    usize::from(settings_item_height(item)),
                )
            })
            .collect();
        let mut table = DataTable::new(
            format!("settings.card.{:?}.{card_index}", model.selected_category),
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
        table.row_ids = card
            .items
            .iter()
            .flat_map(|item| {
                (0..settings_item_height(item)).map(|row| {
                    if row == 0 {
                        format!("{:?}", item.field)
                    } else {
                        format!("{:?}.{row}", item.field)
                    }
                })
            })
            .collect();
        table.selected = selected;
        table.state.focused = selected.is_some();
        table.render_frame(frame, visible_rows, context);

        let mut next_row_y = y + 1;
        for item in &card.items {
            let row_y = next_row_y;
            next_row_y += i32::from(settings_item_height(item));
            if matches!(item.kind, SettingsControlKind::Slider { .. })
                && let Some(slider) = SettingsSliderLayout::new(detail_area, row_y + 1, item)
            {
                slider.render(
                    frame,
                    item,
                    model.focus == SettingsFocus::Content && item.field == model.selected_field,
                    theme,
                );
            }
            if row_y < i32::from(detail_area.y) || row_y >= i32::from(detail_area.bottom()) {
                continue;
            }
            let row = Rect::new(visible_rows.x, row_y as u16, visible_rows.width, 1);
            render_settings_control(
                frame,
                Rect::new(
                    row.right().saturating_sub(value_width),
                    row.y,
                    value_width,
                    row.height,
                ),
                item,
                model.focus == SettingsFocus::Content && item.field == model.selected_field,
                context,
            );
        }
        y += i32::from(height) + 1;
    }

    if model.selected_category == SettingsCategory::Update
        && let Some(update) = &model.update
    {
        if let Some(activity) = &update.activity {
            let activity = UpdateActivity::new(activity);
            activity.render_scrolled(frame, detail_area, y, context);
            y += i32::from(activity.height()) + 1;
        }
        render_update_commits(frame, detail_area, y, update, context);
    }
}

fn settings_content_height(detail_width: u16, model: &SettingsViewModel) -> usize {
    let mut next_y = 0usize;
    let mut content_height = 0usize;

    for card in &model.cards {
        let height = settings_card_height(card);
        content_height = content_height.max(next_y.saturating_add(usize::from(height)));
        next_y = next_y.saturating_add(usize::from(height)).saturating_add(1);
    }

    if model.selected_category == SettingsCategory::Update
        && let Some(update) = &model.update
    {
        if let Some(activity) = &update.activity {
            next_y += usize::from(UpdateActivity::new(activity).height()) + 1;
        }
        let lines = update_commit_lines(update);
        content_height = content_height
            .max(next_y.saturating_add(usize::from(update_commits_height(detail_width, &lines))));
    }
    content_height
}

fn update_commit_lines(update: &SettingsUpdateViewModel) -> Vec<String> {
    if update.commits.is_empty() {
        return vec![update.empty_message.clone()];
    }

    update
        .commits
        .iter()
        .map(|commit| {
            format!(
                "{}  {}",
                commit.sha.chars().take(8).collect::<String>(),
                commit
                    .message
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        })
        .collect()
}

fn update_commits_height(detail_width: u16, lines: &[String]) -> u16 {
    let width = detail_width.max(1);
    let content_height = lines
        .iter()
        .map(|line| crate::management_wrapped_lines(line, width).len())
        .sum::<usize>();
    u16::try_from(content_height)
        .unwrap_or(u16::MAX)
        .saturating_add(1)
        .max(2)
}

fn render_update_commits(
    frame: &mut Frame<'_>,
    detail_area: Rect,
    y: i32,
    update: &SettingsUpdateViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    let lines = update_commit_lines(update);
    let height = update_commits_height(detail_area.width, &lines);
    Surface::new()
        .titled(
            update
                .summary_title
                .clone()
                .unwrap_or_else(|| i18n::tr!("ui-settings-commits-padded")),
        )
        .render_scrolled_frame(frame, detail_area, y, height, context);
    if let Some((content, skipped)) = visible_scrolled_rect(
        detail_area.x,
        y + 1,
        detail_area.width,
        height.saturating_sub(1),
        detail_area,
    ) {
        frame.render_widget(
            Paragraph::new(
                lines
                    .iter()
                    .flat_map(|line| crate::management_wrapped_lines(line, detail_area.width))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .style(theme.body_style())
            .scroll((skipped, 0)),
            content,
        );
    }
}

fn render_update_confirmation(
    frame: &mut Frame<'_>,
    layout: &SettingsLayout,
    confirmation: &SettingsUpdateConfirmationViewModel,
    context: &RenderContext,
) {
    let Some(dialog) = layout.update_confirmation else {
        return;
    };
    let theme = &DialogLayout::footer_theme(context);
    let chrome = DialogLayout::with_min_body(dialog, 1, None, 3);
    chrome.render_frame(frame, &confirmation.title, context);
    frame.render_widget(
        Paragraph::new(confirmation.body.clone()).wrap(Wrap { trim: true }),
        chrome.body,
    );
    if let Some(area) = layout.update_confirm_button {
        let mut button = Button::new("settings.update.confirm", &confirmation.confirm_label)
            .with_bracketed_label(false)
            .with_centered_label(true)
            .with_emphasis(ButtonEmphasis::Primary);
        button.state.selected = confirmation.confirm_selected;
        button.render_inline_frame(frame, area, theme);
    }
    if let Some(area) = layout.update_cancel_button {
        let mut button = Button::new(
            "settings.update.cancel",
            i18n::tr!("ui-settings-touch-cancel"),
        )
        .with_bracketed_label(false)
        .with_centered_label(true);
        button.state.selected = !confirmation.confirm_selected;
        button.render_inline_frame(frame, area, theme);
    }
}

fn render_settings_footer(
    frame: &mut Frame<'_>,
    detail: Rect,
    model: &SettingsViewModel,
    context: &RenderContext,
) {
    if detail.height == 0 {
        return;
    }
    let selected = model
        .cards
        .iter()
        .flat_map(|card| &card.items)
        .find(|item| item.field == model.selected_field);
    let lock = model.locked_message.as_deref().unwrap_or("");
    let status = selected
        .and_then(|item| item.unavailable_reason.as_deref())
        .unwrap_or(&model.status);
    let text = [status, lock]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("  |  ");
    let area = Rect::new(detail.x, detail.bottom().saturating_sub(1), detail.width, 1);
    if !text.is_empty() {
        crate::components::PageActionBar::new("settings")
            .with_status(vec![crate::components::PageActionStatus {
                text,
                tone: if selected.is_some_and(|item| item.unavailable_reason.is_some()) {
                    crate::components::ComponentTone::Warning
                } else {
                    crate::components::ComponentTone::Muted
                },
            }])
            .render(frame, area, context);
    }
}

fn render_picker(
    frame: &mut Frame<'_>,
    area: Rect,
    picker: &SettingsPickerViewModel,
    context: &RenderContext,
) {
    let theme = &DialogLayout::body_theme(context);
    let dialog = settings_picker_dialog(area, picker);
    let chrome = picker_chrome(dialog, picker.searchable);
    chrome.render_frame(frame, &picker.title, context);
    let query_area = Rect::new(
        chrome.body.x,
        chrome.body.y,
        chrome.body.width,
        chrome.body.height.min(1),
    );
    if picker.searchable {
        picker
            .editing
            .input("settings.picker-search", &picker.query)
            .with_cursor_symbol("_")
            .render_borderless_frame_with_prefix(
                frame,
                query_area,
                theme,
                &i18n::tr!("ui-settings-search-padded"),
            );
    }
    let list = picker_list_area(
        dialog,
        picker.kind == SettingsPickerKind::Timezone,
        picker.searchable,
    );
    let rows = picker_rows_area(list, picker.options.len());
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
                    usize::from(rows.width.saturating_sub(2)),
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
    let mut component_list = ComponentList::new("settings.picker", items)
        .with_background(context.theme.surface)
        .with_viewport_start(start);
    component_list.set_focused(true);
    component_list.set_selected(Some(picker.selected_index));
    component_list.render_borderless_frame(frame, rows, theme);
    if picker.options.len() > visible && list.width > 0 && list.height > 0 {
        Scrollbar::new(picker.options.len(), visible, start).render_frame(
            frame,
            Rect::new(list.right() - 1, list.y, 1, list.height),
            context,
        );
    }

    if picker.kind == SettingsPickerKind::Timezone && dialog.width >= 70 {
        let map = Rect::new(
            list.right() + 2,
            list.y,
            chrome.body.right().saturating_sub(list.right() + 2),
            list.height,
        );
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

pub fn settings_text_input_area(dialog: Rect) -> Rect {
    Rect::new(
        dialog.x.saturating_add(1),
        dialog.y.saturating_add(2),
        dialog.width.saturating_sub(2),
        1,
    )
    .intersection(dialog)
}

fn settings_raw_detail_area(layout: &SettingsLayout) -> Rect {
    layout.workspace.content
}

fn render_settings_control(
    frame: &mut Frame<'_>,
    area: Rect,
    item: &SettingsItemViewModel,
    selected: bool,
    context: &RenderContext,
) {
    if area.width == 0
        || area.height == 0
        || matches!(item.kind, SettingsControlKind::Slider { .. })
    {
        return;
    }

    let theme = &context.compatibility_theme();
    if item.kind == SettingsControlKind::ReadOnly {
        let style = Style::default().fg(if !item.enabled {
            theme.disabled_style().fg.unwrap_or(context.theme.muted)
        } else if selected {
            context.theme.text
        } else {
            context.theme.muted
        });
        frame.render_widget(
            Paragraph::new(item.value.clone())
                .alignment(HorizontalAlignment::Right)
                .style(style),
            area,
        );
        return;
    }
    let value = match item.kind {
        SettingsControlKind::Cycle | SettingsControlKind::Picker | SettingsControlKind::Palette
            if selected && item.enabled =>
        {
            format!("{} ↵", item.value)
        }
        _ => item.value.clone(),
    };
    let mut button = Button::new(
        format!("settings.field.{:?}", item.field).to_ascii_lowercase(),
        value,
    )
    .with_bracketed_label(false)
    .with_capture_name(&item.label);
    if item.kind == SettingsControlKind::Toggle {
        button = button
            .with_capture_role("checkbox")
            .with_capture_selected(item.checked.unwrap_or(false));
    }
    button.state.selected = selected;
    button.set_focused(selected);
    button.set_disabled(!item.enabled);
    button.render_inline_frame(frame, area, theme);
}

fn settings_control_width(item: &SettingsItemViewModel) -> u16 {
    let label_width = u16::try_from(terminal_width(&item.value)).unwrap_or(u16::MAX);
    let component_padding = match item.kind {
        SettingsControlKind::Toggle => 2,
        SettingsControlKind::Cycle
        | SettingsControlKind::Picker
        | SettingsControlKind::Palette
        | SettingsControlKind::Action => 2,
        SettingsControlKind::Stepper => 2,
        SettingsControlKind::ReadOnly => 0,
        SettingsControlKind::Slider { .. } => return 0,
    };
    label_width.saturating_add(component_padding)
}

fn settings_item_height(item: &SettingsItemViewModel) -> u16 {
    if matches!(item.kind, SettingsControlKind::Slider { .. }) {
        3
    } else {
        1
    }
}

fn settings_card_height(card: &SettingsCardViewModel) -> u16 {
    card.items
        .iter()
        .map(settings_item_height)
        .sum::<u16>()
        .saturating_add(1)
        .max(1)
}

fn picker_list_area(dialog: Rect, timezone: bool, searchable: bool) -> Rect {
    let mut content = picker_chrome(dialog, searchable).body;
    let search_rows = if searchable { content.height.min(2) } else { 0 };
    content.y += search_rows;
    content.height -= search_rows;
    if timezone && dialog.width >= 70 {
        content.width /= 2;
    }
    content
}

fn picker_chrome(dialog: Rect, searchable: bool) -> DialogLayout {
    DialogLayout::with_min_body(dialog, 1, None, 1 + u16::from(searchable) * 2)
}

fn picker_rows_area(list: Rect, option_count: usize) -> Rect {
    Rect::new(
        list.x,
        list.y,
        list.width
            .saturating_sub(u16::from(option_count > usize::from(list.height)) * 2),
        list.height,
    )
}

fn settings_picker_dialog(area: Rect, picker: &SettingsPickerViewModel) -> Rect {
    // A timezone map needs its canvas; other pickers grow with their visible rows.
    if picker.kind == SettingsPickerKind::Timezone {
        return centered(area, area.width.min(78), area.height.min(24));
    }
    let content = picker
        .options
        .iter()
        .map(|option| terminal_width(&format!("{}  {}", option.label, option.detail)) + 2)
        .max()
        .unwrap_or(0);
    let width = content
        .max(terminal_width(&picker.title))
        .max(
            terminal_width(&i18n::tr!("ui-settings-touch-apply"))
                + terminal_width(&i18n::tr!("ui-settings-touch-cancel"))
                + 4,
        )
        .saturating_add(6)
        .clamp(32, 78)
        .min(usize::from(area.width)) as u16;
    let height = picker
        .options
        .len()
        .max(1)
        .saturating_add(8 + usize::from(picker.searchable) * 2)
        .min(24)
        .min(usize::from(area.height)) as u16;
    centered(area, width, height)
}

fn settings_dialog_buttons(dialog: Rect, apply: &str, cancel: &str) -> [Rect; 2] {
    let footer = DialogLayout::new(dialog, 1, None).footer;
    let apply_width =
        ((terminal_width(apply) as u16).saturating_add(2)).min(footer.width.div_ceil(2));
    let cancel_width = ((terminal_width(cancel) as u16).saturating_add(2))
        .min(footer.width.saturating_sub(apply_width + 1));
    let apply = Rect::new(
        footer.right() - apply_width,
        footer.y,
        apply_width,
        footer.height,
    );
    let cancel = Rect::new(
        apply.x.saturating_sub(cancel_width + 1).max(footer.x),
        footer.y,
        cancel_width,
        footer.height,
    );
    [apply, cancel]
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
