use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::{
    AutoAdminViewModel, CommandLineTerminalSnapshot, MotionFrame, RenderCapabilities,
    RenderContext, TundraTheme,
};

#[test]
fn auto_admin_buttons_and_terminal_fit_small_and_normal_windows() {
    for (width, height) in [(220, 65), (120, 40), (80, 24), (50, 12), (40, 12), (20, 8)] {
        let bounds = Rect::new(0, 0, width, height);
        for confirming in [true, false] {
            let model = model(confirming, false);
            let layout = ui::auto_admin_layout(bounds, &model);
            let main = match ui::compute_shell_layout(bounds) {
                ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
            };
            assert_eq!(layout.dialog.intersection(main), layout.dialog);
            assert!(layout.dialog.width <= 140 && layout.dialog.height <= 36);
            if width >= 120 {
                assert!(layout.dialog.width < main.width);
                assert!(layout.dialog.height < main.height);
            }
            for area in [
                layout.dialog,
                layout.description,
                layout.terminal,
                layout.status,
                layout.input,
            ]
            .into_iter()
            .chain(layout.buttons)
            {
                assert_eq!(area.intersection(bounds), area);
            }
            for (a, left) in layout.buttons.iter().enumerate() {
                if a < if confirming { 2 } else { 3 } {
                    assert!(left.width > 0 && left.height > 0);
                }
                for right in layout.buttons.iter().skip(a + 1) {
                    assert_eq!(left.intersection(*right).width, 0);
                }
            }
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let theme = TundraTheme::default();
            let context = RenderContext::from_theme(
                &theme,
                MotionFrame::default(),
                RenderCapabilities::default(),
            );
            terminal
                .draw(|frame| ui::render_auto_admin(frame, bounds, &model, &context))
                .unwrap();
            let content = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(content.contains("AutoAdmin (AA)"));
            assert!(content.contains("Remove demo"));
        }
    }
}

#[test]
fn auto_admin_uses_authorization_style_and_preserves_chrome_in_every_phase() {
    for (width, height) in [(220, 65), (120, 40), (80, 24), (50, 12), (40, 12), (20, 8)] {
        for (confirming, finished) in [(true, false), (false, false), (false, true)] {
            let bounds = Rect::new(0, 0, width, height);
            let main = match ui::compute_shell_layout(bounds) {
                ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
            };
            let model = model(confirming, finished);
            let layout = ui::auto_admin_layout(bounds, &model);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    for cell in frame.buffer_mut().content.iter_mut() {
                        cell.set_symbol("#");
                    }
                    ui::render_auto_admin(frame, bounds, &model, &RenderContext::default());
                })
                .unwrap();
            for y in 0..height {
                for x in 0..width {
                    if !main.contains((x, y).into()) {
                        assert_eq!(
                            terminal.backend().buffer()[(x, y)].symbol(),
                            "#",
                            "{width}x{height} changed chrome at {x},{y}"
                        );
                    } else if !layout.dialog.contains((x, y).into()) {
                        let cell = &terminal.backend().buffer()[(x, y)];
                        assert_eq!(cell.fg, ratatui::style::Color::DarkGray);
                        assert!(cell.modifier.contains(ratatui::style::Modifier::DIM));
                    }
                }
            }
            let buffer = terminal.backend().buffer();
            assert_eq!(buffer[(layout.dialog.x, layout.dialog.y)].symbol(), "┌");
            assert_eq!(
                buffer[(layout.dialog.x, layout.dialog.y)].fg,
                ratatui::style::Color::LightCyan
            );
            assert_eq!(
                buffer[(layout.dialog.x, layout.dialog.y)].bg,
                ratatui::style::Color::Rgb(14, 27, 38)
            );
            if layout.dialog.width >= 70 {
                assert_eq!(
                    buffer[(layout.dialog.x + 18, layout.dialog.y + 1)].symbol(),
                    "│"
                );
                assert!(layout.description.x > layout.dialog.x + 18);
            }
        }
    }
}

