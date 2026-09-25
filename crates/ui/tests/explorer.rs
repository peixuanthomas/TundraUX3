#[path = "support/composition.rs"]
mod composition;
use composition as ui;
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use support::terminal_output;
use ui::{
    ExplorerBreadcrumbViewModel, ExplorerConflictChoice, ExplorerConflictViewModel,
    ExplorerContextMenuItemViewModel, ExplorerContextMenuViewModel, ExplorerEntryViewModel,
    ExplorerHitTarget, ExplorerNameDialogKind, ExplorerNameDialogViewModel,
    ExplorerOperationProgressViewModel, ExplorerOverlayControl, ExplorerOverlayViewModel,
    ExplorerProgressStage, ExplorerQuickLocationViewModel, ExplorerSortColumn,
    ExplorerToolbarAction, ExplorerViewModel, HomeDisplayMode, MotionFrame, RenderCapabilities,
    RenderContext, ShellChromeViewModel, ShellLayout, StatusViewModel, TundraTheme,
    compute_shell_layout, explorer_first_entry_content_line, explorer_layout, render_explorer,
    render_explorer_with_context,
};

#[test]
fn options_use_body_color_and_mark_non_defaults_independently_of_focus() {
    use ratatui::style::{Color, Modifier};

    for theme in [
        TundraTheme::default_dark(),
        TundraTheme {
            background: Color::White,
            foreground: Color::Black,
            accent_color: Color::Blue,
            ..TundraTheme::default_dark()
        },
    ] {
        for focused_index in 0..=4 {
            let mut model = sample_model();
            model.overlay_selection = focused_index;
            model.overlay = Some(ExplorerOverlayViewModel::Options(
                ui::ExplorerOptionsViewModel {
                    title: "Options".into(),
                    options: (0..4)
                        .map(|index| ui::ExplorerOptionViewModel {
                            id: format!("option-{index}"),
                            label: format!("Option {index}"),
                            value: "On".into(),
                            enabled: index != 3,
                            modified: index != 0,
                            focused: index == focused_index,
                        })
                        .collect(),
                    close_label: "Close".into(),
                },
            ));
            let mut terminal = Terminal::new(TestBackend::new(110, 32)).unwrap();
            terminal
                .draw(|frame| {
                    render_explorer(frame, frame.area(), &chrome_for("Explorer"), &model, &theme)
                })
                .unwrap();
            let output = terminal_output(&terminal);
            assert!(output.contains("[Option 0: On]"));
            for index in 1..4 {
                assert!(output.contains(&format!("[* Option {index}: On]")));
            }
            for index in 0..4 {
                let area = overlay_control_area(&model, &ExplorerOverlayControl::Option(index));
                let cell = &terminal.backend().buffer()[(area.x, area.y)];
                let expected = if index == 3 {
                    theme.muted
                } else if index == focused_index {
                    theme.accent_color
                } else {
                    theme.foreground
                };
                assert_eq!(cell.fg, expected, "option {index}, focus {focused_index}");
                assert_eq!(
                    cell.modifier.contains(Modifier::BOLD),
                    index == focused_index && index != 3
                );
                let ShellLayout::Full { main, .. } = compute_shell_layout(Rect::new(0, 0, 110, 32))
                else {
                    unreachable!()
                };
                assert_eq!(
                    explorer_layout(main, &model).hit_test(area.x, area.y),
                    Some(if index == 3 {
                        ExplorerHitTarget::OverlaySurface
                    } else {
                        ExplorerHitTarget::Overlay(ExplorerOverlayControl::Option(index))
                    })
                );
            }
        }
    }
}

#[test]
fn explorer_context_path_renders_real_sizes_with_ansi_and_reduced_motion() {
    let model = sample_model();
    let chrome = chrome_for("Explorer");
    for (width, height) in [(50, 12), (80, 24), (120, 32)] {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let context = RenderContext::from_theme(
            &TundraTheme::default(),
            MotionFrame::reduced(Default::default()),
            RenderCapabilities::ansi(),
        );
        terminal
            .draw(|frame| {
                render_explorer_with_context(frame, frame.area(), &chrome, &model, &context)
            })
            .expect("context render");
        assert_eq!(terminal.backend().buffer().area.width, width);
    }
}

