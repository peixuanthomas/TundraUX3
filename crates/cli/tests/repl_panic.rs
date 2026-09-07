use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn embedded_repl_panic_requests_the_shell_and_stops_reading_commands() {
    const CHILD: &str = "TUNDRA_TEST_REPL_PANIC_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let code = cli::run_with_platform(
            ["repl", "--embedded"],
            &platform::mock::UnsupportedPlatform,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
        std::process::exit(code);
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "embedded_repl_panic_requests_the_shell_and_stops_reading_commands",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"debug test-watchdog-panic\nhelp\nexit\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(shell::COMMAND_LINE_PANIC_EXIT_CODE as i32),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Triggering a real Shell panic"));
    assert!(!stdout.contains("Usage: tundra-cli"));
}
