#![cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]

use platform::management::{
    HelperReady, ManagementCommand, ManagementKind, OperationEvent, OperationRecord, helper,
};
use std::{
    ffi::{c_int, c_long, c_void},
    fs,
    io::{BufRead, BufReader, Write},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[repr(C)]
struct Credentials {
    pid: c_int,
    uid: u32,
    gid: u32,
}

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

unsafe extern "C" {
    fn getuid() -> u32;
    fn getsockopt(
        fd: c_int,
        level: c_int,
        option: c_int,
        value: *mut c_void,
        length: *mut u32,
    ) -> c_int;
    fn syscall(number: c_long, ...) -> c_long;
    fn poll(fds: *mut PollFd, count: usize, timeout: c_int) -> c_int;
}

struct Fixture {
    directory: PathBuf,
    helper: Option<OwnedFd>,
    completed: bool,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "txh-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            directory,
            helper: None,
            completed: false,
        }
    }

    fn identify_helper(&mut self, socket: &UnixStream, ready: &HelperReady, actor: u32) {
        let mut credentials = Credentials {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut length = std::mem::size_of::<Credentials>() as u32;
        assert_eq!(
            unsafe {
                getsockopt(
                    socket.as_raw_fd(),
                    1,
                    17,
                    (&mut credentials as *mut Credentials).cast(),
                    &mut length,
                )
            },
            0
        );
        assert_eq!(credentials.uid, actor);
        // The listener was bound before fork, so SO_PEERCRED.pid can name the launcher.
        let pid = ready.process_id;
        assert!(pid > 1 && pid != std::process::id());
        // Linux pidfd_open pins this test's helper so cleanup cannot signal a reused PID.
        let fd = unsafe { syscall(434, pid, 0_u32) } as c_int;
        assert!(fd >= 0, "pidfd_open: {}", std::io::Error::last_os_error());
        let helper = unsafe { OwnedFd::from_raw_fd(fd) };
        let process = PathBuf::from(format!("/proc/{pid}"));
        let status = fs::read_to_string(process.join("status")).unwrap();
        let uids = status
            .lines()
            .find(|line| line.starts_with("Uid:"))
            .unwrap()
            .split_whitespace()
            .skip(1)
            .map(|value| value.parse::<u32>().unwrap())
            .collect::<Vec<_>>();
        assert!(uids.iter().all(|uid| *uid == actor));
        let cmdline = fs::read(process.join("cmdline")).unwrap();
        assert!(
            cmdline
                .split(|byte| *byte == 0)
                .any(|argument| argument == b"__system-helper")
        );
        let sockets = fs::read_to_string("/proc/net/unix").unwrap();
        let inode = sockets
            .lines()
            .find_map(|line| {
                let parts = line.split_whitespace().collect::<Vec<_>>();
                (parts.get(7).copied() == ready.socket.to_str())
                    .then(|| parts.get(6).unwrap().to_string())
            })
            .expect("the test-created socket must be listed");
        let socket_fd = PathBuf::from(format!("socket:[{inode}]"));
        assert!(
            fs::read_dir(process.join("fd"))
                .unwrap()
                .flatten()
                .any(|entry| fs::read_link(entry.path()).is_ok_and(|path| path == socket_fd))
        );
        self.helper = Some(helper);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(helper) = &self.helper {
            if !self.completed {
                eprintln!(
                    "No verified terminal event; retaining test helper at {} for its normal cleanup",
                    self.directory.display()
                );
                return;
            }
            // Only the completed, test-created helper is stopped; no running task is cancelled.
            let result = unsafe {
                syscall(
                    424,
                    helper.as_raw_fd(),
                    15 as c_int,
                    std::ptr::null::<c_void>(),
                    0_u32,
                )
            };
            if result < 0 {
                eprintln!(
                    "test helper cleanup signal: {}",
                    std::io::Error::last_os_error()
                );
                return;
            }
            let mut descriptor = PollFd {
                fd: helper.as_raw_fd(),
                events: 1,
                revents: 0,
            };
            if unsafe { poll(&mut descriptor, 1, 5000) } <= 0 {
                eprintln!(
                    "test helper has not exited; retaining {}",
                    self.directory.display()
                );
                return;
            }
        }
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn read_until_finished(stream: UnixStream) -> Vec<OperationRecord> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut reader = BufReader::new(stream);
    let mut records = Vec::new();
    loop {
        let mut packet = String::new();
        assert!(
            reader.read_line(&mut packet).unwrap() > 0,
            "helper disconnected before reporting a result"
        );
        assert!(packet.len() < 128 * 1024);
        let record: OperationRecord = serde_json::from_str(&packet).unwrap();
        let finished = matches!(
            record.event,
            OperationEvent::Completed { .. } | OperationEvent::Failed { .. }
        );
        records.push(record);
        if finished {
            return records;
        }
        assert!(records.len() < 16);
    }
}

#[test]
#[ignore = "requires an ordinary Linux user and pidfd-capable kernel"]
fn ordinary_user_helper_reports_failure_and_replays_it_after_reconnect() {
    let actor = unsafe { getuid() };
    assert_ne!(actor, 0, "run this integration test as a normal Linux user");
    let mut fixture = Fixture::new();
    let command = ManagementCommand {
        kind: ManagementKind::Services,
        action: "invalid-test-action".into(),
        target: Some("tundra-helper-test.service".into()),
        values: [("secret".into(), "test-value-never-recorded".into())].into(),
        identity: Default::default(),
    };
    let mut child = Command::new(env!("CARGO_BIN_EXE_tundra-cli"))
        .args(["__system-helper", &actor.to_string()])
        .env("TMPDIR", &fixture.directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    serde_json::to_writer(&mut input, &command).unwrap();
    input.write_all(b"\n").unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let ready: HelperReady = serde_json::from_slice(&output.stdout).unwrap();
    assert!(ready.socket.starts_with(&fixture.directory));
    assert_eq!(
        helper::operation_kind(&ready.socket),
        Some(ManagementKind::Services)
    );
    let metadata =
        fs::read_to_string(ready.socket.parent().unwrap().join("metadata.json")).unwrap();
    assert_eq!(metadata, r#"{"kind":"services"}"#);

    let stream = helper::connect(&ready.socket, actor).unwrap();
    fixture.identify_helper(&stream, &ready, actor);
    let first = read_until_finished(stream);
    fixture.completed = true;
    assert_eq!(first.len(), 2);
    assert!(
        matches!(&first[0].event, OperationEvent::Started { kind: ManagementKind::Services, action, target } if action == "invalid-test-action" && target.as_deref() == Some("tundra-helper-test.service"))
    );
    assert!(
        matches!(&first[1].event, OperationEvent::Failed { message } if message.contains("Unknown service operation"))
    );
    assert!(
        !serde_json::to_string(&first)
            .unwrap()
            .contains("test-value-never-recorded")
    );

    let replay = read_until_finished(helper::connect(&ready.socket, actor).unwrap());
    assert_eq!(
        serde_json::to_value(&replay).unwrap(),
        serde_json::to_value(&first).unwrap()
    );
    assert_eq!(
        helper::operation_kind(&ready.socket),
        Some(ManagementKind::Services)
    );
    eprintln!(
        "ordinary UID {actor}, verified helper PID {}, Started -> Failed, disconnected -> identical replay, metadata kind=services; completed helper cleanup follows",
        ready.process_id
    );
    let directory = fixture.directory.clone();
    drop(fixture);
    assert!(
        !directory.exists(),
        "the test helper and its own temporary directory must be cleaned up"
    );
}
