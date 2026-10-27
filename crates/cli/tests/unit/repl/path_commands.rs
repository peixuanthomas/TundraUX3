use super::*;
use rustyline::hint::Hint;
use std::time::{Duration, Instant};

#[path = "../../support/repl_watchdog.rs"]
mod support;

#[test]
fn cancellation_wakes_the_idle_scanner_with_its_request_channel_still_open() {
    let runtime = support::ReplWatchdog::new(None);
    let mut cache = PathCommands::with_scanner(Some(&runtime.app), |_, _| Vec::new());
    let session = platform::SystemCommandSession::new().unwrap();
    cache.refresh(session.executable_search_path());
    let deadline = Instant::now() + Duration::from_secs(5);
    while cache.cache.borrow().pending {
        cache.commands();
        assert!(Instant::now() < deadline, "scanner did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
    let worker = cache.worker.take().unwrap();
    worker.cancel();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !worker.is_finished() {
        assert!(
            Instant::now() < deadline,
            "cancellation did not wake scanner"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(worker.join().unwrap(), None);
    assert!(runtime.process.try_recv_incident().is_none());
}

#[test]
fn blocked_scan_never_delays_input_or_publishes_an_old_paths_result() {
    let runtime = support::ReplWatchdog::new(None);
    let (started, starts) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let mut scan_count = 0;
    let mut cache = PathCommands::with_scanner(Some(&runtime.app), move |_, _| {
        scan_count += 1;
        started.send(scan_count).unwrap();
        // Deliberately model a filesystem read that ignores cancellation.
        released.recv_timeout(Duration::from_secs(10)).unwrap();
        vec![format!("tundra-tool-{scan_count}")]
    });
    let mut session = platform::SystemCommandSession::new().unwrap();
    cache.refresh(session.executable_search_path());
    assert_eq!(starts.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
    let start = Instant::now();
    for _ in 0..100 {
        cache.refresh(session.executable_search_path());
        let commands = cache.commands();
        let hint = super::super::completion::command_hint("/debug scr", 10, &commands).unwrap();
        assert_eq!(hint.completion(), Some("een-keyboard"));
        assert_eq!(
            super::super::completion::command_completions("hel", 3, &commands).1,
            ["help"]
        );
    }
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "input waited for the scanner"
    );
    assert!(
        starts.try_recv().is_err(),
        "identical requests queued more scans"
    );

    let result = session
        .run(if cfg!(windows) {
            "set PATH=tundra-new-path"
        } else {
            "export PATH=tundra-new-path"
        })
        .unwrap();
    assert_eq!(result.exit_code, 0);
    cache.refresh(session.executable_search_path());
    release.send(()).unwrap();
    assert_eq!(starts.recv_timeout(Duration::from_secs(5)).unwrap(), 2);
    assert!(
        !cache
            .commands()
            .iter()
            .any(|name| name.starts_with("tundra-tool-"))
    );
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !cache.commands().iter().any(|name| name == "tundra-tool-2") {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!cache.commands().iter().any(|name| name == "tundra-tool-1"));
    cache.refresh(session.executable_search_path());
    assert_eq!(starts.recv_timeout(Duration::from_secs(5)).unwrap(), 3);
    assert!(cache.commands().iter().any(|name| name == "tundra-tool-2"));
    let start = Instant::now();
    drop(cache);
    assert!(
        start.elapsed() < Duration::from_millis(100),
        "exit waited for a scan"
    );
    release.send(()).unwrap();
    assert!(runtime.process.try_recv_incident().is_none());
}
