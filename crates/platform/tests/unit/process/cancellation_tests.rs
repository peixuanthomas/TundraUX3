use super::*;
use std::cell::Cell;
use std::io::Write;
use std::time::{Duration, Instant};

const FIXTURE: &str = "process::cancellation_tests::cancellable_process_fixture";

#[test]
fn cancellable_process_fixture() {
    let Ok(role) = std::env::var("TUNDRA_PROCESS_CANCEL_FIXTURE") else {
        return;
    };
    if role == "parent" {
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", FIXTURE, "--nocapture"])
            .env("TUNDRA_PROCESS_CANCEL_FIXTURE", "child")
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        println!("DESCENDANT={}", child.id());
        std::io::stdout().flush().unwrap();
    } else if role == "exit" {
        println!("first");
        eprintln!("warning");
        return;
    }
    std::thread::sleep(Duration::from_secs(60));
}

fn fixture(role: &str) -> ProcessSpec {
    ProcessSpec::new(std::env::current_exe().unwrap())
        .args(["--exact", FIXTURE, "--nocapture"])
        .env("TUNDRA_PROCESS_CANCEL_FIXTURE", role)
}

#[test]
fn cancellation_stops_quiet_process_and_reaps_direct_child() {
    let started = Instant::now();
    let result = spawn_cancellable_impl(
        &fixture("child"),
        false,
        &|| started.elapsed() > Duration::from_millis(150),
        None,
    );
    assert!(
        matches!(result, Err(PlatformError::Interrupted { .. })),
        "{result:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn cancellation_stops_only_the_launched_tree() {
    let mut unrelated = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", FIXTURE, "--nocapture"])
        .env("TUNDRA_PROCESS_CANCEL_FIXTURE", "child")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let cancelled = Cell::new(false);
    let descendant = Cell::new(None);
    let started = Instant::now();
    let result = spawn_cancellable_impl(
        &fixture("parent"),
        false,
        &|| cancelled.get() || started.elapsed() > Duration::from_secs(5),
        Some(&mut |event| {
            if let Some(pid) = event.text.trim().strip_prefix("DESCENDANT=") {
                descendant.set(Some(pid.parse::<u32>().unwrap()));
                cancelled.set(true);
            }
        }),
    );
    let unrelated_survived = unrelated.try_wait().unwrap().is_none();
    unrelated.kill().unwrap();
    unrelated.wait().unwrap();
    assert!(unrelated_survived);
    assert!(
        matches!(result, Err(PlatformError::Interrupted { .. })),
        "{result:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    let pid = descendant
        .get()
        .expect("child process should report its descendant");
    let deadline = Instant::now() + Duration::from_secs(2);
    while process_is_running(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !process_is_running(pid),
        "descendant {pid} survived cancellation"
    );
}

fn process_is_running(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        // Orphan zombies await PID 1; they cannot run or hold build files.
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            !stat
                .rsplit_once(") ")
                .is_some_and(|(_, tail)| tail.starts_with('Z'))
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        crate::native_platform().is_process_alive(pid).unwrap()
    }
}

#[test]
fn cancellable_wait_preserves_output_and_does_not_cancel_success() {
    let result = spawn_cancellable_impl(&fixture("exit"), false, &|| false, None).unwrap();
    assert_eq!(result.code, Some(0));
    assert!(result.stdout.utf8_lossy().contains("first"));
    assert!(result.stderr.utf8_lossy().contains("warning"));
}

#[test]
fn cancelled_before_spawn_has_no_side_effects() {
    let result = spawn_cancellable_impl(
        &ProcessSpec::new("should-never-exist"),
        false,
        &|| true,
        None,
    );
    assert!(matches!(result, Err(PlatformError::Interrupted { .. })));
}
