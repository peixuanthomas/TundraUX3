use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};
use ui::automation::{FrameCapture, Snapshot};
use ui::components::{
    Button, ContextMenu, ContextMenuItem, DataTable, List, ListItem, Scrollbar, Surface, TabItem,
    Tabs, TextInput,
};
use ui::{RenderContext, TundraTheme};

fn find<'a>(snapshot: &'a Snapshot, suffix: &str) -> &'a shell_control::Node {
    snapshot
        .nodes
        .iter()
        .find(|node| node.id.ends_with(suffix))
        .unwrap_or_else(|| panic!("missing {suffix}: {:?}", snapshot.nodes))
}

fn assert_secret_absent(snapshot: &Snapshot, secret: &str) {
    assert!(!snapshot.lines.join("\n").contains(secret));
    for node in &snapshot.nodes {
        assert!(!node.name.contains(secret), "name leaked in {}", node.id);
        assert!(!node.text.contains(secret), "text leaked in {}", node.id);
    }
}

#[test]
fn shared_controls_keep_ids_state_and_hierarchy_when_labels_change() {
    fn capture(label: &str) -> Snapshot {
        let bounds = Rect::new(0, 0, 80, 24);
        let capture = FrameCapture::begin(bounds, "components");
        capture.set_group("page", "page", bounds, false);
        let context = RenderContext::default();
        let theme = context.compatibility_theme();
        let mut buffer = Buffer::empty(bounds);
        Surface::new()
            .titled(label)
            .bordered(true)
            .render(bounds, &mut buffer, &context);
        let mut button = Button::new("save", label).with_current(true);
        button.set_focused(true);
        button.render(Rect::new(2, 2, 12, 3), &mut buffer, &theme);
        let mut disabled = Button::new("delete", label);
        disabled.set_disabled(true);
        disabled.render(Rect::new(16, 2, 12, 3), &mut buffer, &theme);
        let mut input = TextInput::new("query").with_placeholder(label);
        input.set_value("中文A");
        input.set_focused(true);
        input.render(Rect::new(2, 6, 24, 3), &mut buffer, &theme);
        let mut list = List::new(
            "choices",
            vec![
                ListItem::new("first", label),
                ListItem::new("second", label).disabled(true),
            ],
        );
        list.set_focused(true);
        list.render(Rect::new(30, 2, 20, 6), &mut buffer, &theme);
        let mut table = DataTable::new("rows", [label, "Size"], [["alpha", "2"], ["beta", "3"]]);
        table.state.focused = true;
        table.render(Rect::new(2, 10, 30, 5), &mut buffer, &context);
        Tabs::new(
            "views",
            vec![TabItem::new("files", label), TabItem::new("details", label)],
        )
        .render(Rect::new(35, 10, 35, 3), &mut buffer, &theme);
        Scrollbar::new(100, 10, 20).render(Rect::new(77, 2, 1, 15), &mut buffer, &context);
        capture.finish(&buffer)
    }
    let english = capture("Save");
    let chinese = capture("保存");
    let ids = |snapshot: &Snapshot| {
        snapshot
            .nodes
            .iter()
            .filter(|node| node.role != "text")
            .map(|node| node.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&english), ids(&chinese));
    assert!(find(&chinese, "/save").selected);
    assert!(find(&chinese, "/save").focused);
    assert_eq!(find(&chinese, "/save").name, "保存");
    assert!(!find(&chinese, "/delete").enabled);
    assert!(!find(&chinese, "/delete").actionable);
    assert!(find(&chinese, "/delete").actions.is_empty());
    assert!(find(&chinese, "/query").text.contains("中文A"));
    assert!(!find(&chinese, "/query").text.contains("中 文 A"));
    assert!(find(&chinese, "/choices/first").focused);
    assert!(!find(&chinese, "/choices/second").actionable);
    assert_eq!(find(&chinese, "/rows/row.0/cell.0").text, "alpha");
    assert_eq!(
        find(&chinese, "/rows/row.0/cell.0").parent.as_deref(),
        Some("page/rows/row.0")
    );
    assert!(find(&chinese, "/views/files").selected);
    assert!(
        find(&chinese, "/scrollbar.vertical/thumb")
            .actions
            .iter()
            .any(|action| action == "drag")
    );
    for node in &chinese.nodes {
        if let Some(parent) = &node.parent {
            assert!(
                chinese
                    .nodes
                    .iter()
                    .any(|candidate| &candidate.id == parent),
                "missing {parent}"
            );
        }
    }
}

