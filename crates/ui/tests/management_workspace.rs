use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use std::sync::Arc;
use ui::components::{ButtonFrame, WorkspaceFocus};
use ui::*;

fn model(advanced_open: bool) -> ManagementViewModel {
    ManagementViewModel {
        workspace: Some(ManagementWorkspace {
            navigation_actions: vec![0, 1, 2],
            active_navigation: 0,
            primary_actions: vec![6, 7],
            advanced_toggle: Some(3),
            advanced_actions: vec![4, 5],
            advanced_open,
            focus: WorkspaceFocus::Actions,
        }),
        title: "Packages · Search".into(),
        columns: vec!["Package".into(), "Version".into()],
        rows: (0..40)
            .map(|index| vec![format!("package-{index}"), "1.2".into()])
            .collect(),
        details: "Package information\n".repeat(25),
        actions: [
            "F6 Search",
            "F8 Updates",
            "F7 Installed",
            "F3 Advanced · Software sources",
            "5 Software sources",
            "E Refresh sources",
            "I Install",
            "F4 Details",
        ]
        .map(|label| (label.into(), true))
        .into(),
        action_ids: (0..8)
            .map(|index| format!("package-action-{index}"))
            .collect(),
        actions_focused: true,
        selected_action: 6,
        ..Default::default()
    }
}

#[test]
fn packages_distinguishes_navigation_table_details_and_empty_rows() {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for language in ["en-US", "zh-CN"] {
        let snapshot = i18n::LanguageSnapshot::load(&assets, language, 1)
            .unwrap()
            .snapshot;
        let _language = i18n::enter_snapshot(Arc::new(snapshot));
        for (width, height) in [(108, 20), (120, 36), (180, 50)] {
            for color in [
                ColorCapability::TrueColor,
                ColorCapability::Indexed,
                ColorCapability::Ansi,
            ] {
                let main = Rect::new(0, 1, width, height - 2);
                let mut model = model(false);
                model.rows.truncate(4);
                model.rows[0][0] = "宽字符 e\u{301}".into();
                let layout = management_layout(main, &model);
                let nav = layout.workspace.as_ref().unwrap().navigation[0];
                let context = RenderContext::from_theme(
                    &TundraTheme::default(),
                    MotionFrame::reduced(std::time::Duration::ZERO),
                    RenderCapabilities {
                        color,
                        ..Default::default()
                    },
                );
                let tokens = context.theme;
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| render_management_content(frame, main, &model, &context))
                    .unwrap();
                let buffer = terminal.backend().buffer();
                assert_eq!(buffer[(nav.x, nav.y)].bg, tokens.raised);
                assert_eq!(
                    buffer[(layout.details_text.x, layout.details_text.y)].bg,
                    tokens.raised
                );
                assert_eq!(
                    buffer[(layout.headers[0].1.x, layout.headers[0].1.y)].bg,
                    tokens.raised
                );
                let x = layout.list_rows.right() - 1;
                let y = layout.list_rows.y;
                assert_eq!(buffer[(x, y)].bg, tokens.accent_soft);
                assert_eq!(buffer[(x, y + 1)].bg, tokens.row_background(1));
                assert_eq!(buffer[(x, y + 2)].bg, tokens.row_background(2));
                if color != ColorCapability::Ansi {
                    assert_ne!(buffer[(x, y + 1)].bg, buffer[(x, y + 2)].bg);
                }
                model.rows.clear();
                model.empty_message = "No result".into();
                terminal
                    .draw(|frame| render_management_content(frame, main, &model, &context))
                    .unwrap();
                let buffer = terminal.backend().buffer();
                assert_eq!(buffer[(x, y)].bg, tokens.surface);
                assert_eq!(buffer[(layout.list_rows.x, y)].fg, tokens.muted);
            }
        }
    }
}

#[test]
fn borderless_details_use_the_shared_keyboard_focus_style() {
    let mut model = model(false);
    model.workspace.as_mut().unwrap().focus = WorkspaceFocus::Details;
    let main = Rect::new(0, 0, 120, 30);
    let layout = management_layout(main, &model);
    let theme = TundraTheme::default_dark();
    let mut terminal = Terminal::new(TestBackend::new(main.width, main.height)).unwrap();
    for keyboard in [true, false] {
        let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let mut buttons = ButtonFrame::new(None, None, &theme);
        buttons.keyboard_focus_visible = keyboard;
        context.buttons = Some(buttons);
        terminal
            .draw(|frame| render_management_content(frame, main, &model, &context))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(layout.details.x, layout.details.y)]
                .modifier
                .contains(ratatui::style::Modifier::UNDERLINED),
            keyboard
        );
    }
}

