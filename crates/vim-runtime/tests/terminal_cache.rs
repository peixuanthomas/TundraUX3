#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};
use vim_runtime::{VimConfig, VimHost, VimPoll};
use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

fn until(host: &mut VimHost, predicate: impl Fn(&VimPoll) -> bool) -> VimPoll {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let poll = host.poll().unwrap();
        if predicate(&poll) {
            return poll;
        }
        assert!(Instant::now() < deadline, "terminal output did not arrive");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn contains(poll: &VimPoll, text: &str) -> bool {
    poll.terminal.cells.iter().any(|row| {
        row.iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>()
            .contains(text)
    })
}

#[test]
fn idle_polls_reuse_terminal_until_output_resize_or_exit() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.path().join("reports"),
        root.path().join("fallback"),
        root.path().join("data"),
        "vim-terminal-cache-test",
        env!("CARGO_PKG_VERSION"),
    ))
    .unwrap();
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("vim-terminal-cache-test"),
            "Vim terminal cache test",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::Optional,
        ))
        .unwrap();
    let tasks = app.task_group("terminal");
    let program = root.path().join("terminal-output.sh");
    std::fs::write(
        &program,
        "#!/bin/sh\nprintf '\x1b[2J\x1b[Hready'\nIFS= read -r input\nprintf '\x1b[2J\x1b[Hupdated'\nIFS= read -r input\nprintf '\x1b[2J\x1b[Hfinished'\n",
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut host = VimHost::spawn(VimConfig::new(program), &tasks).unwrap();
    let initial = until(&mut host, |poll| contains(poll, "ready"));
    let started = Instant::now();
    for _ in 0..1_000 {
        assert!(Arc::ptr_eq(
            &initial.terminal,
            &host.poll().unwrap().terminal
        ));
    }
    eprintln!(
        "1000 idle polls reused one terminal snapshot in {:?}",
        started.elapsed()
    );

    host.resize(140, 40).unwrap();
    let resized = host.poll().unwrap();
    assert_eq!((resized.terminal.columns, resized.terminal.rows), (140, 40));
    assert!(!Arc::ptr_eq(&initial.terminal, &resized.terminal));
    host.resize(140, 40).unwrap();
    assert!(Arc::ptr_eq(
        &resized.terminal,
        &host.poll().unwrap().terminal
    ));

    host.send_bytes(b"update\n").unwrap();
    let updated = until(&mut host, |poll| contains(poll, "updated"));
    assert!(!Arc::ptr_eq(&resized.terminal, &updated.terminal));
    host.send_bytes(b"exit\n").unwrap();
    let finished = until(&mut host, |poll| poll.exit.is_some());
    assert!(finished.exit.unwrap().success);
    assert!(contains(&finished, "finished"));
    assert!(Arc::ptr_eq(
        &finished.terminal,
        &host.poll().unwrap().terminal
    ));
    assert_eq!(*finished.terminal, host.terminal_snapshot());
    drop(host);
    drop(tasks);
    runtime.shutdown().unwrap();
}
