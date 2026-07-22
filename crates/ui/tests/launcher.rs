use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ui::{
    HomeDisplayMode, LauncherConfirmationKind, LauncherConfirmationViewModel, LauncherDropSide,
    LauncherDropTarget, LauncherHitTarget, LauncherItemStatus, LauncherItemViewModel,
    LauncherToolbarAction, LauncherViewMode, LauncherViewModel, NotificationTone,
    ShellChromeViewModel, ShellLayout, StatusViewModel, TundraTheme, compute_shell_layout,
    launcher_layout, render_launcher,
};

fn item(index: usize, status: LauncherItemStatus) -> LauncherItemViewModel {
    LauncherItemViewModel::new(
        format!("item-{index}"),
        format!("Application {index}"),
        format!("C:/apps/app-{index}.exe"),
        "Native binary",
        status,
    )
}

fn chrome(size: (u16, u16)) -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".into(),
        build_mode: "test".into(),
        display_mode: HomeDisplayMode::User,
        terminal_size: size,
        screen_stack: vec!["Home".into(), "Launcher".into()],
        status: StatusViewModel {
            status: "Ready".into(),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    }
}

fn render_terminal(model: &LauncherViewModel, width: u16, height: u16) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("test terminal");
    terminal
        .draw(|frame| {
            render_launcher(
                frame,
                frame.area(),
                &chrome((width, height)),
                model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render launcher");
    terminal
}

fn render(model: &LauncherViewModel, width: u16, height: u16) -> String {
    let terminal = render_terminal(model, width, height);
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn text_has_fg(
    terminal: &Terminal<TestBackend>,
    area: Rect,
    text: &str,
    foreground: Color,
) -> bool {
    let symbols = text
        .chars()
        .map(|symbol| symbol.to_string())
        .collect::<Vec<_>>();
    let Ok(text_width) = u16::try_from(symbols.len()) else {
        return false;
    };
    if text_width == 0 || area.width < text_width {
        return false;
    }
    let buffer = terminal.backend().buffer();
    let last_x = area.right().saturating_sub(text_width);
    (area.y..area.bottom()).any(|y| {
        (area.x..=last_x).any(|x| {
            symbols.iter().enumerate().all(|(offset, symbol)| {
                let Ok(offset) = u16::try_from(offset) else {
                    return false;
                };
                buffer
                    .cell((x.saturating_add(offset), y))
                    .is_some_and(|cell| cell.symbol() == symbol && cell.fg == foreground)
            })
        })
    })
}

#[test]
fn large_icons_render_the_default_application_ascii_icon_when_native_icons_are_unavailable() {
    let model = LauncherViewModel::new(
        vec![item(0, LauncherItemStatus::Ready)],
        Some(0),
        LauncherViewMode::LargeIcons,
        false,
    );
    let output = render(&model, 100, 30);
    let icon_line = model
        .default_app_icon()
        .expect("default Application ASCII icon")
        .lines()
        .first()
        .expect("icon line")
        .trim();

    assert!(output.contains("Launcher · Large icons"));
    assert!(output.contains("Application 0"));
    assert!(!icon_line.is_empty());
    assert!(output.contains(icon_line));
}

#[test]
fn selected_ready_status_uses_the_accent_color_in_large_icons() {
    let width = 100;
    let height = 30;
    let theme = TundraTheme::default_dark();
    let selected = LauncherViewModel::new(
        vec![item(0, LauncherItemStatus::Ready)],
        Some(0),
        LauncherViewMode::LargeIcons,
        false,
    );
    let ShellLayout::Full { main, .. } = compute_shell_layout(Rect::new(0, 0, width, height))
    else {
        panic!("Launcher color test requires the full shell layout");
    };
    let item_area = launcher_layout(main, &selected).items[0].area;
    let selected_terminal = render_terminal(&selected, width, height);

    assert!(text_has_fg(
        &selected_terminal,
        item_area,
        "Ready",
        theme.accent_color,
    ));

    let unselected = LauncherViewModel::new(
        vec![
            item(0, LauncherItemStatus::Ready),
            item(1, LauncherItemStatus::Ready),
        ],
        Some(1),
        LauncherViewMode::LargeIcons,
        false,
    );
    let unselected_item_area = launcher_layout(main, &unselected).items[0].area;
    let unselected_terminal = render_terminal(&unselected, width, height);
    assert!(text_has_fg(
        &unselected_terminal,
        unselected_item_area,
        "Ready",
        theme.foreground,
    ));
}

#[test]
fn details_render_columns_and_all_item_integrity_labels() {
    let model = LauncherViewModel::new(
        vec![
            item(0, LauncherItemStatus::Ready),
            item(1, LauncherItemStatus::Changed),
            item(2, LauncherItemStatus::NeedsApproval),
        ],
        Some(1),
        LauncherViewMode::Details,
        false,
    );
    let output = render(&model, 100, 30);

    for label in [
        "Name",
        "Type",
        "Integrity",
        "Path",
        "Ready",
        "Changed",
        "Needs approval",
    ] {
        assert!(output.contains(label), "missing {label} in {output}");
    }
    assert!(output.contains("[A] Application 1"));
}

#[test]
fn toolbar_management_actions_are_admin_only() {
    let admin = LauncherViewModel::new(
        vec![item(0, LauncherItemStatus::NeedsApproval)],
        Some(0),
        LauncherViewMode::LargeIcons,
        true,
    );
    let user = LauncherViewModel::new(
        vec![item(0, LauncherItemStatus::Ready)],
        None,
        LauncherViewMode::LargeIcons,
        false,
    );

    assert_eq!(
        admin
            .toolbar
            .iter()
            .map(|button| button.action)
            .collect::<Vec<_>>(),
        vec![
            LauncherToolbarAction::Remove,
            LauncherToolbarAction::Reapprove,
            LauncherToolbarAction::Refresh,
            LauncherToolbarAction::ToggleView,
        ]
    );
    assert!(admin.toolbar[1].enabled);
    assert_eq!(
        user.toolbar
            .iter()
            .map(|button| button.action)
            .collect::<Vec<_>>(),
        vec![
            LauncherToolbarAction::Refresh,
            LauncherToolbarAction::ToggleView,
        ]
    );
}

#[test]
fn layouts_keep_selection_visible_and_hit_test_toolbar_items_and_scrollbar() {
    let model = LauncherViewModel::new(
        (0..20)
            .map(|index| item(index, LauncherItemStatus::Ready))
            .collect(),
        Some(19),
        LauncherViewMode::LargeIcons,
        true,
    );
    let layout = launcher_layout(Rect::new(0, 0, 45, 15), &model);
    assert!(layout.visible_start > 0);
    assert!(layout.items.iter().any(|item| item.index == 19));
    let button = layout.toolbar_buttons[0];
    assert_eq!(
        layout.hit_test(button.area.x, button.area.y),
        Some(LauncherHitTarget::Toolbar(LauncherToolbarAction::Remove))
    );
    let visible_item = layout.items[0];
    assert_eq!(
        layout.hit_test(visible_item.area.x, visible_item.area.y),
        Some(LauncherHitTarget::Item(visible_item.index))
    );
    let scrollbar = layout.scrollbar.expect("scrollbar for overflowing grid");
    assert_eq!(
        layout.hit_test(scrollbar.x, scrollbar.y),
        Some(LauncherHitTarget::Scrollbar)
    );
}

#[test]
fn large_icon_drop_target_uses_linear_insertion_boundaries_and_renders_a_vertical_line() {
    let mut model = LauncherViewModel::new(
        (0..3)
            .map(|index| item(index, LauncherItemStatus::Ready))
            .collect(),
        Some(0),
        LauncherViewMode::LargeIcons,
        true,
    );
    let layout = launcher_layout(Rect::new(0, 0, 80, 20), &model);
    let first = layout.items[0];
    let last = layout.items[2];

    assert_eq!(
        layout.large_icon_drop_target(first.area.x, first.area.y.saturating_add(1)),
        Some(LauncherDropTarget {
            item_index: 0,
            side: LauncherDropSide::Before,
        })
    );
    assert_eq!(
        layout.large_icon_drop_target(
            last.area.right().saturating_sub(1),
            last.area.y.saturating_add(1),
        ),
        Some(LauncherDropTarget {
            item_index: 2,
            side: LauncherDropSide::After,
        })
    );

    model.drop_target = Some(LauncherDropTarget {
        item_index: 1,
        side: LauncherDropSide::Before,
    });
    let layout = launcher_layout(Rect::new(0, 0, 80, 20), &model);
    let indicator = layout.drop_indicator.expect("large-icon insertion line");
    assert_eq!(indicator.x, layout.items[1].area.x);
    assert_eq!(indicator.height, layout.items[1].area.height);
    assert!(render(&model, 100, 30).contains('┃'));

    model.view_mode = LauncherViewMode::Details;
    assert!(
        launcher_layout(Rect::new(0, 0, 80, 20), &model)
            .drop_indicator
            .is_none()
    );
}

#[test]
fn empty_launcher_directs_users_to_explorer_without_an_add_action() {
    for view_mode in [LauncherViewMode::LargeIcons, LauncherViewMode::Details] {
        let model = LauncherViewModel::new(vec![], None, view_mode, true);
        let output = render(&model, 100, 30);

        assert!(output.contains("Go to Explorer, select a file"));
        assert!(output.contains("right-click and choose Add to Launcher"));
        assert!(
            model
                .toolbar
                .iter()
                .all(|button| button.action != LauncherToolbarAction::Remove)
        );
    }
}

#[test]
fn confirmation_overlay_takes_precedence_in_hit_testing_and_rendering() {
    let mut model = LauncherViewModel::new(
        vec![item(0, LauncherItemStatus::Ready)],
        None,
        LauncherViewMode::LargeIcons,
        true,
    );
    model.confirmation = Some(LauncherConfirmationViewModel {
        kind: LauncherConfirmationKind::Launch,
        title: "Launch application?".into(),
        message: "Open Application 0 using the system default?".into(),
        confirm_label: "Launch".into(),
        cancel_label: "Cancel".into(),
        confirm_selected: true,
    });
    let layout = launcher_layout(Rect::new(0, 0, 80, 20), &model);
    let dialog = layout.confirmation.expect("confirmation layout");
    assert_eq!(
        layout.hit_test(dialog.confirm.x, dialog.confirm.y),
        Some(LauncherHitTarget::Confirm)
    );
    assert_eq!(
        layout.hit_test(dialog.cancel.x, dialog.cancel.y),
        Some(LauncherHitTarget::Cancel)
    );
    assert_eq!(
        layout.hit_test(dialog.area.x, dialog.area.y),
        Some(LauncherHitTarget::OverlaySurface)
    );
    let output = render(&model, 100, 30);
    assert!(output.contains("Launch application?"));
    assert!(output.contains("[Launch]"));
}

#[test]
fn compact_terminal_falls_back_to_the_shared_compact_home() {
    let model = LauncherViewModel::new(
        vec![item(0, LauncherItemStatus::Ready)],
        None,
        LauncherViewMode::LargeIcons,
        false,
    );
    let output = render(&model, 20, 6);
    assert!(!output.contains("Launcher · Large icons"));
    assert!(output.contains("TundraUX 3"));
    assert!(output.contains("Ready"));
}
