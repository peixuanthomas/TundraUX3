use super::*;
use std::process::{Child, Command, Stdio};

struct Interaction;
impl OperationInteraction for Interaction {
    fn emit(&mut self, _: OperationEvent) {}
    fn ask(&mut self, _: &str, _: &str, _: &[String], _: bool) -> Result<String, ManagementError> {
        panic!("process backend must never collect administrator passwords")
    }
}

struct ChildGuard(Child);
impl ChildGuard {
    fn new() -> Self {
        Self(
            Command::new("/bin/sleep")
                .arg("30")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn command(&self, action: &str) -> ManagementCommand {
        let pid = self.0.id();
        let info = read_process(Path::new("/proc"), pid, 4096).unwrap();
        let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap();
        ManagementCommand {
            kind: ManagementKind::Processes,
            action: action.into(),
            target: Some(pid.to_string()),
            values: BTreeMap::new(),
            identity: row(&info, boot.trim(), 0).identity,
        }
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn context() -> ExecutionContext {
    ExecutionContext {
        actor_uid: unsafe { libc::getuid() },
        helper_path: PathBuf::new(),
    }
}

#[test]
fn stat_parser_keeps_parentheses_and_reads_correct_kernel_fields() {
    let mut fields = vec!["0"; 22];
    fields[0] = "S";
    fields[1] = "7";
    fields[11] = "300";
    fields[12] = "50";
    fields[16] = "9";
    fields[17] = "2";
    fields[19] = "555";
    fields[20] = "1024";
    fields[21] = "3";
    let parsed = parse_stat(&format!(
        "123 (worker (test) with spaces) {}",
        fields.join(" ")
    ))
    .unwrap();
    assert_eq!(
        (
            parsed.pid,
            parsed.parent,
            parsed.cpu_ticks,
            parsed.started,
            parsed.nice,
            parsed.threads,
            parsed.resident_pages
        ),
        (123, 7, 350, 555, 9, 2, 3)
    );
    assert_eq!(parsed.name, "worker (test) with spaces");
    assert!(parse_stat("123 (broken) S 1").is_none());
}

fn fixture(pid: u32, parent: u32) -> ProcessRecord {
    ProcessRecord {
        pid,
        parent,
        uid: 1000,
        name: format!("worker-{pid}"),
        command: format!("worker {pid}"),
        state: 'S',
        nice: 0,
        threads: 1,
        started: u64::from(pid),
        cpu_ticks: 0,
        memory: u64::from(pid) * 1024,
        cpu: Some(f64::from(pid)),
    }
}

#[test]
fn complete_snapshot_search_sort_and_tree_never_truncate_or_hide_orphans() {
    let processes = (2..62)
        .map(|pid| fixture(pid, if pid % 3 == 0 { 2 } else { 9999 }))
        .collect::<Vec<_>>();
    let mut query = ManagementQuery::new(ManagementKind::Processes);
    let complete = snapshot(processes.clone(), &query, "boot", 0).unwrap();
    assert_eq!(complete.rows.len(), 60);
    assert_eq!(complete.rows[0].id, "61");
    query.options.insert("sort".into(), "pid".into());
    query.options.insert("descending".into(), "false".into());
    query.options.insert("tree".into(), "true".into());
    let tree = snapshot(processes.clone(), &query, "boot", 0).unwrap();
    assert_eq!(tree.rows.len(), 60);
    assert_eq!(tree.rows[0].id, "2");
    assert_eq!(tree.rows[1].id, "3");
    assert!(tree.rows[1].cells[7].starts_with("  "));
    query.filter = "worker-61".into();
    let filtered = snapshot(processes, &query, "boot", 0).unwrap();
    assert_eq!(filtered.rows.len(), 1);
    assert_eq!(filtered.rows[0].id, "61");
}

#[test]
fn main_thread_nice_requests_authorization_even_for_owned_processes() {
    let mut process = fixture(200, 2);
    process.uid = unsafe { libc::getuid() };
    let owned = row(&process, "boot", 0);
    assert!(
        owned
            .actions
            .iter()
            .find(|action| action.id == "nice")
            .unwrap()
            .privileged
    );
    assert!(
        owned
            .actions
            .iter()
            .filter(|action| action.id != "nice")
            .all(|action| !action.privileged)
    );

    process.uid = process.uid.wrapping_add(1);
    let foreign = row(&process, "boot", 0);
    assert!(foreign.actions.iter().all(|action| action.privileged));
}

#[test]
fn stale_pid_identity_and_invalid_nice_are_rejected_before_changes() {
    let child = ChildGuard::new();
    let mut command = child.command("kill");
    command
        .identity
        .insert("start_time_ticks".into(), "0".into());
    assert!(matches!(
        execute(
            &command,
            &context(),
            &mut Interaction,
            &AtomicBool::new(false)
        ),
        Err(ManagementError::Conflict(_))
    ));
    assert!(read_process(Path::new("/proc"), child.0.id(), 4096).is_ok());
    let mut command = child.command("nice");
    command.values.insert("nice".into(), "20".into());
    assert!(matches!(
        execute(
            &command,
            &context(),
            &mut Interaction,
            &AtomicBool::new(false)
        ),
        Err(ManagementError::InvalidInput(_))
    ));
    command.target = Some("0".into());
    assert!(matches!(
        execute(
            &command,
            &context(),
            &mut Interaction,
            &AtomicBool::new(false)
        ),
        Err(ManagementError::InvalidInput(_))
    ));
}

fn wait_state(pid: u32, stopped: bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let info = read_process(Path::new("/proc"), pid, 4096).unwrap();
        if (info.state == 'T') == stopped {
            return;
        }
        assert!(Instant::now() < deadline, "child state did not change");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn native_pidfd_stop_continue_term_and_kill_only_touch_owned_children() {
    let cancelled = AtomicBool::new(false);
    for finish in ["term", "kill"] {
        let mut child = ChildGuard::new();
        execute(
            &child.command("stop"),
            &context(),
            &mut Interaction,
            &cancelled,
        )
        .unwrap();
        wait_state(child.0.id(), true);
        execute(
            &child.command("cont"),
            &context(),
            &mut Interaction,
            &cancelled,
        )
        .unwrap();
        wait_state(child.0.id(), false);
        execute(
            &child.command(finish),
            &context(),
            &mut Interaction,
            &cancelled,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while child.0.try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn native_nice_changes_the_child_main_thread_and_cancellation_does_nothing() {
    let child = ChildGuard::new();
    let mut command = child.command("nice");
    command.values.insert("nice".into(), "10".into());
    execute(
        &command,
        &context(),
        &mut Interaction,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        read_process(Path::new("/proc"), child.0.id(), 4096)
            .unwrap()
            .nice,
        10
    );
    let command = child.command("kill");
    assert_eq!(
        execute(
            &command,
            &context(),
            &mut Interaction,
            &AtomicBool::new(true)
        ),
        Err(ManagementError::Cancelled)
    );
    assert!(read_process(Path::new("/proc"), child.0.id(), 4096).is_ok());
}
