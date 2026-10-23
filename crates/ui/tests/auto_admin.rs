use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::{
    AutoAdminViewModel, CommandLineTerminalSnapshot, MotionFrame, RenderCapabilities,
    RenderContext, TundraTheme,
};

#[test]
fn live_output_scrollbar_keeps_text_clear_and_tracks_history() {
    let bounds = Rect::new(0, 0, 120, 40);
    let mut model = model(false, false);
    let initial = ui::auto_admin_layout(bounds, &model);
    for finished in [false, true] {
        model.finished = finished;
        for offset in [0, 60, 120] {
            let snapshot = std::sync::Arc::make_mut(&mut model.terminal);
            *snapshot =
                CommandLineTerminalSnapshot::blank(initial.terminal.width, initial.terminal.height);
            snapshot.scrollback_rows = 120;
            snapshot.scrollback_offset = offset;
            for row in 0..snapshot.rows {
                snapshot.cells[usize::from(row + 1) * usize::from(snapshot.columns) - 1].symbol =
                    "X".into();
            }
            let layout = ui::auto_admin_layout(bounds, &model);
            assert_eq!(layout.terminal.width, initial.terminal.width);
            let bar = layout.terminal_scrollbar.unwrap();
            assert_eq!(bar.track.x, layout.terminal.right());
            assert_eq!(bar.track.intersection(layout.terminal).width, 0);
            if offset == 0 {
                assert_eq!(bar.thumb.bottom(), bar.track.bottom());
            } else if offset == 120 {
                assert_eq!(bar.thumb.y, bar.track.y);
            }
            let mut terminal =
                Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
            let context = RenderContext::default();
            terminal
                .draw(|frame| ui::render_auto_admin(frame, bounds, &model, &context))
                .unwrap();
            let buffer = terminal.backend().buffer();
            for y in layout.terminal.y..layout.terminal.bottom() {
                assert_eq!(buffer[(layout.terminal.right() - 1, y)].symbol(), "X");
            }
            assert_eq!(buffer[(bar.thumb.x, bar.thumb.y)].symbol(), "█");
            assert_eq!(
                buffer[(bar.thumb.x, bar.thumb.y)].fg,
                context.theme.accent_strong
            );
        }
    }
}

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
                if a < if confirming { 2 } else { 0 } {
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
fn running_auto_admin_highlights_buttons_and_preserves_masked_input() {
    for width in [50, 60, 180] {
        let bounds = Rect::new(0, 0, width, 50);
        let layout = ui::auto_admin_layout(bounds, &model(false, false));
        let mut model = AutoAdminViewModel {
            input_sensitive: false,
            description: "Remove demo".into(),
            status: "Running".into(),
            confirming: false,
            finished: false,
            stop: Default::default(),
            can_stop: false,
            can_kill: false,
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
                if area.is_empty() {
                    continue;
                }
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
                    !hint.contains(shortcut),
                    "unexpected instruction {shortcut} in {width}-column window: {hint}"
                );
            }
            assert!(hint.contains('•'));
        }
    }
}

#[test]
fn stop_controls_fit_and_the_warning_explains_damage_in_both_languages() {
    use ui::AutoAdminStopState;
    use unicode_width::UnicodeWidthStr;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for language in ["en-US", "zh-CN"] {
        let snapshot = i18n::LanguageSnapshot::load(&root, language, 1)
            .unwrap()
            .snapshot;
        let _language = i18n::enter_snapshot(std::sync::Arc::new(snapshot));
        for (width, height) in [(120, 40), (80, 24), (40, 12), (20, 8)] {
            let bounds = Rect::new(0, 0, width, height);
            let mut model = model(false, false);
            model.can_stop = true;
            model.can_kill = true;
            model.button_focus = Some(1);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    ui::render_auto_admin(frame, bounds, &model, &RenderContext::default())
                })
                .unwrap();
            let layout = ui::auto_admin_layout(bounds, &model);
            assert!(layout.buttons[1].width >= 8);
            let theme = TundraTheme::default();
            assert_eq!(
                terminal.backend().buffer()[(layout.buttons[1].x, layout.buttons[1].y)].fg,
                theme.accent_color
            );
            model.stop = AutoAdminStopState::Warning;
            model.button_focus = Some(0);
            let layout = ui::auto_admin_layout(bounds, &model);
            assert!(!layout.buttons[0].is_empty() && !layout.buttons[1].is_empty());
            assert!(layout.buttons[0].intersection(layout.buttons[1]).is_empty());
            for area in [layout.description, layout.buttons[0], layout.buttons[1]] {
                assert_eq!(area.intersection(layout.dialog), area);
            }
            // Scroll each text row, as a user can on a short terminal; none of
            // the damage warning may be permanently clipped by the action row.
            let mut displayed = String::new();
            for scroll in 0..=ui::auto_admin_max_scroll(bounds, &model) {
                model.scroll = scroll;
                terminal
                    .draw(|frame| {
                        ui::render_auto_admin(frame, bounds, &model, &RenderContext::default())
                    })
                    .unwrap();
                for y in layout.description.y..layout.description.bottom() {
                    let mut x = layout.description.x;
                    while x < layout.description.right() {
                        let cell = &terminal.backend().buffer()[(x, y)];
                        displayed.push_str(cell.symbol());
                        // The test backend can retain an old symbol in the
                        // second cell physically covered by a wide CJK glyph.
                        x += cell.symbol().width().max(1) as u16;
                    }
                }
            }
            let displayed = displayed.split_whitespace().collect::<String>();
            assert!(
                displayed.contains(if language == "zh-CN" {
                    "系统损坏"
                } else {
                    "damagethesystem"
                }),
                "{width}x{height}: {displayed}"
            );
            let buffer = terminal.backend().buffer();
            assert_eq!(
                buffer[(layout.buttons[0].x, layout.buttons[0].y)].fg,
                theme.accent_color
            );
            assert_eq!(
                buffer[(layout.buttons[1].x, layout.buttons[1].y)].fg,
                theme.foreground
            );
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
            (false, false, &[][..]),
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
        input_sensitive: false,
        description: "Remove demo\nTarget: demo-1.0".into(),
        status: "Waiting".into(),
        confirming,
        finished,
        stop: Default::default(),
        can_stop: false,
        can_kill: false,
        approve_selected: true,
        button_focus: None,
        scroll: 0,
        input: None,
        terminal: std::sync::Arc::new(CommandLineTerminalSnapshot::blank(104, 20)),
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
