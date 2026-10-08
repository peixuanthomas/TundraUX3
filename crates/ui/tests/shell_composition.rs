use ratatui::{Terminal, backend::TestBackend, layout::Rect, widgets::Paragraph};
use ui::*;

fn chrome(width: u16, height: u16) -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".into(),
        build_mode: "debug".into(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (width, height),
        back_button_hovered: false,
        back_shortcut: "Esc",
        screen_stack: vec!["中文页面路径很长需要正确截断".repeat(8)],
        status: StatusViewModel {
            status: "系统状态消息很长但不应覆盖时钟".repeat(8),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: Some("09:30".into()),
            time_button_selected: false,
        },
    }
}
fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
    (0..terminal.backend().buffer().area.width)
        .map(|x| terminal.backend().buffer()[(x, y)].symbol())
        .collect()
}

#[test]
fn back_button_shows_and_fits_the_actual_page_shortcut() {
    for size in [(50, 12), (108, 30), (40, 10)] {
        for shortcut in ["Esc", "Ctrl+Shift+X"] {
            let mut model = chrome(size.0, size.1);
            model.back_shortcut = shortcut;
            let (terminal, layout) = render(&model, &RenderContext::default());
            let back = layout.back_button.unwrap();
            let y = back.y + u16::from(back.height > 1);
            let label: String = (back.x..back.right())
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect();
            assert!(label.contains(&format!("[◀ {shortcut}]")), "{label}");
            assert!(back.intersection(layout.main).is_empty());
        }
    }
}
fn render(
    model: &ShellChromeViewModel,
    context: &RenderContext,
) -> (Terminal<TestBackend>, ShellFrameLayout) {
    let (width, height) = model.terminal_size;
    let layout = ShellFrameLayout::new(
        Rect::new(0, 0, width, height),
        model.status.time_button_label.as_deref(),
        context,
    )
    .with_back_shortcut(model.back_shortcut);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(Paragraph::new("PAGE CONTENT"), layout.main);
            render_shell_chrome(frame, &layout, model, context);
        })
        .unwrap();
    (terminal, layout)
}
#[test]
fn chrome_title_and_information_share_one_row_without_spilling_into_content() {
    for width in [50, 80, 120] {
        let (terminal, layout) = render(&chrome(width, 24), &RenderContext::default());
        let ShellLayout::Full { top, .. } = layout.shell else {
            panic!("full layout")
        };
        assert_eq!(top.height, 3);
        let text = row(&terminal, top.y + 1);
        assert!(text.contains("TundraUX 3"));
        assert_eq!(text.contains("debug"), cfg!(debug_assertions));
        if cfg!(debug_assertions) && width >= 80 {
            assert!(text.replace(' ', "").contains("中文"));
        }
        assert!(!row(&terminal, top.y).contains("debug"));
        assert!(!row(&terminal, top.bottom() - 1).contains("debug"));
        assert!(row(&terminal, layout.main.y).contains("PAGE CONTENT"));
        let message = layout.status_message.unwrap();
        let time = layout.time_button.unwrap();
        assert!(message.right() <= time.x);
        assert_eq!(message.intersection(time).area(), 0);
        assert!(row(&terminal, time.y + 1).contains("09:30"));
        let back = layout.back_button.unwrap();
        assert_eq!(back.right(), top.right());
        assert_eq!(back.height, top.height);
        let label = (back.x..back.right())
            .map(|x| terminal.backend().buffer()[(x, back.y + 1)].symbol())
            .collect::<String>();
        assert!(label.contains("[◀ Esc]"));
    }
}
#[test]
fn long_chinese_title_is_clipped_to_top_inner_row() {
    let mut model = chrome(50, 12);
    model.app_name = "终端交互环境".repeat(20);
    let (terminal, layout) = render(&model, &RenderContext::default());
    assert!(row(&terminal, 1).replace(' ', "").contains("终端交互环境"));
    assert!(row(&terminal, 1).contains("..."));
    assert!(row(&terminal, 1).contains("[◀ Esc]"));
    assert!(!row(&terminal, 2).contains('终'));
    assert!(row(&terminal, layout.main.y).contains("PAGE CONTENT"));
}
#[test]
fn compact_threshold_keeps_escape_above_the_content() {
    for (width, height, compact) in [
        (49, 12, true),
        (50, 11, true),
        (50, 12, false),
        (80, 24, false),
    ] {
        let (terminal, layout) = render(&chrome(width, height), &RenderContext::default());
        assert_eq!(layout.is_compact(), compact);
        assert_eq!(layout.time_button.is_none(), compact);
        assert!(layout.back_button.is_some());
        assert_eq!(layout.status_message.is_none(), compact);
        if compact {
            assert_eq!(layout.main.y, 1);
            assert_eq!(layout.main.height, height - 1);
            assert!(row(&terminal, 0).contains("TundraUX"));
            assert!(row(&terminal, 0).contains("[◀ Esc]"));
            assert!(row(&terminal, layout.main.y).contains("PAGE CONTENT"));
            assert_eq!(
                layout.back_button.unwrap().intersection(layout.main).area(),
                0
            );
            assert_eq!(layout.modal_area(), layout.main);
        }
    }
}
#[test]
fn page_transition_projects_only_main_and_leaves_chrome_pixels_and_hit_regions_fixed() {
    let idle = RenderContext::default();
    let mut moving = idle.clone();
    moving.transitions.screen = Some(MotionTransition {
        kind: MotionTransitionKind::Page,
        direction: MotionDirection::Entering,
        progress: 100,
        phase_progress: 100,
        active: true,
        next_redraw_in: std::time::Duration::from_millis(16),
    });
    let model = chrome(80, 24);
    let (before, base) = render(&model, &idle);
    let (during, shifted) = render(&model, &moving);
    assert_eq!(shifted.main.y, base.main.y + 1);
    assert_eq!(shifted.main.height, base.main.height - 1);
    assert_eq!(shifted.time_button, base.time_button);
    assert_eq!(shifted.back_button, base.back_button);
    assert_eq!(shifted.status_message, base.status_message);
    for y in [0, 1, 2, 21, 22, 23] {
        assert_eq!(row(&before, y), row(&during, y));
    }
    assert!(row(&during, shifted.main.y).contains("PAGE CONTENT"));
    assert!(!row(&during, base.main.y).contains("PAGE CONTENT"));
}

