mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use support::terminal_output;
use ui::{
    BorderShape, HomeDisplayMode, NotificationTone, SettingsAppearancePreview,
    SettingsCardViewModel, SettingsCategory, SettingsColorEditorViewModel, SettingsControlKind,
    SettingsField, SettingsFileExtensionsEditorViewModel, SettingsHitTarget, SettingsItemViewModel,
    SettingsPickerKind, SettingsPickerOptionViewModel, SettingsPickerViewModel,
    SettingsTimeSyncServerEditorViewModel, SettingsUpdateCommitViewModel,
    SettingsUpdateConfirmationViewModel, SettingsUpdateViewModel, SettingsViewModel,
    SettingsWeatherLocationEditorViewModel, ShellChromeViewModel, StatusViewModel, TundraTheme,
    render_settings, settings_hit_test, settings_layout,
};

#[test]
fn full_layout_keeps_categories_and_fields_visible_at_supported_sizes() {
    let model = sample_model();
    for (width, height) in [(80, 24), (120, 32)] {
        let layout = settings_layout(Rect::new(0, 0, width, height), &model);
        assert_eq!(layout.category_cards.len(), SettingsCategory::ALL.len());
        for (index, category) in layout.category_cards.iter().enumerate() {
            assert_eq!(category.category, SettingsCategory::ALL[index]);
            assert_eq!(category.area, Rect::new(2, 2 + index as u16, 16, 1));
        }
        assert!(layout.fields.iter().all(|field| field.area.x == 22));
        for expected in [SettingsField::BorderColor, SettingsField::ShowHidden] {
            assert!(layout.fields.iter().any(|field| field.field == expected));
        }
    }
}