#[test]
fn explorer_address_editor_renders_the_controlled_value_and_cursor() {
    let mut model = sample_model();
    model.address_editing = true;
    model.address_value = "/Users/strix/projects/src".to_string();

    let output = render_output(&model);

    assert!(output.contains("> /Users/strix/projects/src_"));
}

#[test]
fn explorer_first_entry_line_accounts_for_wrapped_header_text() {
    let model = sample_model();

    assert!(
        explorer_first_entry_content_line(&model, 40)
            > explorer_first_entry_content_line(&model, 120)
    );
}

#[test]
fn explorer_quick_access_scrolls_current_location_into_view() {
    let mut model = sample_model();
    model.quick_locations = (0..12)
        .map(|index| {
            let mut location = ExplorerQuickLocationViewModel::new(
                format!("location-{index}"),
                format!("Location {index}"),
                format!("/location/{index}"),
                "folder",
            );
            location.current = index == 10;
            location
        })
        .collect();

    let layout = explorer_layout(Rect::new(0, 0, 96, 14), &model);
    assert_eq!(layout.quick_location_visible_capacity, 7);
    assert_eq!(layout.quick_location_visible_start, 4);
    assert_eq!(
        layout
            .quick_locations
            .iter()
            .map(|location| location.index)
            .collect::<Vec<_>>(),
        vec![4, 5, 6, 7, 8, 9, 10]
    );
    let last = layout.quick_locations.last().expect("current location row");
    assert_eq!(
        layout.hit_test(last.area.x, last.area.y),
        Some(ExplorerHitTarget::QuickLocation(10))
    );

    model.quick_locations[10].current = false;
    model.quick_locations[0].current = true;
    let first_layout = explorer_layout(Rect::new(0, 0, 96, 14), &model);
    assert_eq!(first_layout.quick_location_visible_start, 0);

    let narrow = explorer_layout(Rect::new(0, 0, 95, 14), &model);
    assert_eq!(narrow.quick_location_visible_capacity, 0);
    assert_eq!(narrow.quick_location_visible_start, 0);
    assert!(narrow.quick_locations.is_empty());
}

#[test]
fn explorer_layout_exposes_shared_mouse_hit_geometry() {
    let mut model = sample_model();
    model.set_history_availability(true, false);
    model.quick_locations = vec![ExplorerQuickLocationViewModel::new(
        "documents",
        "Documents",
        "/Users/strix/Documents",
        "documents",
    )];
    model.breadcrumbs = vec![ExplorerBreadcrumbViewModel::new(
        "projects",
        "projects",
        "/Users/strix/projects",
    )];
    model.operation = Some(ExplorerOperationProgressViewModel {
        phase: ExplorerProgressStage::Copying,
        label: "Copying to Documents".to_string(),
        completed_items: 1,
        total_items: Some(4),
        completed_bytes: 25,
        total_bytes: Some(100),
        cancellable: true,
        cancel_label: "Cancel".to_string(),
    });

    let layout = explorer_layout(Rect::new(0, 0, 110, 28), &model);
    let back = layout
        .toolbar_buttons
        .iter()
        .find(|button| button.action == ExplorerToolbarAction::Back)
        .expect("back button");
    assert_eq!(
        layout.hit_test(back.area.x, back.area.y),
        Some(ExplorerHitTarget::Toolbar(ExplorerToolbarAction::Back))
    );
    let location = layout.quick_locations.first().expect("quick location");
    assert_eq!(
        layout.hit_test(location.area.x, location.area.y),
        Some(ExplorerHitTarget::QuickLocation(0))
    );
    let row = layout.rows.first().expect("entry row");
    assert_eq!(
        layout.hit_test(row.area.x, row.area.y),
        Some(ExplorerHitTarget::Entry(row.index))
    );
    let modified = layout
        .columns
        .iter()
        .find(|column| column.column == ExplorerSortColumn::Modified)
        .expect("modified column");
    assert_eq!(
        layout.hit_test(modified.area.x, modified.area.y),
        Some(ExplorerHitTarget::Column(ExplorerSortColumn::Modified))
    );
    let cancel = layout.cancel_operation.expect("cancel operation");
    assert_eq!(
        layout.hit_test(cancel.x, cancel.y),
        Some(ExplorerHitTarget::CancelOperation)
    );
}

