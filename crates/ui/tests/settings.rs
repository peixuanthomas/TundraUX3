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
    ShellChromeViewModel, StatusViewModel, TundraTheme, render_settings, settings_hit_test,
    settings_layout,
};

#[test]
fn application_list_stays_left_of_settings_and_uses_the_same_hit_areas() {
    for (width, height) in [(40, 12), (50, 16), (108, 20), (140, 40)] {
        let mut model = sample_model();
        model.selected_category = SettingsCategory::Applications;
        model.selected_application = ui::SettingsApplication::Editor;
        model.selected_field = SettingsField::ApplicationList;
        let area = Rect::new(0, 0, width, height);
        let layout = settings_layout(area, &model);
        let applications = layout.applications.as_ref().unwrap();
        assert!(applications.list.right() < layout.detail.x);
        for (application, button) in &applications.options {
            assert!(!button.is_empty());
            assert_eq!(button.intersection(applications.list), *button);
            assert_eq!(
                settings_hit_test(&layout, (button.x, button.y)),
                Some(SettingsHitTarget::Application(*application))
            );
        }
        assert!(
            layout
                .fields
                .iter()
                .all(|field| field.area.x >= layout.detail.x)
        );
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
        if width >= 108 {
            let output = terminal_output(&terminal);
            assert!(output.contains("File Explorer"));
            assert!(output.contains("Editor"));
        }
        let button = applications.options[0].1;
        model.color_editor = Some(SettingsColorEditorViewModel {
            title: "Color".into(),
            value: "#123456".into(),
            error: None,
        });
        assert_ne!(
            settings_hit_test(&settings_layout(area, &model), (button.x, button.y)),
            Some(SettingsHitTarget::Application(
                ui::SettingsApplication::FileExplorer
            ))
        );
    }
}

#[test]
fn operating_system_group_has_a_separate_hittable_entry_and_respects_overlays() {
    for (width, height) in [(40, 12), (50, 16), (108, 20), (140, 40)] {
        let mut model = sample_model();
        let area = Rect::new(0, 0, width, height);
        let layout = settings_layout(area, &model);
        let button = layout.operating_system;
        assert!(!button.is_empty());
        assert_eq!(button.intersection(area), button);
        assert!(
            layout
                .category_cards
                .iter()
                .all(|category| !category.area.intersects(button))
        );
        assert_eq!(
            settings_hit_test(&layout, (button.x, button.y)),
            Some(SettingsHitTarget::OperatingSystem)
        );
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
        assert!(terminal_output(&terminal).contains("F8"));
        model.color_editor = Some(SettingsColorEditorViewModel {
            title: "Color".into(),
            value: "#123456".into(),
            error: None,
        });
        assert_ne!(
            settings_hit_test(&settings_layout(area, &model), (button.x, button.y)),
            Some(SettingsHitTarget::OperatingSystem)
        );
    }
}

#[test]
fn unavailable_settings_retain_gray_buttons_and_register_disabled_hit_regions() {
    use ui::components::ButtonFrame;
    for kind in [
        SettingsControlKind::Action,
        SettingsControlKind::Toggle,
        SettingsControlKind::Cycle,
        SettingsControlKind::Picker,
        SettingsControlKind::Palette,
        SettingsControlKind::Stepper,
    ] {
        for enabled in [false, true] {
            let mut model = sample_model();
            model.appearance_preview = None;
            model.selected_field = SettingsField::CheckUpdates;
            model.cards = vec![SettingsCardViewModel::new(
                "Actions",
                vec![
                    SettingsItemViewModel::new(
                        SettingsField::CheckUpdates,
                        "Action",
                        "55",
                        "Unavailable while busy",
                        kind,
                    )
                    .enabled(enabled),
                ],
            )];
            let mut theme = TundraTheme {
                muted: Color::Magenta,
                ..TundraTheme::default()
            };
            let buttons = ButtonFrame::new(None, None, &theme);
            theme.buttons = Some(buttons.clone());
            let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
            terminal
                .draw(|frame| {
                    render_settings(frame, frame.area(), &chrome(), &model, &theme);
                })
                .unwrap();
            let regions: Vec<_> = buttons
                .regions()
                .into_iter()
                .filter(|region| {
                    region.id.as_str().starts_with("settings.")
                        && region.id.as_str() != "settings.operating-system"
                        && !region.id.as_str().starts_with("settings.category.")
                })
                .collect();
            assert_eq!(
                regions.len(),
                if kind == SettingsControlKind::Stepper {
                    2
                } else {
                    1
                }
            );
            for region in regions {
                assert_eq!(region.disabled, !enabled);
                let label: String = (region.area.x..region.area.right())
                    .map(|x| terminal.backend().buffer()[(x, region.area.y)].symbol())
                    .collect();
                assert!(label.contains('[') && label.contains(']'), "{label}");
                if !enabled {
                    for x in region.area.x..region.area.right() {
                        let cell = &terminal.backend().buffer()[(x, region.area.y)];
                        assert_eq!(cell.fg, Color::DarkGray);
                        assert!(cell.modifier.is_empty());
                    }
                }
            }
        }
    }
}