#[test]
fn workspace_actions_stay_separate_from_search_list_and_details() {
    for open in [false, true] {
        let model = model(open);
        for (width, height) in [(108, 20), (160, 40), (50, 12)] {
            let theme = TundraTheme::default_dark();
            let mut context =
                RenderContext::from_theme(&theme, Default::default(), Default::default());
            context.buttons = Some(ButtonFrame::new(None, None, &theme));
            let main = ShellFrameLayout::new(Rect::new(0, 0, width, height), None, &context).main;
            let layout = management_layout(main, &model);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render_management_content(frame, main, &model, &context))
                .unwrap();
            let painted = context.buttons.as_ref().unwrap().regions();
            let expected = management_button_regions(main, &model);
            for button in &expected {
                if button.area == layout.filter {
                    continue;
                }
                assert!(painted.contains(button), "{width}x{height} {button:?}");
            }
            for (index, action) in layout.actions.iter().enumerate() {
                if !open && [4, 5].contains(&index) {
                    assert!(action.is_empty());
                    continue;
                }
                assert!(!action.is_empty(), "{width}x{height} action {index}");
                assert_eq!(action.intersection(main), *action);
                assert!(action.intersection(layout.filter).is_empty());
                assert!(action.intersection(layout.list_rows).is_empty());
                assert!(action.intersection(layout.details_text).is_empty());
                for other in layout.actions.iter().skip(index + 1) {
                    assert!(
                        action.intersection(*other).is_empty(),
                        "{width}x{height} open={open} action {index}: {action:?} overlaps {other:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn workspace_without_advanced_actions_has_no_advanced_buttons_or_focus_targets() {
    let mut model = model(false);
    let workspace = model.workspace.as_mut().unwrap();
    workspace.advanced_toggle = None;
    workspace.advanced_actions.clear();
    assert!(workspace.action_group(WorkspaceFocus::Advanced).is_empty());
    for (width, height) in [(108, 20), (160, 40), (50, 12)] {
        let main = Rect::new(0, 0, width, height);
        let layout = management_layout(main, &model);
        for index in [3, 4, 5] {
            assert!(layout.actions[index].is_empty());
        }
        for index in [0, 1, 2, 6, 7] {
            assert!(!layout.actions[index].is_empty());
        }
        let theme = TundraTheme::default_dark();
        let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render_management_content(frame, main, &model, &context))
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(!text.contains("Advanced"));
    }
}

#[test]
fn workspace_overlay_captures_only_its_own_buttons() {
    let mut model = model(true);
    model.form = Some(ManagementForm {
        identity: "review".into(),
        title: "Review".into(),
        ..Default::default()
    });
    let theme = TundraTheme::default_dark();
    let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    context.buttons = Some(ButtonFrame::new(None, None, &theme));
    let area = Rect::new(0, 0, 120, 30);
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|frame| {
            render_management_content(frame, area, &model, &context);
            render_management_overlay(frame, area, &model, &context);
        })
        .unwrap();
    assert_eq!(
        context.buttons.as_ref().unwrap().regions(),
        management_button_regions(area, &model)
    );
}

#[test]
fn workspace_wraps_long_action_labels_without_losing_their_shortcuts() {
    for label in [
        "[U] Upgrade package and the entire system",
        "[U] 更新所选软件包并升级整个系统",
    ] {
        let mut model = model(false);
        model.actions[6].0 = label.into();
        let theme = TundraTheme::default_dark();
        let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let main = ShellFrameLayout::new(Rect::new(0, 0, 108, 20), None, &context).main;
        let layout = management_layout(main, &model);
        let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
        terminal
            .draw(|frame| render_management_content(frame, main, &model, &context))
            .unwrap();
        let area = layout.actions[6];
        let text = (area.y..area.bottom())
            .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
            .map(|point| terminal.backend().buffer()[point].symbol())
            .collect::<String>();
        assert_eq!(
            text.split_whitespace().collect::<String>(),
            label.split_whitespace().collect::<String>()
        );
    }
}

#[test]
fn expanded_details_scroll_to_clipped_text_without_covering_it_with_scrollbars() {
    for (width, height) in [(108, 20), (109, 20), (160, 40)] {
        let mut model = model(false);
        model.details_only = true;
        model.details = (0..80)
            .map(|index| format!("{} {index:02}END", "软件包".repeat(80)))
            .collect::<Vec<_>>()
            .join("\n");
        model.details_scroll = u16::MAX;
        model.details_horizontal_scroll = u16::MAX;
        let theme = TundraTheme::default_dark();
        let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let main = Rect::new(0, 0, width, height);
        let layout = management_layout(main, &model);
        for target in [
            ManagementScrollTarget::Details,
            ManagementScrollTarget::DetailsColumns,
        ] {
            let bar = layout
                .scrollbars
                .iter()
                .find(|bar| bar.target == target)
                .unwrap();
            assert_eq!(bar.offset, bar.content_len - bar.viewport_len);
            assert!(bar.track.intersection(layout.details_text).is_empty());
            assert_eq!(bar.track.intersection(layout.details), bar.track);
        }
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| render_management_content(frame, main, &model, &context))
            .unwrap();
        let area = layout.details_text;
        let columns = layout
            .scrollbars
            .iter()
            .find(|bar| bar.target == ManagementScrollTarget::DetailsColumns)
            .unwrap();
        if columns.offset % 2 == 1 {
            assert_eq!(terminal.backend().buffer()[(area.x, area.y)].symbol(), " ");
        }
        let last_line = (area.x..area.right())
            .map(|x| terminal.backend().buffer()[(x, area.bottom() - 1)].symbol())
            .collect::<String>();
        assert!(
            last_line.ends_with("79END"),
            "{width}x{height}: {last_line}"
        );
    }
}
