use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use tundra_ui::{
    HomeDisplayMode, NotificationTone, ShellChromeViewModel, StatusViewModel, TundraTheme,
    UserManagementAction, UserManagementColumnMode, UserManagementFeedbackTone,
    UserManagementField, UserManagementFocus, UserManagementFormKind, UserManagementFormViewModel,
    UserManagementUserViewModel, UserManagementViewModel, compute_shell_layout,
    render_user_management, user_management_action_at, user_management_form_control_at,
    user_management_layout, user_management_row_index_at,
};

#[test]
fn layout_switches_columns_at_72_cells_and_keeps_one_row_at_minimum_height() {
    let model = model_with_users(3);
    let account = user_management_layout(Rect::new(0, 0, 71, 6), &model);
    let detailed = user_management_layout(Rect::new(0, 0, 72, 6), &model);

    assert_eq!(account.column_mode, UserManagementColumnMode::Account);
    assert_eq!(detailed.column_mode, UserManagementColumnMode::Detailed);
    assert_eq!(account.visible_capacity, 1);
    assert_eq!(account.rows.len(), 1);
    let new_user = detailed
        .actions
        .iter()
        .find(|action| action.action == UserManagementAction::NewUser)
        .expect("new user action");
    assert_eq!(new_user.area.y, detailed.summary.y);
    assert!(new_user.area.x >= detailed.summary.right());
}

#[test]
fn selected_user_is_forced_into_the_visible_window() {
    let mut model = model_with_users(12);
    model.selected_index = 10;
    model.user_window_start = 0;

    let layout = user_management_layout(Rect::new(4, 7, 80, 10), &model);

    assert_eq!(layout.visible_capacity, 4);
    assert_eq!(layout.visible_start, 7);
    assert_eq!(layout.rows.first().map(|row| row.index), Some(7));
    assert_eq!(layout.rows.last().map(|row| row.index), Some(10));
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
fn renderer_draws_detailed_table_status_precedence_and_current_marker() {
    let mut model = UserManagementViewModel::new(
        "root",
        vec![
            user("root", "Administrator", "Admin", true, false, true),
            user("locked", "Locked User", "User", true, true, false),
            user("off", "Disabled User", "User", false, true, false),
        ],
        0,
        None,
        true,
        None,
    );
    model.focus = UserManagementFocus::UserList;
    let (terminal, main) = render(100, 24, &model);
    let output = terminal_output(&terminal);
    let layout = user_management_layout(main, &model);

    assert!(output.contains("Signed in: root"));
    assert!(output.contains("USERNAME"));
    assert!(output.contains("DISPLAY NAME"));
    assert!(output.contains("Enabled · You"));
    assert!(output.contains("Locked"));
    assert!(output.contains("Disabled"));
    assert!(!output.contains("Disabled · You"));
    assert!(region_has_fg(
        &terminal,
        layout.rows[0].area,
        TundraTheme::default_dark().accent
    ));
}

#[test]
fn medium_renderer_uses_account_column_and_truncates_long_values() {
    let model = UserManagementViewModel::new(
        "root",
        vec![user(
            "an-extremely-long-username",
            "An extraordinarily long display name",
            "User",
            true,
            false,
            false,
        )],
        0,
        None,
        true,
        None,
    );
    let (terminal, _) = render(71, 18, &model);
    let output = terminal_output(&terminal);

    assert!(output.contains("ACCOUNT"));
    assert!(!output.contains("DISPLAY NAME"));
    assert!(output.contains('…'));
}

#[test]
fn disabled_action_is_muted_and_exposes_its_reason() {
    let mut model = UserManagementViewModel::new(
        "root",
        vec![user("root", "Administrator", "Admin", true, false, true)],
        0,
        None,
        true,
        None,
    );
    model.focus = UserManagementFocus::Action(UserManagementAction::Delete);
    let (terminal, main) = render(120, 24, &model);
    let layout = user_management_layout(main, &model);
    let delete = layout
        .actions
        .iter()
        .find(|action| action.action == UserManagementAction::Delete)
        .expect("delete layout");

    assert!(terminal_output(&terminal).contains("last enabled administrator"));
    assert!(region_has_fg(
        &terminal,
        delete.area,
        TundraTheme::default_dark().muted
    ));
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
    assert!(output.contains("Username: alice"));
    assert!(output.contains("Display name: Alice Chen"));
    assert!(output.contains("Role: User"));
    assert!(output.contains("Password: ************"));
    assert!(output.contains("[ Create ]"));
    assert!(output.contains("[ Cancel ]"));
    assert!(region_has_fg(
        &terminal,
        form.submit,
        TundraTheme::default_dark().accent
    ));
}

#[test]
fn minimum_full_create_form_keeps_every_control_visible_and_hittable() {
    let mut model = model_with_users(1);
    model.form = Some(create_form(UserManagementField::Password));
    let layout = user_management_layout(Rect::new(0, 3, 50, 6), &model);
    let form = layout.form.expect("compact form geometry");

    assert!(form.compact);
    for field in [
        UserManagementField::Username,
        UserManagementField::DisplayName,
        UserManagementField::Role,
        UserManagementField::Password,
    ] {
        let area = form
            .fields
            .iter()
            .find(|candidate| candidate.field == field)
            .map(|candidate| candidate.area)
            .expect("every create field has geometry");
        assert!(area.width > 0 && area.height > 0);
    }
    assert!(form.submit.width > 0 && form.submit.height > 0);
    assert!(form.cancel.width > 0 && form.cancel.height > 0);

    let (terminal, _) = render(50, 12, &model);
    let output = terminal_output(&terminal);
    assert!(output.contains("Username: alice"));
    assert!(output.contains("Display name: Alice Chen"));
    assert!(output.contains("Role: User"));
    assert!(output.contains("Password: ************"));
}

#[test]
fn edit_and_password_forms_only_render_relevant_inputs() {
    let mut model = model_with_users(1);
    model.form = Some(UserManagementFormViewModel {
        kind: UserManagementFormKind::EditInfo,
        title: "Edit user info".to_string(),
        username: "user0".to_string(),
        display_name: "Renamed".to_string(),
        role: String::new(),
        password_len: 0,
        focused_field: UserManagementField::DisplayName,
        error: None,
    });
    let (terminal, _) = render(90, 22, &model);
    let edit = terminal_output(&terminal);
    assert!(edit.contains("Display name: Renamed"));
    assert!(!edit.contains("Role:"));
    assert!(!edit.contains("Password:"));

    model.form = Some(UserManagementFormViewModel {
        kind: UserManagementFormKind::Password,
        title: "Set password".to_string(),
        username: "user0".to_string(),
        display_name: String::new(),
        role: String::new(),
        password_len: 9,
        focused_field: UserManagementField::Cancel,
        error: Some("Password is too weak".to_string()),
    });
    let (terminal, _) = render(90, 22, &model);
    let password = terminal_output(&terminal);
    assert!(password.contains("Password: *********"));
    assert!(password.contains("Password is too weak"));
}

#[test]
fn feedback_error_uses_error_color() {
    let mut model = model_with_users(1);
    model.message = Some("Unable to save user".to_string());
    model.feedback_tone = UserManagementFeedbackTone::Error;
    let (terminal, main) = render(90, 22, &model);
    let layout = user_management_layout(main, &model);

    assert!(region_has_fg(
        &terminal,
        layout.feedback,
        TundraTheme::default_dark().error
    ));
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
        tundra_ui::ShellLayout::Full { main, .. } => main,
        tundra_ui::ShellLayout::Compact(_) => panic!("test requires full layout"),
    };
    (terminal, main)
}

fn terminal_output(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
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
