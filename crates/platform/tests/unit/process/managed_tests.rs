use super::*;
use std::io::Write;
use std::path::Path;
use std::time::Instant;
use watchdog::*;

const FIXTURE: &str = "managed_process::tests::process_fixture";

#[test]
fn process_fixture() {
    let Ok(role) = std::env::var("TUNDRA_MANAGED_PROCESS_FIXTURE") else {
        return;
    };
    if role == "output" {
        for _ in 0..64 {
            std::io::stdout().write_all(&[b'O'; 8192]).unwrap();
            std::io::stderr().write_all(&[b'E'; 8192]).unwrap();
        }
        return;
    }
    if let Ok(path) = std::env::var("TUNDRA_MANAGED_PROCESS_READY") {
        std::fs::write(path, std::process::id().to_string()).unwrap();
    }
    std::thread::sleep(Duration::from_secs(30));
}

fn command(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", FIXTURE, "--nocapture"])
        .env("TUNDRA_MANAGED_PROCESS_FIXTURE", role)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

fn runtime(root: &Path) -> (WatchdogRuntime, AppWatchdog) {
    let config = WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("state"),
        "process-tests",
        "test",
    );
    let (runtime, process) = WatchdogRuntime::start_isolated(config).unwrap();
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("process-tests"),
            "Process tests",
            "test",
            AppCriticality::Optional,
        ))
        .unwrap();
    (runtime, app)
}

fn in_app<T>(app: &AppWatchdog, f: impl FnOnce() -> T) -> T {
    app.run_boundary(
        BoundarySpec::new("process-test", BoundaryKind::Worker),
        std::panic::AssertUnwindSafe(f),
    )
    .unwrap_or_else(|_| panic!("process test boundary panicked"))
}

#[test]
fn shutdown_stops_a_child_even_while_its_owner_is_alive() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, app) = runtime(root.path());
    let mut child = in_app(&app, || command("sleep").spawn_managed().unwrap());
    assert!(child.try_wait().unwrap().is_none());
    let start = Instant::now();
    runtime.shutdown().unwrap();
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(child.try_wait().unwrap().is_some());
    assert!(in_app(&app, || command("sleep").spawn_managed()).is_err());
}

#[test]
fn output_drains_both_pipes_beyond_one_read_batch() {
    let output = command("output").output_managed().unwrap();
    assert!(output.status.success());
    assert!(output.stdout.iter().filter(|byte| **byte == b'O').count() >= 64 * 8192);
    assert_eq!(
        output.stderr.iter().filter(|byte| **byte == b'E').count(),
        64 * 8192
    );
}

#[cfg(target_os = "macos")]
#[test]
fn an_exited_process_group_is_cleaned_without_a_permission_failure() {
    let mut child = Command::new("/usr/bin/true").spawn_managed().unwrap();
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe {
            libc::waitid(
                libc::P_PID,
                child.id(),
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        },
        0
    );
    assert!(crate::process::macos::group_is_dead(child.id()).unwrap());
    assert!(child.try_wait().unwrap().unwrap().success());
}

#[test]
fn dropping_a_child_after_an_error_stops_it() {
    let mut child = command("sleep").spawn_managed().unwrap();
    let tree = child.tree.clone();
    // A broken pipe / serialization error can return before the ordinary wait.
    child.stdin.take();
    drop(child);
    assert!(tree.lock().unwrap().child.try_wait().unwrap().is_some());
}

#[test]
fn external_application_survives_watchdog_shutdown() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, app) = runtime(root.path());
    let ready = root.path().join("ready");
    in_app(&app, || {
        spawn_external(command("sleep").env("TUNDRA_MANAGED_PROCESS_READY", &ready))
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let pid = loop {
        if let Ok(text) = std::fs::read_to_string(&ready) {
            if let Ok(pid) = text.parse::<u32>() {
                break pid;
            }
        }
        assert!(Instant::now() < deadline, "external child did not start");
        std::thread::sleep(Duration::from_millis(5));
    };
    // Open a stable identity before shutdown; cleanup cannot target a reused PID.
    #[cfg(windows)]
    let handle = unsafe {
        windows_sys::Win32::System::Threading::OpenProcess(
            windows_sys::Win32::System::Threading::PROCESS_TERMINATE
                | windows_sys::Win32::System::Threading::PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    let shutdown = runtime.shutdown();
    #[cfg(windows)]
    let survived = unsafe {
        use windows_sys::Win32::System::Threading::*;
        assert!(!handle.is_null());
        let alive = WaitForSingleObject(handle, 0) == windows_sys::Win32::Foundation::WAIT_TIMEOUT;
        TerminateProcess(handle, 1);
        WaitForSingleObject(handle, 2000);
        windows_sys::Win32::Foundation::CloseHandle(handle);
        alive
    };
    #[cfg(unix)]
    let survived = unsafe {
        // This is still our unreaped direct child, so its PID cannot be reused.
        let alive = libc::kill(pid as i32, 0) == 0;
        libc::kill(pid as i32, libc::SIGKILL);
        libc::waitpid(pid as i32, std::ptr::null_mut(), 0);
        alive
    };
    shutdown.unwrap();
    assert!(survived, "external applications must keep running");
}
