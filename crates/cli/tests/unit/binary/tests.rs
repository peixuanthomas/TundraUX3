use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn only_the_embedded_repl_uses_parent_managed_lifecycle() {
    assert!(is_parent_managed_command_line(&args(&[
        "repl",
        "--embedded"
    ])));
    assert!(!is_parent_managed_command_line(&args(&["repl"])));
    assert!(!is_parent_managed_command_line(&args(&["help"])));
    assert!(!is_parent_managed_command_line(&args(&["--embedded"])));
}

#[cfg(target_os = "linux")]
#[test]
fn only_explicitly_confirmed_management_scripts_satisfy_root_startup_confirmation() {
    assert!(confirmed_management_script(&args(&[
        "services",
        "list",
        "--yes",
        "--non-interactive"
    ])));
    assert!(confirmed_management_script(&args(&[
        "system-config",
        "read",
        "/etc/ssh/sshd_config",
        "--yes",
        "--non-interactive"
    ])));
    for values in [
        &["services", "list"][..],
        &["services", "list", "--yes"][..],
        &["services", "list", "--non-interactive"][..],
        &["services", "unknown", "--yes", "--non-interactive"][..],
        &["repl", "--yes", "--non-interactive"][..],
        &["repl", "--embedded"][..],
        &["config", "show", "--yes", "--non-interactive"][..],
        &["logs", "query"][..],
        &["logs", "query", "--yes"][..],
    ] {
        assert!(!confirmed_management_script(&args(values)), "{values:?}");
    }
    assert!(confirmed_management_script(&args(&[
        "logs",
        "query",
        "--yes",
        "--non-interactive"
    ])));
    assert!(confirmed_management_script(&args(&[
        "logs",
        "follow",
        "--yes",
        "--non-interactive"
    ])));
}
