#![cfg(target_os = "linux")]
use app::script::{ScriptBreakpoint, ScriptLaunchSpec};
use script_runtime::{RuntimeCommand as Command, RuntimeEvent as Event, ScriptSession};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};

fn setup(root: &Path) -> (WatchdogRuntime, watchdog::AppWatchdog) {
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("data"),
        "script-test",
        env!("CARGO_PKG_VERSION"),
    ))
    .unwrap();
    let app = process
        .register_app(AppDescriptor::new(
            AppId::from_static("script-test"),
            "Script Test",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::Optional,
        ))
        .unwrap();
    (runtime, app)
}

fn spec(root: &Path, source: &str, interpreter: &str) -> ScriptLaunchSpec {
    let path = root.join("中 文 script.sh");
    std::fs::write(&path, source).unwrap();
    ScriptLaunchSpec {
        path,
        interpreter: interpreter.into(),
        interpreter_args: vec![],
        args: vec!["a  b\\c".into()],
        cwd: root.to_owned(),
    }
}

fn wait(session: &mut ScriptSession, predicate: impl Fn(&Event) -> bool) -> Event {
    let deadline = Instant::now() + Duration::from_secs(12);
    loop {
        for event in session.poll().unwrap() {
            if predicate(&event) {
                return event;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out, output: {:?}",
            text(session)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn text(session: &ScriptSession) -> String {
    session
        .terminal()
        .snapshot()
        .cells
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn ordinary_run_preserves_arguments_cwd_stdin_and_exit_code() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, app) = setup(root.path());
    let launch = spec(
        root.path(),
        "printf 'arg=%s\\ncwd=%s\\n' \"$1\" \"$PWD\"\nread -r answer\nprintf 'answer=%s\\n' \"$answer\"\nexit 7\n",
        "/bin/dash",
    );
    let mut session =
        ScriptSession::spawn(&launch, false, 1, &app.task_group("run"), 108, 20).unwrap();
    session.terminal().write(b"hello\n").unwrap();
    assert!(matches!(
        wait(&mut session, |event| matches!(event, Event::Exited { .. })),
        Event::Exited { code: 7 }
    ));
    let output = text(&session);
    assert!(output.contains("arg=a  b\\c"), "{output}");
    assert!(output.contains("answer=hello"), "{output}");
    drop(session);
    let launch = spec(root.path(), "printf 'debug exit\\n'\nexit 7\n", "/bin/bash");
    let mut session =
        ScriptSession::spawn(&launch, true, 2, &app.task_group("debug-exit"), 108, 20).unwrap();
    wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    session.command(Command::Continue).unwrap();
    assert!(matches!(
        wait(&mut session, |event| matches!(event, Event::Exited { .. })),
        Event::Exited { code: 7 }
    ));
    session.into_final_snapshot().unwrap();
    runtime.shutdown().unwrap();
}

#[test]
fn bashdb_stops_steps_inspects_and_keeps_eval_out_of_program_output() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, app) = setup(root.path());
    let launch = spec(
        root.path(),
        "value=1\narr=(\"中 文\" two)\nfn() {\n local inside='a  b'\n value=3\n}\nfn\nprintf 'self=%s\\nvalue=%s\\n' \"$0\" \"$value\"\n",
        "/bin/bash",
    );
    let mut session =
        ScriptSession::spawn(&launch, true, 8, &app.task_group("debug"), 108, 20).unwrap();
    session
        .command(Command::SetBreakpoints(vec![ScriptBreakpoint {
            path: launch.path.clone(),
            line: 5,
            enabled: true,
            verified: false,
            rejected: false,
        }]))
        .unwrap();
    let stopped = wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    assert!(
        matches!(stopped,Event::Stopped{ref location,..} if location.line==1),
        "{stopped:?}"
    );
    assert!(
        matches!(stopped,Event::Stopped{ref breakpoints,..} if !breakpoints[0].verified && !breakpoints[0].rejected),
        "{stopped:?}"
    );
    session.command(Command::Continue).unwrap();
    let stopped = wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    if let Event::Stopped {
        location,
        variables,
        stack,
        breakpoints,
        ..
    } = stopped
    {
        assert_eq!(location.line, 5);
        assert!(
            variables
                .iter()
                .any(|v| v.name == "inside" && v.value.contains("a  b")),
            "{variables:?}"
        );
        assert!(
            variables
                .iter()
                .any(|v| v.name == "arr" && v.children.len() == 2),
            "{variables:?}"
        );
        assert!(stack.iter().any(|frame| frame.name == "fn"), "{stack:?}");
        assert!(breakpoints[0].verified);
        assert!(!breakpoints[0].rejected);
    }
    let input = "value=9; capture='a  b\\c'; printf '%s' \"$capture\"".to_owned();
    session
        .command(Command::Evaluate {
            input: input.clone(),
            execute: true,
        })
        .unwrap();
    let evaluation = wait(&mut session, |event| {
        matches!(event, Event::Evaluation { .. })
    });
    assert!(
        matches!(evaluation,Event::Evaluation{ref output,..} if output.contains("a  b\\c")),
        "{evaluation:?}"
    );
    wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    session
        .command(Command::Evaluate {
            input: "value".into(),
            execute: false,
        })
        .unwrap();
    let value = wait(&mut session, |event| {
        matches!(event, Event::Evaluation { .. })
    });
    assert!(
        matches!(value,Event::Evaluation{ref output,..} if output.contains("9")),
        "{value:?}"
    );
    wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    for (input, execute, expected) in [
        ("$capture", false, "a  b\\c"),
        ("printf '<%s>' a\\\n", true, "<a>"),
    ] {
        session
            .command(Command::Evaluate {
                input: input.into(),
                execute,
            })
            .unwrap();
        let evaluation = wait(&mut session, |event| {
            matches!(event, Event::Evaluation { .. })
        });
        assert!(
            matches!(evaluation,Event::Evaluation{ref output,..} if output.contains(expected)),
            "{evaluation:?}"
        );
        wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    }
    session
        .command(Command::Evaluate {
            input: "tundra_missing_test_command".into(),
            execute: true,
        })
        .unwrap();
    let error = wait(&mut session, |event| {
        matches!(event, Event::Evaluation { .. })
    });
    assert!(
        matches!(error,Event::Evaluation{ref output,..} if output.contains("tundra_missing_test_command")),
        "{error:?}"
    );
    wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    session.command(Command::StepOver).unwrap();
    wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    session.command(Command::Continue).unwrap();
    wait(&mut session, |event| matches!(event, Event::Exited { .. }));
    let output = text(&session);
    assert!(output.contains(launch.path.to_str().unwrap()), "{output}");
    assert!(!output.contains("a  b\\c"), "{output}");
    drop(session);
    runtime.shutdown().unwrap();
}

#[test]
fn source_functions_step_into_and_out_and_stop_cleans_descendants() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, app) = setup(root.path());
    std::fs::write(
        root.path().join("lib file.sh"),
        "work() {\n local label='inside'\n printf '%s\\n' \"$label\"\n}\n",
    )
    .unwrap();
    let launch = spec(
        root.path(),
        "source './lib file.sh'\nwork\nprintf 'done\\n'\n",
        "/bin/bash",
    );
    let mut session =
        ScriptSession::spawn(&launch, true, 9, &app.task_group("source"), 108, 20).unwrap();
    wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    session.command(Command::StepOver).unwrap();
    let next = wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    assert!(
        matches!(next,Event::Stopped{ref location,..} if location.line==2),
        "{next:?}"
    );
    session.command(Command::StepIn).unwrap();
    let entered = wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    assert!(
        matches!(entered,Event::Stopped{ref location,..} if location.path.ends_with("lib file.sh")),
        "{entered:?}"
    );
    session.command(Command::StepOut).unwrap();
    let returned = wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    assert!(
        matches!(returned,Event::Stopped{ref location,..} if location.path==launch.path),
        "{returned:?}"
    );
    session.stop().unwrap();
    session.into_final_snapshot().unwrap();

    let launch = spec(
        root.path(),
        "sleep 90 &\nprintf '%s' \"$!\" > child.pid\nwait\n",
        "/bin/dash",
    );
    let mut session =
        ScriptSession::spawn(&launch, false, 10, &app.task_group("tree"), 108, 20).unwrap();
    let pidfile = root.path().join("child.pid");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !pidfile.exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let pid = std::fs::read_to_string(pidfile)
        .unwrap()
        .parse::<u32>()
        .unwrap();
    session.stop().unwrap();
    session.into_final_snapshot().unwrap();
    let state = std::fs::read_to_string(format!("/proc/{pid}/stat"));
    assert!(state.is_err() || state.unwrap().split_whitespace().nth(2) == Some("Z"));
    runtime.shutdown().unwrap();
}

