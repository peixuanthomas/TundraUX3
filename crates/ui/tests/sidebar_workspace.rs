use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::{
    RenderContext, TundraTheme,
    components::{Button, ButtonFrame, SidebarWorkspaceLayout, WorkspaceFocus},
};

#[test]
fn navigation_and_panes_fit_english_chinese_and_small_content_areas() {
    for labels in [
        ["Search [F6]", "Updates [F8]", "Installed [F7]"],
        ["搜索 [F6]", "更新 [F8]", "已安装 [F7]"],
    ] {
        let labels = labels.map(str::to_owned);
        for (width, height) in [(108, 14), (160, 30), (50, 6), (20, 4), (0, 0)] {
            let area = Rect::new(3, 2, width, height);
            let layout = SidebarWorkspaceLayout::new(area, &labels);
            for rect in &layout.navigation {
                assert_eq!(rect.intersection(area), *rect);
                assert!(rect.intersection(layout.content).is_empty());
            }
            let (list, details) = SidebarWorkspaceLayout::panes(layout.content, false);
            assert!(list.intersection(details).is_empty());
            assert_eq!(list.intersection(layout.content), list);
            assert_eq!(details.intersection(layout.content), details);
            assert_eq!(layout.vertical, width >= 92);
        }
    }
}

#[test]
fn navigation_uses_shared_button_regions_and_keyboard_focus_color() {
    let theme = TundraTheme::default_dark();
    let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let mut buttons = vec![
        Button::new("search", "Search [F6]"),
        Button::new("updates", "Updates [F8]"),
    ];
    buttons[1].set_focused(true);
    context.buttons = Some(ButtonFrame::new(None, None, &theme));
    let area = Rect::new(0, 0, 108, 20);
    let layout = SidebarWorkspaceLayout::new(
        area,
        &buttons
            .iter()
            .map(|button| button.label.clone())
            .collect::<Vec<_>>(),
    );
    let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
    terminal
        .draw(|frame| layout.render_navigation(frame, &buttons, 0, &context))
        .unwrap();
    let regions = context.buttons.as_ref().unwrap().regions();
    for (button, area) in buttons.iter().zip(&layout.navigation) {
        assert!(
            regions
                .iter()
                .any(|region| region.id == button.id && region.area == *area)
        );
    }
    let focused = layout.navigation[1];
    assert_eq!(
        terminal.backend().buffer()[(focused.x, focused.y)].fg,
        theme.accent_color
    );
}

#[test]
fn focus_cycles_all_six_regions_in_both_directions() {
    let mut focus = WorkspaceFocus::Navigation;
    let mut visited = Vec::new();
    for _ in 0..6 {
        visited.push(focus);
        assert_eq!(focus.step(false).step(true), focus);
        focus = focus.step(false);
    }
    assert_eq!(focus, WorkspaceFocus::Navigation);
    assert!(
        visited.contains(&WorkspaceFocus::Details) && visited.contains(&WorkspaceFocus::Advanced)
    );
}

#[test]
fn current_page_keeps_accent_and_literal_arrow_in_pointer_mode() {
    for width in [60, 120] {
        let theme = TundraTheme::default_dark();
        let mut context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let mut interactions = ButtonFrame::new(None, None, &theme);
        interactions.keyboard_focus_visible = false;
        context.buttons = Some(interactions);
        let buttons = [
            Button::new("installed", "[F7] 已安装"),
            Button::new("updates", "[F8] 更新"),
        ];
        let layout = SidebarWorkspaceLayout::new(
            Rect::new(0, 0, width, 20),
            &buttons
                .iter()
                .map(|button| button.label.clone())
                .collect::<Vec<_>>(),
        );
        let mut terminal = Terminal::new(TestBackend::new(width, 20)).unwrap();
        terminal
            .draw(|frame| layout.render_navigation(frame, &buttons, 0, &context))
            .unwrap();
        let area = layout.navigation[0];
        let buffer = terminal.backend().buffer();
        let text = (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            text.split_whitespace()
                .collect::<String>()
                .contains("->[F7]已安装"),
            "{text}"
        );
        assert!(!text.contains("[->"), "{text}");
        let arrow = (area.y..area.bottom())
            .flat_map(|y| (area.x..area.right()).map(move |x| (x, y)))
            .find(|point| buffer[*point].symbol() == ">")
            .unwrap();
        assert_eq!(buffer[arrow].fg, theme.accent_color);
        if layout.vertical {
            assert_eq!(buffer[(area.x, area.y)].fg, theme.accent_color);
        }
        let inactive = layout.navigation[1];
        assert_ne!(buffer[(inactive.x, inactive.y)].fg, theme.accent_color);
    }
}
