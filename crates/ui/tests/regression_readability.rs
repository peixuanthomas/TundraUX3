use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::*;
use unicode_width::UnicodeWidthStr;

fn context() -> RenderContext {
    RenderContext::from_theme(
        &TundraTheme::default_dark(),
        Default::default(),
        Default::default(),
    )
}

fn rectangle_text(terminal: &Terminal<TestBackend>, area: Rect) -> String {
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn process_summary_columns_fit_together_and_share_header_hit_regions() {
    let context = context();
    for columns in [
        [
            "Name", "PID", "CPU %", "Memory", "PPID", "UID", "State", "Nice",
        ],
        [
            "名称", "PID", "CPU %", "内存", "PPID", "UID", "状态", "Nice",
        ],
    ] {
        let model = ManagementViewModel {
            columns: columns.map(String::from).to_vec(),
            column_width_limits: vec![20, 8, 8, 12],
            rows: vec![vec![
                "database-worker-with-a-long-name".into(),
                "4194304".into(),
                "1200.5".into(),
                "12345.6 MiB".into(),
                "1".into(),
                "1000".into(),
                "S".into(),
                "0".into(),
            ]],
            ..Default::default()
        };
        for width in [108, 116, 140] {
            let bounds = Rect::new(0, 0, width, 41);
            let main = ShellFrameLayout::new(bounds, None, &context).main;
            let layout = management_layout(main, &model);
            let regions = management_button_regions(main, &model);
            for index in 0..4 {
                let (_, header) = layout
                    .headers
                    .iter()
                    .find(|(column, _)| *column == index)
                    .unwrap();
                assert_eq!(usize::from(header.width), layout.column_widths[index]);
                assert!(regions.iter().any(|region| region.area == *header));
            }
            let mut terminal = Terminal::new(TestBackend::new(width, 41)).unwrap();
            terminal
                .draw(|frame| render_management_content(frame, main, &model, &context))
                .unwrap();
            let output = rectangle_text(&terminal, layout.list_rows);
            for text in ["database-worker", "4194304", "1200.5", "12345.6 MiB"] {
                assert!(output.contains(text), "{width}: missing {text}: {output}");
            }
        }
    }
}

#[test]
fn management_details_wrap_words_and_keep_wide_text_within_the_viewport() {
    let graphemes = management_wrapped_lines("a 👩‍💻 e\u{301} b", 3);
    assert!(graphemes.iter().any(|line| line.contains("👩‍💻")));
    assert!(graphemes.iter().any(|line| line.contains("e\u{301}")));
    assert_eq!(
        management_wrapped_lines("alpha beta gamma", 10),
        ["alpha beta", "gamma"]
    );
    assert_eq!(
        management_wrapped_lines("short words\n  indented", 10),
        ["short", "words", "  indented"]
    );
    for width in [8, 12, 24] {
        let text = "Backend unavailable: permission denied. 中文说明无法访问 example.service";
        let lines = management_wrapped_lines(text, width);
        assert!(lines.iter().all(|line| line.width() <= usize::from(width)));
        assert_eq!(
            lines.join("").split_whitespace().collect::<String>(),
            text.split_whitespace().collect::<String>()
        );
    }
}

#[test]
fn backend_error_notifications_wrap_complete_words_and_keep_actions_accessible() {
    let mut model = NotificationViewModel::new(
        "backend-error",
        NotificationLevel::Modal,
        NotificationTone::Error,
        "Cannot read",
        "The backend rejected this operation because permission to read the configuration is unavailable.",
        vec![NotificationActionViewModel::new("close", "Close").with_shortcut("Esc")],
    );
    let area = Rect::new(0, 0, 50, 12);
    let NotificationLayout::Dialog(layout) = notification_layout(area, &model) else {
        panic!("error dialog must fit");
    };
    let mut rendered = String::new();
    for offset in 0..=layout.max_scroll_offset {
        model.scroll_offset = offset;
        let NotificationLayout::Dialog(layout) = notification_layout(area, &model) else {
            unreachable!()
        };
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| {
                render_notification_overlay(frame, area, &model, &TundraTheme::default_dark())
            })
            .unwrap();
        rendered.push_str(&rectangle_text(&terminal, layout.message));
        assert!(rectangle_text(&terminal, layout.actions[0].area).contains("Close"));
    }
    for word in [
        "operation",
        "because",
        "permission",
        "configuration",
        "unavailable",
    ] {
        assert!(
            rendered.contains(word),
            "word split across rows: {word}: {rendered}"
        );
    }
}

