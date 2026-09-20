use super::*;

#[test]
fn classification_uses_ids_while_cli_output_keeps_original_diagnostic_text() {
    let mut terminal = platform::terminal_environment_check_with_graphics_protocol(
        PlatformKind::Macos,
        None,
        Some("Sixel"),
    );
    let mut original = Vec::new();
    write_environment_check(&mut original, &terminal);
    terminal.id = "another-stable-id";
    let mut with_changed_id = Vec::new();
    write_environment_check(&mut with_changed_id, &terminal);
    assert_eq!(with_changed_id, original);
    terminal.id = "terminal";
    terminal.label = "终端".into();
    assert!(is_terminal_check(&terminal));
    let mut capability =
        EnvironmentCheck::capability("open_path", platform::CapabilityStatus::Supported);
    capability.label = "打开文件".into();
    assert!(is_capability_check(&capability));
    assert!(!is_platform_check(&capability));
}
