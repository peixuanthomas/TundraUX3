use storage::{
    LauncherCustomCommand, LauncherEntryRecord, LauncherExecutableKind, LauncherTerminalSettings,
    StorageConfig,
};

#[test]
fn custom_exit_choice_defaults_to_immediate_and_roundtrips_both_values() {
    let mut custom: LauncherCustomCommand = toml::from_str(
        r#"
name = "tool"
icon = "[T]"
command = "printf done"
"#,
    )
    .unwrap();
    assert!(!custom.wait_for_enter);
    for wait_for_enter in [false, true] {
        custom.wait_for_enter = wait_for_enter;
        let encoded = toml::to_string(&custom).unwrap();
        let decoded: LauncherCustomCommand = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded, custom);
    }
}

#[test]
fn existing_launcher_entries_keep_system_launch_without_terminal_settings() {
    let entry: LauncherEntryRecord = toml::from_str(
        r#"
id = "editor"
path = "/usr/bin/editor"
executable_kind = "native_binary"
"#,
    )
    .unwrap();
    assert_eq!(entry.terminal, None);
}

#[test]
fn terminal_arguments_and_directory_roundtrip_without_command_interpretation() {
    let mut config = StorageConfig::default();
    config.launcher.entries.push(LauncherEntryRecord {
        id: "terminal-editor".into(),
        path: "/usr/bin/editor".into(),
        executable_kind: Some(LauncherExecutableKind::NativeBinary),
        terminal: Some(LauncherTerminalSettings {
            arguments: vec!["two words".into(), "$(not-a-command)".into(), "".into()],
            working_directory: Some("/home/user/中文 project".into()),
        }),
        ..LauncherEntryRecord::default()
    });
    let encoded = toml::to_string(&config).unwrap();
    let decoded: StorageConfig = toml::from_str(&encoded).unwrap();
    assert_eq!(decoded.launcher, config.launcher);
}
