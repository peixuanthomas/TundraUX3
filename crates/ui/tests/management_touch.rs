use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::*;

#[test]
fn detail_motion_draws_borders_then_restores_complete_text() {
    let mut model = ManagementViewModel {
        columns: vec!["Name".into()],
        rows: vec![vec!["list content".into()]],
        details: "details content 中文".into(),
        ..Default::default()
    };
    let context = RenderContext::default();
    for (width, height) in [(120, 40), (60, 24)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        for progress in [500, 999, 1_000, 500, 0] {
            model.details_expansion = Some(progress);
            let bounds = Rect::new(0, 0, width, height);
            let layout = management_layout(bounds, &model);
            terminal
                .draw(|frame| render_management_content(frame, bounds, &model, &context))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
            assert_eq!(buffer[(layout.details.x, layout.details.y)].symbol(), "╭");
            if progress > 0 && progress < 1_000 {
                assert!(
                    layout
                        .details
                        .inner(ratatui::layout::Margin::new(1, 1))
                        .positions()
                        .all(|position| buffer[position].symbol() == " ")
                );
                assert!(!text.contains("list content"));
            } else {
                assert!(text.contains("details content"));
                if progress == 0 {
                    assert!(text.contains("list content"));
                }
            }
        }
    }
}

#[test]
fn form_toggle_keeps_its_label_left_and_on_off_value_right() {
    let theme = TundraTheme::default_dark();
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    for checked in [false, true] {
        let model = ManagementViewModel {
            form: Some(ManagementForm {
                identity: "toggle".into(),
                fields: vec![ManagementFormField {
                    label: "Keep screen after exit".into(),
                    value: checked.to_string(),
                    control: ManagementFormControl::Toggle,
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        let bounds = Rect::new(0, 0, 80, 24);
        let layout = management_layout(bounds, &model);
        let mut terminal = Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
        terminal
            .draw(|frame| render_management_overlay(frame, bounds, &model, &context))
            .unwrap();
        let area = layout.fields[0].1;
        assert_eq!(area.height, 1);
        let line: String = (area.x..area.right())
            .map(|x| terminal.backend().buffer()[(x, area.y)].symbol())
            .collect();
        assert!(line.starts_with("Keep screen after exit"));
        assert!(line.ends_with(if checked { "[On]" } else { "[Off]" }));
        assert!(
            management_button_regions(bounds, &model)
                .iter()
                .any(|region| region.area == area)
        );
    }
}

#[test]
fn clicked_form_field_keeps_accent_and_blinking_cursor_in_mouse_mode() {
    use ratatui::style::Modifier;
    let theme = TundraTheme::default_dark();
    let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let mut buttons = ui::components::ButtonFrame::new(None, None, &theme);
    buttons.keyboard_focus_visible = false;
    context.buttons = Some(buttons);
    let mut model = ManagementViewModel {
        form: Some(ManagementForm {
            fields: vec![
                ManagementFormField {
                    label: "账户名称".into(),
                    ..Default::default()
                },
                ManagementFormField {
                    label: "密码".into(),
                    value: "secret".into(),
                    secret: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
        ..Default::default()
    };
    let bounds = Rect::new(0, 0, 60, 20);
    let mut terminal = Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
    for selected in [0, 1] {
        model.form.as_mut().unwrap().selected = selected;
        let layout = management_layout(bounds, &model);
        terminal
            .draw(|frame| render_management_overlay(frame, bounds, &model, &context))
            .unwrap();
        let field = &model.form.as_ref().unwrap().fields[selected];
        let area = management_field_input_area(&layout, selected, field);
        let cursor = (area.x + if selected == 0 { 0 } else { 6 }, area.y);
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[cursor].symbol(), "_");
        assert_eq!(buffer[cursor].fg, context.theme.accent);
        assert!(buffer[cursor].modifier.contains(Modifier::SLOW_BLINK));
        assert_eq!(buffer[(area.x, area.y - 1)].fg, context.theme.accent);
        assert!(
            !buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("secret")
        );
        let blinks = buffer
            .content
            .iter()
            .filter(|cell| cell.modifier.contains(Modifier::SLOW_BLINK))
            .count();
        assert_eq!(blinks, 1);
    }
}

#[test]
fn editable_form_fields_use_the_shared_text_cursor_and_keep_actions_visible() {
    let mut input = ui::components::TextInput::new("arguments").with_horizontal_scroll(true);
    input.set_value("abcd");
    input.set_cursor(2);
    let model = ManagementViewModel {
        form: Some(ManagementForm {
            identity: "terminal-run".into(),
            title: "Run settings".into(),
            fields: vec![ManagementFormField {
                label: "Arguments".into(),
                input: Some(input),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let theme = TundraTheme::default_dark();
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    for bounds in [Rect::new(0, 0, 80, 24), Rect::new(0, 0, 32, 14)] {
        let layout = management_layout(bounds, &model);
        let mut terminal = Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
        terminal
            .draw(|frame| render_management_overlay(frame, bounds, &model, &context))
            .unwrap();
        let row = layout.fields[0].1.y + 1;
        let text = (layout.fields[0].1.x..layout.fields[0].1.right())
            .map(|x| terminal.backend().buffer()[(x, row)].symbol())
            .collect::<String>();
        assert!(text.contains("ab_cd"), "visible cursor: {text:?}");
        assert!(!layout.submit.is_empty() && !layout.cancel.is_empty());
        assert!(
            layout.submit.bottom() <= bounds.bottom() && layout.cancel.bottom() <= bounds.bottom()
        );
    }
}

#[test]
fn multiple_selected_rows_and_the_current_keyboard_row_are_distinct() {
    use ratatui::style::Modifier;
    let theme = TundraTheme::default_dark();
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let bounds = Rect::new(0, 0, 140, 40);
    let mut model = ManagementViewModel {
        columns: vec!["包名".into()],
        rows: ["软件甲", "软件乙", "软件丙"]
            .map(|name| vec![name.into()])
            .into(),
        selected: 1,
        selected_rows: Some(vec![0, 2]),
        ..Default::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
    for selected in [vec![0, 2], vec![]] {
        model.selected_rows = Some(selected.clone());
        terminal
            .draw(|frame| render_management_content(frame, bounds, &model, &context))
            .unwrap();
        let layout = management_layout(bounds, &model);
        for index in 0..3 {
            let cell =
                &terminal.backend().buffer()[(layout.list_rows.x, layout.list_rows.y + index)];
            assert_eq!(
                cell.modifier.contains(Modifier::REVERSED),
                selected.contains(&(index as usize))
            );
            assert_eq!(cell.modifier.contains(Modifier::UNDERLINED), index == 1);
        }
    }
}

#[test]
fn package_actions_remain_visible_in_real_shell_content_areas() {
    let theme = TundraTheme::default_dark();
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    for labels in [
        [
            "Refresh repositories",
            "Installed packages",
            "More actions",
            "Current task",
            "Remove",
            "Install",
            "Upgrade selected package",
            "Search packages",
        ],
        [
            "刷新软件源",
            "已安装的软件包",
            "更多操作",
            "当前任务",
            "卸载",
            "安装",
            "更新所选软件包",
            "搜索软件包",
        ],
    ] {
        let shortcuts = [
            "[E]", "[F7]", "[3]", "[Ctrl+T]", "[X]", "[I]", "[U]", "[F6]",
        ];
        let model = ManagementViewModel {
            title: "Packages".into(),
            detail_action_start: Some(4),
            columns: vec!["Package".into()],
            rows: vec![vec!["example".into()]],
            details: "Selected package details".into(),
            actions: labels
                .into_iter()
                .zip(shortcuts)
                .enumerate()
                .map(|(index, (label, shortcut))| (format!("{shortcut} {label}"), index != 5))
                .collect(),
            ..Default::default()
        };
        for (width, height) in [(50, 12), (108, 20), (140, 40)] {
            let bounds = Rect::new(0, 0, width, height);
            let main = ShellFrameLayout::new(bounds, None, &context).main;
            let layout = management_layout(main, &model);
            assert_eq!(layout.actions.len(), 8);
            assert_eq!(layout.compact_actions, width == 50);
            if width != 50 {
                assert!(!layout.details_text.is_empty() && !layout.list_rows.is_empty());
            }
            let regions = management_button_regions(main, &model);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render_management_content(frame, main, &model, &context))
                .unwrap();
            for (index, area) in layout.actions.iter().enumerate() {
                assert!(!area.is_empty(), "{width}x{height} action {index}");
                assert_eq!(area.intersection(main), *area);
                assert!(
                    regions
                        .iter()
                        .any(|region| region.area == *area && region.disabled == (index == 5))
                );
                let painted = (area.x..area.right())
                    .map(|x| terminal.backend().buffer()[(x, area.y)].symbol())
                    .collect::<String>();
                assert!(
                    painted.contains(shortcuts[index]),
                    "{width}x{height} action {index}: {painted}"
                );
                for other in layout.actions.iter().skip(index + 1) {
                    assert!(area.intersection(*other).is_empty());
                }
                assert!(area.intersection(layout.filter).is_empty());
                for (_, control) in &layout.controls {
                    assert!(area.intersection(*control).is_empty());
                }
            }
        }
    }
}

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
    for width in [5, 12, 20, 50, 108, 116] {
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
            assert!(layout.filter.width >= 5);
            let buttons = management_button_regions(area, &model);
            for (control, rect) in &layout.controls {
                let label = match control {
                    ManagementControl::Refresh => management_controls()[0].1.clone(),
                    ManagementControl::ApplySearch => {
                        i18n::tr!("regression-management-search-enter")
                    }
                    ManagementControl::ClearSearch => "× [Ctrl+U]".into(),
                    _ => unreachable!(),
                };
                assert!(
                    usize::from(rect.width) >= label.width() + 2,
                    "{width}: {label}"
                );
                assert!(rect.intersection(layout.filter).is_empty());
                let id = management_control_id(&model, *control);
                assert_eq!(
                    buttons
                        .iter()
                        .find(|button| button.id.as_str() == id)
                        .unwrap()
                        .area,
                    *rect
                );
            }
            assert_eq!(
                layout
                    .controls
                    .iter()
                    .any(|(control, _)| *control == ManagementControl::ApplySearch),
                width >= 50
            );
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
            assert!(painted.contains(if rect == form_layout.submit {
                "F10"
            } else {
                "Esc"
            }));
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
        assert_eq!(layout.controls.len(), 2);
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
    assert_eq!(layout.actions.len(), 6);
    let last = ManagementViewModel {
        action_scroll: Some(34),
        ..model
    };
    assert_eq!(
        management_layout(Rect::new(0, 0, 120, 30), &last).action_start,
        34
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
fn an_empty_shell_main_has_no_clickable_management_buttons() {
    let model = ManagementViewModel {
        form: Some(ManagementForm::default()),
        ..Default::default()
    };
    assert!(management_button_regions(Rect::new(0, 1, 60, 0), &model).is_empty());
    assert!(management_button_regions(Rect::new(0, 1, 0, 20), &model).is_empty());
}

#[test]
fn informational_dialog_has_one_close_button_and_long_messages_scroll() {
    let mut model = ManagementViewModel {
        form: Some(ManagementForm {
            title: "About".into(),
            message: "Tundra Editor\nMicrosoft Edit".into(),
            dismiss_only: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let main = Rect::new(0, 0, 120, 40);
    let layout = management_layout(main, &model);
    assert_eq!(layout.form.height, 8);
    assert!(layout.submit.is_empty());
    let regions = management_button_regions(main, &model);
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].area, layout.cancel);
    model.form.as_mut().unwrap().message = (0..40)
        .map(|n| format!("Line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let main = Rect::new(0, 0, 50, 16);
    let layout = management_layout(main, &model);
    assert!(
        layout
            .scrollbars
            .iter()
            .any(|b| b.target == ManagementScrollTarget::FormMessage)
    );
    assert!(layout.message.intersection(layout.cancel).is_empty());
}

#[test]
fn ordinary_form_controls_stay_inside_even_a_tiny_main_area() {
    let model = ManagementViewModel {
        form: Some(ManagementForm {
            fields: vec![ManagementFormField {
                label: "Path".into(),
                value: "/etc/hosts".into(),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    for width in 1..=35 {
        for height in 1..=16 {
            let main = Rect::new(2, 1, width, height);
            for region in management_button_regions(main, &model) {
                assert_eq!(
                    region.area.intersection(main),
                    region.area,
                    "{width}x{height}"
                );
            }
        }
    }
}