#[test]
fn running_auto_admin_highlights_keyboard_buttons_and_shows_the_return_hint() {
    for width in [50, 60, 180] {
        let bounds = Rect::new(0, 0, width, 50);
        let layout = ui::auto_admin_layout(bounds, &model(false, false));
        let mut model = AutoAdminViewModel {
            description: "Remove demo".into(),
            status: "Running".into(),
            confirming: false,
            finished: false,
            approve_selected: false,
            button_focus: None,
            scroll: 0,
            input: Some("> ••••".into()),
            terminal: std::sync::Arc::new(CommandLineTerminalSnapshot::blank(
                layout.terminal.width,
                layout.terminal.height,
            )),
        };
        let mut terminal = Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
        let layout = ui::auto_admin_layout(bounds, &model);
        terminal
            .draw(|frame| ui::render_auto_admin(frame, bounds, &model, &RenderContext::default()))
            .unwrap();
        let unfocused = terminal.backend().buffer().clone();
        for index in 0..3 {
            model.button_focus = Some(index);
            terminal
                .draw(|frame| {
                    ui::render_auto_admin(frame, bounds, &model, &RenderContext::default())
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            for (other, area) in layout.buttons.iter().enumerate() {
                let changed = (area.x..area.right())
                    .any(|x| buffer[(x, area.y)].style() != unfocused[(x, area.y)].style());
                assert_eq!(
                    changed,
                    index == other,
                    "only the focused button is highlighted"
                );
                let theme = TundraTheme::default();
                assert_eq!(
                    buffer[(area.x, area.y)].fg,
                    if index == other {
                        theme.accent_color
                    } else {
                        theme.foreground
                    }
                );
            }
            let hint = (layout.input.x..layout.input.right())
                .map(|x| buffer[(x, layout.input.y)].symbol())
                .collect::<String>();
            for shortcut in ["F6", "Tab", "Enter"] {
                assert!(
                    hint.contains(shortcut),
                    "missing {shortcut} in {width}-column window: {hint}"
                );
            }
            assert!(!hint.contains('•'));
        }
    }
}

#[test]
fn auto_admin_and_previews_show_close_only_after_the_task_ends() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    let bounds = Rect::new(0, 0, 120, 40);
    for language in ["en-US", "zh-CN"] {
        let snapshot = i18n::LanguageSnapshot::load(&root, language, 1)
            .unwrap()
            .snapshot;
        let _language = i18n::enter_snapshot(std::sync::Arc::new(snapshot));
        assert!(!i18n::tr!("aa-terminal-hint").contains("F12"));
        assert!(!i18n::tr!("aa-input-required").contains("F12"));
        for (confirming, finished, expected) in [
            (true, false, &["aa.approve", "aa.deny"][..]),
            (false, false, &["aa.y", "aa.n", "aa.enter"][..]),
            (false, true, &["aa.close"][..]),
        ] {
            let model = model(confirming, finished);
            for style in [
                None,
                Some(ui::AutoAdminPreviewStyle::Caution),
                Some(ui::AutoAdminPreviewStyle::Danger),
                Some(ui::AutoAdminPreviewStyle::Authorization),
            ] {
                let theme = TundraTheme::default();
                let buttons = ui::components::ButtonFrame::new(None, None, &theme);
                let mut context =
                    RenderContext::from_theme(&theme, Default::default(), Default::default());
                context.buttons = Some(buttons.clone());
                let mut terminal =
                    Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
                terminal
                    .draw(|frame| {
                        if let Some(style) = style {
                            ui::render_auto_admin_preview(frame, bounds, &model, style, &context);
                        } else {
                            ui::render_auto_admin(frame, bounds, &model, &context);
                        }
                    })
                    .unwrap();
                let layout = style.map_or_else(
                    || ui::auto_admin_layout(bounds, &model),
                    |style| ui::auto_admin_preview_layout(bounds, &model, style).0,
                );
                let regions = buttons.regions();
                assert_eq!(
                    regions
                        .iter()
                        .map(|region| region.id.as_str())
                        .collect::<Vec<_>>(),
                    expected,
                    "{language}, {style:?}, confirming={confirming}, finished={finished}"
                );
                for (region, area) in regions.iter().zip(layout.buttons) {
                    assert_eq!(region.area, area);
                    assert!(!region.disabled);
                }
                for area in layout.buttons.iter().skip(expected.len()) {
                    assert_eq!(*area, Rect::default());
                }
            }
        }
    }
}

fn model(confirming: bool, finished: bool) -> AutoAdminViewModel {
    AutoAdminViewModel {
        description: "Remove demo\nTarget: demo-1.0".into(),
        status: "Waiting".into(),
        confirming,
        finished,
        approve_selected: true,
        button_focus: None,
        scroll: 0,
        input: None,
        terminal: std::sync::Arc::new(CommandLineTerminalSnapshot::blank(104, 20)),
    }
}

#[test]
fn aa_candidates_preserve_chrome_and_show_warning_and_actions() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for language in ["en-US", "zh-CN"] {
        let snapshot = i18n::LanguageSnapshot::load(&root, language, 1)
            .unwrap()
            .snapshot;
        let _language = i18n::enter_snapshot(std::sync::Arc::new(snapshot));
        for (width, height) in [(120, 40), (108, 22), (80, 24), (40, 12), (20, 8)] {
            for (confirming, finished) in [(true, false), (false, false), (false, true)] {
                let model = model(confirming, finished);
                let bounds = Rect::new(0, 0, width, height);
                let mut buffers = Vec::new();
                for style in [
                    ui::AutoAdminPreviewStyle::Caution,
                    ui::AutoAdminPreviewStyle::Danger,
                    ui::AutoAdminPreviewStyle::Authorization,
                ] {
                    let (layout, warning) = ui::auto_admin_preview_layout(bounds, &model, style);
                    let main = match ui::compute_shell_layout(bounds) {
                        ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
                    };
                    for area in [
                        layout.dialog,
                        warning,
                        layout.description,
                        layout.terminal,
                        layout.status,
                        layout.input,
                    ]
                    .into_iter()
                    .chain(layout.buttons)
                    {
                        if area.width > 0 && area.height > 0 {
                            assert_eq!(area.intersection(main), area);
                        }
                    }
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    terminal
                        .draw(|frame| {
                            for cell in frame.buffer_mut().content.iter_mut() {
                                cell.set_symbol("#");
                            }
                            ui::render_auto_admin_preview(
                                frame,
                                bounds,
                                &model,
                                style,
                                &RenderContext::default(),
                            );
                        })
                        .unwrap();
                    let buffer = terminal.backend().buffer();
                    for y in 0..height {
                        for x in 0..width {
                            if !main.contains((x, y).into()) {
                                assert_eq!(buffer[(x, y)].symbol(), "#");
                            } else if !layout.dialog.contains((x, y).into()) {
                                assert_eq!(buffer[(x, y)].fg, ratatui::style::Color::DarkGray);
                                assert!(
                                    buffer[(x, y)]
                                        .modifier
                                        .contains(ratatui::style::Modifier::DIM)
                                );
                            }
                        }
                    }
                    let text = buffer
                        .content()
                        .iter()
                        .map(|cell| cell.symbol())
                        .collect::<String>();
                    assert!(text.contains("AA / AutoAdmin"));
                    if width >= 80 && height >= 22 {
                        let visible_warning = (warning.y..warning.bottom())
                            .map(|y| {
                                (warning.x..warning.right())
                                    .map(|x| buffer[(x, y)].symbol())
                                    .collect::<String>()
                            })
                            .collect::<String>();
                        assert!(visible_warning.contains(if language == "en-US" {
                            "ELEVATED"
                        } else {
                            "需"
                        }));
                        for area in layout.buttons.iter().take(if confirming {
                            2
                        } else if finished {
                            1
                        } else {
                            3
                        }) {
                            assert!(area.width > 0 && area.height > 0);
                            assert!(
                                (area.x..area.right())
                                    .any(|x| !buffer[(x, area.y)].symbol().trim().is_empty())
                            );
                        }
                    }
                    buffers.push(buffer.clone());
                }
                assert_ne!(buffers[0], buffers[1]);
                assert_ne!(buffers[1], buffers[2]);
                // Shape/text must differ even without color support.
                let symbols = |buffer: &ratatui::buffer::Buffer| {
                    buffer
                        .content()
                        .iter()
                        .map(|cell| cell.symbol())
                        .collect::<String>()
                };
                assert_ne!(symbols(&buffers[0]), symbols(&buffers[1]));
                assert_ne!(symbols(&buffers[1]), symbols(&buffers[2]));
            }
        }
    }
}