#[test]
fn explorer_breadcrumb_geometry_uses_terminal_columns_for_wide_labels() {
    let mut model = sample_model();
    model.breadcrumbs = vec![ExplorerBreadcrumbViewModel::new(
        "documents",
        "目录",
        "/Users/strix/目录",
    )];

    let layout = explorer_layout(Rect::new(0, 0, 110, 28), &model);
    let breadcrumb = layout.breadcrumbs.first().expect("wide breadcrumb");

    assert_eq!(breadcrumb.area.width, 7);
    assert_eq!(
        layout.hit_test(breadcrumb.area.right().saturating_sub(1), breadcrumb.area.y,),
        Some(ExplorerHitTarget::Breadcrumb(0)),
    );
}

#[test]
fn explorer_layout_keeps_focused_entry_visible_and_adds_scrollbar() {
    let entries = (0..20)
        .map(|index| ExplorerEntryViewModel {
            name: format!("file{index}.txt"),
            kind: "File".to_string(),
            size: Some(format!("{index} B")),
            modified: None,
            attributes: Vec::new(),
            selected: index == 15,
        })
        .collect();
    let model = ExplorerViewModel::new("/tmp", entries, Some(15));
    let layout = explorer_layout(Rect::new(0, 0, 80, 12), &model);

    let scrollbar = layout.scrollbar.expect("overflowing Explorer scrollbar");
    assert_eq!(
        layout.hit_test(scrollbar.thumb.x, scrollbar.thumb.y),
        Some(ExplorerHitTarget::Scrollbar)
    );
    assert!(layout.visible_start > 0);
    assert!(layout.rows.iter().any(|row| row.index == 15));
}

#[test]
fn explorer_toolbar_keeps_every_action_at_supported_widths() {
    let model = sample_model();
    for width in [72, 95, 96, 110] {
        let layout = explorer_layout(Rect::new(0, 0, width, 20), &model);
        assert_eq!(
            layout
                .toolbar_buttons
                .iter()
                .map(|button| button.action)
                .collect::<Vec<_>>(),
            ExplorerToolbarAction::REGULAR,
            "toolbar actions at width {width}"
        );
    }
}

#[test]
fn explorer_toolbar_renders_actual_shortcuts_at_narrow_and_wide_widths() {
    let model = sample_model();
    for width in [72, 110, 200] {
        let mut terminal = Terminal::new(TestBackend::new(width, 32)).unwrap();
        terminal
            .draw(|frame| {
                render_explorer(
                    frame,
                    frame.area(),
                    &chrome_for("Explorer"),
                    &model,
                    &TundraTheme::default_dark(),
                );
            })
            .unwrap();
        let ShellLayout::Full { main, .. } = compute_shell_layout(Rect::new(0, 0, width, 32))
        else {
            panic!("full shell layout");
        };
        let layout = explorer_layout(main, &model);
        assert_eq!(
            layout.toolbar_buttons.len(),
            ExplorerToolbarAction::REGULAR.len()
        );
        for button in &layout.toolbar_buttons {
            let text: String = (button.area.x..button.area.right())
                .map(|x| {
                    terminal
                        .backend()
                        .buffer()
                        .cell((x, button.area.y))
                        .unwrap()
                        .symbol()
                })
                .collect();
            assert!(
                text.contains(button.action.shortcut_label()),
                "width {width}: {text}"
            );
            if width == 200 {
                assert!(button.show_label);
                assert!(text.contains(&button.action.label()), "{text}");
            }
            assert_eq!(
                layout.hit_test(button.area.x, button.area.y),
                button
                    .enabled
                    .then_some(ExplorerHitTarget::Toolbar(button.action))
            );
        }
    }
}

