use storage::{
    LauncherEntryRecord, LauncherExecutableKind, LauncherTerminalSettings, StorageConfig,
};

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
