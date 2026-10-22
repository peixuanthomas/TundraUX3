use std::path::PathBuf;
use std::time::{Duration, Instant};
use vim_runtime::{BufferInput, SaveResult, VimConfig, VimHost, VimPoll};
use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

fn until(host: &mut VimHost, predicate: impl Fn(&VimPoll) -> bool) -> VimPoll {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut saves = Vec::new();
    loop {
        let mut poll = host.poll().expect("poll real Vim");
        saves.append(&mut poll.save_requests);
        poll.save_requests = saves.clone();
        if predicate(&poll) {
            return poll;
        }
        assert!(
            Instant::now() < deadline,
            "Vim did not reach expected state: {:?}",
            poll.state
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn export(host: &mut VimHost) -> VimPoll {
    let command = host.request_export().unwrap();
    until(host, |poll| poll.state.last_command >= command)
}

fn buffer_text(poll: &VimPoll, host_id: u64) -> &str {
    &poll
        .state
        .buffers
        .iter()
        .find(|buffer| buffer.host_id == Some(host_id))
        .unwrap()
        .text
}

#[test]
#[ignore = "requires a full bundled Vim and PTY/loopback access"]
fn real_vim_owns_buffers_undo_and_acknowledged_writes() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.path().join("reports"),
        root.path().join("fallback"),
        root.path().join("data"),
        "vim-runtime-test",
        env!("CARGO_PKG_VERSION"),
    ))
    .unwrap();
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("vim-runtime-test"),
            "Vim runtime test",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::Optional,
        ))
        .unwrap();
    let tasks = app.task_group("vim");
    let program = std::env::var_os("TUNDRA_TEST_VIM")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/vim/vim")
        });
    let mut config = VimConfig::new(&program);
    config.runtime_path = Some(program.parent().unwrap().join("runtime"));
    config.user_config = false;
    let path = root.path().join("unsaved.txt");
    let mut input = BufferInput::new(10, "alpha beta gamma\nsecond line\nthird line\n");
    input.path = Some(path.clone());
    config.initial_buffers.push(input.clone());
    let mut host = VimHost::spawn(config, &tasks).unwrap();
    until(&mut host, |poll| {
        poll.state.ready && poll.state.last_command >= 1
    });

    host.send_bytes(b"u").unwrap();
    assert_eq!(
        buffer_text(&export(&mut host), 10),
        input.text,
        "import must not create a user undo entry"
    );
    host.send_bytes(b"gg0$x").unwrap();
    until(&mut host, |poll| {
        buffer_text(poll, 10) == "alpha beta gamm\nsecond line\nthird line\n"
    });
    host.send_bytes(b"u").unwrap();
    until(&mut host, |poll| buffer_text(poll, 10) == input.text);

    host.send_bytes(b"gg0i123\x1b").unwrap();
    until(&mut host, |poll| {
        poll.state.mode == "n" && buffer_text(poll, 10).starts_with("123alpha")
    });
    let other = host.open_buffer(BufferInput::new(20, "other\n")).unwrap();
    until(&mut host, |poll| poll.state.last_command >= other);
    let activate = host.activate_buffer(10).unwrap();
    until(&mut host, |poll| poll.state.last_command >= activate);
    host.send_bytes(b"u").unwrap();
    until(&mut host, |poll| buffer_text(poll, 10) == input.text);
    host.send_bytes(b"\x12").unwrap();
    until(&mut host, |poll| {
        buffer_text(poll, 10).starts_with("123alpha")
    });

    host.send_bytes(b":w\r").unwrap();
    let save = until(&mut host, |poll| !poll.save_requests.is_empty())
        .save_requests
        .remove(0);
    assert!(save.text.starts_with("123alpha"));
    assert!(
        !path.exists(),
        "Vim must not write the destination before host approval"
    );
    host.respond_save(save.id, Err("denied by test".into()))
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let failed = export(&mut host);
    assert!(
        failed
            .state
            .buffers
            .iter()
            .find(|b| b.host_id == Some(10))
            .unwrap()
            .modified
    );
    assert!(failed.exit.is_none());
    host.send_bytes(b"\r:wq\r").unwrap();
    let save = until(&mut host, |poll| !poll.save_requests.is_empty())
        .save_requests
        .remove(0);
    std::fs::write(&path, &save.text).unwrap();
    host.respond_save(save.id, Ok(SaveResult { path: path.clone() }))
        .unwrap();
    let exited = until(&mut host, |poll| poll.exit.is_some());
    assert!(exited.exit.unwrap().success);
    assert!(exited.state.exiting);
    assert!(!exited.state.discard_modified_on_exit);
    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .starts_with("123alpha")
    );
    drop(host);
    drop(tasks);
    runtime.shutdown().unwrap();
}

