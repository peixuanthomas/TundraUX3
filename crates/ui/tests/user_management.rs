#[path = "support/composition.rs"]
mod composition;
use composition as ui;
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use support::terminal_output;
use ui::{
    HomeDisplayMode, NotificationTone, ShellChromeViewModel, StatusViewModel, TundraTheme,
    UserManagementAction, UserManagementField, UserManagementFocus, UserManagementFormKind,
    UserManagementFormViewModel, UserManagementUserViewModel, UserManagementViewModel,
    compute_shell_layout, render_user_management, user_management_action_at,
    user_management_form_control_at, user_management_layout, user_management_row_index_at,
};

#[test]
fn narrow_user_actions_wrap_and_short_forms_expose_the_remaining_fields() {
    let mut model = model_with_users(20);
    let area = Rect::new(0, 0, 40, 12);
    let layout = user_management_layout(area, &model);
    assert!(layout.actions.iter().all(|action| !action.area.is_empty()
        && action.area.right() <= area.right()
        && action.area.bottom() <= area.bottom()));
    assert!(
        layout
            .actions
            .iter()
            .any(|action| action.area.y > layout.actions_area.y)
    );
    assert!(layout.scrollbar.is_some());
    model.form = Some(create_form(UserManagementField::Password));
    let form = user_management_layout(Rect::new(0, 0, 30, 4), &model)
        .form
        .unwrap();
    assert!(form.scrollbar.is_some());
    assert!(
        form.fields
            .iter()
            .any(|field| field.field == UserManagementField::Password)
    );
    assert!(!form.submit.is_empty() && !form.cancel.is_empty());
}

#[test]
fn manual_user_window_can_move_away_from_the_selected_account() {
    let mut model = model_with_users(12);
    model.selected_index = 10;
    model.user_window_start = 0;

    let layout = user_management_layout(Rect::new(4, 7, 80, 10), &model);

    assert_eq!(layout.visible_capacity, 5);
    assert_eq!(layout.visible_start, 0);
    assert_eq!(layout.rows.first().map(|row| row.index), Some(0));
    assert_eq!(layout.rows.last().map(|row| row.index), Some(4));
    assert!(layout.scrollbar.is_some());
}

#[test]
fn row_action_and_form_hit_helpers_use_the_rendered_rectangles() {
    let mut model = model_with_users(4);
    model.form = Some(create_form(UserManagementField::Role));
    let layout = user_management_layout(Rect::new(5, 3, 100, 18), &model);

    let row = layout.rows[1];
    assert_eq!(
        user_management_row_index_at(&layout, (row.area.x, row.area.y)),
        Some(row.index)
    );
    let action = layout.actions[0];
    assert_eq!(
        user_management_action_at(&layout, (action.area.x, action.area.y)),
        Some(action.action)
    );
    let form = layout.form.as_ref().expect("form layout");
    let role = form
        .fields
        .iter()
        .find(|field| field.field == UserManagementField::Role)
        .expect("role field");
    assert_eq!(
        user_management_form_control_at(&layout, (role.area.x, role.area.y)),
        Some(UserManagementField::Role)
    );
    assert_eq!(
        layout.form_control_at(form.cancel.x, form.cancel.y),
        Some(UserManagementField::Cancel)
    );
}

#[test]
fn default_admin_actions_protect_the_last_enabled_administrator() {
    let model = UserManagementViewModel::new(
        "root",
        vec![user("root", "Administrator", "Admin", true, false, true)],
        0,
        None,
        true,
        None,
    );

    assert_eq!(model.actions.len(), 6);
    assert!(
        model.actions.iter().all(|action| action.shortcut.is_some()),
        "page actions must contain account operations only"
    );
    for action in [
        UserManagementAction::ToggleEnabled,
        UserManagementAction::ToggleRole,
        UserManagementAction::Delete,
    ] {
        let action = model
            .actions
            .iter()
            .find(|candidate| candidate.action == action)
            .expect("standard action");
        assert!(!action.enabled);
        assert!(action.disabled_reason.is_some());
    }
    assert!(
        model
            .actions
            .iter()
            .all(|action| !action.label.to_ascii_lowercase().contains("debug"))
    );
}

