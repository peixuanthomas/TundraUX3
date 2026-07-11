use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use tundra_ui::{
    ExplorerBreadcrumbViewModel, ExplorerConflictChoice, ExplorerConflictViewModel,
    ExplorerContextMenuItemViewModel, ExplorerContextMenuViewModel, ExplorerDialogViewModel,
    ExplorerEntryViewModel, ExplorerHitTarget, ExplorerLayoutMode, ExplorerNameDialogKind,
    ExplorerNameDialogViewModel, ExplorerOperationPhase, ExplorerOperationProgressViewModel,
    ExplorerOptionViewModel, ExplorerOptionsViewModel, ExplorerOverlayControl,
    ExplorerOverlayViewModel, ExplorerPropertiesViewModel, ExplorerPropertyViewModel,
    ExplorerQuickLocationViewModel, ExplorerSearchViewModel, ExplorerSortColumn,
    ExplorerToolbarAction, ExplorerViewModel, HomeDisplayMode, NotificationTone,
    ShellChromeViewModel, StatusViewModel, TundraTheme, explorer_first_entry_content_line,
    explorer_layout, render_explorer,
};

#[test]
fn explorer_renderer_shows_path_entries_details_search_and_message() {
    let mut model = ExplorerViewModel::new(
        "/Users/strix/projects",
        vec![
            ExplorerEntryViewModel {
                name: "src".to_string(),
                kind: "Directory".to_string(),
                size: None,
                modified: Some("2026-07-02 09:10".to_string()),
                attributes: vec!["hidden".to_string()],
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
    );
    model.search = Some(ExplorerSearchViewModel::new("read", true, Some(1)));
    model.show_hidden = true;
    model.message = Some("Copied README.md".to_string());

    let output = render_output(&model);

    assert!(output.contains("Path: /Users/strix/projects"));
    assert!(output.contains("Hidden files: shown"));
    assert!(output.contains("Search: read (1 match, active)"));
    assert!(output.contains("[+] src"));
    assert!(output.contains("Directory"));
    assert!(output.contains("[T] README.md"));
    assert!(output.contains("1.2 KB"));
    assert!(output.contains("Selected: README.md"));
    assert!(output.contains("Name: README.md"));
    assert!(output.contains("Type: File"));
    assert!(output.contains("Size: 1.2 KB"));
    assert!(output.contains("Modified: 2026-07-02 10:15"));
    assert!(output.contains("Attributes: readonly"));
    assert!(output.contains("Copied README.md"));
    assert!(output.contains("Explorer"));
    assert!(output.contains("Ready"));
    assert!(output.contains("TundraUX 3"));
    assert!(output.contains("Enter: open"));
    assert!(output.contains("Backspace: parent"));
    assert!(output.contains("/: search"));
}

#[test]
fn explorer_renderer_shows_pending_confirmation_dialog() {
    let mut model = sample_model();
    model.pending_dialog = Some(ExplorerDialogViewModel::new(
        "Delete File",
        "Delete README.md?",
        "Enter: delete",
        "Esc: cancel",
    ));

    let output = render_output(&model);

    assert!(output.contains("Delete File"));
    assert!(output.contains("Delete README.md?"));
    assert!(output.contains("Enter: delete"));
    assert!(output.contains("Esc: cancel"));
}

#[test]
fn explorer_renderer_shows_error() {
    let mut model = sample_model();
    model.error = Some("Permission denied: README.md".to_string());

    let output = render_output(&model);

    assert!(output.contains("Error: Permission denied: README.md"));
}

#[test]
fn explorer_view_model_returns_selected_entry() {
    let model = sample_model();

    assert_eq!(
        model.selected_entry().map(|entry| entry.name.as_str()),
        Some("README.md")
    );
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
fn compact_explorer_shows_highest_priority_notification() {
    let model = sample_model();
    let mut chrome = chrome_for("Explorer");
    chrome.terminal_size = (49, 11);
    chrome.status = StatusViewModel {
        status: "Compact status".to_string(),
        toast: Some("Compact toast".to_string()),
        error: Some("Explorer alert".to_string()),
        alert_tone: NotificationTone::Critical,
        time_button_label: None,
        time_button_selected: false,
    };
    let mut terminal = Terminal::new(TestBackend::new(49, 11)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_explorer(
                frame,
                frame.area(),
                &chrome,
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render compact explorer");

    let output = terminal_output(&terminal);
    assert!(output.contains("[CRITICAL] Explorer alert"));
    assert!(!output.contains("Compact toast"));
    assert!(!output.contains("Compact status"));
}

#[test]
fn explorer_layout_uses_the_documented_responsive_breakpoints() {
    let mut model = sample_model();
    model.quick_locations = vec![ExplorerQuickLocationViewModel::new(
        "desktop",
        "Desktop",
        "/Users/strix/Desktop",
        "desktop",
    )];

    let wide = explorer_layout(Rect::new(0, 0, 96, 24), &model);
    assert_eq!(wide.mode, ExplorerLayoutMode::DetailedWithSidebar);
    assert_eq!(wide.sidebar.expect("wide sidebar").width, 20);
    assert_eq!(wide.columns.len(), 4);

    let detailed = explorer_layout(Rect::new(0, 0, 95, 24), &model);
    assert_eq!(detailed.mode, ExplorerLayoutMode::Detailed);
    assert!(detailed.sidebar.is_none());
    assert_eq!(detailed.columns.len(), 4);

    let compact = explorer_layout(Rect::new(0, 0, 71, 24), &model);
    assert_eq!(compact.mode, ExplorerLayoutMode::Compact);
    assert!(compact.sidebar.is_none());
    assert_eq!(
        compact
            .columns
            .iter()
            .map(|column| column.column)
            .collect::<Vec<_>>(),
        vec![ExplorerSortColumn::Name, ExplorerSortColumn::Type]
    );
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
        phase: ExplorerOperationPhase::Copying,
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

    assert!(layout.scrollbar.is_some());
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
            ExplorerToolbarAction::ALL,
            "toolbar actions at width {width}"
        );
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
    assert!(output.contains("README-new.md"));
    assert!(output.contains("Save"));
}

#[test]
fn explorer_progress_prefers_byte_percent_and_is_bounded() {
    let progress = ExplorerOperationProgressViewModel {
        phase: ExplorerOperationPhase::Moving,
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

#[test]
fn explorer_advanced_options_conflict_and_properties_overlays_render() {
    let mut model = sample_model();
    model.overlay = Some(ExplorerOverlayViewModel::Options(
        ExplorerOptionsViewModel {
            title: "Advanced options".to_string(),
            options: vec![ExplorerOptionViewModel {
                id: "hidden".to_string(),
                label: "Show hidden".to_string(),
                value: "Off".to_string(),
                enabled: true,
                selected: true,
                focused: true,
            }],
            close_label: "Done".to_string(),
        },
    ));
    let options = render_output(&model);
    assert!(options.contains("Advanced options"));
    assert!(options.contains("Show hidden: Off"));
    assert!(options.contains("Done"));

    model.overlay = Some(ExplorerOverlayViewModel::Conflict(
        ExplorerConflictViewModel {
            title: "Name conflict".to_string(),
            source: "README.md".to_string(),
            destination: "/tmp/README.md".to_string(),
            choices: vec![
                ExplorerConflictChoice::KeepBoth,
                ExplorerConflictChoice::Replace,
                ExplorerConflictChoice::Skip,
                ExplorerConflictChoice::Cancel,
            ],
            selected_choice: ExplorerConflictChoice::KeepBoth,
            apply_to_remaining: true,
        },
    ));
    let conflict = render_output(&model);
    assert!(conflict.contains("Name conflict"));
    assert!(conflict.contains("README.md"));
    assert!(conflict.contains("Keep both"));
    assert!(conflict.contains("Apply to remaining"));

    model.overlay = Some(ExplorerOverlayViewModel::Properties(
        ExplorerPropertiesViewModel {
            title: "Properties".to_string(),
            properties: vec![ExplorerPropertyViewModel {
                label: "Type".to_string(),
                value: "Markdown file".to_string(),
            }],
            close_label: "Close".to_string(),
        },
    ));
    let properties = render_output(&model);
    assert!(properties.contains("Properties"));
    assert!(properties.contains("Type: Markdown file"));
    assert!(properties.contains("Close"));
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
        screen_stack: vec![screen.to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            alert_tone: tundra_ui::NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    }
}

fn render_output(model: &ExplorerViewModel) -> String {
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
    terminal_output(&terminal)
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
