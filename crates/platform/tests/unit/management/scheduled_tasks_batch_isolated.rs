use super::*;

fn batch(kind: ManagementKind, action: &str, rows: &[ManagementRow]) -> ManagementCommand {
    let targets = rows
        .iter()
        .map(|row| selection::Target {
            id: row.id.clone(),
            identity: row.identity.clone(),
        })
        .collect::<Vec<_>>();
    ManagementCommand {
        kind,
        action: action.into(),
        target: None,
        identity: BTreeMap::new(),
        values: BTreeMap::from([(
            selection::KEY.into(),
            serde_json::to_string(&targets).unwrap(),
        )]),
    }
}

#[test]
#[ignore = "Run only with isolated-systemd.sh selection; never against the host manager"]
fn private_management_batches_preserve_targets_and_cron_sources() {
    assert_eq!(
        fs::read_to_string("/run/tundra-scheduled-isolated").unwrap(),
        "private\n"
    );
    for (path, name) in [
        ("mnt", "TUNDRA_HOST_MNT_NS"),
        ("net", "TUNDRA_HOST_NET_NS"),
        ("pid", "TUNDRA_HOST_PID_NS"),
    ] {
        assert_ne!(
            fs::read_link(format!("/proc/self/ns/{path}"))
                .unwrap()
                .to_str()
                .unwrap(),
            std::env::var(name).unwrap()
        );
    }
    assert_eq!(unsafe { libc::geteuid() }, 0);
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
    let names = ["batch-one.service", "batch-two.service"];
    for name in names {
        fs::write(format!("/etc/systemd/system/{name}"), "[Unit]\nDefaultDependencies=no\n[Service]\nExecStart=/usr/bin/sleep infinity\nExecReload=/usr/bin/true\n[Install]\nWantedBy=fixture.target\n").unwrap();
    }
    run("/usr/bin/systemctl", &["daemon-reload".into()], &flag).unwrap();
    let services = super::super::super::services::query(
        &ManagementQuery::new(ManagementKind::Services),
        &flag,
    )
    .unwrap();
    let rows = services
        .rows
        .into_iter()
        .filter(|row| names.contains(&row.id.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    for action in ["enable", "start", "restart", "reload", "stop", "disable"] {
        super::super::super::services::execute(
            &batch(ManagementKind::Services, action, &rows),
            &context,
            &mut io,
            &flag,
        )
        .unwrap();
        for name in names {
            let property = if matches!(action, "enable" | "disable") {
                "UnitFileState"
            } else {
                "ActiveState"
            };
            let actual = run(
                "/usr/bin/systemctl",
                &[
                    "show".into(),
                    "--value".into(),
                    format!("--property={property}"),
                    name.into(),
                ],
                &flag,
            )
            .unwrap();
            let expected = match action {
                "enable" => "enabled",
                "disable" => "disabled",
                "stop" => "inactive",
                _ => "active",
            };
            assert_eq!(actual.trim(), expected, "{action} {name}");
        }
    }
    for name in ["batch-one", "batch-two"] {
        execute(
            &ManagementCommand {
                kind: ManagementKind::ScheduledTasks,
                action: "create_task".into(),
                target: None,
                identity: BTreeMap::new(),
                values: BTreeMap::from([
                    ("name".into(), name.into()),
                    ("command".into(), "/usr/bin/true".into()),
                    ("working_directory".into(), "/tmp".into()),
                    ("schedule".into(), "2099-01-01 00:00:00".into()),
                    ("user".into(), account.username.clone()),
                    ("yes".into(), "true".into()),
                ]),
            },
            &context,
            &mut io,
            &flag,
        )
        .unwrap();
    }
    let timers = || {
        [
            "tundra-scheduled-batch-one.timer",
            "tundra-scheduled-batch-two.timer",
        ]
        .map(|name| selected(name, &flag))
    };
    execute(
        &batch(ManagementKind::ScheduledTasks, "pause", &timers()),
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    assert!(
        timers()
            .iter()
            .all(|row| row.cells[4].contains("inactive; disabled"))
    );
    execute(
        &batch(ManagementKind::ScheduledTasks, "resume", &timers()),
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    assert!(
        timers()
            .iter()
            .all(|row| row.cells[4].contains("active; enabled"))
    );
    execute(
        &batch(ManagementKind::ScheduledTasks, "run_now", &timers()),
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    let cron_text = "# keep\nSHELL=/bin/sh\n@daily /usr/bin/true\n0 1 * * * /usr/bin/true\n";
    assert!(
        native(
            "/usr/bin/crontab",
            &["-u".into(), account.username.clone(), "-".into()],
            Some(cron_text.as_bytes()),
            &flag
        )
        .unwrap()
        .status
        .success()
    );
    let cron_rows = || {
        query_as_actor(
            &ManagementQuery::new(ManagementKind::ScheduledTasks),
            uid,
            &flag,
        )
        .unwrap()
        .rows
        .into_iter()
        .filter(|row| row.identity.get("source").map(String::as_str) == Some("crontab"))
        .collect::<Vec<_>>()
    };
    assert_eq!(cron_rows().len(), 2);
    execute(
        &batch(ManagementKind::ScheduledTasks, "pause", &cron_rows()),
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    assert!(cron_rows().iter().all(|row| row.cells[4] == "Paused"));
    let resumed = execute(
        &batch(ManagementKind::ScheduledTasks, "resume", &cron_rows()),
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    let current = run(
        "/usr/bin/crontab",
        &["-u".into(), account.username.clone(), "-l".into()],
        &flag,
    )
    .unwrap();
    assert_eq!(current, cron_text);
    let restored = recover(backup_id(&resumed), &context, &mut io, &flag).unwrap();
    assert!(cron_rows().iter().all(|row| row.cells[4] == "Paused"));
    recover(backup_id(&restored), &context, &mut io, &flag).unwrap();
    let crons = cron_rows();
    execute(
        &batch(ManagementKind::ScheduledTasks, "run_now", &crons),
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    let paths = [
        "tundra-scheduled-batch-one".into(),
        "tundra-scheduled-batch-two".into(),
    ]
    .into_iter()
    .chain(crons.iter().map(|row| row.identity["result_key"].clone()))
    .map(|name| {
        Path::new(RESULTS)
            .join(uid.to_string())
            .join(format!("{name}.result"))
    })
    .collect::<Vec<_>>();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !paths.iter().all(|path| path.exists()) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    for path in paths {
        assert!(
            fs::read_to_string(path)
                .unwrap()
                .contains("\nsuccess\nexited\n0\n")
        );
    }
}
