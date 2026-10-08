#[path = "support/composition.rs"]
mod composition;
use composition as ui;
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
        back_button_hovered: false,
        back_shortcut: "Esc",
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
            command_status: None,
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
fn command_line_preserves_unstyled_child_prompt_and_input() {
    let prompt = "AdminUser@/home/user/space and 中文 >> ";
    let mut terminal = CommandLineTerminalSnapshot::blank(106, 14);
    let mut column = 0;
    for symbol in prompt.chars() {
        terminal.set_cell(
            column,
            0,
            CommandLineCell {
                symbol: symbol.to_string(),
                ..CommandLineCell::default()
            },
        );
        column += u16::try_from(ratatui::text::Line::from(symbol.to_string()).width()).unwrap();
    }
    terminal.set_cell(
        column,
        0,
        CommandLineCell {
            symbol: "h".to_string(),
            ..CommandLineCell::default()
        },
    );
    terminal.set_cell(
        0,
        1,
        CommandLineCell {
            symbol: "O".into(),
            ..CommandLineCell::default()
        },
    );
    let model = CommandLineViewModel::new(terminal);
    let theme = TundraTheme::default_dark().with_accent_color(ratatui::style::Color::LightMagenta);
    let mut screen = Terminal::new(TestBackend::new(108, 22)).unwrap();
    screen
        .draw(|frame| {
            render_command_line(frame, frame.area(), &chrome((108, 22)), &model, &theme);
        })
        .unwrap();
    let buffer = screen.backend().buffer();
    let output_color = buffer.cell((1, 5)).unwrap().fg;
    assert_ne!(output_color, theme.accent_color);

    let mut prompt_column = 0;
    for symbol in prompt.trim_end().chars() {
        let cell = buffer.cell((1 + prompt_column, 4)).unwrap();
        assert_eq!(cell.symbol(), symbol.to_string());
        assert_eq!(cell.fg, output_color);
        // Wide glyphs use the leading cell's style; their continuation cell
        // has no independently painted symbol or foreground color.
        prompt_column +=
            u16::try_from(ratatui::text::Line::from(symbol.to_string()).width()).unwrap();
    }
    assert_eq!(
        buffer.cell((1 + column, 4)).unwrap().fg,
        output_color,
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
    assert_eq!(buffer.cell((106, 17)).unwrap().symbol(), "█");
}

#[test]
fn short_history_scrollbar_layout_matches_the_painted_thumb() {
    use ratatui::layout::Rect;

    let main = Rect::new(0, 0, 10, 4);
    let terminal_area = Rect::new(1, 1, 8, 2);
    let context = ui::RenderContext::from_theme(
        &TundraTheme::default_dark(),
        Default::default(),
        Default::default(),
    );
    for scrollback_offset in [0, 1, 2] {
        let mut snapshot = CommandLineTerminalSnapshot::blank(7, 1);
        snapshot.scrollback_rows = 2;
        snapshot.scrollback_offset = scrollback_offset;
        let layout = ui::command_line_scrollbar_layout(terminal_area, &snapshot).unwrap();
        let (start, length) =
            ui::components::Scrollbar::new(3, 1, 2 - scrollback_offset).thumb_range(layout.track);
        assert_eq!(
            layout.thumb,
            Rect::new(layout.track.x, layout.track.y + start, 1, length),
        );
        assert!(layout.thumb.height < layout.track.height);

        let model = CommandLineViewModel::new(snapshot);
        let mut screen = Terminal::new(TestBackend::new(main.width, main.height)).unwrap();
        screen
            .draw(|frame| {
                ui::render_command_line_content(frame, main, Some(terminal_area), &model, &context);
            })
            .unwrap();
        for y in layout.track.y..layout.track.bottom() {
            let expected = if y >= layout.thumb.y && y < layout.thumb.bottom() {
                "█"
            } else {
                "│"
            };
            assert_eq!(
                screen
                    .backend()
                    .buffer()
                    .cell((layout.track.x, y))
                    .unwrap()
                    .symbol(),
                expected
            );
        }
    }
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

#[test]
fn command_markers_use_theme_colors_without_recoloring_command_text() {
    use ui::components::CommandStatus;
    for theme in [
        TundraTheme::default_dark(),
        TundraTheme::default_dark().with_accent_color(ratatui::style::Color::LightMagenta),
    ] {
        let cases = [
            (CommandStatus::Pending, "○", theme.muted),
            (CommandStatus::Succeeded, "●", theme.accent_color),
            (CommandStatus::Failed, "×", theme.error),
        ];
        let mut snapshot = CommandLineTerminalSnapshot::blank(106, 14);
        for (row, (status, _, _)) in cases.iter().enumerate() {
            snapshot.set_cell(
                0,
                row as u16,
                CommandLineCell {
                    symbol: "○".into(),
                    command_status: Some(*status),
                    ..Default::default()
                },
            );
            snapshot.set_cell(
                2,
                row as u16,
                CommandLineCell {
                    symbol: "h".into(),
                    style: CommandLineCellStyle {
                        foreground: CommandLineColor::Rgb(12, 34, 56),
                        ..Default::default()
                    },
                    cursor: row == 0,
                    ..Default::default()
                },
            );
        }
        let model = CommandLineViewModel::new(snapshot);
        let mut screen = Terminal::new(TestBackend::new(108, 22)).unwrap();
        screen
            .draw(|frame| {
                render_command_line(frame, frame.area(), &chrome((108, 22)), &model, &theme)
            })
            .unwrap();
        let buffer = screen.backend().buffer();
        for (row, (_, symbol, color)) in cases.iter().enumerate() {
            let marker = buffer.cell((1, 4 + row as u16)).unwrap();
            assert_eq!(marker.symbol(), *symbol);
            assert_eq!(marker.fg, *color);
            let input = buffer.cell((3, 4 + row as u16)).unwrap();
            assert_eq!(input.symbol(), "h");
            assert_eq!(input.fg, ratatui::style::Color::Rgb(12, 34, 56));
        }
    }
}
