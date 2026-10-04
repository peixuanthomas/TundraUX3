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
            let layout = ui::auto_admin_layout(bounds, confirming);
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
                assert!(left.width > 0 && left.height > 0);
                for right in layout.buttons.iter().skip(a + 1) {
                    assert_eq!(left.intersection(*right).width, 0);
                }
            }
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let model = AutoAdminViewModel {
                description: "Remove demo\nTarget: demo-1.0".into(),
                status: "Waiting".into(),
                confirming,
                finished: false,
                approve_selected: false,
                scroll: 0,
                input: None,
                terminal: std::sync::Arc::new(CommandLineTerminalSnapshot::blank(
                    layout.terminal.width,
                    layout.terminal.height,
                )),
            };
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
fn auto_admin_preserves_title_and_status_cells_in_every_phase() {
    for (width, height) in [(220, 65), (120, 40), (80, 24), (50, 12), (40, 12), (20, 8)] {
        for (confirming, finished) in [(true, false), (false, false), (false, true)] {
            let bounds = Rect::new(0, 0, width, height);
            let main = match ui::compute_shell_layout(bounds) {
                ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
            };
            let layout = ui::auto_admin_layout(bounds, confirming);
            let model = AutoAdminViewModel {
                description: "Remove neofetch".into(),
                status: "Status".into(),
                confirming,
                finished,
                approve_selected: false,
                scroll: 0,
                input: None,
                terminal: std::sync::Arc::new(CommandLineTerminalSnapshot::blank(
                    layout.terminal.width,
                    layout.terminal.height,
                )),
            };
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
                    }
                }
            }
        }
    }
}