#[test]
fn logs_show_compact_time_in_rows_and_original_timestamp_in_details() {
    let mut model = LogsViewModel::default();
    model.diagnostics.can_view_details = true;
    model.can_view_system = true;
    model.events = vec![LogsEventViewModel {
        id: "event-1".into(),
        timestamp: "2026-10-19T07:34:56+00:00".into(),
        time_label: "15:34:56".into(),
        operation: "Read".into(),
        ..Default::default()
    }];
    let context = context();
    let main = ShellFrameLayout::new(Rect::new(0, 0, 116, 41), None, &context).main;
    let layout = logs_layout(main, &model);
    let mut terminal = Terminal::new(TestBackend::new(116, 41)).unwrap();
    terminal
        .draw(|frame| render_logs_content(frame, main, &model, &context))
        .unwrap();
    assert!(rectangle_text(&terminal, layout.content.list_rows_area).contains("15:34:56"));
    assert!(!rectangle_text(&terminal, layout.content.list_rows_area).contains("2026-10-19"));
    assert!(rectangle_text(&terminal, layout.detail_text).contains("2026-10-19T07:34:56+00:00"));
}

#[test]
fn system_and_update_settings_keep_errors_in_footer_without_description_pane() {
    let context = context();
    for category in [SettingsCategory::System, SettingsCategory::Update] {
        let mut model = SettingsViewModel {
            selected_application: Default::default(),
            selected_category: category,
            selected_field: SettingsField::SystemLowPercentage,
            cards: Vec::new(),
            appearance_preview: None,
            status: "Backend error: permission denied while reading configuration".into(),
            locked_message: None,
            scroll_offset: 0,
            animation_editor: None,
            number_editor: None,
            picker: None,
            color_editor: None,
            weather_location_editor: None,
            file_extensions_editor: None,
            time_sync_server_editor: None,
            update: None,
        };
        model.cards = vec![SettingsCardViewModel::new(
            "Limits",
            vec![SettingsItemViewModel::new(
                model.selected_field,
                "Warning",
                "15%",
                "Critical must be below warning. Keep at least 5 percent free for system operation.",
                SettingsControlKind::ReadOnly,
            )],
        )];
        for (height, needs_scroll) in [(7, true), (15, false)] {
            model.scroll_offset = 0;
            let area = Rect::new(0, 0, 60, height);
            let layout = settings_layout(area, &model);
            assert_eq!(layout.scrollbar.is_some(), needs_scroll);
            assert_eq!(layout.max_scroll_offset > 0, needs_scroll);
            let mut rendered = String::new();
            for offset in 0..=layout.max_scroll_offset {
                model.scroll_offset = offset;
                let layout = settings_layout(area, &model);
                let mut terminal =
                    Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
                terminal
                    .draw(|frame| render_settings_content(frame, &layout, &model, &context))
                    .unwrap();
                rendered.push_str(&rectangle_text(&terminal, layout.detail));
                let content = rectangle_text(&terminal, layout.workspace.content);
                assert!(
                    content.contains("Backend error: permission denied"),
                    "{content}"
                );
                assert!(
                    !content.contains("Critical must be below warning"),
                    "{content}"
                );
            }
            assert!(rendered.contains("Warning"), "{rendered}");
            assert!(rendered.contains("15%"), "{rendered}");
        }
    }
}
