use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::*;

#[test]
fn package_table_long_values_keep_both_version_columns_visible_and_full_details() {
    let name = "超长软件包名称".repeat(10);
    let version = "2026.10.08-very-long-distribution-revision";
    let model = ManagementViewModel {
        columns: vec![
            "Package".into(),
            "Installed".into(),
            "Available".into(),
            "Description".into(),
        ],
        column_width_limits: vec![22, 16, 16, 42],
        rows: vec![vec![
            name.clone(),
            version.into(),
            "2.0".into(),
            "Package summary".into(),
        ]],
        details: format!("Package: {name}\nInstalled: {version}"),
        ..Default::default()
    };
    let area = Rect::new(0, 0, 140, 40);
    let layout = management_layout(area, &model);
    let theme = TundraTheme::default_dark();
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let mut terminal = Terminal::new(TestBackend::new(140, 40)).unwrap();
    terminal
        .draw(|frame| render_management_content(frame, area, &model, &context))
        .unwrap();
    let line = |y| {
        (layout.list_rows.x..layout.list_rows.right())
            .map(|x| terminal.backend().buffer()[(x, y)].symbol())
            .collect::<String>()
    };
    let header = line(layout.list_rows.y - 1);
    assert!(
        header.contains("Installed") && header.contains("Available"),
        "{header}"
    );
    let row = line(layout.list_rows.y);
    assert!(row.contains('…') && row.contains("2.0"), "{row}");
    assert_eq!(layout.column_widths[0], 24);
    assert_eq!(layout.column_widths[1], 18);
    assert!(model.details.contains(&name) && model.details.contains(version));
}

#[test]
fn management_search_cursor_is_visible_for_empty_chinese_and_long_input() {
    use unicode_width::UnicodeWidthStr;
    for width in [20, 50] {
        for filter in [
            String::new(),
            "中文搜索".into(),
            format!("{}末尾", "软件包".repeat(30)),
        ] {
            let mut model = ManagementViewModel {
                filter,
                filtering: true,
                ..Default::default()
            };
            let area = Rect::new(0, 0, width, 24);
            let theme = TundraTheme::default_dark();
            let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
            let layout = management_layout(area, &model);
            let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
            terminal
                .draw(|frame| render_management_content(frame, area, &model, &context))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let cursor = (layout.filter.x..layout.filter.right())
                .find(|&x| buffer[(x, layout.filter.y)].symbol() == "_")
                .expect("focused search shows a visible cursor");
            if model.filter.is_empty() {
                assert_eq!(
                    cursor - layout.filter.x,
                    management_search_prefix(layout.filter.width).width() as u16
                );
            } else {
                assert_eq!(
                    buffer[(cursor - 2, layout.filter.y)].symbol(),
                    model.filter.chars().last().unwrap().to_string(),
                    "the last Chinese character stays beside the cursor even when text overflows"
                );
            }
            model.filtering = false;
            terminal
                .draw(|frame| render_management_content(frame, area, &model, &context))
                .unwrap();
            assert!(
                (layout.filter.x..layout.filter.right())
                    .all(|x| { terminal.backend().buffer()[(x, layout.filter.y)].symbol() != "_" })
            );
        }
    }
}

