use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ui::{
    CommandLineCell, CommandLineCellStyle, CommandLineColor, CommandLineProcessState,
    CommandLineTerminalSnapshot, CommandLineViewModel, HomeDisplayMode, NotificationTone,
    ShellChromeViewModel, StatusViewModel, TundraTheme, render_command_line,
};

fn chrome(size: (u16, u16)) -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".into(),
        build_mode: "test".into(),
        display_mode: HomeDisplayMode::User,
        terminal_size: size,
        screen_stack: vec!["Home".into(), "Launcher".into(), "Command Line".into()],
        status: StatusViewModel {
            status: "Ready".into(),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: Some("2026-07-27 10:15".into()),
            time_button_selected: false,
        },
    }
}

#[test]
fn command_line_renders_snapshot_inside_the_standard_shell_chrome() {
    let mut terminal = CommandLineTerminalSnapshot::blank(106, 14);
    terminal.set_cell(
        0,
        0,
        CommandLineCell {
            symbol: "C".into(),
            style: CommandLineCellStyle {
                foreground: CommandLineColor::Rgb(12, 34, 56),
                bold: true,
                ..CommandLineCellStyle::default()
            },
            cursor: true,
        },
    );
    terminal.set_cell(
        2,
        0,
        CommandLineCell {
            symbol: "界".into(),
            ..CommandLineCell::default()
        },
    );
    let model = CommandLineViewModel::new(terminal);
    let mut screen = Terminal::new(TestBackend::new(108, 22)).unwrap();
    screen
        .draw(|frame| {
            render_command_line(
                frame,
                frame.area(),
                &chrome((108, 22)),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .unwrap();
    let buffer = screen.backend().buffer();

    // 108x22 uses the normal 3-row top bar, then a bordered main panel.
    assert_eq!(buffer.cell((1, 4)).unwrap().symbol(), "C");
    assert_eq!(buffer.cell((3, 4)).unwrap().symbol(), "界");
    assert_eq!(
        buffer.cell((1, 4)).unwrap().fg,
        ratatui::style::Color::Rgb(12, 34, 56)
    );
    let output = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(output.contains("TundraUX 3"));
    assert!(output.contains("Command Line"));
    assert!(output.contains("Status"));
    assert!(output.contains("2026-07-27 10:15"));
}

#[test]
fn command_line_prompt_uses_the_application_accent_color() {
    let prompt = "AdminUser >> ";
    let mut terminal = CommandLineTerminalSnapshot::blank(106, 14);
    for (column, symbol) in prompt.chars().enumerate() {
        terminal.set_cell(
            u16::try_from(column).unwrap(),
            0,
            CommandLineCell {
                symbol: symbol.to_string(),
                ..CommandLineCell::default()
            },
        );
    }
    terminal.set_cell(
        u16::try_from(prompt.len()).unwrap(),
        0,
        CommandLineCell {
            symbol: "h".to_string(),
            ..CommandLineCell::default()
        },
    );
    let model = CommandLineViewModel::new(terminal).with_prompt_username("AdminUser");
    let theme = TundraTheme::default_dark().with_accent_color(ratatui::style::Color::LightMagenta);
    let mut screen = Terminal::new(TestBackend::new(108, 22)).unwrap();
    screen
        .draw(|frame| {
            render_command_line(frame, frame.area(), &chrome((108, 22)), &model, &theme);
        })
        .unwrap();
    let buffer = screen.backend().buffer();

    for column in 0..u16::try_from("AdminUser >>".len()).unwrap() {
        assert_eq!(buffer.cell((1 + column, 4)).unwrap().fg, theme.accent_color);
    }
    assert_ne!(
        buffer
            .cell((1 + u16::try_from(prompt.len()).unwrap(), 4))
            .unwrap()
            .fg,
        theme.accent_color,
        "typed command text must keep the child terminal style"
    );
}

#[test]
fn command_line_force_wraps_a_snapshot_row_wider_than_the_viewport() {
    let mut terminal = CommandLineTerminalSnapshot::blank(160, 14);
    for column in 0..120 {
        terminal.set_cell(
            column,
            0,
            CommandLineCell {
                symbol: "x".to_string(),
                ..CommandLineCell::default()
            },
        );
    }
    terminal.set_cell(
        0,
        1,
        CommandLineCell {
            symbol: "N".to_string(),
            ..CommandLineCell::default()
        },
    );
    let model = CommandLineViewModel::new(terminal);
    let mut screen = Terminal::new(TestBackend::new(108, 22)).unwrap();

    screen
        .draw(|frame| {
            render_command_line(
                frame,
                frame.area(),
                &chrome((108, 22)),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .unwrap();
    let buffer = screen.backend().buffer();

    assert!((1..107).all(|x| buffer.cell((x, 4)).unwrap().symbol() == "x"));
    assert!((1..15).all(|x| buffer.cell((x, 5)).unwrap().symbol() == "x"));
    assert_eq!(buffer.cell((1, 6)).unwrap().symbol(), "N");
}

#[test]
fn command_line_history_renders_the_glacier_scrollbar_style() {
    let mut terminal = CommandLineTerminalSnapshot::blank(105, 14);
    terminal.scrollback_rows = 14;
    let model = CommandLineViewModel::new(terminal);
    let mut screen = Terminal::new(TestBackend::new(108, 22)).unwrap();
    screen
        .draw(|frame| {
            render_command_line(
                frame,
                frame.area(),
                &chrome((108, 22)),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .unwrap();
    let buffer = screen.backend().buffer();

    // The inner panel spans x=1..106 and y=4..17. At the live bottom, the
    // upper track remains visible while the thumb occupies its lower half.
    assert_eq!(buffer.cell((106, 4)).unwrap().symbol(), "│");
    assert_eq!(buffer.cell((106, 17)).unwrap().symbol(), "┃");
}

#[test]
fn undersized_command_line_is_blocked() {
    let model = CommandLineViewModel::new(CommandLineTerminalSnapshot::blank(108, 20));
    let mut screen = Terminal::new(TestBackend::new(80, 20)).unwrap();
    screen
        .draw(|frame| {
            render_command_line(
                frame,
                frame.area(),
                &chrome((80, 20)),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .unwrap();
    let output = screen
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(output.contains("TundraUX 3"));
    assert!(output.contains("Command Line"));
    assert!(output.contains("Status"));
    assert!(output.contains("Resize to continue"));
}

#[test]
fn stopped_and_failed_cli_states_replace_the_running_shortcut_hint() {
    for (state, expected) in [
        (
            CommandLineProcessState::Exited { code: 75 },
            "CLI exited (75); Enter restart · Esc Launcher",
        ),
        (
            CommandLineProcessState::Failed {
                message: "Unable to start CLI".into(),
            },
            "Unable to start CLI",
        ),
    ] {
        let mut model = CommandLineViewModel::new(CommandLineTerminalSnapshot::blank(106, 14));
        model.process_state = state;
        let mut screen = Terminal::new(TestBackend::new(108, 22)).unwrap();
        screen
            .draw(|frame| {
                render_command_line(
                    frame,
                    frame.area(),
                    &chrome((108, 22)),
                    &model,
                    &TundraTheme::default_dark(),
                );
            })
            .unwrap();
        let output = screen
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(output.contains(expected));
        assert!(output.contains("Status"));
        assert!(output.contains("2026-07-27 10:15"));
    }
}