#[test]
fn boolean_status_survives_debug_steps_and_breakpoints_distinguish_rejection_from_pending() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, app) = setup(root.path());
    let launch = spec(
        root.path(),
        "# comment\nwork() {\n true\n}\nfalse\nstatus=$?\nwork\nprintf 'status=%s\\n' \"$status\"\n",
        "/bin/bash",
    );
    let mut session =
        ScriptSession::spawn(&launch, true, 11, &app.task_group("status"), 108, 20).unwrap();
    session
        .command(Command::SetBreakpoints(
            [1, 2, 8]
                .map(|line| ScriptBreakpoint {
                    path: launch.path.clone(),
                    line,
                    enabled: true,
                    verified: false,
                    rejected: false,
                })
                .to_vec(),
        ))
        .unwrap();
    let first = wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    assert!(
        matches!(first, Event::Stopped { ref breakpoints, ref location, .. } if location.line == 5 && breakpoints.iter().all(|breakpoint| !breakpoint.verified)),
        "{first:?}"
    );
    assert!(
        matches!(first, Event::Stopped { ref breakpoints, .. } if breakpoints[0].rejected && breakpoints[1].rejected && !breakpoints[2].rejected),
        "{first:?}"
    );
    session.command(Command::StepOver).unwrap();
    wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    session.command(Command::StepOver).unwrap();
    let stopped = wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    assert!(
        matches!(stopped, Event::Stopped { ref variables, .. } if variables.iter().any(|variable| variable.name == "status" && variable.value == "\"1\"")),
        "{stopped:?}"
    );
    session.command(Command::Continue).unwrap();
    let hit = wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    assert!(
        matches!(hit, Event::Stopped { ref breakpoints, ref location, .. } if location.line == 8 && breakpoints[2].verified && !breakpoints[2].rejected),
        "{hit:?}"
    );
    session.command(Command::Continue).unwrap();
    wait(&mut session, |event| matches!(event, Event::Exited { .. }));
    let output = session
        .into_final_snapshot()
        .unwrap()
        .cells
        .iter()
        .map(|row| {
            row.iter()
                .map(|cell| cell.text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(output.contains("status=1"), "{output}");
    runtime.shutdown().unwrap();
}

#[test]
fn two_debug_sessions_keep_their_channels_and_stop_counters_separate() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, app) = setup(root.path());
    let first_root = root.path().join("first");
    let second_root = root.path().join("second");
    std::fs::create_dir_all(&first_root).unwrap();
    std::fs::create_dir_all(&second_root).unwrap();
    let first = spec(&first_root, "x=1\nx=2\nprintf '%s' \"$x\"\n", "/bin/bash");
    let second = spec(&second_root, "x=1\nx=2\nprintf '%s' \"$x\"\n", "/bin/bash");
    let mut first_session =
        ScriptSession::spawn(&first, true, 101, &app.task_group("first"), 108, 20).unwrap();
    let mut second_session =
        ScriptSession::spawn(&second, true, 102, &app.task_group("second"), 108, 20).unwrap();
    assert_eq!(first_session.session_id(), 101);
    assert_eq!(second_session.session_id(), 102);
    let event = wait(&mut first_session, |event| {
        matches!(event, Event::Stopped { .. })
    });
    assert!(matches!(event,Event::Stopped{stop_id:1,ref location,..} if location.path==first.path));
    let event = wait(&mut second_session, |event| {
        matches!(event, Event::Stopped { .. })
    });
    assert!(
        matches!(event,Event::Stopped{stop_id:1,ref location,..} if location.path==second.path)
    );
    first_session.command(Command::StepOver).unwrap();
    let event = wait(&mut first_session, |event| {
        matches!(event, Event::Stopped { .. })
    });
    assert!(
        matches!(event,Event::Stopped{stop_id:2,ref location,..} if location.line==2 && location.path==first.path)
    );
    assert!(second_session.poll().unwrap().is_empty());
    first_session.into_final_snapshot().unwrap();
    second_session.into_final_snapshot().unwrap();
    runtime.shutdown().unwrap();
}