#[test]
fn create_form_is_a_modal_with_role_password_and_action_focus() {
    let mut model = model_with_users(2);
    model.form = Some(create_form(UserManagementField::Submit));
    let (terminal, main) = render(100, 24, &model);
    let output = terminal_output(&terminal);
    let layout = user_management_layout(main, &model);
    let form = layout.form.expect("form geometry");

    assert!(output.contains("Create user"));
    assert!(output.contains("Username") && output.contains("alice"));
    assert!(output.contains("Display name") && output.contains("Alice Chen"));
    assert!(output.contains("ROLE") && output.contains("User"));
    assert!(output.contains("Password") && output.contains("••••••••••••"));
    assert!(output.contains("[F10 Create]"));
    assert!(output.contains("[Esc Cancel]"));
    assert!(!output.contains("Ctrl+Enter: Create"));
    assert!(region_has_fg(
        &terminal,
        form.submit,
        TundraTheme::default_dark().accent_color
    ));
}

#[test]
fn form_text_inputs_are_controlled_and_keep_mask_cursor_and_inline_frame() {
    let mut model = model_with_users(1);
    model.form = Some(create_form(UserManagementField::Username));
    let (terminal, _) = render(100, 24, &model);
    assert!(terminal_output(&terminal).contains("alice_"));

    model.form = Some(create_form(UserManagementField::Password));
    let (terminal, _) = render(100, 24, &model);
    let output = terminal_output(&terminal);
    assert!(output.contains("••••••••••••_"));
    assert!(!output.contains("alice_"));
}

#[test]
fn minimum_full_create_form_keeps_the_focused_field_and_actions_visible() {
    let mut model = model_with_users(1);
    for focused in [
        UserManagementField::Username,
        UserManagementField::DisplayName,
        UserManagementField::Role,
        UserManagementField::Password,
    ] {
        model.form = Some(create_form(focused));
        let area = Rect::new(0, 3, 50, 6);
        let form = user_management_layout(area, &model).form.unwrap();
        let field = form
            .fields
            .iter()
            .find(|field| field.field == focused)
            .unwrap();
        assert!(!field.area.is_empty());
        assert!(!form.submit.is_empty() && !form.cancel.is_empty());
        assert!(form.scrollbar.is_some());
        let (terminal, _) = render(50, 12, &model);
        assert!(!terminal_output(&terminal).contains("************"));
    }
}

#[test]
fn edit_and_password_forms_only_render_relevant_inputs() {
    let mut model = model_with_users(1);
    model.form = Some(UserManagementFormViewModel {
        interaction: Default::default(),
        editing: Default::default(),
        kind: UserManagementFormKind::EditInfo,
        title: "Edit user info".to_string(),
        username: "user0".to_string(),
        display_name: "Renamed".to_string(),
        role: String::new(),
        password_len: 0,
        focused_field: UserManagementField::DisplayName,
        error: None,
    });
    let (terminal, main) = render(90, 22, &model);
    let edit = form_output(&terminal, main, &model);
    assert!(edit.contains("Display name") && edit.contains("Renamed"));
    assert!(!edit.contains("ROLE"));
    assert!(!edit.contains("Password"));

    model.form = Some(UserManagementFormViewModel {
        interaction: Default::default(),
        editing: Default::default(),
        kind: UserManagementFormKind::Password,
        title: "Set password".to_string(),
        username: "user0".to_string(),
        display_name: String::new(),
        role: String::new(),
        password_len: 9,
        focused_field: UserManagementField::Cancel,
        error: Some("Password is too weak".to_string()),
    });
    let (terminal, main) = render(90, 22, &model);
    let password = form_output(&terminal, main, &model);
    assert!(password.contains("Password") && password.contains("•••••••••"));
    assert!(password.contains("Password is too weak"));
}