#[test]
fn management_action_labels_fit_touch_regions_in_small_windows() {
    use unicode_width::UnicodeWidthStr;
    let mut model = ManagementViewModel {
        actions: vec![("[A] Start".into(), true)],
        ..Default::default()
    };
    for (width, height) in [(50, 14), (60, 18), (80, 24)] {
        let area = Rect::new(0, 0, width, height);
        let theme = TundraTheme::default_dark();
        let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let layout = management_layout(area, &model);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render_management_content(frame, area, &model, &context))
            .unwrap();
        for (index, ((_, rect), (_, label))) in layout
            .controls
            .iter()
            .zip(management_controls())
            .enumerate()
        {
            assert!(rect.height > 0 && usize::from(rect.width) >= label.width());
            let painted = (rect.x..rect.right())
                .map(|x| terminal.backend().buffer()[(x, rect.y)].symbol())
                .collect::<String>();
            assert!(painted.contains(&label));
            for (_, other) in layout.controls.iter().skip(index + 1) {
                assert!(rect.intersection(*other).is_empty());
            }
        }
        model.form = Some(ManagementForm {
            title: "Review".into(),
            fields: vec![ManagementFormField {
                label: "Choice".into(),
                choices: vec!["No".into(), "Yes".into()],
                ..Default::default()
            }],
            ..Default::default()
        });
        let form_layout = management_layout(area, &model);
        terminal
            .draw(|frame| render_management_overlay(frame, area, &model, &context))
            .unwrap();
        for (rect, hint) in [
            (form_layout.submit, "Confirm"),
            (form_layout.cancel, "Cancel"),
        ] {
            let painted = (rect.x..rect.right())
                .map(|x| terminal.backend().buffer()[(x, rect.y)].symbol())
                .collect::<String>();
            assert!(painted.contains(hint), "{width}x{height}: {hint}");
        }
        assert!(
            form_layout
                .submit
                .intersection(form_layout.cancel)
                .is_empty()
        );
        model.form.as_mut().unwrap().choice = Some(ManagementChoices {
            values: vec!["No".into(), "Yes".into()],
            selected: 1,
            ..Default::default()
        });
        let choice_layout = management_layout(area, &model);
        terminal
            .draw(|frame| render_management_overlay(frame, area, &model, &context))
            .unwrap();
        let cancel = choice_layout.choice_cancel;
        let painted = (cancel.x..cancel.right())
            .map(|x| terminal.backend().buffer()[(x, cancel.y)].symbol())
            .collect::<String>();
        assert!(painted.contains("Close"));
        model.form = None;
    }
}

#[test]
fn compact_toolbar_keeps_every_basic_touch_action_visible() {
    let model = ManagementViewModel {
        actions: vec![("Start selected service".into(), true)],
        ..Default::default()
    };
    for size in [(64, 14), (60, 18), (80, 24)] {
        let area = Rect::new(0, 0, size.0, size.1);
        let layout = management_layout(area, &model);
        assert_eq!(layout.controls.len(), 1);
        assert_eq!(layout.controls[0].0, ManagementControl::Refresh);
        assert_eq!(layout.controls[0].1.x, area.x);
        assert!(layout.controls.iter().all(|(_, rect)| rect.width > 0
            && rect.height > 0
            && rect.right() <= area.right()
            && rect.bottom() <= area.bottom()));
        assert!(!layout.actions.is_empty());
    }
}

#[test]
fn overflowing_lists_text_and_actions_have_matching_drag_geometry() {
    let model = ManagementViewModel {
        columns: vec!["Name".into()],
        rows: (0..80).map(|i| vec![format!("row-{i}")]).collect(),
        details: "long description\n".repeat(80),
        actions: (0..40).map(|i| (format!("Action {i}"), true)).collect(),
        ..Default::default()
    };
    let layout = management_layout(Rect::new(0, 0, 120, 30), &model);
    for target in [
        ManagementScrollTarget::Rows,
        ManagementScrollTarget::Details,
    ] {
        let bar = layout
            .scrollbars
            .iter()
            .find(|bar| bar.target == target)
            .unwrap();
        assert_eq!(bar.offset_at((bar.track.x, bar.track.y), 0), 0);
        assert_eq!(
            bar.offset_at((bar.track.x, bar.track.bottom()), 0),
            bar.content_len - bar.viewport_len
        );
    }
    assert!(!layout.action_previous.is_empty() && !layout.action_next.is_empty());
    assert_eq!(layout.actions.len(), 4);
    let last = ManagementViewModel {
        action_scroll: Some(36),
        ..model
    };
    assert_eq!(
        management_layout(Rect::new(0, 0, 120, 30), &last).action_start,
        36
    );
}