#[test]
fn full_layout_keeps_categories_and_fields_visible_at_supported_sizes() {
    let model = sample_model();
    for (width, height) in [(80, 24), (120, 32)] {
        let layout = settings_layout(Rect::new(0, 0, width, height), &model);
        assert_eq!(layout.category_cards.len(), SettingsCategory::ALL.len());
        assert_eq!(layout.workspace.vertical, width >= 94);
        for category in &layout.category_cards {
            let index = SettingsCategory::ALL
                .iter()
                .position(|entry| *entry == category.category)
                .unwrap();
            assert_eq!(category.area, layout.workspace.navigation[index]);
        }
        assert!(
            layout
                .fields
                .iter()
                .all(|field| field.area.x == layout.detail.x + 1)
        );
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
        .find(|entry| entry.category == SettingsCategory::Applications)
        .expect("application settings category");
    let field = layout
        .fields
        .iter()
        .find(|entry| entry.field == SettingsField::ShowHidden)
        .expect("show hidden field");

    assert_eq!(
        settings_hit_test(&layout, (category.area.x, category.area.y)),
        Some(SettingsHitTarget::Category(SettingsCategory::Applications))
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
        system_server_label: None,
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
        selected_application: Default::default(),
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
        animation_editor: None,
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
    let main = match ui::compute_shell_layout(Rect::new(0, 0, 80, 24)) {
        ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
    };
    let detail = settings_layout(main, &model).detail;
    let content = (detail.y..detail.bottom())
        .map(|y| {
            (detail.x + 1..detail.right() - 1)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for part in ["First line", "second line", "commit message visible"] {
        assert!(
            content.contains(part),
            "missing wrapped commit text: {part}: {content}"
        );
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
                (cancel, "[Esc Cancel]".into()),
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
        help_available: false,
        help_button_hovered: false,
        help_button_focused: false,
        back_shortcut: "Esc",
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
    for (width, height) in [(80, 12), (120, 18)] {
        for category in SettingsCategory::ALL {
            let mut model = sample_model();
            model.selected_category = category;
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
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
                let line = (entry.area.y..entry.area.bottom())
                    .flat_map(|y| (entry.area.x..entry.area.right()).map(move |x| (x, y)))
                    .map(|point| terminal.backend().buffer()[point].symbol())
                    .collect::<String>();
                assert!(
                    line.contains(&entry.category.label()),
                    "{category:?}: {line}"
                );
            }
            if layout.workspace.vertical {
                assert!(layout.category_scrollbar.is_some());
                assert!(layout.category_cards.len() < SettingsCategory::ALL.len());
            }
        }
    }
}

#[test]
fn animation_modal_scrolls_and_captures_only_its_visible_controls() {
    let phases = [
        SettingsField::PageAcceleration,
        SettingsField::PageCruise,
        SettingsField::PageDeceleration,
        SettingsField::DialogAcceleration,
        SettingsField::DialogCruise,
        SettingsField::DialogDeceleration,
        SettingsField::MenuAcceleration,
        SettingsField::MenuCruise,
        SettingsField::MenuDeceleration,
    ];
    for label in ["Page acceleration", "页面加速时长"] {
        for (width, height) in [(120, 32), (80, 12), (46, 11)] {
            let mut model = sample_model();
            let mut items = phases
                .map(|field| {
                    SettingsItemViewModel::new(
                        field,
                        label,
                        "55 ms",
                        "",
                        SettingsControlKind::Stepper,
                    )
                })
                .to_vec();
            items.push(SettingsItemViewModel::new(
                SettingsField::ResetAnimationSpeed,
                "Restore defaults",
                "Reset",
                "",
                SettingsControlKind::Action,
            ));
            model.animation_editor = Some(ui::SettingsAnimationEditorViewModel {
                items,
                selected_index: 0,
                scroll_offset: 0,
            });
            let area = Rect::new(0, 0, width, height);
            let layout = settings_layout(area, &model);
            let editor = layout.animation_editor.as_ref().unwrap();
            assert_eq!(editor.dialog.intersection(area), editor.dialog);
            assert!(!editor.fields.is_empty());
            for (field, delta, button) in &editor.adjustments {
                assert_eq!(button.intersection(editor.viewport), *button);
                assert_eq!(
                    settings_hit_test(&layout, (button.x, button.y)),
                    Some(SettingsHitTarget::AdjustField(*field, *delta))
                );
            }
            assert_eq!(
                settings_hit_test(&layout, (editor.close.x, editor.close.y)),
                Some(SettingsHitTarget::OverlayCancel)
            );
            assert_eq!(settings_hit_test(&layout, (area.x, area.y)), None);
            model.animation_editor.as_mut().unwrap().scroll_offset = u16::MAX;
            let layout = settings_layout(area, &model);
            let editor = layout.animation_editor.as_ref().unwrap();
            assert_eq!(
                editor.fields.last().unwrap().field,
                SettingsField::ResetAnimationSpeed
            );
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    let context = ui::RenderContext::default();
                    ui::render_settings_content(frame, &layout, &model, &context);
                    ui::render_settings_overlay(frame, &layout, &model, &context);
                })
                .unwrap();
            assert!(terminal_output(&terminal).contains("[Reset]"));
        }
    }
}