#[test]
fn finished_debug_output_keeps_history_without_the_debugger_directory_or_process() {
    let root = tempfile::tempdir().unwrap();
    let (runtime, app) = setup(root.path());
    let launch = spec(
        root.path(),
        "printf '%s' \"$_Dbg_tundra_root\" > debugger-directory\ni=0\nwhile (( i < 40 )); do\n printf 'line-%02d\\n' \"$i\"\n ((i+=1))\ndone\n",
        "/bin/bash",
    );
    let mut session =
        ScriptSession::spawn(&launch, true, 103, &app.task_group("history"), 20, 4).unwrap();
    let pid = session.terminal().process_id().unwrap().unwrap();
    wait(&mut session, |event| matches!(event, Event::Stopped { .. }));
    session.command(Command::Continue).unwrap();
    wait(&mut session, |event| matches!(event, Event::Exited { .. }));
    let debugger_directory =
        std::fs::read_to_string(root.path().join("debugger-directory")).unwrap();
    assert!(Path::new(&debugger_directory).is_dir());
    let output = session.into_output().unwrap();
    assert!(!Path::new(&debugger_directory).exists());
    assert!(!Path::new(&format!("/proc/{pid}")).exists());
    runtime.shutdown().unwrap();

    let snapshot_text = |snapshot: terminal_runtime::pty::TerminalSnapshot| {
        snapshot
            .cells
            .iter()
            .flatten()
            .map(|cell| cell.text.as_str())
            .collect::<String>()
    };
    assert!(snapshot_text(output.snapshot()).contains("line-39"));
    output.set_scrollback(usize::MAX).unwrap();
    assert!(snapshot_text(output.snapshot()).contains("line-00"));
    output.resize(32, 6).unwrap();
    output.set_scrollback(usize::MAX).unwrap();
    let resized = output.snapshot();
    assert_eq!((resized.columns, resized.rows), (32, 6));
    assert!(snapshot_text(resized).contains("line-00"));
}
