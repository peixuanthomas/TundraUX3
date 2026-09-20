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
