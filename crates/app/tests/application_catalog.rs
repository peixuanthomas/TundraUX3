use app::BUILT_IN_LAUNCHER_APPLICATIONS;

#[test]
fn management_and_logs_are_fixed_launcher_apps_with_platform_visibility() {
    for id in [
        "builtin.services",
        "builtin.processes",
        "builtin.packages",
        "builtin.network",
        "builtin.disks",
        "builtin.logs",
    ] {
        let application = BUILT_IN_LAUNCHER_APPLICATIONS
            .iter()
            .find(|item| item.id == id)
            .expect("built-in Launcher entry");
        assert!(application.fixed_in_launcher && !application.admin_only);
        assert!(!application.description.is_empty());
        assert_eq!(
            application.available_on_platform(false),
            id == "builtin.logs"
        );
        assert!(application.available_on_platform(true));
    }
}

#[test]
fn user_management_is_a_fixed_launcher_application_on_all_platforms() {
    let users = BUILT_IN_LAUNCHER_APPLICATIONS
        .iter()
        .find(|item| item.id == "builtin.users")
        .expect("user management entry");
    assert!(users.fixed_in_launcher && !users.admin_only);
    assert!(users.available_on_platform(true) && users.available_on_platform(false));
}
