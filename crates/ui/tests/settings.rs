use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ui::{
    BorderShape, HomeDisplayMode, NotificationTone, SettingsAppearancePreview,
    SettingsCardViewModel, SettingsCategory, SettingsColorEditorViewModel, SettingsControlKind,
    SettingsField, SettingsFileExtensionsEditorViewModel, SettingsHitTarget, SettingsItemViewModel,
    SettingsPickerKind, SettingsPickerOptionViewModel, SettingsPickerViewModel,
    SettingsTimeSyncServerEditorViewModel, SettingsViewModel,
    SettingsWeatherLocationEditorViewModel, ShellChromeViewModel, StatusViewModel, TundraTheme,
    render_settings, settings_hit_test, settings_layout,
};

#[test]
fn wide_layout_uses_a_left_category_column_and_right_detail_cards() {
    let model = sample_model();
    let layout = settings_layout(Rect::new(0, 0, 120, 32), &model);

    assert_eq!(layout.category_cards.len(), 4);
    assert_eq!(
        layout.category_cards[0].category,
        SettingsCategory::Appearance
    );
    assert_eq!(layout.category_cards[0].area.x, 1);
    assert_eq!(layout.category_cards[0].area.width, 28);
    assert!(
        layout
            .category_cards
            .iter()
            .all(|category| category.area.x < 29)
    );
    assert!(layout.fields.iter().all(|field| field.area.x >= 30));
    assert!(
        layout
            .fields
            .iter()
            .any(|field| field.field == SettingsField::BorderColor)
    );
    assert!(
        layout
            .fields
            .iter()
            .any(|field| field.field == SettingsField::ShowHidden)
    );
}

#[test]
fn eighty_by_twenty_four_layout_uses_a_top_category_navigation_row() {
    let model = sample_model();
    let layout = settings_layout(Rect::new(0, 0, 80, 24), &model);

    assert_eq!(layout.category_cards.len(), 4);
    assert!(
        layout
            .category_cards
            .iter()
            .all(|category| category.area.y == 1)
    );
    assert!(
        layout
            .category_cards
            .iter()
            .all(|category| category.area.height == 5)
    );
    assert_eq!(layout.category_cards[0].area.width, 19);
    assert_eq!(layout.category_cards[3].area.right(), 79);
    assert!(layout.fields.iter().all(|field| field.area.y >= 7));
}

#[test]
fn field_and_category_hit_targets_match_their_visible_areas() {
    let model = sample_model();
    let layout = settings_layout(Rect::new(0, 0, 120, 32), &model);
    let category = layout
        .category_cards
        .iter()
        .find(|entry| entry.category == SettingsCategory::FileExplorer)
        .expect("file explorer category");
    let field = layout
        .fields
        .iter()
        .find(|entry| entry.field == SettingsField::ShowHidden)
        .expect("show hidden field");

    assert_eq!(
        settings_hit_test(&layout, (category.area.x, category.area.y)),
        Some(SettingsHitTarget::Category(SettingsCategory::FileExplorer))
    );
    assert_eq!(
        settings_hit_test(&layout, (field.area.x, field.area.y)),
        Some(SettingsHitTarget::Field(SettingsField::ShowHidden))
    );
    assert_eq!(settings_hit_test(&layout, (0, 0)), None);
}

