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
