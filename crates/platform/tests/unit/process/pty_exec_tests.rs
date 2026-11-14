use super::*;
use portable_pty::{PtySize, native_pty_system};

#[test]
fn supervised_pty_shim_process() {
    if std::env::var_os("TUNDRA_TEST_PTY_SHIM").is_none() {
        return;
    }
    let args = std::env::args_os().skip(4);
    let error = match run_shim(args) {
        Err(error) => error,
        Ok(never) => match never {},
    };
    eprintln!("{error}");
    std::process::exit(1);
}

#[test]
fn program_waits_for_release_and_receives_original_launch_environment() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    for release in [false, true] {
        let result = root.path().join("result");
        let mut command = CommandBuilder::new("sh");
        command.cwd(root.path());
        command.args([
            "-c",
            "printf '%s:%s' \"$PTY_TEST\" \"$1\" > result",
            "sh",
            "参数",
        ]);
        let environment = [
            (OsString::from("PATH"), OsString::from("/bin")),
            (OsString::from("PTY_TEST"), OsString::from("from parent")),
        ]
        .into_iter()
        .collect();
        let gate = Gate::wrap_in(root.path(), &mut command, environment).unwrap();
        command.get_argv_mut().splice(
            1..2,
            [
                "--exact",
                "pty_exec::tests::supervised_pty_shim_process",
                "--",
            ]
            .map(OsString::from),
        );
        command.env("TUNDRA_TEST_PTY_SHIM", "1");
        let pty = native_pty_system().openpty(PtySize::default()).unwrap();
        let mut child = pty.slave.spawn_command(command).unwrap();
        drop(pty.slave);
        let pid = child.process_id().unwrap();
        let mut reader = pty.master.try_clone_reader().unwrap();
        let read = std::thread::spawn(move || {
            let mut output = String::new();
            let _ = reader.read_to_string(&mut output);
            output
        });
        let deadline = Instant::now() + TIMEOUT;
        let stream = gate.accept(pid).unwrap_or_else(|error| {
            let status = child.try_wait().unwrap();
            if status.is_none() {
                let _ = child.kill();
            }
            panic!("could not connect to PTY shim: {error}, {status:?}");
        });
        assert!(!result.exists(), "target ran before the gate released");
        let gate_directory = gate._directory.path().to_path_buf();
        if release {
            gate.start_target(stream).unwrap();
        } else {
            stream.shutdown(std::net::Shutdown::Both).unwrap();
            drop(stream);
            drop(gate);
        }
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert_eq!(status.success(), release);
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                panic!(
                    "PTY shim did not exit with release={release}, result exists={}",
                    result.exists()
                );
            }
            std::thread::sleep(INTERVAL);
        }
        let output = read.join().unwrap();
        if release {
            assert_eq!(
                std::fs::read_to_string(&result).unwrap(),
                "from parent:参数"
            );
        } else {
            assert!(!result.exists(), "{output}");
        }
        assert!(!gate_directory.exists());
    }
}