#[test]
fn back_button_uses_the_active_theme_and_hover_accent() {
    for border_shape in [BorderShape::Rounded, BorderShape::Square] {
        for capability in [ColorCapability::TrueColor, ColorCapability::Ansi] {
            let theme = TundraTheme {
                border_shape,
                accent_color: ratatui::style::Color::Magenta,
                ..TundraTheme::default()
            };
            let context = RenderContext::from_theme(
                &theme,
                Default::default(),
                RenderCapabilities {
                    color: capability,
                    image_protocol: false,
                },
            );
            let mut model = chrome(80, 24);
            model.back_button_hovered = true;
            let (terminal, layout) = render(&model, &context);
            let back = layout.back_button.unwrap();
            assert_eq!(
                terminal.backend().buffer()[(back.x + 3, back.y + 1)].fg,
                context.compatibility_theme().button_hover_color()
            );
            let offset = ShellFrameLayout::new(Rect::new(5, 8, 80, 24), None, &context);
            assert_eq!(offset.back_button.unwrap(), Rect::new(76, 8, 9, 3));
        }
    }
}

#[test]
fn editor_screen_content_uses_the_shared_main_without_erasing_shell_chrome() {
    let model = chrome(80, 24);
    let context = RenderContext::default();
    let editor = EditorViewModel::source("sample.txt", "editor content marker");
    let content = ScreenContent::Editor(&editor);
    let layout = ShellFrameLayout::new(Rect::new(0, 0, 80, 24), Some("09:30"), &context);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| {
            content.render_content(frame, &layout, &context, None, None);
            content.render_overlay(frame, &layout, &context);
            render_shell_chrome(frame, &layout, &model, &context);
        })
        .unwrap();
    assert!(row(&terminal, 1).contains("TundraUX 3"));
    assert!(row(&terminal, 22).contains("09:30"));
    assert!(
        (layout.main.y..layout.main.bottom())
            .any(|y| row(&terminal, y).contains("editor content marker"))
    );
}