#[test]
fn explorer_explicit_wheel_viewport_does_not_snap_to_focus() {
    let entries = (0..20)
        .map(|index| ExplorerEntryViewModel {
            name: format!("file{index}.txt"),
            kind: "File".to_string(),
            size: None,
            modified: None,
            attributes: Vec::new(),
            selected: index == 0,
        })
        .collect();
    let mut model = ExplorerViewModel::new("/tmp", entries, Some(0));
    model.viewport_offset = 8;
    model.viewport_follows_focus = false;

    let layout = explorer_layout(Rect::new(0, 0, 80, 12), &model);
    assert_eq!(layout.visible_start, 8);
    assert!(!layout.rows.iter().any(|row| row.index == 0));
}

#[test]
fn explorer_context_menu_is_modal_and_hit_testable() {
    let mut model = sample_model();
    model.overlay = Some(ExplorerOverlayViewModel::ContextMenu(
        ExplorerContextMenuViewModel {
            x: 20,
            y: 8,
            title: "File".to_string(),
            items: vec![ExplorerContextMenuItemViewModel {
                id: "open".to_string(),
                label: "Open".to_string(),
                shortcut: Some("Enter".to_string()),
                enabled: true,
                dangerous: false,
            }],
            selected_index: Some(0),
        },
    ));
    let layout = explorer_layout(Rect::new(0, 0, 110, 28), &model);
    let overlay = layout.overlay.as_ref().expect("context overlay");
    let item = overlay.controls.first().expect("context item");

    assert_eq!(
        layout.hit_test(item.area.x, item.area.y),
        Some(ExplorerHitTarget::Overlay(
            ExplorerOverlayControl::ContextItem(0)
        ))
    );
    assert_eq!(layout.hit_test(0, 0), None);

    let output = render_output(&model);
    assert!(output.contains("File"));
    assert!(output.contains("Open"));
    assert!(output.contains("Enter"));
}

#[test]
fn explorer_wide_context_labels_and_shortcuts_use_cell_width_hit_geometry() {
    let mut model = sample_model();
    model.overlay = Some(ExplorerOverlayViewModel::ContextMenu(
        ExplorerContextMenuViewModel {
            x: 1,
            y: 1,
            title: "Wide".to_string(),
            items: vec![ExplorerContextMenuItemViewModel {
                id: "wide".to_string(),
                label: "打开🙂".to_string(),
                shortcut: Some("确认界面".to_string()),
                enabled: true,
                dangerous: false,
            }],
            selected_index: Some(0),
        },
    ));
    let layout = explorer_layout(Rect::new(0, 0, 80, 24), &model);
    let overlay = layout.overlay.as_ref().expect("wide context overlay");
    assert!(overlay.area.width >= 20);
    let row = overlay.controls[0].area;
    assert_eq!(
        layout.hit_test(row.x.saturating_add(row.width - 1), row.y),
        Some(ExplorerHitTarget::Overlay(
            ExplorerOverlayControl::ContextItem(0)
        ))
    );
}

#[test]
fn explorer_name_dialog_renders_clickable_input_and_actions() {
    let mut model = sample_model();
    model.overlay = Some(ExplorerOverlayViewModel::Name(
        ExplorerNameDialogViewModel {
            kind: ExplorerNameDialogKind::Rename,
            title: "Rename".to_string(),
            prompt: "Enter a new name".to_string(),
            value: "README-new.md".to_string(),
            error: None,
            confirm_label: "Save".to_string(),
            cancel_label: "Cancel".to_string(),
        },
    ));
    let layout = explorer_layout(Rect::new(0, 0, 110, 28), &model);
    let controls = &layout.overlay.as_ref().expect("rename dialog").controls;

    assert!(
        controls
            .iter()
            .any(|control| control.control == ExplorerOverlayControl::NameInput)
    );
    assert!(
        controls
            .iter()
            .any(|control| control.control == ExplorerOverlayControl::Confirm)
    );
    assert!(
        controls
            .iter()
            .any(|control| control.control == ExplorerOverlayControl::Cancel)
    );

    let output = render_output(&model);
    assert!(output.contains("Rename"));
    assert!(output.contains("Enter a new name"));
    assert!(output.contains("> README-new.md_"));
    assert!(output.contains("Save"));
}