fn model_with_users(count: usize) -> UserManagementViewModel {
    let users = (0..count)
        .map(|index| {
            user(
                &format!("user{index}"),
                &format!("User {index}"),
                if index == 0 { "Admin" } else { "User" },
                true,
                false,
                index == 0,
            )
        })
        .collect();
    UserManagementViewModel::new("user0", users, 0, None, true, None)
}

fn user(
    username: &str,
    display_name: &str,
    role: &str,
    enabled: bool,
    locked: bool,
    is_current: bool,
) -> UserManagementUserViewModel {
    UserManagementUserViewModel {
        username: username.to_string(),
        display_name: display_name.to_string(),
        role: role.to_string(),
        enabled,
        locked,
        is_current,
    }
}

fn create_form(focused_field: UserManagementField) -> UserManagementFormViewModel {
    UserManagementFormViewModel {
        interaction: Default::default(),
        editing: Default::default(),
        kind: UserManagementFormKind::Create,
        title: "Create user".to_string(),
        username: "alice".to_string(),
        display_name: "Alice Chen".to_string(),
        role: "User".to_string(),
        password_len: 12,
        focused_field,
        error: None,
    }
}

fn render(
    width: u16,
    height: u16,
    model: &UserManagementViewModel,
) -> (Terminal<TestBackend>, Rect) {
    let chrome = ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (width, height),
        back_button_hovered: false,
        help_available: false,
        help_button_hovered: false,
        help_button_focused: false,
        back_shortcut: "Esc",
        screen_stack: vec!["UserManagement".to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    };
    let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
    terminal
        .draw(|frame| {
            render_user_management(
                frame,
                frame.area(),
                &chrome,
                model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render user management");
    let main = match compute_shell_layout(Rect::new(0, 0, width, height)) {
        ui::ShellLayout::Full { main, .. } => main,
        ui::ShellLayout::Compact(_) => panic!("test requires full layout"),
    };
    (terminal, main)
}

fn region_has_fg(terminal: &Terminal<TestBackend>, area: Rect, fg: Color) -> bool {
    let buffer = terminal.backend().buffer();
    (area.y..area.bottom()).any(|y| {
        (area.x..area.right()).any(|x| {
            let cell = &buffer[(x, y)];
            cell.fg == fg && !cell.symbol().trim().is_empty()
        })
    })
}

#[test]
fn page_actions_and_more_menu_share_visible_targets() {
    let mut model = model_with_users(3);
    let bounds = Rect::new(0, 0, 120, 36);
    let layout = user_management_layout(bounds, &model);
    assert_eq!(layout.action_bar.actions.len(), 5);
    assert!(
        layout
            .action_bar
            .actions
            .iter()
            .any(|action| action.id.as_str() == "action.Delete"
                && action.emphasis == ui::components::ButtonEmphasis::Danger)
    );
    assert!(
        !layout
            .actions
            .iter()
            .any(|action| action.action == UserManagementAction::ToggleRole)
    );
    model.more_open = true;
    model.focus = UserManagementFocus::Action(UserManagementAction::ToggleRole);
    let layout = user_management_layout(bounds, &model);
    assert!(layout.more_menu.is_some());
    assert!(
        layout
            .actions
            .iter()
            .any(|action| action.action == UserManagementAction::ToggleRole)
    );
    assert!(
        layout
            .row_index_at(layout.rows[0].area.x, layout.rows[0].area.y)
            .is_none()
    );
    for action in &layout.actions {
        assert_eq!(
            layout.action_at(action.area.x, action.area.y),
            Some(action.action)
        );
    }
}

fn form_output(
    terminal: &Terminal<TestBackend>,
    main: Rect,
    model: &UserManagementViewModel,
) -> String {
    let area = user_management_layout(main, model).form.unwrap().dialog;
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