#[test]
#[ignore = "requires a full bundled Vim and PTY/loopback access"]
fn real_vim_preserves_encoded_bytes_write_policies_and_forced_discard() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.path().join("reports"),
        root.path().join("fallback"),
        root.path().join("data"),
        "vim-runtime-byte-test",
        env!("CARGO_PKG_VERSION"),
    ))
    .unwrap();
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("vim-runtime-byte-test"),
            "Vim byte test",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::Optional,
        ))
        .unwrap();
    let tasks = app.task_group("vim");
    let program = std::env::var_os("TUNDRA_TEST_VIM")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/vim/vim")
        });
    let mut config = VimConfig::new(&program);
    config.runtime_path = Some(program.parent().unwrap().join("runtime"));
    config.user_config = false;
    let path = root.path().join("encoded.txt");
    let mut input = BufferInput::new(30, "中文\r\n🙂\r\n");
    input.path = Some(path.clone());
    input.encoding = "utf-16le".into();
    input.line_ending = vim_runtime::LineEnding::Dos;
    input.bom = true;
    input.protected = true;
    input.modified = true;
    config.initial_buffers.push(input.clone());
    let mut host = VimHost::spawn(config, &tasks).unwrap();
    until(&mut host, |poll| {
        poll.state.ready && poll.state.last_command >= 1
    });
    host.send_bytes(b":w\r").unwrap();
    let save = until(&mut host, |poll| !poll.save_requests.is_empty())
        .save_requests
        .remove(0);
    let expected = [
        vec![0xff, 0xfe],
        input
            .text
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect(),
    ]
    .concat();
    assert_eq!(save.bytes, expected);
    assert!(save.protected);
    assert!(save.whole_buffer);
    assert!(!path.exists());
    std::fs::write(&path, &save.bytes).unwrap();
    host.respond_save(save.id, Ok(SaveResult { path: path.clone() }))
        .unwrap();
    until(&mut host, |poll| !poll.state.buffers[0].modified);

    input.encoding = "utf-16".into();
    let command = host.open_buffer(input.clone()).unwrap();
    until(&mut host, |poll| poll.state.last_command >= command);
    host.send_bytes(b":w\r").unwrap();
    let save = until(&mut host, |poll| !poll.save_requests.is_empty())
        .save_requests
        .remove(0);
    let expected = [
        vec![0xfe, 0xff],
        input
            .text
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect(),
    ]
    .concat();
    assert_eq!(
        save.bytes, expected,
        "generic utf-16 follows Vim's big-endian file encoding, not iconv's native-endian BOM behavior"
    );
    host.respond_save(save.id, Ok(SaveResult { path: path.clone() }))
        .unwrap();
    input.encoding = "utf-16le".into();
    let command = host.open_buffer(input.clone()).unwrap();
    until(&mut host, |poll| poll.state.last_command >= command);

    let appended = root.path().join("appended.txt");
    std::fs::write(&appended, b"prefix").unwrap();
    host.send_bytes(format!(":1,1w >> {}\r", appended.display()).as_bytes())
        .unwrap();
    let save = until(&mut host, |poll| !poll.save_requests.is_empty())
        .save_requests
        .remove(0);
    assert!(save.append);
    assert!(!save.whole_buffer);
    assert!(!save.bom);
    assert_eq!(
        save.bytes,
        "中文\r\n"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    assert_eq!(save.current_path.as_deref(), Some(path.as_path()));
    assert_eq!(std::fs::read(&appended).unwrap(), b"prefix");
    host.respond_save(
        save.id,
        Ok(SaveResult {
            path: appended.clone(),
        }),
    )
    .unwrap();
    let barrier = host.request_export().unwrap();
    until(&mut host, |poll| poll.state.last_command >= barrier);
    host.send_bytes(format!(":w {}\r", appended.display()).as_bytes())
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let rejected = export(&mut host);
    assert!(
        rejected.save_requests.is_empty(),
        "existing other files require ! before any host write request"
    );
    assert_eq!(std::fs::read(&appended).unwrap(), b"prefix");

    host.send_bytes(format!("\r:file {}\r:w\r", appended.display()).as_bytes())
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let renamed = export(&mut host);
    assert!(
        renamed.save_requests.is_empty(),
        ":file must not bypass Vim's existing-but-never-read E13 protection"
    );
    assert_eq!(std::fs::read(&appended).unwrap(), b"prefix");
    host.send_bytes(b"\r:w!\r").unwrap();
    let save = until(&mut host, |poll| !poll.save_requests.is_empty())
        .save_requests
        .remove(0);
    assert!(save.force);
    assert_eq!(save.current_path.as_deref(), Some(appended.as_path()));
    assert_eq!(save.path.as_deref(), Some(appended.as_path()));
    host.respond_save(
        save.id,
        Ok(SaveResult {
            path: appended.clone(),
        }),
    )
    .unwrap();
    let barrier = host.request_export().unwrap();
    until(&mut host, |poll| poll.state.last_command >= barrier);
    host.send_bytes(b":w\r").unwrap();
    let save = until(&mut host, |poll| !poll.save_requests.is_empty())
        .save_requests
        .remove(0);
    assert!(
        !save.force,
        "a successful current-path write establishes the read baseline"
    );
    host.respond_save(
        save.id,
        Ok(SaveResult {
            path: appended.clone(),
        }),
    )
    .unwrap();

    host.send_bytes(b"\r").unwrap();
    let nul = BufferInput::new(35, "A\0B\n");
    let mut nul = nul;
    nul.path = Some(root.path().join("nul.txt"));
    let command = host.open_buffer(nul).unwrap();
    let imported = until(&mut host, |poll| poll.state.last_command >= command);
    assert_eq!(buffer_text(&imported, 35), "A\0B\n");
    host.send_bytes(b":w\r").unwrap();
    let save = until(&mut host, |poll| !poll.save_requests.is_empty())
        .save_requests
        .remove(0);
    assert_eq!(save.text, "A\0B\n");
    assert_eq!(save.bytes, b"A\0B\n");
    host.respond_save(
        save.id,
        Ok(SaveResult {
            path: save.path.unwrap(),
        }),
    )
    .unwrap();
    let command = host.open_buffer(BufferInput::new(36, "\n")).unwrap();
    let blank = until(&mut host, |poll| poll.state.last_command >= command);
    assert_eq!(
        buffer_text(&blank, 36),
        "\n",
        "one blank line differs from an empty buffer"
    );
    let command = host.open_buffer(BufferInput::new(37, "")).unwrap();
    let blank = until(&mut host, |poll| poll.state.last_command >= command);
    assert_eq!(buffer_text(&blank, 37), "");

    let mut binary = BufferInput::new(38, "AB");
    binary.path = Some(root.path().join("binary.bin"));
    binary.line_ending = vim_runtime::LineEnding::Dos;
    binary.encoding = "utf-16le".into();
    binary.bom = true;
    let command = host.open_buffer(binary).unwrap();
    until(&mut host, |poll| poll.state.last_command >= command);
    host.send_bytes(b":set binary\r:w\r").unwrap();
    let save = until(&mut host, |poll| !poll.save_requests.is_empty())
        .save_requests
        .remove(0);
    assert_eq!(save.bytes, b"A\0B\0");
    assert!(!save.bom);
    assert!(!save.end_of_line);
    assert_eq!(save.line_ending, vim_runtime::LineEnding::Unix);
    host.respond_save(
        save.id,
        Ok(SaveResult {
            path: save.path.unwrap(),
        }),
    )
    .unwrap();

    let mut read_only = BufferInput::new(40, "immutable log\n");
    read_only.read_only = true;
    read_only.path = Some(root.path().join("log.txt"));
    let command = host.open_buffer(read_only).unwrap();
    until(&mut host, |poll| poll.state.last_command >= command);
    host.send_bytes(b"iX\x1b").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(buffer_text(&export(&mut host), 40), "immutable log\n");

    let mut dirty = BufferInput::new(50, "unsaved protected draft\n");
    dirty.modified = true;
    dirty.protected = true;
    let command = host.open_buffer(dirty).unwrap();
    until(&mut host, |poll| poll.state.last_command >= command);
    let close = host.request_close(false).unwrap();
    let refused = until(&mut host, |poll| poll.state.last_command >= close);
    assert!(refused.exit.is_none());
    assert!(
        refused
            .state
            .error
            .as_ref()
            .is_some_and(|error| error.contains("E37"))
    );
    host.request_close(true).unwrap();
    let discarded = until(&mut host, |poll| poll.exit.is_some());
    assert!(discarded.exit.unwrap().success);
    assert!(discarded.state.discard_modified_on_exit);
    drop(host);
    drop(tasks);
    runtime.shutdown().unwrap();
}
