use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::components::{ButtonFrame, WorkspaceFocus};
use ui::*;

fn model(advanced_open: bool) -> ManagementViewModel {
    ManagementViewModel {
        workspace: Some(ManagementWorkspace {
            navigation_actions: vec![0, 1, 2],
            active_navigation: 0,
            primary_actions: vec![6, 7],
            advanced_toggle: 3,
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
fn details_focus_border_follows_keyboard_visibility() {
    let mut model = model(false);
    model.workspace.as_mut().unwrap().focus = WorkspaceFocus::Details;
    model.actions_focused = false;
    let theme = TundraTheme::default_dark();
    for keyboard in [false, true] {
        let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let mut buttons = ButtonFrame::new(None, None, &theme);
        buttons.keyboard_focus_visible = keyboard;
        context.buttons = Some(buttons);
        let area = Rect::new(0, 0, 120, 30);
        let layout = management_layout(area, &model);
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|frame| render_management_content(frame, area, &model, &context))
            .unwrap();
        assert_eq!(
            terminal.backend().buffer()[(layout.details.x, layout.details.y)].fg,
            if keyboard {
                theme.accent_color
            } else {
                theme.border_style().fg.unwrap()
            }
        );
    }
}

#[test]
fn selected_package_row_remains_visible_after_pointer_input_and_focus_changes() {
    use ratatui::style::Modifier;
    for keyboard in [false, true] {
        for focus in [
            WorkspaceFocus::List,
            WorkspaceFocus::Details,
            WorkspaceFocus::Actions,
        ] {
            let mut model = model(false);
            model.selected = 1;
            model.workspace.as_mut().unwrap().focus = focus;
            model.actions_focused = focus == WorkspaceFocus::Actions;
            let theme = TundraTheme::default_dark();
            let mut context =
                RenderContext::from_theme(&theme, Default::default(), Default::default());
            let mut buttons = ButtonFrame::new(None, None, &theme);
            buttons.keyboard_focus_visible = keyboard;
            context.buttons = Some(buttons);
            let area = Rect::new(0, 0, 120, 30);
            let layout = management_layout(area, &model);
            let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
            terminal
                .draw(|frame| render_management_content(frame, area, &model, &context))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let row = layout.list_rows.y + 1;
            assert!(
                buffer[(layout.list_rows.x, row)]
                    .modifier
                    .contains(Modifier::REVERSED),
                "keyboard={keyboard}, focus={focus:?}"
            );
            assert!(
                !buffer[(layout.list_rows.x, layout.list_rows.y)]
                    .modifier
                    .contains(Modifier::REVERSED)
            );
        }
    }
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
fn search_button_shows_the_key_that_submits_in_each_focus_state() {
    for filtering in [false, true] {
        let mut model = model(false);
        model.filtering = filtering;
        let theme = TundraTheme::default_dark();
        let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let main = Rect::new(0, 0, 108, 20);
        let layout = management_layout(main, &model);
        let area = layout
            .controls
            .iter()
            .find(|(control, _)| *control == ManagementControl::ApplySearch)
            .unwrap()
            .1;
        let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
        terminal
            .draw(|frame| render_management_content(frame, main, &model, &context))
            .unwrap();
        let text = (area.x..area.right())
            .map(|x| terminal.backend().buffer()[(x, area.y)].symbol())
            .collect::<String>();
        assert!(text.contains(if filtering { "[Enter]" } else { "[Ctrl+Enter]" }));
    }
}