#[test]
fn explorer_progress_prefers_byte_percent_and_is_bounded() {
    let progress = ExplorerOperationProgressViewModel {
        phase: ExplorerProgressStage::Moving,
        label: "Moving".to_string(),
        completed_items: 10,
        total_items: Some(20),
        completed_bytes: 125,
        total_bytes: Some(100),
        cancellable: true,
        cancel_label: "Cancel".to_string(),
    };

    assert_eq!(progress.percent(), Some(100));
}

fn sample_model() -> ExplorerViewModel {
    ExplorerViewModel::new(
        "/Users/strix/projects",
        vec![
            ExplorerEntryViewModel {
                name: "src".to_string(),
                kind: "Directory".to_string(),
                size: None,
                modified: None,
                attributes: Vec::new(),
                selected: false,
            },
            ExplorerEntryViewModel {
                name: "README.md".to_string(),
                kind: "File".to_string(),
                size: Some("1.2 KB".to_string()),
                modified: Some("2026-07-02 10:15".to_string()),
                attributes: vec!["readonly".to_string()],
                selected: true,
            },
        ],
        Some(1),
    )
}

fn chrome_for(screen: &str) -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::User,
        terminal_size: (110, 32),
        back_button_hovered: false,
        screen_stack: vec![screen.to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            alert_tone: ui::NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    }
}

fn render_output(model: &ExplorerViewModel) -> String {
    terminal_output(&render_terminal(model))
}

fn render_terminal(model: &ExplorerViewModel) -> Terminal<TestBackend> {
    let chrome = chrome_for("Explorer");
    let mut terminal = Terminal::new(TestBackend::new(110, 32)).expect("test terminal");
    terminal
        .draw(|frame| {
            render_explorer(
                frame,
                frame.area(),
                &chrome,
                model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render explorer");
    terminal
}

fn overlay_control_area(model: &ExplorerViewModel, target: &ExplorerOverlayControl) -> Rect {
    let ShellLayout::Full { main, .. } = compute_shell_layout(Rect::new(0, 0, 110, 32)) else {
        panic!("test terminal must use the full shell layout");
    };
    explorer_layout(main, model)
        .overlay
        .expect("overlay layout")
        .controls
        .iter()
        .find(|control| &control.control == target)
        .expect("overlay control")
        .area
}

#[test]
fn conflict_focus_highlights_exactly_one_control_even_when_apply_is_enabled() {
    use ratatui::style::Modifier;
    let mut model = sample_model();
    let choices = vec![
        ExplorerConflictChoice::KeepBoth,
        ExplorerConflictChoice::Replace,
        ExplorerConflictChoice::Skip,
        ExplorerConflictChoice::Cancel,
    ];
    for selection in 0..=choices.len() {
        model.overlay_selection = selection;
        model.overlay = Some(ExplorerOverlayViewModel::Conflict(
            ExplorerConflictViewModel {
                title: "Name conflict".into(),
                source: "/source".into(),
                destination: "/destination".into(),
                selected_choice: choices
                    .get(selection)
                    .copied()
                    .unwrap_or(ExplorerConflictChoice::KeepBoth),
                choices: choices.clone(),
                apply_to_remaining: true,
                allow_apply_to_remaining: true,
            },
        ));
        let terminal = render_terminal(&model);
        let controls = choices
            .iter()
            .copied()
            .map(ExplorerOverlayControl::ConflictChoice)
            .chain(std::iter::once(ExplorerOverlayControl::ApplyToRemaining));
        for (index, control) in controls.enumerate() {
            let area = overlay_control_area(&model, &control);
            let cell = (area.x..area.right())
                .map(|x| &terminal.backend().buffer()[(x, area.y)])
                .find(|cell| !cell.symbol().trim().is_empty())
                .unwrap();
            assert_eq!(
                cell.modifier.contains(Modifier::BOLD),
                index == selection,
                "{control:?}, focus={selection}"
            );
        }
    }
}
