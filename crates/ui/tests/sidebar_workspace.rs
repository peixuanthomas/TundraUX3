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
fn scrollable_sidebar_keeps_active_button_and_hit_regions_inside_short_viewports() {
    let labels = [
        "Appearance",
        "Region and time",
        "System",
        "File explorer",
        "Editor",
        "Update",
    ]
    .map(str::to_owned);
    for height in [1, 3, 8, 15, 30] {
        for active in 0..labels.len() {
            let area = Rect::new(2, 1, 110, height);
            let layout = SidebarWorkspaceLayout::scrollable(area, &labels, active);
            assert!(layout.vertical);
            assert!(!layout.navigation[active].is_empty());
            let theme = TundraTheme::default_dark();
            let mut context =
                RenderContext::from_theme(&theme, Default::default(), Default::default());
            context.buttons = Some(ButtonFrame::new(None, None, &theme));
            let buttons = labels
                .iter()
                .enumerate()
                .map(|(index, label)| Button::new(format!("category-{index}"), label))
                .collect::<Vec<_>>();
            let mut terminal = Terminal::new(TestBackend::new(120, height + 2)).unwrap();
            terminal
                .draw(|frame| layout.render_navigation(frame, &buttons, active, &context))
                .unwrap();
            let regions = context.buttons.as_ref().unwrap().regions();
            for (index, rect) in layout.navigation.iter().enumerate() {
                let region = regions.iter().find(|region| region.id == buttons[index].id);
                if rect.is_empty() {
                    assert!(region.is_none());
                } else {
                    assert_eq!(rect.intersection(area), *rect);
                    assert!(!rect.intersects(layout.content));
                    assert_eq!(region.unwrap().area, *rect);
                }
            }
        }
    }
}