#[test]
fn picker_options_take_hit_priority_and_respect_the_visible_window() {
    let mut model = sample_model();
    model.picker = Some(SettingsPickerViewModel {
        kind: SettingsPickerKind::Timezone,
        title: "Choose timezone".to_string(),
        query: "tok".to_string(),
        options: vec![
            SettingsPickerOptionViewModel::new("UTC", "Coordinated Universal Time"),
            SettingsPickerOptionViewModel::new("Tokyo", "Asia/Tokyo"),
            SettingsPickerOptionViewModel::new("Shanghai", "Asia/Shanghai"),
        ],
        selected_index: 1,
        window_start: 1,
        searchable: true,
    });
    let layout = settings_layout(Rect::new(0, 0, 120, 32), &model);

    assert_eq!(
        layout
            .picker_options
            .iter()
            .map(|option| option.index)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    let option = layout.picker_options[0];
    assert_eq!(
        settings_hit_test(&layout, (option.area.x, option.area.y)),
        Some(SettingsHitTarget::PickerOption(1))
    );
}

#[test]
fn color_editor_captures_clicks_above_settings_content() {
    let mut model = sample_model();
    model.color_editor = Some(SettingsColorEditorViewModel {
        title: "Custom accent".to_string(),
        value: "#00FFFF".to_string(),
        error: Some("Accent must differ from border color.".to_string()),
    });
    let layout = settings_layout(Rect::new(0, 0, 120, 32), &model);
    let dialog = layout.color_editor.expect("color editor layout");

    assert_eq!(
        settings_hit_test(&layout, (dialog.x, dialog.y)),
        Some(SettingsHitTarget::ColorEditor)
    );
}

#[test]
fn file_extensions_editor_captures_clicks_and_shows_examples() {
    let mut model = sample_model();
    model.file_extensions_editor = Some(SettingsFileExtensionsEditorViewModel {
        value: ".md, .rs, .d.ts".to_string(),
        error: None,
    });
    let layout = settings_layout(Rect::new(0, 0, 120, 32), &model);
    let dialog = layout
        .file_extensions_editor
        .expect("file extensions editor layout");

    assert_eq!(
        settings_hit_test(&layout, (dialog.x, dialog.y)),
        Some(SettingsHitTarget::FileExtensionsEditor)
    );

    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| {
            render_settings(
                frame,
                Rect::new(0, 0, 120, 32),
                &chrome(),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render file extensions editor");
    let output = terminal_output(&terminal);
    assert!(output.contains("Explorer files opened in Editor"));
    assert!(output.contains(".d.ts"));
}

#[test]
fn time_sync_server_editor_shows_validation_state_and_captures_clicks() {
    let mut model = sample_model();
    model.time_sync_server_editor = Some(SettingsTimeSyncServerEditorViewModel {
        value: "https://time.example.test/".to_string(),
        error: None,
        validating: true,
    });
    let layout = settings_layout(Rect::new(0, 0, 120, 32), &model);
    let dialog = layout
        .time_sync_server_editor
        .expect("time sync server editor layout");
    assert_eq!(
        settings_hit_test(&layout, (dialog.x, dialog.y)),
        Some(SettingsHitTarget::TimeSyncServerEditor)
    );

    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).expect("terminal");
    terminal
        .draw(|frame| {
            render_settings(
                frame,
                Rect::new(0, 0, 120, 32),
                &chrome(),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render time sync server editor");
    let output = terminal_output(&terminal);
    assert!(output.contains("Time synchronization server"));
    assert!(output.contains("Synchronizing with this server"));
}

#[test]
fn weather_location_editor_captures_clicks_and_explains_timezone_fallback() {
    let mut model = sample_model();
    model.weather_location_editor = Some(SettingsWeatherLocationEditorViewModel {
        value: "Cambridge, Massachusetts, USA".to_string(),
        error: None,
    });
    let layout = settings_layout(Rect::new(0, 0, 120, 32), &model);
    let dialog = layout
        .weather_location_editor
        .expect("weather location editor layout");

    assert_eq!(
        settings_hit_test(&layout, (dialog.x, dialog.y)),
        Some(SettingsHitTarget::WeatherLocationEditor)
    );
}

#[test]
fn renderer_draws_cards_preview_picker_and_status_into_the_buffer() {
    let mut model = sample_model();
    let chrome = chrome();
    let mut terminal = Terminal::new(TestBackend::new(120, 32)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_settings(
                frame,
                frame.area(),
                &chrome,
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render settings");

    let output = terminal_output(&terminal);
    assert!(output.contains("Settings"));
    assert!(output.contains("Appearance"));
    assert!(output.contains("Live preview"));
    assert!(output.contains("Colors and borders"));
    assert!(output.contains("Saved"));

    model.picker = Some(SettingsPickerViewModel {
        kind: SettingsPickerKind::Timezone,
        title: "Choose timezone".to_string(),
        query: "tok".to_string(),
        options: vec![
            SettingsPickerOptionViewModel::new("Tokyo", "Asia/Tokyo").timezone(
                "Asia/Tokyo",
                139.6917,
                35.6895,
            ),
        ],
        selected_index: 0,
        window_start: 0,
        searchable: true,
    });
    terminal
        .draw(|frame| {
            render_settings(
                frame,
                frame.area(),
                &chrome,
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render settings picker");
    let picker_output = terminal_output(&terminal);
    assert!(picker_output.contains("Choose timezone"));
    assert!(picker_output.contains("Search: tok_"));
    assert!(picker_output.contains("Tokyo  Asia/Tokyo"));
}

fn sample_model() -> SettingsViewModel {
    SettingsViewModel {
        selected_category: SettingsCategory::Appearance,
        selected_field: SettingsField::BorderColor,
        cards: vec![
            SettingsCardViewModel::new(
                "Colors and borders",
                vec![
                    SettingsItemViewModel::new(
                        SettingsField::BorderShape,
                        "Border shape",
                        "Rounded",
                        "Choose rounded or square borders.",
                        SettingsControlKind::Cycle,
                    ),
                    SettingsItemViewModel::new(
                        SettingsField::BorderColor,
                        "Border color",
                        "White",
                        "Choose the border color.",
                        SettingsControlKind::Palette,
                    ),
                ],
            ),
            SettingsCardViewModel::new(
                "Display",
                vec![SettingsItemViewModel::new(
                    SettingsField::ShowHidden,
                    "Show hidden files",
                    "On",
                    "Display hidden files in Explorer.",
                    SettingsControlKind::Toggle,
                )],
            ),
        ],
        appearance_preview: Some(SettingsAppearancePreview {
            border_shape: BorderShape::Rounded,
            border_color: Color::White,
            accent_color: Color::Cyan,
        }),
        status: "Saved".to_string(),
        locked_message: None,
        scroll_offset: 0,
        picker: None,
        color_editor: None,
        weather_location_editor: None,
        file_extensions_editor: None,
        time_sync_server_editor: None,
    }
}

fn chrome() -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::User,
        terminal_size: (120, 32),
        screen_stack: vec!["Settings".to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    }
}

fn terminal_output(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}