#[test]
fn long_form_fields_messages_and_choices_remain_scrollable() {
    let mut model = ManagementViewModel {
        form: Some(ManagementForm {
            message: "dependency\n".repeat(100),
            fields: (0..30)
                .map(|_| ManagementFormField {
                    label: "Field".into(),
                    value: "A value".into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let area = Rect::new(0, 0, 64, 18);
    let layout = management_layout(area, &model);
    for target in [
        ManagementScrollTarget::FormMessage,
        ManagementScrollTarget::FormFields,
    ] {
        assert!(layout.scrollbars.iter().any(|bar| bar.target == target));
    }
    model.form.as_mut().unwrap().choice = Some(ManagementChoices {
        values: (0..80)
            .map(|i| format!("A very long choice path /directory/{i}/{}", "x".repeat(100)))
            .collect(),
        ..Default::default()
    });
    let layout = management_layout(area, &model);
    assert!(
        layout
            .scrollbars
            .iter()
            .any(|bar| bar.target == ManagementScrollTarget::Choices)
    );
    assert!(
        layout
            .scrollbars
            .iter()
            .any(|bar| bar.target == ManagementScrollTarget::ChoiceColumns)
    );
    assert!(layout.choice_cancel.width > 0);
}

#[test]
fn management_panels_and_form_inherit_square_and_rounded_borders() {
    for (shape, corner) in [(BorderShape::Square, "┌"), (BorderShape::Rounded, "╭")] {
        let theme = TundraTheme::default_dark().with_border_shape(shape);
        let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let mut model = ManagementViewModel::default();
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        let area = Rect::new(0, 0, 120, 36);
        let layout = management_layout(area, &model);
        terminal
            .draw(|frame| render_management_content(frame, area, &model, &context))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(layout.list.x, layout.list.y)].symbol(),
            corner
        );
        assert_eq!(
            terminal.backend().buffer()[(layout.details.x, layout.details.y)].symbol(),
            corner
        );
        model.form = Some(ManagementForm {
            title: "Confirm".into(),
            ..Default::default()
        });
        let layout = management_layout(area, &model);
        terminal
            .draw(|frame| render_management_overlay(frame, area, &model, &context))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(layout.form.x, layout.form.y)].symbol(),
            corner
        );
    }
}

#[test]
fn management_rendered_button_ids_match_capture_regions_and_show_pressed_color() {
    use ui::components::ButtonFrame;
    let model = ManagementViewModel {
        scope_id: "services".into(),
        action_ids: vec!["management.action.services.start.row-42".into()],
        actions: vec![("Start".into(), true)],
        ..Default::default()
    };
    let area = Rect::new(0, 0, 120, 36);
    let region = management_button_regions(area, &model)
        .into_iter()
        .find(|region| region.id.as_str() == "management.action.services.start.row-42")
        .unwrap();
    let theme = TundraTheme::default_dark();
    let buttons = ButtonFrame::new(Some(region.clone()), Some(region.clone()), &theme);
    let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    context.buttons = Some(buttons.clone());
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    terminal
        .draw(|frame| render_management_content(frame, area, &model, &context))
        .unwrap();
    assert!(buttons.regions().contains(&region));
    assert_eq!(
        terminal.backend().buffer()[(region.area.x, region.area.y)].fg,
        theme.button_pressed_color()
    );
    let mut model = model;
    model.form = Some(ManagementForm {
        identity: "question-1".into(),
        fields: vec![ManagementFormField {
            id: "answer".into(),
            label: "Answer".into(),
            value: "Yes".into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let capture = management_button_regions(area, &model);
    let buttons = ButtonFrame::new(None, None, &theme);
    context.buttons = Some(buttons.clone());
    terminal
        .draw(|frame| render_management_overlay(frame, area, &model, &context))
        .unwrap();
    for region in capture {
        assert!(buttons.regions().contains(&region), "missing {:?}", region);
    }
    model.form.as_mut().unwrap().choice = Some(ManagementChoices {
        values: vec!["No".into(), "Yes".into()],
        ..Default::default()
    });
    let capture = management_button_regions(area, &model);
    let buttons = ButtonFrame::new(None, None, &theme);
    context.buttons = Some(buttons.clone());
    terminal
        .draw(|frame| render_management_overlay(frame, area, &model, &context))
        .unwrap();
    for region in capture {
        assert!(buttons.regions().contains(&region), "missing {:?}", region);
    }
}

#[test]
fn rendering_management_with_a_form_registers_only_the_form_buttons() {
    use ui::components::ButtonFrame;
    let main = Rect::new(0, 1, 120, 36);
    let mut model = ManagementViewModel {
        scope_id: "services".into(),
        actions: vec![("Start".into(), true)],
        actions_focused: true,
        filtering: true,
        ..Default::default()
    };
    let background = management_button_regions(main, &model)
        .into_iter()
        .find(|region| region.id.as_str().contains("management.action."))
        .unwrap();
    let theme = TundraTheme::default_dark();
    let mut terminal = Terminal::new(TestBackend::new(120, 38)).unwrap();
    for choice in [false, true] {
        model.form = Some(ManagementForm {
            identity: if choice { "more-actions" } else { "review" }.into(),
            title: "Review".into(),
            fields: vec![ManagementFormField {
                id: "option".into(),
                label: "Option".into(),
                value: "One".into(),
                ..Default::default()
            }],
            choice: choice.then(|| ManagementChoices {
                values: vec!["One    O".into(), "Two    T".into()],
                ..Default::default()
            }),
            ..Default::default()
        });
        let buttons = ButtonFrame::new(Some(background.clone()), Some(background.clone()), &theme);
        let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        context.buttons = Some(buttons.clone());
        terminal
            .draw(|frame| {
                render_management_content(frame, main, &model, &context);
                render_management_overlay(frame, main, &model, &context);
            })
            .unwrap();
        let registered = buttons.regions();
        let expected = management_button_regions(main, &model);
        assert_eq!(registered.len(), expected.len());
        for region in expected {
            assert!(registered.contains(&region));
        }
        assert!(!registered.contains(&background));
        if choice {
            assert_eq!(
                terminal.backend().buffer()[(background.area.x, background.area.y)].fg,
                theme.body_style().fg.unwrap()
            );
            let filter = management_layout(main, &model).filter;
            assert!(
                (filter.x..filter.right())
                    .all(|x| terminal.backend().buffer()[(x, filter.y)].symbol() != "_")
            );
        }
    }
}

#[test]
fn bilingual_search_actions_and_overlays_stay_inside_the_shell_main_area() {
    use ratatui::widgets::Paragraph;
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for language in ["en-US", "zh-CN"] {
        let snapshot = i18n::LanguageSnapshot::load(&assets, language, 1)
            .unwrap()
            .snapshot;
        let _language = i18n::enter_snapshot(std::sync::Arc::new(snapshot));
        for (width, height) in [(20, 12), (32, 18), (60, 18), (90, 24), (120, 40)] {
            let main = Rect::new(2, 1, width - 4, height - 2);
            let theme = TundraTheme::default_dark();
            let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
            let mut model = ManagementViewModel {
                title: if language == "zh-CN" {
                    "网络"
                } else {
                    "Network"
                }
                .into(),
                filter: "Wi-Fi".into(),
                columns: vec!["SSID".into()],
                rows: vec![vec!["网络名称".into()]],
                actions: vec![
                    (
                        if language == "zh-CN" {
                            "连接"
                        } else {
                            "Connect"
                        }
                        .into(),
                        true,
                    ),
                    (
                        if language == "zh-CN" {
                            "更多操作"
                        } else {
                            "More actions"
                        }
                        .into(),
                        true,
                    ),
                ],
                ..Default::default()
            };
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            for overlay in [false, true] {
                if overlay {
                    model.form = Some(ManagementForm {
                        identity: "more-actions".into(),
                        title: if language == "zh-CN" {
                            "更多操作"
                        } else {
                            "More actions"
                        }
                        .into(),
                        choice: Some(ManagementChoices {
                            values: vec!["编辑配置    E".into(), "查看日志    L".into()],
                            ..Default::default()
                        }),
                        ..Default::default()
                    });
                }
                terminal
                    .draw(|frame| {
                        for y in 0..height {
                            frame.render_widget(
                                Paragraph::new("#".repeat(width as usize)),
                                Rect::new(0, y, width, 1),
                            );
                        }
                        render_management_content(frame, main, &model, &context);
                        render_management_overlay(frame, main, &model, &context);
                    })
                    .unwrap();
                for y in 0..height {
                    for x in 0..width {
                        if !main.contains((x, y).into()) {
                            assert_eq!(
                                terminal.backend().buffer()[(x, y)].symbol(),
                                "#",
                                "{language} {width}x{height}: page paints outside main at {x},{y}"
                            );
                        }
                    }
                }
                for region in management_button_regions(main, &model) {
                    assert_eq!(
                        region.area.intersection(main),
                        region.area,
                        "{language} {width}x{height}: capture extends outside main"
                    );
                }
            }
        }
    }
}

#[test]
fn search_clear_is_part_of_the_input_and_main_buttons_have_plain_labels() {
    let area = Rect::new(0, 1, 60, 18);
    let mut model = ManagementViewModel {
        filter: "ssh".into(),
        actions: vec![
            ("Edit configuration".into(), true),
            ("More actions".into(), true),
        ],
        ..Default::default()
    };
    let layout = management_layout(area, &model);
    assert_eq!(layout.controls.len(), 2);
    let clear = layout
        .controls
        .iter()
        .find(|(control, _)| *control == ManagementControl::ClearSearch)
        .unwrap()
        .1;
    assert_eq!(clear.x, layout.filter.right());
    assert_eq!(clear.y, layout.filter.y);
    assert!(layout.actions.iter().all(|rect| rect.height == 1));
    model.filter.clear();
    assert_eq!(management_layout(area, &model).controls.len(), 1);
}

#[test]
fn menu_labels_are_centered_with_nearby_shortcuts_and_disabled_entries_keep_the_disabled_color() {
    use ratatui::style::Color;
    use ui::components::ButtonFrame;
    let area = Rect::new(0, 1, 60, 18);
    let model = ManagementViewModel {
        form: Some(ManagementForm {
            identity: "config-menu".into(),
            choice: Some(ManagementChoices {
                values: vec![
                    "Edit configuration    E".into(),
                    "Restore old version    R".into(),
                ],
                disabled: vec![false, true],
                selected: 1,
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let layout = management_layout(area, &model);
    let enabled = management_button_regions(area, &model)
        .into_iter()
        .find(|r| r.id.as_str().contains("choice.config-menu.0.0"))
        .unwrap();
    let disabled = management_button_regions(area, &model)
        .into_iter()
        .find(|r| r.id.as_str().contains("choice.config-menu.0.1"))
        .unwrap();
    assert!(disabled.disabled);
    let theme = TundraTheme::default_dark().with_accent_color(Color::Rgb(12, 98, 176));
    let buttons = ButtonFrame::new(Some(enabled.clone()), None, &theme);
    let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    context.buttons = Some(buttons);
    let mut terminal = Terminal::new(TestBackend::new(60, 20)).unwrap();
    terminal
        .draw(|frame| render_management_overlay(frame, area, &model, &context))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(enabled.area.x, enabled.area.y)].fg,
        theme.accent_color
    );
    assert_eq!(
        buffer[(disabled.area.x, disabled.area.y)].fg,
        theme.disabled_style().fg.unwrap()
    );
    for (index, rect) in layout.choice_rows {
        let value = &model.form.as_ref().unwrap().choice.as_ref().unwrap().values[index];
        let painted = (rect.x..rect.right())
            .map(|x| buffer[(x, rect.y)].symbol())
            .collect::<String>();
        assert_eq!(painted.trim(), value);
        let leading = painted.len() - painted.trim_start().len();
        let trailing = painted.len() - painted.trim_end().len();
        assert!(leading.abs_diff(trailing) <= 1);
        assert_ne!(buffer[(rect.x, rect.y)].symbol(), "●");
    }
}

#[test]
fn more_action_menus_fit_their_items_and_leave_space_inside_every_border() {
    for identity in ["more-actions", "config-menu"] {
        let model = ManagementViewModel {
            form: Some(ManagementForm {
                identity: identity.into(),
                title: "More actions".into(),
                choice: Some(ManagementChoices {
                    values: (0..6).map(|i| format!("Action {i}    {i}")).collect(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let main = Rect::new(3, 2, 160, 48);
        let layout = management_layout(main, &model);
        assert_eq!(layout.form.height, 12, "six items do not fill the screen");
        assert!(layout.form.width < 40, "width follows the item labels");
        assert_eq!(layout.form.intersection(main), layout.form);
        assert!((layout.form.x - main.x).abs_diff(main.right() - layout.form.right()) <= 1);
        assert!((layout.form.y - main.y).abs_diff(main.bottom() - layout.form.bottom()) <= 1);
        assert_eq!(layout.choice_rows.len(), 6);
        for (_, row) in &layout.choice_rows {
            assert!(row.x >= layout.form.x + 3);
            assert!(row.right() <= layout.form.right() - 3);
            assert!(row.y >= layout.form.y + 2);
        }
        let last = layout.choice_rows.last().unwrap().1;
        assert!(layout.choice_cancel.y >= last.bottom() + 1);
        assert!(layout.choice_cancel.bottom() <= layout.form.bottom() - 2);
    }
}

#[test]
fn small_choice_menus_keep_scrollbars_away_from_items_and_close_button() {
    let model = ManagementViewModel {
        form: Some(ManagementForm {
            identity: "more-actions".into(),
            title: "More actions".into(),
            choice: Some(ManagementChoices {
                values: (0..30)
                    .map(|i| format!("Long item {i} /{}    X", "path/".repeat(20)))
                    .collect(),
                scroll: usize::MAX,
                columns: 20,
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    for (width, height) in [(30, 7), (40, 10), (60, 14)] {
        let main = Rect::new(2, 1, width, height);
        let layout = management_layout(main, &model);
        assert!(!layout.choice_rows.is_empty());
        assert_eq!(layout.choice_rows.last().unwrap().0, 29);
        for target in [
            ManagementScrollTarget::Choices,
            ManagementScrollTarget::ChoiceColumns,
        ] {
            let bar = layout
                .scrollbars
                .iter()
                .find(|bar| bar.target == target)
                .unwrap();
            assert_eq!(bar.track.intersection(layout.form), bar.track);
            if bar.thumb == bar.track {
                assert_eq!(bar.offset_at((bar.track.x, bar.track.y), 0), bar.offset);
            }
            assert!(bar.track.intersection(layout.choice_cancel).is_empty());
            for (_, row) in &layout.choice_rows {
                assert!(bar.track.intersection(*row).is_empty());
            }
            assert_eq!(
                bar.offset_at((bar.track.right(), bar.track.bottom()), 0),
                bar.content_len - bar.viewport_len
            );
        }
        assert!(
            management_button_regions(main, &model)
                .iter()
                .all(|region| region.area.intersection(main) == region.area)
        );
    }
}

#[test]
fn choice_menus_remain_inside_the_main_area_even_when_it_is_only_a_few_cells() {
    let model = ManagementViewModel {
        form: Some(ManagementForm {
            identity: "more-actions".into(),
            title: "More actions".into(),
            choice: Some(ManagementChoices {
                values: vec!["One    O".into(), "Two    T".into()],
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let theme = TundraTheme::default_dark();
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    for width in 1..=20 {
        for height in 1..=10 {
            let main = Rect::new(2, 1, width, height);
            let layout = management_layout(main, &model);
            assert_eq!(layout.form.intersection(main), layout.form);
            for bar in layout.scrollbars.iter().filter(|bar| {
                matches!(
                    bar.target,
                    ManagementScrollTarget::Choices | ManagementScrollTarget::ChoiceColumns
                )
            }) {
                assert!(
                    bar.track.intersection(layout.choice_cancel).is_empty(),
                    "{width}x{height}: scrollbar covers close"
                );
            }
            for region in management_button_regions(main, &model) {
                assert_eq!(
                    region.area.intersection(main),
                    region.area,
                    "{width}x{height}"
                );
            }
            let mut terminal = Terminal::new(TestBackend::new(width + 4, height + 2)).unwrap();
            terminal
                .draw(|frame| render_management_overlay(frame, main, &model, &context))
                .unwrap();
            for y in 0..height + 2 {
                for x in 0..width + 4 {
                    if !main.contains((x, y).into()) {
                        assert_eq!(
                            terminal.backend().buffer()[(x, y)].symbol(),
                            " ",
                            "{width}x{height} at {x},{y}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn configuration_form_uses_its_action_name_and_blocks_a_failed_check() {
    let area = Rect::new(0, 1, 80, 24);
    let model = ManagementViewModel {
        form: Some(ManagementForm {
            identity: "config-check".into(),
            submit_label: Some("Authorize save".into()),
            submit_disabled: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let layout = management_layout(area, &model);
    let regions = management_button_regions(area, &model);
    assert!(
        regions
            .iter()
            .find(|r| r.area == layout.submit)
            .unwrap()
            .disabled
    );
    let theme = TundraTheme::default_dark();
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let mut terminal = Terminal::new(TestBackend::new(80, 26)).unwrap();
    terminal
        .draw(|frame| render_management_overlay(frame, area, &model, &context))
        .unwrap();
    let text = (layout.submit.x..layout.submit.right())
        .map(|x| terminal.backend().buffer()[(x, layout.submit.y)].symbol())
        .collect::<String>();
    assert!(text.contains("Authorize save"));
    assert_eq!(
        terminal.backend().buffer()[(layout.submit.x, layout.submit.y)].fg,
        theme.disabled_style().fg.unwrap()
    );
}

#[test]
fn keyboard_tab_to_submit_keeps_the_action_in_the_accent_color() {
    use ratatui::style::Color;
    let area = Rect::new(0, 1, 80, 24);
    let model = ManagementViewModel {
        form: Some(ManagementForm {
            identity: "wifi-connect".into(),
            submit_label: Some("Connect".into()),
            selected: 1,
            fields: vec![ManagementFormField {
                id: "password".into(),
                label: "Password".into(),
                value: "••••".into(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let layout = management_layout(area, &model);
    let theme = TundraTheme::default_dark().with_accent_color(Color::Rgb(19, 113, 211));
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let mut terminal = Terminal::new(TestBackend::new(80, 26)).unwrap();
    terminal
        .draw(|frame| render_management_overlay(frame, area, &model, &context))
        .unwrap();
    assert_eq!(
        terminal.backend().buffer()[(layout.submit.x, layout.submit.y)].fg,
        theme.accent_color
    );
    assert_ne!(
        terminal.backend().buffer()[(layout.fields[0].1.x, layout.fields[0].1.y)].fg,
        theme.accent_color
    );
}

#[test]
fn an_empty_shell_main_has_no_clickable_management_buttons() {
    let model = ManagementViewModel {
        form: Some(ManagementForm::default()),
        ..Default::default()
    };
    assert!(management_button_regions(Rect::new(0, 1, 60, 0), &model).is_empty());
    assert!(management_button_regions(Rect::new(0, 1, 0, 20), &model).is_empty());
}
