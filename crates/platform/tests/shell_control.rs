use interprocess::local_socket::Stream;
use platform::shell_control::{Server, connect, exchange, sessions};
use shell_control::{Action, Request, Response};
use std::fs;
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn server() -> (tempfile::TempDir, Server) {
    let root = tempfile::tempdir().unwrap();
    let server = Server::start(&root.path().join("control")).unwrap();
    (root, server)
}

fn close(stream: Stream) {
    #[cfg(windows)]
    {
        let Stream::NamedPipe(pipe) = &stream;
        pipe.inner().assume_flushed();
    }
    drop(stream);
}

fn send_request(server: &mut Server) -> (Stream, u64) {
    let mut stream = connect(&server.info).unwrap();
    let mut bytes = serde_json::to_vec(&Request::new(Action::Snapshot)).unwrap();
    bytes.push(b'\n');
    stream.write_all(&bytes).unwrap();
    let start = Instant::now();
    loop {
        if let Some((id, _)) = server.poll().into_iter().next() {
            return (stream, id);
        }
        assert!(start.elapsed() < Duration::from_secs(2));
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn separate_process_discovers_and_exchanges_fragmented_messages() {
    let (root, mut server) = server();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "subprocess_client", "--nocapture"])
        .env("TUNDRA_CONTROL_TEST_DIRECTORY", root.path().join("control"))
        .env("TUNDRA_CONTROL_TEST_SESSION", &server.info.session)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let mut received = false;
    loop {
        for (id, bytes) in server.poll() {
            let request: Request = serde_json::from_slice(&bytes).unwrap();
            let Action::Text { text } = request.action else {
                panic!("expected text request");
            };
            assert_eq!(text, "界".repeat(90_000));
            received = true;
            server.respond(id, &Response::failure("echo", text));
        }
        if let Some(status) = child.try_wait().unwrap() {
            let output = child.wait_with_output().unwrap();
            assert!(
                status.success(),
                "request received: {received}; {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(received);
            break;
        }
        if start.elapsed() > Duration::from_secs(10) {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("client process did not finish");
        }
        thread::sleep(Duration::from_millis(1));
    }
    let registration = root
        .path()
        .join("control")
        .join(format!("{}.json", server.info.session));
    assert!(!fs::read_to_string(registration).unwrap().contains('界'));
}

#[test]
fn subprocess_client() {
    let Some(directory) = std::env::var_os("TUNDRA_CONTROL_TEST_DIRECTORY") else {
        return;
    };
    let discovered = sessions(std::path::Path::new(&directory)).unwrap();
    let expected = std::env::var("TUNDRA_CONTROL_TEST_SESSION").unwrap();
    let info = discovered
        .into_iter()
        .find(|info| info.session == expected)
        .unwrap();
    let response = exchange(
        &info,
        &Request::new(Action::Text {
            text: "界".repeat(90_000),
        }),
        Duration::from_secs(8),
    )
    .unwrap();
    assert_eq!(response.error.unwrap().message, "界".repeat(90_000));
}

#[test]
fn incomplete_and_disconnected_requests_do_not_reach_shell() {
    let (_root, mut server) = server();
    let mut stream = connect(&server.info).unwrap();
    assert!(server.poll().is_empty());
    stream.write_all(b"{\"protocol\":1").unwrap();
    assert!(server.poll().is_empty());
    close(stream);
    assert!(server.poll().is_empty());

    let mut stream = connect(&server.info).unwrap();
    stream.write_all(b"{}\n{}\n").unwrap();
    assert!(server.poll().is_empty());
    close(stream);

    let mut stream = connect(&server.info).unwrap();
    stream.write_all(b"{}\n").unwrap();
    close(stream);
    assert!(server.poll().is_empty());
}

#[test]
fn slow_reader_does_not_block_other_clients_and_disconnect_is_observed() {
    let (_root, mut server) = server();
    let (slow, slow_id) = send_request(&mut server);
    server.respond(
        slow_id,
        &Response::failure("large", "x".repeat(2 * 1024 * 1024)),
    );
    let start = Instant::now();
    for _ in 0..128 {
        assert!(server.poll().is_empty());
    }
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(server.connected(slow_id));

    let info = server.info.clone();
    let client = thread::spawn(move || {
        exchange(
            &info,
            &Request::new(Action::Snapshot),
            Duration::from_secs(3),
        )
    });
    while !client.is_finished() {
        for (id, _) in server.poll() {
            server.respond(id, &Response::failure("ready", "ready"));
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(client.join().unwrap().unwrap().error.unwrap().code, "ready");
    close(slow);
    server.poll();
    assert!(!server.connected(slow_id));
}

#[test]
fn client_deadline_covers_busy_connection_and_unresponsive_server() {
    let (_root, server) = server();
    let occupied = connect(&server.info).unwrap();
    let start = Instant::now();
    let error = exchange(
        &server.info,
        &Request::new(Action::Snapshot),
        Duration::from_millis(100),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(start.elapsed() < Duration::from_secs(2));
    close(occupied);
}

#[test]
fn wrong_server_pid_is_rejected_and_exit_removes_registration() {
    let (root, server) = server();
    let mut wrong = server.info.clone();
    wrong.pid += 1;
    assert_eq!(
        connect(&wrong).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    let registration = root
        .path()
        .join("control")
        .join(format!("{}.json", server.info.session));
    assert!(registration.exists());
    let endpoint = server.info.endpoint.clone();
    drop(server);
    assert!(!registration.exists());
    #[cfg(unix)]
    assert!(!std::path::Path::new(&endpoint).exists());
    #[cfg(windows)]
    let _ = endpoint;
    assert!(sessions(&root.path().join("control")).unwrap().is_empty());
}

#[test]
fn oversized_client_request_is_rejected_before_connecting() {
    let (_root, server) = server();
    let request = Request::new(Action::Text {
        text: "x".repeat(shell_control::MAX_REQUEST_BYTES),
    });
    assert_eq!(
        exchange(&server.info, &request, Duration::from_secs(1))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
}

#[cfg(unix)]
#[test]
fn unix_directory_socket_and_registration_are_private() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let (root, server) = server();
    let directory = root.path().join("control");
    let registration = directory.join(format!("{}.json", server.info.session));
    for (path, mode) in [
        (directory.as_path(), 0o700),
        (std::path::Path::new(&server.info.endpoint), 0o600),
        (registration.as_path(), 0o600),
    ] {
        let metadata = fs::symlink_metadata(path).unwrap();
        // SAFETY: geteuid has no preconditions.
        assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
        assert_eq!(metadata.permissions().mode() & 0o777, mode);
    }
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        sessions(&directory).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}
