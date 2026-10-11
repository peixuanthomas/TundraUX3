use super::*;

#[test]
fn cancellable_wait_drains_large_stdout_and_stderr_together() {
    let spec = ProcessSpec::new("/bin/sh").args([
        "-c",
        "head -c 1048576 /dev/zero & head -c 1048576 /dev/zero >&2 & wait",
    ]);
    let started = std::time::Instant::now();
    let result = spawn_cancellable_impl(
        &spec,
        false,
        &|| started.elapsed() > std::time::Duration::from_secs(10),
        None,
    )
    .unwrap();
    assert_eq!(result.code, Some(0));
    assert_eq!(result.stdout.bytes(), vec![0; 1048576]);
    assert_eq!(result.stderr.bytes(), vec![0; 1048576]);
}

#[test]
fn closed_pipes_do_not_prevent_cancellation_or_exit_wait() {
    let started = std::time::Instant::now();
    let result = spawn_cancellable_impl(
        &ProcessSpec::new("/bin/sh").args(["-c", "exec 1>&- 2>&-; sleep 60"]),
        false,
        &|| started.elapsed() > std::time::Duration::from_millis(100),
        None,
    );
    assert!(matches!(result, Err(PlatformError::Interrupted { .. })));
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    let result = spawn_cancellable_impl(
        &ProcessSpec::new("/bin/sh").args(["-c", "exec 1>&- 2>&-; sleep 0.05; exit 7"]),
        false,
        &|| false,
        None,
    )
    .unwrap();
    assert_eq!(result.code, Some(7));
}

#[test]
fn streaming_delivers_stdout_before_exit_and_drains_stderr() {
    let root = std::env::temp_dir().join(format!("tundra-stream-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let ready = root.join("ready");
    let spec = ProcessSpec::new("/bin/sh").args(["-c", "printf 'first\\r'; i=0; while [ ! -f \"$1\" ] && [ $i -lt 100 ]; do sleep 0.02; i=$((i+1)); done; test -f \"$1\" || exit 7; printf 'warning\\n' >&2; printf 'last'; exit 3", "stream-test"]).arg(ready.to_string_lossy());
    let mut events = Vec::new();
    let result = spawn_streaming_impl(&spec, false, &mut |event| {
        if event.text == "first" {
            std::fs::write(&ready, "ready").unwrap();
        }
        events.push(event);
    })
    .unwrap();
    assert_eq!(result.code, Some(3));
    assert!(
        events
            .iter()
            .any(|event| event.stderr && event.text == "warning")
    );
    assert!(
        events
            .iter()
            .any(|event| !event.stderr && event.text == "last")
    );
    assert!(result.stdout.utf8_lossy().contains("last"));
    std::fs::remove_dir_all(root).unwrap();
}
