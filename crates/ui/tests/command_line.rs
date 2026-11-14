#[path = "support/composition.rs"]
mod composition;
use composition as ui;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
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
        help_available: false,
        tasks_label: None,
        tasks_button_focused: false,
        tasks_button_hovered: false,
        help_button_hovered: false,
        help_button_focused: false,
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

fn viewport(size: (u16, u16)) -> Rect {
    ui::command_line_terminal_area(Rect::new(0, 0, size.0, size.1))
        .expect("full child terminal viewport")
}

#[test]
fn failed_external_terminal_keeps_output_and_uses_the_application_title() {
    let mut terminal = CommandLineTerminalSnapshot::blank(108, 18);
    terminal.set_cell(
        0,
        0,
        CommandLineCell {
            symbol: "R".into(),
            ..Default::default()
        },
    );
    let mut model = CommandLineViewModel::new(terminal);
    model.title = Some("neovim".into());
    model.process_state = CommandLineProcessState::Exited { code: 17 };
    let mut screen = Terminal::new(TestBackend::new(108, 20)).unwrap();
    screen
        .draw(|frame| {
            render_command_line(
                frame,
                frame.area(),
                &chrome((108, 20)),
                &model,
                &TundraTheme::default_dark(),
            )
        })
        .unwrap();
    let buffer = screen.backend().buffer();
    let area = viewport((108, 20));
    assert_eq!(buffer[(area.x, area.y)].symbol(), "R");
    let text = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("neovim"));
    assert!(text.contains("17"));
}

#[test]
fn command_line_renders_snapshot_inside_the_standard_shell_chrome() {
    let mut terminal = CommandLineTerminalSnapshot::blank(108, 18);
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
    terminal.set_cell(
        107,
        17,
        CommandLineCell {
            symbol: "Z".into(),
            ..Default::default()
        },
    );
    let model = CommandLineViewModel::new(terminal);
    let mut screen = Terminal::new(TestBackend::new(108, 20)).unwrap();
    screen
        .draw(|frame| {
            render_command_line(
                frame,
                frame.area(),
                &chrome((108, 20)),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .unwrap();
    let buffer = screen.backend().buffer();

    let area = viewport((108, 20));
    assert_eq!(buffer.cell((area.x, area.y)).unwrap().symbol(), "C");
    assert_eq!(buffer.cell((area.x + 2, area.y)).unwrap().symbol(), "界");
    assert_eq!(
        buffer
            .cell((area.right() - 1, area.bottom() - 1))
            .unwrap()
            .symbol(),
        "Z"
    );
    assert_eq!(
        buffer.cell((area.x, area.y)).unwrap().fg,
        ratatui::style::Color::Rgb(12, 34, 56)
    );
    let output = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(output.contains("TundraUX 3"));
    assert!(output.contains("Command Line"));
    assert!(output.contains("Ready"));
    assert!(output.contains("2026-07-27 10:15"));
}

#[test]
fn command_line_preserves_unstyled_child_prompt_and_input() {
    let prompt = "AdminUser@/home/user/space and 中文 >> ";
    let mut terminal = CommandLineTerminalSnapshot::blank(108, 18);
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
    let mut screen = Terminal::new(TestBackend::new(108, 20)).unwrap();
    screen
        .draw(|frame| {
            render_command_line(frame, frame.area(), &chrome((108, 20)), &model, &theme);
        })
        .unwrap();
    let buffer = screen.backend().buffer();
    let area = viewport((108, 20));
    let output_color = buffer.cell((area.x, area.y + 1)).unwrap().fg;
    assert_ne!(output_color, theme.accent_color);

    let mut prompt_column = 0;
    for symbol in prompt.trim_end().chars() {
        let cell = buffer.cell((area.x + prompt_column, area.y)).unwrap();
        assert_eq!(cell.symbol(), symbol.to_string());
        assert_eq!(cell.fg, output_color);
        // Wide glyphs use the leading cell's style; their continuation cell
        // has no independently painted symbol or foreground color.
        prompt_column +=
            u16::try_from(ratatui::text::Line::from(symbol.to_string()).width()).unwrap();
    }
    assert_eq!(
        buffer.cell((area.x + column, area.y)).unwrap().fg,
        output_color,
        "typed command text must keep the child terminal style"
    );
}

#[test]
fn command_line_force_wraps_a_snapshot_row_wider_than_the_viewport() {
    let mut terminal = CommandLineTerminalSnapshot::blank(160, 18);
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
    let mut screen = Terminal::new(TestBackend::new(108, 20)).unwrap();

    screen
        .draw(|frame| {
            render_command_line(
                frame,
                frame.area(),
                &chrome((108, 20)),
                &model,
                &TundraTheme::default_dark(),
            );
        })
        .unwrap();
    let buffer = screen.backend().buffer();

    let area = viewport((108, 20));
    assert!(area.width < 120);
    assert!((area.x..area.right()).all(|x| buffer.cell((x, area.y)).unwrap().symbol() == "x"));
    assert!(
        (0..120 - area.width)
            .all(|x| buffer.cell((area.x + x, area.y + 1)).unwrap().symbol() == "x")
    );
    assert_eq!(buffer.cell((area.x, area.y + 2)).unwrap().symbol(), "N");
}

#[test]
fn short_history_scrollbar_layout_matches_the_painted_thumb() {
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
    assert!(output.contains("Ready"));
    assert!(output.contains("Resize to continue"));
}

#[test]
fn stopped_and_failed_cli_states_offer_restart_and_the_return_button() {
    for (state, expected) in [
        (
            CommandLineProcessState::Exited { code: 75 },
            "Command Line exited (75)",
        ),
        (
            CommandLineProcessState::Failed {
                message: "Unable to start CLI".into(),
            },
            "Unable to start CLI",
        ),
    ] {
        let mut model = CommandLineViewModel::new(CommandLineTerminalSnapshot::blank(108, 18));
        model.process_state = state;
        let mut screen = Terminal::new(TestBackend::new(108, 20)).unwrap();
        screen
            .draw(|frame| {
                render_command_line(
                    frame,
                    frame.area(),
                    &chrome((108, 20)),
                    &model,
                    &TundraTheme::default_dark(),
                );
            })
            .unwrap();
        let buffer = screen.backend().buffer();
        let output = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(output.contains(expected), "{output}");
        assert!(output.contains("Enter restart · use the top-right button to return"));
        let title_bar = buffer.content()[..usize::from(buffer.area.width)]
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(title_bar.trim_end().ends_with("Esc"), "{title_bar}");
        assert!(!output.contains("Esc Launcher"));
        assert!(!output.contains("Ctrl+Shift+X"));
        assert!(output.contains("Ready"));
        assert!(output.contains("2026-07-27 10:15"));
    }
}