#[test]
fn confirmation_and_empty_results_are_compact_with_actions_centered_in_content() {
    let bounds = Rect::new(0, 0, 208, 55);
    for (confirming, finished, count) in [(true, false, 2), (false, true, 1), (false, false, 3)] {
        let model = model(confirming, finished);
        let layout = ui::auto_admin_layout(bounds, &model);
        let first = layout.buttons[0];
        let last = layout.buttons[count - 1];
        assert!(
            (i32::from(first.x + last.right())
                - i32::from(layout.description.x * 2 + layout.description.width))
            .abs()
                <= 1
        );
        assert!(layout.description.x > layout.dialog.x + 1);
        assert!(layout.description.y > layout.dialog.y + 1);
        if confirming || finished {
            assert!(
                layout.dialog.height <= 16,
                "sparse requests should not fill the screen"
            );
            assert_eq!(layout.terminal.height, 0);
        } else {
            assert_eq!(layout.terminal.height, 20);
        }
    }
}

#[test]
fn completed_output_is_retained_without_an_empty_terminal_tail() {
    let bounds = Rect::new(0, 0, 208, 55);
    let mut model = model(false, true);
    let terminal = std::sync::Arc::make_mut(&mut model.terminal);
    terminal.cells[0].symbol = "O".into();
    terminal.cells[104 * 2].symbol = "K".into();
    let layout = ui::auto_admin_layout(bounds, &model);
    assert_eq!(layout.terminal.height, 3);
    let mut terminal = Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
    terminal
        .draw(|frame| ui::render_auto_admin(frame, bounds, &model, &RenderContext::default()))
        .unwrap();
    assert_eq!(
        terminal.backend().buffer()[(layout.terminal.x, layout.terminal.y + 2)].symbol(),
        "K"
    );
}

