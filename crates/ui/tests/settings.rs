#[path = "support/composition.rs"]
mod composition;
use composition as ui;
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
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
        summary_title: None,
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
        .chunks(80)
        .position(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("12345678")
        })
        .expect("commit row");
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
        .chunks(80)
        .position(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("12345678")
        })
        .expect("scrolled commit row");
    assert!(second_row < first_row);
}

#[test]
fn update_confirmation_draws_complete_buttons_and_blocks_underlying_hits() {
    for confirm_label in ["Update and restart", "Replace and restart"] {
        for (width, height) in [(50, 12), (80, 24), (120, 32)] {
            let mut model = sample_model();
            model.update = Some(SettingsUpdateViewModel {
        summary_title: None,
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
        summary_title: None,
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
        back_button_hovered: false,
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

#[test]
fn scrolled_categories_render_and_hit_the_same_rows() {
    for category in SettingsCategory::ALL {
        let mut model = sample_model();
        model.selected_category = category;
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        let mut layout = None;
        terminal
            .draw(|frame| {
                layout = Some(render_settings(
                    frame,
                    frame.area(),
                    &chrome(),
                    &model,
                    &TundraTheme::default_dark(),
                ));
            })
            .unwrap();
        let layout = layout.unwrap();
        assert!(
            layout
                .category_cards
                .iter()
                .any(|entry| entry.category == category)
        );
        for entry in &layout.category_cards {
            assert_eq!(
                settings_hit_test(&layout, (entry.area.x, entry.area.y)),
                Some(SettingsHitTarget::Category(entry.category))
            );
            let line = (entry.area.x..entry.area.right())
                .map(|x| terminal.backend().buffer()[(x, entry.area.y)].symbol())
                .collect::<String>();
            assert!(
                line.contains(&entry.category.label()),
                "{category:?}: {line}"
            );
        }
        let last = layout.category_cards.last().unwrap();
        assert_eq!(
            settings_hit_test(&layout, (last.area.x, last.area.bottom())),
            None,
            "sidebar bottom border cannot select a hidden category"
        );
    }
}

#[test]
fn unavailable_settings_use_localized_reasons_and_theme_without_permission_lock() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for (language, reason, missing) in [
        (
            "en-US",
            "This feature is not supported on this platform yet.",
            "Not obtained",
        ),
        ("zh-CN", "当前平台暂不支持此功能", "未获取"),
    ] {
        let snapshot = i18n::LanguageSnapshot::load(&root, language, 1)
            .unwrap()
            .snapshot;
        let _language = i18n::enter_snapshot(std::sync::Arc::new(snapshot));
        for category in [
            SettingsCategory::Sound,
            SettingsCategory::Display,
            SettingsCategory::Wifi,
            SettingsCategory::Bluetooth,
        ] {
            assert!(!category.label().starts_with("ui-settings-"));
            assert!(!category.description().starts_with("ui-settings-"));
            let mut model = sample_model();
            model.selected_category = category;
            model.selected_field = SettingsField::SoundOutputVolume;
            model.appearance_preview = None;
            model.locked_message = None;
            model.cards = vec![SettingsCardViewModel::new(
                category.label(),
                vec![
                    SettingsItemViewModel::new(
                        SettingsField::SoundOutputVolume,
                        i18n::tr!("settings-device-sound-output-volume"),
                        i18n::tr!("settings-device-not-obtained"),
                        i18n::tr!("settings-device-sound-output-volume-help"),
                        SettingsControlKind::Stepper,
                    )
                    .unavailable(i18n::tr!("settings-device-unsupported")),
                ],
            )];
            for (width, height) in [(80, 24), (108, 20), (120, 32)] {
                let theme = TundraTheme {
                    muted: Color::Magenta,
                    ..TundraTheme::default_dark()
                };
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| {
                        render_settings(frame, frame.area(), &chrome(), &model, &theme);
                    })
                    .unwrap();
                let output = terminal_output(&terminal);
                assert!(
                    output.replace(' ', "").contains(&reason.replace(' ', "")),
                    "{language}: {output}"
                );
                assert!(output.replace(' ', "").contains(&missing.replace(' ', "")));
                assert!(!output.contains("locked") && !output.contains("已锁定"));
                assert!(
                    terminal
                        .backend()
                        .buffer()
                        .content()
                        .iter()
                        .any(
                            |cell| cell.symbol() == missing.chars().next().unwrap().to_string()
                                && cell.fg == theme.muted
                        )
                );
            }
        }
    }
}

#[test]
fn clipped_setting_cards_keep_borders_off_content_rows() {
    let mut model = sample_model();
    model.appearance_preview = None;
    model.cards = vec![SettingsCardViewModel::new(
        "Clipped card",
        (0..20)
            .map(|index| {
                SettingsItemViewModel::new(
                    SettingsField::BluetoothUnpair,
                    format!("Device {index}"),
                    "Unavailable",
                    "Not integrated",
                    SettingsControlKind::Action,
                )
                .unavailable("Not integrated")
            })
            .collect(),
    )];
    model.scroll_offset = 3;
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    let mut layout = None;
    terminal
        .draw(|frame| {
            layout = Some(render_settings(
                frame,
                frame.area(),
                &chrome(),
                &model,
                &TundraTheme::default_dark(),
            ));
        })
        .unwrap();
    let detail = layout.unwrap().detail;
    for y in [detail.y, detail.bottom() - 1] {
        assert_eq!(terminal.backend().buffer()[(detail.x, y)].symbol(), "│");
        assert_eq!(
            terminal.backend().buffer()[(detail.right() - 1, y)].symbol(),
            "│"
        );
    }
    assert!(!terminal_output(&terminal).contains("Clipped card"));
    assert!(terminal_output(&terminal).contains("Device 2"));
}