#[test]
fn blocked_background_stays_readable_but_only_menu_items_can_act() {
    let bounds = Rect::new(0, 0, 60, 20);
    let capture = FrameCapture::begin(bounds, "home");
    let mut buffer = Buffer::empty(bounds);
    let theme = TundraTheme::default();
    capture.set_group("page", "page", bounds, true);
    let mut background = Button::new("delete", "Delete");
    background.set_focused(true);
    background.render(Rect::new(1, 1, 12, 3), &mut buffer, &theme);
    capture.set_group("menu", "dialog", Rect::new(20, 4, 20, 6), false);
    let mut menu = ContextMenu::new("more", vec![ContextMenuItem::new("open", "Open")]);
    menu.open();
    menu.render(Rect::new(20, 4, 20, 6), &mut buffer, &theme);
    let snapshot = capture.finish(&buffer);
    let background = find(&snapshot, "/delete");
    assert!(!background.actionable && !background.focused);
    assert!(background.enabled);
    assert!(background.text.contains("Delete"));
    let item = find(&snapshot, "/more/more.items/open");
    assert!(item.actionable && item.focused);
}

#[test]
fn canvas_projection_clips_rows_and_disables_partly_visible_actions() {
    let bounds = Rect::new(0, 0, 40, 20);
    let capture = FrameCapture::begin(bounds, "setup");
    capture.set_group("page", "page", bounds, false);
    let mut virtual_buffer = Buffer::empty(Rect::new(0, 0, 30, 30));
    let theme = TundraTheme::default();
    {
        let _projection = ui::automation::transform(5, -3, Rect::new(5, 4, 25, 8));
        Button::new("visible", "Visible").render(
            Rect::new(2, 8, 10, 3),
            &mut virtual_buffer,
            &theme,
        );
        Button::new("partial", "Partial").render(
            Rect::new(2, 6, 10, 3),
            &mut virtual_buffer,
            &theme,
        );
        Button::new("hidden", "Hidden").render(Rect::new(2, 1, 10, 3), &mut virtual_buffer, &theme);
    }
    let snapshot = capture.finish(&Buffer::empty(bounds));
    let visible = find(&snapshot, "/visible");
    assert_eq!(
        visible.rect,
        shell_control::Rect {
            x: 7,
            y: 5,
            width: 10,
            height: 3
        }
    );
    assert!(visible.actionable);
    assert!(!find(&snapshot, "/partial").actionable);
    assert!(
        !snapshot
            .nodes
            .iter()
            .any(|node| node.id.ends_with("/hidden"))
    );
}

#[test]
fn login_reveal_exports_no_password_and_uses_the_scrolled_screen_coordinates() {
    let secret = "Secret中文42";
    let bounds = Rect::new(0, 0, 108, 22);
    for (main, offset) in [(Rect::new(3, 4, 100, 16), 0), (Rect::new(3, 5, 100, 10), 5)] {
        let mut login = ui::LoginViewModel::new(
            vec![ui::LoginUserOptionViewModel {
                username: "alice".into(),
                display_name: "Alice".into(),
                role: "admin".into(),
                enabled: true,
                locked: false,
            }],
            0,
            0,
            secret.chars().count(),
            ui::LoginField::Password,
            None,
        )
        .with_visible_password(secret);
        login.scroll_offset = offset;
        let viewport = ui::login_viewport(main, &login);
        let logical = ui::login_layout(viewport.content).password;
        let expected = viewport
            .project(Rect::new(
                logical.x + 1,
                logical.y + 1,
                logical.width - 2,
                1,
            ))
            .unwrap();
        let capture = FrameCapture::begin(bounds, "login");
        capture.set_group("page", "page", main, false);
        let mut terminal = Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
        terminal
            .draw(|frame| ui::render_login_content(frame, main, &login, &RenderContext::default()))
            .unwrap();
        let snapshot = capture.finish(terminal.backend().buffer());
        let input = find(&snapshot, "/login.password");
        assert_eq!(
            (input.rect.x, input.rect.y, input.rect.width),
            (expected.x, expected.y, expected.width)
        );
        assert!(input.focused && input.sensitive && input.actionable);
        assert_eq!(input.text, "••••");
        assert_secret_absent(&snapshot, secret);
        assert_eq!(
            terminal.backend().buffer()[(expected.x, expected.y)].symbol(),
            "S"
        );
    }
}