fn assert_page_leaves_shell_chrome_untouched(content: ScreenContent<'_>) {
    let name = match content {
        ScreenContent::Home(_) => "Home",
        ScreenContent::Setup(_) => "Setup",
        ScreenContent::Login(_) => "Login",
        ScreenContent::BootstrapAdmin(_) => "BootstrapAdmin",
        ScreenContent::UserManagement(_) => "UserManagement",
        ScreenContent::Explorer(_) => "Explorer",
        ScreenContent::Launcher(_) => "Launcher",
        ScreenContent::CommandLine(_) => "CommandLine",
        ScreenContent::Editor(_) => "Editor",
        ScreenContent::Settings(_) => "Settings",
        ScreenContent::Logs(_) => "Logs",
        ScreenContent::Management(_) => "Management",
        ScreenContent::Diagnostics(_) => "Diagnostics",
        ScreenContent::SystemStatus(_) => "SystemStatus",
        ScreenContent::Clock(_) => "Clock",
    };
    for (width, height) in [(120, 40), (80, 24), (50, 12), (49, 11)] {
        for shape in [BorderShape::Rounded, BorderShape::Square] {
            let theme = TundraTheme::default().with_border_shape(shape);
            let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
            let context = content.render_context(&context);
            let model = chrome(width, height);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    let layout = ShellFrameLayout::new(
                        frame.area(),
                        model.status.time_button_label.as_deref(),
                        &context,
                    );
                    render_shell_chrome(frame, &layout, &model, &context);
                    let before = frame.buffer_mut().clone();
                    for stage in ["content", "overlay"] {
                        if stage == "content" {
                            content.render_content(frame, &layout, &context, None, None);
                        } else {
                            content.render_overlay(frame, &layout, &context);
                        }
                        for position in frame.area().positions() {
                            if !layout.main.contains(position) {
                                assert_eq!(
                                    frame.buffer_mut()[position], before[position],
                                    "{name} {stage} changed {position:?} outside main at {width}x{height}"
                                );
                            }
                        }
                    }
                })
                .unwrap();
        }
    }
}

