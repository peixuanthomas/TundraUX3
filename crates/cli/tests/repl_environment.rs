use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn repl_reuses_system_command_state_across_builtin_commands() {
    const CHILD: &str = "TUNDRA_TEST_REPL_ENVIRONMENT_CHILD";
    if std::env::var_os(CHILD).is_some() {
        let code = cli::run_with_platform(
            ["repl", "--embedded"],
            &platform::mock::UnsupportedPlatform,
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        );
        std::process::exit(code);
    }
    let input = if cfg!(windows) {
        "/set TUNDRA_RETAINED=kept\nhelp\n/echo result:%TUNDRA_RETAINED%\n/set TUNDRA_RETAINED=\n/if not defined TUNDRA_RETAINED echo removed\nexit\n"
    } else {
        "/export TUNDRA_RETAINED=kept\nhelp\n/printf 'result:%s\\n' \"$TUNDRA_RETAINED\"\n/unset TUNDRA_RETAINED\n/test \"${TUNDRA_RETAINED+x}\" != x && printf 'removed\\n'\nexit\n"
    };
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "repl_reuses_system_command_state_across_builtin_commands",
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
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("result:kept"), "{stdout}\n{stderr}");
    assert!(stdout.contains("removed"), "{stdout}\n{stderr}");
    assert!(stdout.contains("Usage: tundra-cli"), "{stdout}\n{stderr}");
    assert!(
        !stderr.contains("could not retain command state"),
        "{stderr}"
    );
}