#[test]
fn management_password_and_actual_table_cells_are_exported() {
    let bounds = Rect::new(0, 0, 120, 40);
    let secret = "never-export-this";
    let model = ui::ManagementViewModel {
        scope_id: "users".into(),
        title: "Users".into(),
        columns: vec!["Name".into(), "ID".into()],
        rows: vec![vec!["alice".into(), "1000".into()]],
        form: Some(ui::ManagementForm {
            identity: "create-user".into(),
            title: "Create user".into(),
            fields: vec![ui::ManagementFormField {
                id: "password".into(),
                label: "Password".into(),
                value: secret.into(),
                secret: true,
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let capture = FrameCapture::begin(bounds, "users");
    let mut terminal = Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
    terminal
        .draw(|frame| {
            capture.set_group("page", "page", bounds, true);
            ui::render_management_content(frame, bounds, &model, &RenderContext::default());
            capture.set_group("form", "dialog", bounds, false);
            ui::render_management_overlay(frame, bounds, &model, &RenderContext::default());
        })
        .unwrap();
    let snapshot = capture.finish(terminal.backend().buffer());
    assert_secret_absent(&snapshot, secret);
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.role == "cell" && node.name == "alice")
    );
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.role == "textbox" && node.sensitive && node.focused)
    );
    assert!(!find(&snapshot, "/management.rows.users/row.0/select").actionable);
}

#[test]
fn aa_secret_is_hidden_while_real_terminal_text_and_wide_characters_remain_visible() {
    let secret = "aa-secret-password";
    let bounds = Rect::new(0, 0, 120, 40);
    let mut terminal_output = ui::CommandLineTerminalSnapshot::blank(60, 4);
    terminal_output.cells[0].symbol = "中".into();
    terminal_output.cells[2].symbol = "文".into();
    terminal_output.cells[4].symbol = "A".into();
    let model = ui::AutoAdminViewModel {
        description: "Run task".into(),
        status: "Waiting".into(),
        confirming: false,
        finished: false,
        approve_selected: false,
        button_focus: None,
        scroll: 0,
        input: Some(secret.into()),
        input_sensitive: true,
        terminal: std::sync::Arc::new(terminal_output),
        stop: Default::default(),
        can_kill: false,
        can_stop: true,
    };
    let capture = FrameCapture::begin(bounds, "users");
    capture.set_group("authorization", "dialog", bounds, false);
    let mut terminal = Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
    terminal
        .draw(|frame| ui::render_auto_admin(frame, bounds, &model, &RenderContext::default()))
        .unwrap();
    let snapshot = capture.finish(terminal.backend().buffer());
    assert_secret_absent(&snapshot, secret);
    assert!(find(&snapshot, "/aa/aa.input").sensitive);
    assert!(find(&snapshot, "/aa/terminal").text.contains("中文A"));
    assert!(snapshot.lines.iter().any(|line| line.contains("中文A")));
    assert!(!snapshot.lines.iter().any(|line| line.contains("中 文 A")));
}

#[test]
fn native_vim_canvas_exports_its_visible_terminal_and_obeys_group_blocking() {
    fn write_line(snapshot: &mut ui::CommandLineTerminalSnapshot, row: u16, text: &str) {
        let mut column = 0;
        for character in text.chars() {
            snapshot.set_cell(
                column,
                row,
                ui::CommandLineCell {
                    symbol: character.to_string(),
                    ..Default::default()
                },
            );
            column += unicode_width::UnicodeWidthChar::width(character).unwrap_or(0) as u16;
        }
    }

    let bounds = Rect::new(0, 0, 80, 24);
    let area = Rect::new(3, 2, 60, 18);
    let canvas = ui::editor_vim_terminal_area(area);
    let mut output = ui::CommandLineTerminalSnapshot::blank(canvas.width + 8, canvas.height + 1);
    write_line(&mut output, 0, "中文A native Vim");
    write_line(&mut output, 1, "E486: Pattern not found: missing");
    write_line(
        &mut output,
        canvas.height - 2,
        "-- INSERT --                 3,5",
    );
    write_line(&mut output, canvas.height - 1, ":%s/old/new/g");
    write_line(&mut output, canvas.height, "hidden terminal row");
    output.set_cell(
        canvas.width,
        0,
        ui::CommandLineCell {
            symbol: "X".into(),
            ..Default::default()
        },
    );
    let cursor_column = 12;
    let cursor_row = canvas.height - 1;
    let cursor_index =
        usize::from(cursor_row) * usize::from(output.columns) + usize::from(cursor_column);
    output.cells[cursor_index].cursor = true;
    let mut model = ui::EditorViewModel::source("host.txt", "hidden host document");
    model.status_message = Some("hidden host status".into());
    model.vim_terminal = Some(std::sync::Arc::new(output));

    for blocked in [false, true] {
        let capture = FrameCapture::begin(bounds, "editor");
        capture.set_group("page", "page", area, blocked);
        let mut terminal = Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
        let mut layout = None;
        terminal
            .draw(|frame| {
                layout = Some(ui::render_editor_contextual(
                    frame,
                    area,
                    &model,
                    &RenderContext::default(),
                ));
            })
            .unwrap();
        let snapshot = capture.finish(terminal.backend().buffer());
        let native = find(&snapshot, "/terminal");
        assert_eq!(native.id, "page/terminal");
        assert_eq!(native.role, "terminal");
        assert_eq!(
            (
                native.rect.x,
                native.rect.y,
                native.rect.width,
                native.rect.height
            ),
            (canvas.x, canvas.y, canvas.width, canvas.height)
        );
        assert!(native.enabled);
        assert_eq!(native.focused, !blocked);
        assert_eq!(native.actionable, !blocked);
        assert_eq!(
            native.actions,
            if blocked {
                Vec::<String>::new()
            } else {
                ["click", "text", "key", "scroll"]
                    .map(String::from)
                    .to_vec()
            }
        );
        for text in [
            "中文A native Vim",
            "E486: Pattern not found: missing",
            "-- INSERT --",
            ":%s/old/new/g",
        ] {
            assert!(
                native.text.contains(text),
                "missing {text}: {}",
                native.text
            );
            assert!(snapshot.lines.iter().any(|line| line.contains(text)));
        }
        assert_eq!(
            native.text.lines().nth(1),
            Some("E486: Pattern not found: missing")
        );
        for hidden in [
            "中 文 A",
            "hidden terminal row",
            "hidden host document",
            "hidden host status",
        ] {
            assert!(!snapshot.lines.join("\n").contains(hidden));
        }
        assert!(!native.text.contains('X'));
        assert!(
            !snapshot
                .nodes
                .iter()
                .any(|node| node.id.ends_with("/editor.document"))
        );
        let toggle = find(&snapshot, "/editor.vim-toggle");
        assert!(toggle.text.contains("F8"));
        assert_eq!(toggle.actionable, !blocked);
        assert!(toggle.rect.y + toggle.rect.height <= native.rect.y);
        assert_eq!(layout.unwrap().canvas, canvas);
        assert_eq!(
            terminal.get_cursor_position().unwrap(),
            (canvas.x + cursor_column, canvas.y + cursor_row).into()
        );
    }
}

#[test]
fn ending_or_aborting_a_frame_stops_capture() {
    assert!(!ui::automation::enabled());
    let frame = FrameCapture::begin(Rect::new(0, 0, 2, 2), "home");
    assert!(ui::automation::enabled());
    drop(frame);
    assert!(!ui::automation::enabled());
}

#[test]
fn settings_checkbox_reports_the_saved_value_separately_from_focus() {
    let bounds = Rect::new(0, 0, 120, 40);
    for (label, checked) in [("已开启", true), ("Disabled", false)] {
        let model = ui::SettingsViewModel {
            selected_application: Default::default(),
            selected_category: ui::SettingsCategory::System,
            selected_field: ui::SettingsField::AutomationEnabled,
            cards: vec![ui::SettingsCardViewModel::new(
                "Control",
                vec![
                    ui::SettingsItemViewModel::new(
                        ui::SettingsField::AutomationEnabled,
                        "Allow external control",
                        label,
                        "",
                        ui::SettingsControlKind::Toggle,
                    )
                    .checked(checked),
                ],
            )],
            appearance_preview: None,
            status: String::new(),
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
        let capture = FrameCapture::begin(bounds, "settings");
        capture.set_group("page", "page", bounds, false);
        let mut terminal = Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
        terminal
            .draw(|frame| {
                ui::render_settings_content(
                    frame,
                    &ui::settings_layout(bounds, &model),
                    &model,
                    &RenderContext::default(),
                )
            })
            .unwrap();
        let snapshot = capture.finish(terminal.backend().buffer());
        let checkbox = find(&snapshot, "/settings.field.automationenabled");
        assert_eq!(checkbox.role, "checkbox");
        assert_eq!(checkbox.name, "Allow external control");
        assert_eq!(checkbox.selected, checked);
        assert!(checkbox.focused);
    }
}

#[test]
fn graphics_protocol_bytes_are_not_exported_as_visible_text() {
    let bounds = Rect::new(0, 0, 12, 2);
    let capture = FrameCapture::begin(bounds, "editor");
    let mut buffer = Buffer::empty(bounds);
    buffer[(0, 0)]
        .set_symbol("\x1b_GAAAA\x1b\\")
        .set_diff_option(ratatui::buffer::CellDiffOption::ForcedWidth(
            std::num::NonZeroU16::new(5).unwrap(),
        ));
    buffer[(5, 0)].set_symbol("Z");
    let snapshot = capture.finish(&buffer);
    assert_eq!(snapshot.lines[0], "     Z");
    for node in &snapshot.nodes {
        assert!(!node.name.contains('\u{1b}') && !node.text.contains('\u{1b}'));
    }
}
