use super::*;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Interaction;
impl OperationInteraction for Interaction {
    fn emit(&mut self, _event: OperationEvent) {}
    fn ask(
        &mut self,
        _id: &str,
        _prompt: &str,
        _choices: &[String],
        _secret: bool,
    ) -> Result<String, ManagementError> {
        Ok("Save and enable".into())
    }
}

fn selected(name: &str, flag: &AtomicBool) -> ManagementRow {
    systemd::rows("system", flag)
        .unwrap()
        .into_iter()
        .find(|row| row.cells[0] == name)
        .unwrap()
}

fn command(action: &str, row: &ManagementRow) -> ManagementCommand {
    ManagementCommand {
        kind: ManagementKind::ScheduledTasks,
        action: action.into(),
        target: Some(row.id.clone()),
        identity: row.identity.clone(),
        values: BTreeMap::new(),
    }
}

fn backup_id(message: &str) -> &str {
    message.split_whitespace().last().unwrap()
}

fn recover(
    id: &str,
    context: &ExecutionContext,
    io: &mut Interaction,
    flag: &AtomicBool,
) -> Result<String, ManagementError> {
    let record: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(format!(
            "/var/lib/tundraux3/config-backups/operations/{id}.json"
        ))
        .unwrap(),
    )
    .unwrap();
    restore_backup(id, &record["payload"], context, io, flag)
}