#[test]
fn settings_scrollbar_only_appears_when_content_overflows() {
    let mut model = sample_model();
    model.scroll_offset = u16::MAX;

    let fitting = settings_layout(Rect::new(0, 0, 120, 32), &model);
    assert_eq!(fitting.max_scroll_offset, 0);
    assert_eq!(fitting.scroll_offset, 0);
    assert!(fitting.scrollbar.is_none());

    let no_viewport = settings_layout(Rect::new(0, 0, 80, 1), &model);
    assert_eq!(no_viewport.max_scroll_offset, 0);
    assert_eq!(no_viewport.scroll_offset, 0);
    assert!(no_viewport.scrollbar.is_none());

    let overflowing = settings_layout(Rect::new(0, 0, 80, 12), &model);
    let track = overflowing.scrollbar.expect("settings scrollbar");
    assert!(overflowing.max_scroll_offset > 0);
    assert_eq!(overflowing.scroll_offset, overflowing.max_scroll_offset);
    assert_eq!(track.x, overflowing.detail.right());
    assert_eq!(track.y, overflowing.detail.y);
    assert_eq!(track.height, overflowing.detail.height);

    let mut terminal = Terminal::new(TestBackend::new(80, 12)).expect("test terminal");
    let mut rendered_layout = None;
    terminal
        .draw(|frame| {
            rendered_layout = Some(render_settings(
                frame,
                frame.area(),
                &chrome(),
                &model,
                &TundraTheme::default_dark(),
            ));
        })
        .expect("render overflowing settings");
    let rendered_track = rendered_layout
        .expect("rendered settings layout")
        .scrollbar
        .expect("rendered settings scrollbar");
    let symbols = (rendered_track.y..rendered_track.bottom())
        .map(|y| {
            terminal
                .backend()
                .buffer()
                .cell((rendered_track.x, y))
                .expect("scrollbar cell")
                .symbol()
        })
        .collect::<Vec<_>>();
    assert!(symbols.iter().all(|symbol| matches!(*symbol, "│" | "█")));
    assert!(symbols.contains(&"█"));
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

    let mut terminal = Terminal::new(TestBackend::new(120, 32)).expect("test terminal");
    terminal
        .draw(|frame| {
            render_settings(
                frame,
                frame.area(),
                &chrome(),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render color editor");
    let output = terminal_output(&terminal);
    assert!(output.contains("> #00FFFF_"));
    assert!(output.contains("Accent must differ from border color."));
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
    assert!(output.contains("> .md, .rs, .d.ts_"));
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
    assert!(output.contains("> https://time.example.test/_"));
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

    let mut terminal = Terminal::new(TestBackend::new(120, 32)).expect("test terminal");
    terminal
        .draw(|frame| {
            render_settings(
                frame,
                frame.area(),
                &chrome(),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render weather location editor");
    let output = terminal_output(&terminal);
    assert!(output.contains("> Cambridge, Massachusetts, USA_"));
    assert!(output.contains("Leave empty to use the timezone location."));
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
    assert!(output.contains("Rounded"));
    assert!(output.contains("White"));
    assert!(output.contains("On"));
    assert!(!output.contains("< Rounded >"));
    assert!(output.contains("[Rounded]"));
    assert!(output.contains("[White]"));
    assert!(output.contains("[On]"));

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
    assert!(picker_output.contains("> Tokyo  Asia/Tokyo"));
}

#[test]
fn unavailable_default_theme_image_option_is_rendered_dimmed() {
    let mut model = sample_model();
    model.picker = Some(SettingsPickerViewModel {
        kind: SettingsPickerKind::DefaultThemeIcons,
        title: "Default theme".to_string(),
        query: String::new(),
        options: vec![
            SettingsPickerOptionViewModel::new("ASCII icons", "Always available"),
            SettingsPickerOptionViewModel::new("Image icons", "Unsupported by this terminal")
                .enabled(false),
        ],
        selected_index: 1,
        window_start: 0,
        searchable: false,
    });
    let mut terminal = Terminal::new(TestBackend::new(120, 32)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_settings(
                frame,
                frame.area(),
                &chrome(),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render disabled image icon option");

    let buffer = terminal.backend().buffer();
    let mut found = false;
    for y in 0..buffer.area.height {
        let row = (0..buffer.area.width)
            .map(|x| buffer.cell((x, y)).map(|cell| cell.symbol()).unwrap_or(" "))
            .collect::<String>();
        let Some(start) = row.find("Image icons") else {
            continue;
        };
        found = true;
        for x in start..start + "Image icons".len() {
            assert!(
                buffer
                    .cell((u16::try_from(x).unwrap(), y))
                    .is_some_and(|cell| cell.modifier.contains(Modifier::DIM))
            );
        }
    }
    assert!(found, "disabled Image icons option should be visible");
}

#[test]
fn picker_truncates_wide_labels_by_terminal_columns() {
    let mut model = sample_model();
    model.picker = Some(SettingsPickerViewModel {
        kind: SettingsPickerKind::Language,
        title: "Choose language".to_string(),
        query: String::new(),
        options: vec![SettingsPickerOptionViewModel::new("界".repeat(50), "")],
        selected_index: 0,
        window_start: 0,
        searchable: false,
    });
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_settings(
                frame,
                frame.area(),
                &chrome(),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render wide picker label");

    let buffer = terminal.backend().buffer();
    let wide_cells = buffer
        .content()
        .iter()
        .filter(|cell| cell.symbol() == "界")
        .count();
    let ellipsis_cells = buffer
        .content()
        .iter()
        .filter(|cell| cell.symbol() == "…")
        .count();
    assert_eq!(wide_cells, 35);
    assert_eq!(ellipsis_cells, 1);

    let row = (0..buffer.area.height)
        .find(|y| {
            (0..buffer.area.width).any(|x| {
                buffer
                    .cell((x, *y))
                    .is_some_and(|cell| cell.symbol() == "界")
            })
        })
        .expect("picker row containing the wide label");
    let columns = (0..buffer.area.width)
        .filter(|x| {
            buffer
                .cell((*x, row))
                .is_some_and(|cell| cell.symbol() == "界")
        })
        .collect::<Vec<_>>();
    assert!(
        columns
            .windows(2)
            .all(|columns| columns[1] - columns[0] == 2)
    );
    let ellipsis_column = (0..buffer.area.width)
        .find(|x| {
            buffer
                .cell((*x, row))
                .is_some_and(|cell| cell.symbol() == "…")
        })
        .expect("terminal-width truncation ellipsis");
    assert_eq!(ellipsis_column, columns.last().copied().unwrap() + 2);
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
        update: None,
    }
}

#[test]
fn update_commits_wrap_complete_messages_and_follow_detail_scroll() {
    let mut model = sample_model();
    model.selected_category = SettingsCategory::Update;
    model.appearance_preview = None;
    model.cards.clear();
    model.update = Some(SettingsUpdateViewModel {
        activity: None,
        commits: vec![SettingsUpdateCommitViewModel {
            sha: "1234567890abcdef".to_string(),
            message: format!(
                "First line\nsecond line keeps all of the commit message visible {}",
                "with additional details ".repeat(80)
            ),
        }],
        empty_message: "No new commits".to_string(),
        confirmation: None,
    });
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    terminal
        .draw(|frame| {
            render_settings(
                frame,
                frame.area(),
                &chrome(),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render update commits");
    let output = terminal_output(&terminal);
    assert!(output.contains("12345678"));
    for part in ["First line", "second line", "commit message visible"] {
        assert!(output.contains(part), "missing wrapped commit text: {part}");
    }

    let first_row = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .position(|cell| cell.symbol() == "1")
        .expect("commit row")
        / 80;
    model.scroll_offset = 1;
    terminal
        .draw(|frame| {
            render_settings(
                frame,
                frame.area(),
                &chrome(),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render scrolled commits");
    let second_row = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .position(|cell| cell.symbol() == "1")
        .expect("scrolled commit row")
        / 80;
    assert!(second_row < first_row);
}

#[test]
fn update_confirmation_draws_complete_buttons_and_blocks_underlying_hits() {
    for confirm_label in ["Update and restart", "Replace and restart"] {
        for (width, height) in [(50, 12), (80, 24), (120, 32)] {
            let mut model = sample_model();
            model.update = Some(SettingsUpdateViewModel {
                activity: None,
                commits: Vec::new(),
                empty_message: "Up to date".to_string(),
                confirmation: Some(SettingsUpdateConfirmationViewModel {
                    title: "Install update?".to_string(),
                    body: "The source will be downloaded and compiled.\nThe app will restart immediately.".to_string(),
                    confirm_label: confirm_label.to_string(),
                    confirm_selected: true,
                }),
            });
            let ui::ShellLayout::Full { main, .. } =
                ui::compute_shell_layout(Rect::new(0, 0, width, height))
            else {
                panic!("supported terminal size")
            };
            let layout = settings_layout(main, &model);
            let dialog = layout.update_confirmation.expect("dialog");
            let confirm = layout.update_confirm_button.expect("confirm button");
            let cancel = layout.update_cancel_button.expect("cancel button");
            assert!(confirm.intersection(cancel).is_empty());
            assert_eq!(confirm.intersection(dialog), confirm);
            assert_eq!(cancel.intersection(dialog), cancel);
            for (button, target) in [
                (confirm, SettingsHitTarget::UpdateConfirm),
                (cancel, SettingsHitTarget::UpdateCancel),
            ] {
                for x in button.x..button.right() {
                    assert_eq!(settings_hit_test(&layout, (x, button.y)), Some(target));
                }
            }
            assert_eq!(
                settings_hit_test(&layout, (confirm.right(), confirm.y)),
                None
            );
            assert_eq!(settings_hit_test(&layout, (0, 0)), None);

            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    render_settings(
                        frame,
                        frame.area(),
                        &chrome(),
                        &model,
                        &TundraTheme::default_dark(),
                    );
                })
                .unwrap();
            for (button, label) in [
                (confirm, format!("[{confirm_label}]")),
                (cancel, "[Cancel]".into()),
            ] {
                let rendered: String = (button.x..button.right())
                    .map(|x| terminal.backend().buffer()[(x, button.y)].symbol())
                    .collect();
                assert_eq!(rendered.trim(), label, "button clipped at {width}x{height}");
            }
            let output = terminal_output(&terminal);
            assert!(output.contains("Install update?"));
            if height >= 24 {
                assert!(output.contains("The source will be downloaded and compiled."));
                assert!(output.contains("The app will restart immediately."));
            }
        }
    }
}

#[test]
fn update_activity_shows_both_meters_and_latest_output_with_page_scrolling() {
    use ui::components::{UpdateActivityViewModel, UpdateMeterViewModel};
    let mut model = sample_model();
    model.selected_category = SettingsCategory::Update;
    model.appearance_preview = None;
    model.cards.clear();
    model.status = "Compiling release executables".into();
    model.update = Some(SettingsUpdateViewModel {
        activity: Some(UpdateActivityViewModel {
            download: UpdateMeterViewModel {
                percent: Some(100),
                display_basis_points: None,
                label: "Download: 100%".into(),
            },
            compilation: UpdateMeterViewModel {
                percent: Some(42),
                display_basis_points: Some(1200),
                label: "Compilation: 42%".into(),
            },
            output: (0..200).map(|n| format!("Compiling crate-{n}")).collect(),
        }),
        commits: Vec::new(),
        empty_message: "No new commits".into(),
        confirmation: None,
    });
    for theme in [
        TundraTheme::default_dark(),
        TundraTheme {
            accent_color: Color::Yellow,
            ..TundraTheme::default_dark()
        },
    ] {
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| {
                render_settings(frame, frame.area(), &chrome(), &model, &theme);
            })
            .unwrap();
        let output = terminal_output(&terminal);
        for text in [
            "Download: 100%",
            "Compilation: 42%",
            "Live output",
            "Compiling crate-199",
        ] {
            assert!(output.contains(text), "missing {text}");
        }
        assert!(!output.contains("Compiling crate-0"));
        let main = match ui::compute_shell_layout(Rect::new(0, 0, 108, 20)) {
            ui::ShellLayout::Full { main, .. } => main,
            ui::ShellLayout::Compact(main) => main,
        };
        let layout = settings_layout(main, &model);
        assert!(layout.max_scroll_offset > 0);
        let mut small = Terminal::new(TestBackend::new(108, 20)).unwrap();
        model.scroll_offset = layout.max_scroll_offset;
        small
            .draw(|frame| {
                render_settings(frame, frame.area(), &chrome(), &model, &theme);
            })
            .unwrap();
        assert!(terminal_output(&small).contains("Compiling crate-199"));
        model.scroll_offset = 0;
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
