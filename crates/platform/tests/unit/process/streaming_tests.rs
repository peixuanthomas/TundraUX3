use super::*;

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