#[test]
#[ignore = "Run only inside the private systemd environment created by isolated-systemd.sh"]
fn private_systemd_create_execute_persist_pause_resume_and_restore() {
    assert_eq!(
        fs::read_to_string("/run/tundra-scheduled-isolated").unwrap(),
        "private\n"
    );
    assert_eq!(unsafe { libc::geteuid() }, 0);
    assert!(fs::read_dir("/usr/lib/systemd/system").unwrap().count() < 20);
    let uid = std::env::var("TUNDRA_FIXTURE_UID")
        .unwrap()
        .parse()
        .unwrap();
    let account = actor(uid).unwrap();
    let context = ExecutionContext {
        caller_pid: 0,
        caller_start_ticks: 0,
        actor_uid: uid,
        helper_path: PathBuf::from("/usr/bin/false"),
    };
    let flag = AtomicBool::new(false);
    let mut io = Interaction;
    let values = BTreeMap::from([
        ("name".into(), "isolated".into()),
        ("command".into(), "/usr/bin/sleep 0.1; /usr/bin/true".into()),
        ("working_directory".into(), "/tmp".into()),
        ("schedule".into(), "2099-01-01 00:00:00".into()),
        ("user".into(), account.username.clone()),
        ("yes".into(), "true".into()),
    ]);
    let created = execute(
        &ManagementCommand {
            kind: ManagementKind::ScheduledTasks,
            action: "create_task".into(),
            target: None,
            identity: BTreeMap::new(),
            values,
        },
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    let timer = "tundra-scheduled-isolated.timer";
    let row = selected(timer, &flag);
    assert!(row.cells[4].contains("active; enabled"));
    execute(&command("run_now", &row), &context, &mut io, &flag).unwrap();
    let result_path = Path::new(RESULTS)
        .join(uid.to_string())
        .join("tundra-scheduled-isolated.result");
    let started = Instant::now();
    while !result_path.exists() && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let saved = fs::read_to_string(&result_path).unwrap();
    assert!(saved.contains("\nsuccess\nexited\n0\n"), "{saved}");
    let paused = execute(
        &command("pause", &selected(timer, &flag)),
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    assert!(selected(timer, &flag).cells[4].contains("inactive; disabled"));
    let resumed = execute(
        &command("resume", &selected(timer, &flag)),
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    assert!(selected(timer, &flag).cells[4].contains("active; enabled"));
    assert_eq!(fs::read_to_string(result_path).unwrap(), saved);
    assert!(selected(timer, &flag).cells[5].contains("success"));
    assert!(paused.contains("Backup ") && resumed.contains("Backup "));
    assert!(matches!(
        recover(backup_id(&paused), &context, &mut io, &flag),
        Err(ManagementError::Conflict(_))
    ));
    let restored = recover(backup_id(&resumed), &context, &mut io, &flag).unwrap();
    assert!(selected(timer, &flag).cells[4].contains("inactive; disabled"));
    recover(backup_id(&restored), &context, &mut io, &flag).unwrap();
    assert!(selected(timer, &flag).cells[4].contains("active; enabled"));
    let removed = recover(backup_id(&created), &context, &mut io, &flag).unwrap();
    assert!(!Path::new("/etc/systemd/system/tundra-scheduled-isolated.timer").exists());
    recover(backup_id(&removed), &context, &mut io, &flag).unwrap();
    assert!(selected(timer, &flag).cells[4].contains("active; enabled"));
    // A lost working directory must fail before running the task and still record a result.
    let directory = Path::new("/tmp/tundra-fixture-work");
    fs::create_dir(directory).unwrap();
    let values = BTreeMap::from([
        ("name".into(), "missing-workdir".into()),
        ("command".into(), "/usr/bin/true".into()),
        ("working_directory".into(), directory.display().to_string()),
        ("schedule".into(), "2099-01-01 00:00:00".into()),
        ("user".into(), account.username),
        ("yes".into(), "true".into()),
    ]);
    execute(
        &ManagementCommand {
            kind: ManagementKind::ScheduledTasks,
            action: "create_task".into(),
            target: None,
            identity: BTreeMap::new(),
            values,
        },
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    fs::remove_dir(directory).unwrap();
    execute(
        &command(
            "run_now",
            &selected("tundra-scheduled-missing-workdir.timer", &flag),
        ),
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    let path = Path::new(RESULTS)
        .join(uid.to_string())
        .join("tundra-scheduled-missing-workdir.result");
    let started = Instant::now();
    while !path.exists() && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(fs::read_to_string(path).unwrap().contains("\nexit-code\n"));
    let cron_text = "# preserve this comment\nSHELL=/bin/sh\nCRON_TZ=Asia/Shanghai\n0 1 * * * cat%first%second\n";
    let account = actor(uid).unwrap();
    let output = native(
        "/usr/bin/crontab",
        &["-u".into(), account.username.clone(), "-".into()],
        Some(cron_text.as_bytes()),
        &flag,
    )
    .unwrap();
    assert!(output.status.success());
    let cron_row = |flag: &AtomicBool| {
        query_as_actor(
            &ManagementQuery::new(ManagementKind::ScheduledTasks),
            uid,
            flag,
        )
        .unwrap()
        .rows
        .into_iter()
        .find(|row| {
            row.id
                .starts_with(&format!("crontab:user:{}:", account.username))
        })
        .unwrap()
    };
    let paused = execute(
        &command("pause", &cron_row(&flag)),
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    assert_eq!(cron_row(&flag).cells[4], "Paused");
    let restored = recover(backup_id(&paused), &context, &mut io, &flag).unwrap();
    assert_eq!(cron_row(&flag).cells[4], "Scheduled");
    recover(backup_id(&restored), &context, &mut io, &flag).unwrap();
    assert_eq!(cron_row(&flag).cells[4], "Paused");
    let cron = cron_row(&flag);
    execute(&command("run_now", &cron), &context, &mut io, &flag).unwrap();
    let path = Path::new(RESULTS)
        .join(uid.to_string())
        .join(format!("{}.result", cron.identity["result_key"]));
    let started = Instant::now();
    while !path.exists() && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        fs::read_to_string(path)
            .unwrap()
            .contains("\nsuccess\nexited\n0\n")
    );
    assert_eq!(cron_row(&flag).cells[5], "Not recorded by cron");
    assert!(
        cron_row(&flag)
            .detail
            .iter()
            .any(|(key, value)| key == "Manual execution result" && value.contains("success"))
    );
}