#[test]
fn localized_confirmation_wraps_and_registers_the_same_centered_button_areas() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for language in ["en-US", "zh-CN"] {
        let snapshot = i18n::LanguageSnapshot::load(&root, language, 1)
            .unwrap()
            .snapshot;
        let _language = i18n::enter_snapshot(std::sync::Arc::new(snapshot));
        for width in [80, 208] {
            let bounds = Rect::new(0, 0, width, 55);
            let mut model = model(true, false);
            model.description = i18n::tr!("aa-local-user-enable", user = "TestUser");
            model.status = i18n::tr!("aa-request");
            let layout = ui::auto_admin_layout(bounds, &model);
            let theme = TundraTheme::default();
            let buttons = ui::components::ButtonFrame::new(None, None, &theme);
            let mut context =
                RenderContext::from_theme(&theme, Default::default(), Default::default());
            context.buttons = Some(buttons.clone());
            let mut terminal = Terminal::new(TestBackend::new(width, 55)).unwrap();
            terminal
                .draw(|frame| ui::render_auto_admin(frame, bounds, &model, &context))
                .unwrap();
            let regions = buttons.regions();
            assert_eq!(regions.len(), 2);
            assert_eq!(regions[0].area, layout.buttons[0]);
            assert_eq!(regions[1].area, layout.buttons[1]);
            for (text, area) in [
                (&model.description, layout.description),
                (&model.status, layout.status),
                (&i18n::tr!("aa-confirm-hint"), layout.input),
            ] {
                let expected = ui::management_wrapped_lines(text, area.width);
                assert!(
                    expected.len() <= usize::from(area.height),
                    "{language} {width}: clipped {text}"
                );
                let actual = (area.y..area.bottom())
                    .map(|y| {
                        (area.x..area.right())
                            .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>();
                // Wide glyph continuation cells are spaces; check Latin shortcuts
                // separately while verifying every row has rendered content.
                for (row, expected) in actual.iter().zip(expected) {
                    assert!(!row.trim().is_empty() || expected.is_empty());
                }
            }
        }
    }
}