#[test]
fn all_pages_and_page_overlays_leave_global_chrome_to_the_compositor() {
    let home = HomeViewModel::user(
        "User",
        "09:30",
        vec![ShellEntry::new("Editor", "Edit text")],
    );
    let login = LoginViewModel::new(Vec::new(), 0, 0, 0, LoginField::Password, None);
    let bootstrap = BootstrapAdminViewModel::new("Admin", 0, AuthField::Username, None);
    let mut setup = SetupViewModel {
        scroll_offset: 0,
        step: SetupStep::Language,
        languages: setup_language_options(),
        timezones: setup_timezone_options(),
        selected_language_index: 0,
        selected_timezone_index: 0,
        timezone_window_start: 0,
        admin_username: "Admin".into(),
        admin_password_len: 12,
        admin_password_confirm_len: 12,
        password_requirements: Vec::new(),
        password_hint: String::new(),
        focused_field: SetupField::Submit,
        can_submit: true,
        border_shape: BorderShape::Rounded,
        theme_color: ratatui::style::Color::White,
        theme_color_value: "white".into(),
        accent_color: ratatui::style::Color::Cyan,
        accent_color_value: "cyan".into(),
        custom_color_target: None,
        custom_color_input: String::new(),
        custom_color_valid: true,
        custom_color_conflicts_with_theme: false,
        custom_color_error: None,
        error: None,
    };
    let mut users = UserManagementViewModel::new("Admin", Vec::new(), 0, None, true, None);
    let mut explorer = ExplorerViewModel::new("/tmp", Vec::new(), None);
    let mut launcher = LauncherViewModel::new(Vec::new(), None, LauncherViewMode::Details, true);
    let command_line = CommandLineViewModel::new(CommandLineTerminalSnapshot::blank(120, 40));
    let mut editor = EditorViewModel::source("sample.txt", "editor content marker");
    let mut settings = SettingsViewModel {
        selected_category: SettingsCategory::Appearance,
        selected_field: SettingsField::BorderColor,
        cards: vec![SettingsCardViewModel::new(
            "Appearance",
            vec![SettingsItemViewModel::new(
                SettingsField::BorderColor,
                "Border color",
                "White",
                "Choose the border color",
                SettingsControlKind::Palette,
            )],
        )],
        appearance_preview: None,
        status: "Ready".into(),
        locked_message: None,
        scroll_offset: 0,
        picker: None,
        color_editor: None,
        weather_location_editor: None,
        file_extensions_editor: None,
        time_sync_server_editor: None,
        update: None,
    };
    let logs = LogsViewModel::default();
    let mut management = ManagementViewModel::default();
    let mut diagnostics = DiagnosticsViewModel::default();
    let mut system_status = SystemStatusViewModel {
        content: SystemStatusContentViewModel::User(UserSystemStatusViewModel {
            storage_status: "Healthy".into(),
            storage_tone: components::ComponentTone::Success,
            system_volume_usage: "42%".into(),
            system_volume_used_percentage: Some(42),
            network_status: "Connected".into(),
            network_tone: components::ComponentTone::Success,
            last_refreshed: "now".into(),
        }),
        diagnostics: DiagnosticsViewModel::default(),
        route: SystemStatusRoute::Dashboard,
        dashboard: SystemStatusDashboardViewModel::default(),
        process_sort: SystemStatusProcessSort::default(),
        selected_row: 0,
        scroll_offset: 0,
        refreshing: false,
        feedback: None,
    };
    let mut clock = ClockViewModel::new("09:30");
    for content in [
        ScreenContent::Home(&home),
        ScreenContent::Setup(&setup),
        ScreenContent::Login(&login),
        ScreenContent::BootstrapAdmin(&bootstrap),
        ScreenContent::UserManagement(&users),
        ScreenContent::Explorer(&explorer),
        ScreenContent::Launcher(&launcher),
        ScreenContent::CommandLine(&command_line),
        ScreenContent::Editor(&editor),
        ScreenContent::Settings(&settings),
        ScreenContent::Logs(&logs),
        ScreenContent::Management(&management),
        ScreenContent::Diagnostics(&diagnostics),
        ScreenContent::SystemStatus(&system_status),
        ScreenContent::Clock(&clock),
    ] {
        assert_page_leaves_shell_chrome_untouched(content);
    }
    for step in [SetupStep::Timezone, SetupStep::Admin, SetupStep::Appearance] {
        setup.step = step;
        assert_page_leaves_shell_chrome_untouched(ScreenContent::Setup(&setup));
    }
    setup.custom_color_target = Some(SetupCustomColorTarget::Theme);
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Setup(&setup));
    users.form = Some(UserManagementFormViewModel {
        kind: UserManagementFormKind::Create,
        title: "Create user".into(),
        username: "User".into(),
        display_name: "User".into(),
        role: "User".into(),
        password_len: 12,
        focused_field: UserManagementField::Username,
        error: None,
    });
    assert_page_leaves_shell_chrome_untouched(ScreenContent::UserManagement(&users));
    explorer.pending_dialog = Some(ExplorerDialogViewModel::new(
        "Confirm", "Message", "Yes", "No",
    ));
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Explorer(&explorer));
    launcher.confirmation = Some(LauncherConfirmationViewModel {
        kind: LauncherConfirmationKind::Remove,
        title: "Remove entry".into(),
        message: "Remove selected entry?".into(),
        confirm_label: "Remove".into(),
        cancel_label: "Cancel".into(),
        confirm_selected: false,
    });
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Launcher(&launcher));
    for menu in [EditorMenu::File, EditorMenu::Edit, EditorMenu::View] {
        editor.open_menu = Some(menu);
        assert_page_leaves_shell_chrome_untouched(ScreenContent::Editor(&editor));
    }
    settings.color_editor = Some(SettingsColorEditorViewModel {
        title: "Color".into(),
        value: "#123456".into(),
        error: None,
    });
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Settings(&settings));
    management.form = Some(ManagementForm {
        title: "Action".into(),
        message: "Confirm action".into(),
        ..Default::default()
    });
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Management(&management));
    for tab in DiagnosticsTab::ALL {
        diagnostics.tab = tab;
        assert_page_leaves_shell_chrome_untouched(ScreenContent::Diagnostics(&diagnostics));
    }
    diagnostics.repair_dialog = Some(DiagnosticsRepairDialogViewModel::default());
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Diagnostics(&diagnostics));
    for kind in SystemStatusWidgetKind::ALL {
        system_status.route = SystemStatusRoute::Detail(kind.detail());
        assert_page_leaves_shell_chrome_untouched(ScreenContent::SystemStatus(&system_status));
    }
    system_status.route = SystemStatusRoute::Dashboard;
    system_status.dashboard.dialog = Some(SystemStatusDialogViewModel {
        title: "Discard?".into(),
        message: "Discard changes?".into(),
        confirm_label: "Discard".into(),
        cancel_label: "Cancel".into(),
        selected_action: 1,
    });
    assert_page_leaves_shell_chrome_untouched(ScreenContent::SystemStatus(&system_status));
    clock.create_dialog = Some(ClockCreateDialogViewModel::default());
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Clock(&clock));
}
