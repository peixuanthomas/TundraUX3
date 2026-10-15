use super::*;
#[test]
fn unrelated_accounts_and_fields_are_not_recovered() {
    let command = ManagementCommand {
        kind: ManagementKind::Users,
        action: "user_shell".into(),
        target: Some("alice".into()),
        values: BTreeMap::new(),
        identity: BTreeMap::new(),
    };
    let row = |shell: &str| {
        vec!["alice", "x", "1000", "1000", "Alice", "/home/alice", shell]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
    };
    let before = Snapshot::from([(
        "passwd".into(),
        Database::from([("alice".into(), row("/bin/sh"))]),
    )]);
    let mut after = before.clone();
    after
        .get_mut("passwd")
        .unwrap()
        .insert("alice".into(), row("/bin/bash"));
    after
        .get_mut("passwd")
        .unwrap()
        .insert("external".into(), row("/bin/zsh"));
    let backup: AccountBackup =
        serde_json::from_value(payload(&command, &before, &after).unwrap()).unwrap();
    assert_eq!(backup.changes.len(), 1);
    assert_eq!(backup.changes[0].fields, vec![6]);
    assert_eq!(backup.users.len(), 1);
    assert_eq!(backup.users["alice"].after, Some(1000));
    assert!(backup.groups.is_empty());
    assert!(describe_backup(&payload(&command, &before, &after).unwrap()).contains("Shell:"));
    let mut current = after.clone();
    current.get_mut("passwd").unwrap().get_mut("alice").unwrap()[4] = "New display name".into();
    assert!(check_current(&backup, &current).is_ok());
    current.get_mut("passwd").unwrap().get_mut("alice").unwrap()[2] = "1001".into();
    assert!(matches!(
        check_current(&backup, &current),
        Err(ManagementError::Conflict(_))
    ));
    current.get_mut("passwd").unwrap().get_mut("alice").unwrap()[2] = "1000".into();
    current.get_mut("passwd").unwrap().get_mut("alice").unwrap()[6] = "/bin/zsh".into();
    assert!(check_current(&backup, &current).is_err());
}
fn identity_fixture() -> Snapshot {
    let entries = |text: &str| {
        text.lines()
            .map(|line| {
                let fields = line.split(':').map(String::from).collect::<Vec<_>>();
                (fields[0].clone(), fields)
            })
            .collect()
    };
    Snapshot::from([
        (
            "passwd".into(),
            entries(
                "alice:x:1000:1000:Alice:/home/alice:/bin/sh\nbob:x:1002:1000:Bob:/home/bob:/bin/sh",
            ),
        ),
        (
            "shadow".into(),
            entries("alice:!old-secret:20000:0:99999:7:::"),
        ),
        (
            "group".into(),
            entries("staff:x:1000:alice,bob\nnext:x:1001:\nexternal:x:1002:"),
        ),
    ])
}
fn identity_command(action: &str, target: &str) -> ManagementCommand {
    ManagementCommand {
        kind: ManagementKind::Users,
        action: action.into(),
        target: Some(target.into()),
        values: BTreeMap::new(),
        identity: BTreeMap::new(),
    }
}
#[test]
fn shadow_recovery_rejects_recreated_account() {
    let before = identity_fixture();
    let mut after = before.clone();
    after.get_mut("shadow").unwrap().get_mut("alice").unwrap()[1] = "old-secret".into();
    let saved = payload(&identity_command("user_unlock", "alice"), &before, &after).unwrap();
    let backup: AccountBackup = serde_json::from_value(saved.clone()).unwrap();
    assert_eq!(backup.changes.len(), 1);
    assert_eq!(backup.changes[0].database, "shadow");
    assert_eq!(backup.users.len(), 1);
    assert_eq!(backup.users["alice"].before, Some(1000));
    assert!(backup.groups.is_empty());
    assert!(check_current(&backup, &after).is_ok());
    after.get_mut("passwd").unwrap().get_mut("alice").unwrap()[2] = "1003".into();
    assert!(matches!(
        check_current(&backup, &after),
        Err(ManagementError::Conflict(_))
    ));
    assert!(!describe_backup(&saved).contains("old-secret"));
    let mut legacy = saved;
    legacy.as_object_mut().unwrap().remove("users");
    assert!(serde_json::from_value::<AccountBackup>(legacy).is_err());
}
#[test]
fn membership_recovery_rejects_recreated_user_or_group() {
    let before = identity_fixture();
    let mut after = before.clone();
    after.get_mut("group").unwrap().get_mut("staff").unwrap()[3] = "bob".into();
    let backup: AccountBackup = serde_json::from_value(
        payload(&identity_command("user_groups", "alice"), &before, &after).unwrap(),
    )
    .unwrap();
    assert_eq!(backup.users.len(), 1);
    assert_eq!(backup.groups.len(), 1);
    let mut current = after.clone();
    current.get_mut("passwd").unwrap().get_mut("bob").unwrap()[2] = "1009".into();
    assert!(check_current(&backup, &current).is_ok());
    current.get_mut("passwd").unwrap().get_mut("alice").unwrap()[2] = "1003".into();
    assert!(matches!(
        check_current(&backup, &current),
        Err(ManagementError::Conflict(_))
    ));
    current = after.clone();
    current.get_mut("group").unwrap().get_mut("staff").unwrap()[2] = "1004".into();
    assert!(matches!(
        check_current(&backup, &current),
        Err(ManagementError::Conflict(_))
    ));
    let group_backup: AccountBackup = serde_json::from_value(
        payload(
            &identity_command("group_members", "group:staff"),
            &before,
            &after,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(group_backup.users.len(), 2);
    current = after;
    current.get_mut("passwd").unwrap().get_mut("bob").unwrap()[2] = "1009".into();
    assert!(matches!(
        check_current(&group_backup, &current),
        Err(ManagementError::Conflict(_))
    ));
}
#[test]
fn primary_group_recovery_rejects_reused_original_group() {
    let before = identity_fixture();
    let mut after = before.clone();
    after.get_mut("passwd").unwrap().get_mut("alice").unwrap()[3] = "1001".into();
    let backup: AccountBackup = serde_json::from_value(
        payload(
            &identity_command("user_primary_group", "alice"),
            &before,
            &after,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(backup.groups.len(), 2);
    assert_eq!(backup.groups["staff"].before, Some(1000));
    assert_eq!(backup.groups["next"].after, Some(1001));
    let mut current = after.clone();
    current
        .get_mut("group")
        .unwrap()
        .get_mut("external")
        .unwrap()[2] = "1009".into();
    assert!(check_current(&backup, &current).is_ok());
    let mut row = current.get_mut("group").unwrap().remove("staff").unwrap();
    row[0] = "replacement".into();
    current
        .get_mut("group")
        .unwrap()
        .insert("replacement".into(), row);
    assert!(matches!(
        check_current(&backup, &current),
        Err(ManagementError::Conflict(_))
    ));
    let mut inverse_changes = backup.changes.clone();
    for change in &mut inverse_changes {
        change.before = after
            .get(&change.database)
            .and_then(|db| db.get(&change.name))
            .cloned();
        change.after = before
            .get(&change.database)
            .and_then(|db| db.get(&change.name))
            .cloned();
    }
    let inverse = account_backup(
        backup.action,
        backup.target,
        inverse_changes,
        &after,
        &before,
    )
    .unwrap();
    assert!(check_current(&inverse, &before).is_ok());
    let mut actual = before;
    actual.get_mut("passwd").unwrap().get_mut("alice").unwrap()[2] = "1010".into();
    let inverse = account_backup(
        inverse.action,
        inverse.target,
        inverse.changes,
        &after,
        &actual,
    )
    .unwrap();
    assert_eq!(inverse.users["alice"].before, Some(1000));
    assert_eq!(inverse.users["alice"].after, Some(1010));
    assert!(check_identities(&inverse, &actual, true).is_ok());
}
#[test]
fn membership_recovery_preserves_other_members() {
    let c = Change {
        database: "group".into(),
        name: "staff".into(),
        before: Some(vec![
            "staff".into(),
            "x".into(),
            "1000".into(),
            "alice".into(),
        ]),
        after: Some(vec![
            "staff".into(),
            "x".into(),
            "1000".into(),
            String::new(),
        ]),
        fields: vec![3],
        member: Some("alice".into()),
    };
    let current = vec!["staff".into(), "x".into(), "1000".into(), "bob".into()];
    assert!(expected(&c, Some(&current), true));
}
#[test]
#[ignore = "requires root in a private mount namespace; never run against the host account databases"]
fn real_isolated_account_recovery_preserves_unrelated_changes_and_homes() {
    assert_eq!(unsafe { libc::geteuid() }, 0);
    assert_ne!(
        fs::read_link("/proc/self/ns/mnt").unwrap(),
        fs::read_link("/proc/1/ns/mnt").unwrap()
    );
    let temp = tempfile::tempdir().unwrap();
    for name in ["etc", "run", "lib", "log", "spool", "home"] {
        fs::create_dir(temp.path().join(name)).unwrap();
    }
    let etc = temp.path().join("etc");
    fs::write(etc.join("passwd"),"root:x:0:0:root:/root:/bin/bash\nqa_person:x:55001:55000:Original:/home/qa_person:/bin/sh\nqa_other:x:55003:0:Other:/home/qa_other:/bin/sh\n").unwrap();
    fs::write(etc.join("shadow"),"root:*:20000:0:99999:7:::\nqa_person:!original:20000:0:99999:7:::\nqa_other:*:20000:0:99999:7:::\n").unwrap();
    fs::write(etc.join("group"), "root:x:0:\nqa_team:x:55000:qa_person\n").unwrap();
    fs::write(etc.join("gshadow"), "root:*::\nqa_team:*::qa_person\n").unwrap();
    fs::write(
        etc.join("subuid"),
        "qa_person:300000:65536\nqa_person:400000:65536\n",
    )
    .unwrap();
    fs::write(
        etc.join("subgid"),
        "qa_person:300000:65536\nqa_person:400000:65536\n",
    )
    .unwrap();
    fs::write(
        etc.join("nsswitch.conf"),
        "passwd: files\ngroup: files\nshadow: files\ngshadow: files\n",
    )
    .unwrap();
    fs::write(etc.join("login.defs"),"UID_MIN 55000\nUID_MAX 55999\nGID_MIN 55000\nGID_MAX 55999\nSUB_UID_COUNT 0\nSUB_GID_COUNT 0\nCREATE_MAIL_SPOOL no\nUSERGROUPS_ENAB no\n").unwrap();
    for name in ["qa_person", "qa_other", "qa_created"] {
        fs::create_dir(temp.path().join("home").join(name)).unwrap();
    }
    fs::write(temp.path().join("home/qa_person/keep.txt"), "retained").unwrap();
    for (source, target) in [
        ("etc", "/etc"),
        ("run", "/run"),
        ("lib", "/var/lib"),
        ("log", "/var/log"),
        ("spool", "/var/spool"),
        ("home", "/home"),
    ] {
        let from = std::ffi::CString::new(temp.path().join(source).as_os_str().as_encoded_bytes())
            .unwrap();
        let to = std::ffi::CString::new(target).unwrap();
        assert_eq!(
            unsafe {
                libc::mount(
                    from.as_ptr(),
                    to.as_ptr(),
                    std::ptr::null(),
                    libc::MS_BIND,
                    std::ptr::null(),
                )
            },
            0,
            "{}",
            std::io::Error::last_os_error()
        );
    }
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
            panic!("Unexpected interactive recovery")
        }
    }
    let context = ExecutionContext {
        actor_uid: 0,
        helper_path: PathBuf::new(),
        caller_pid: 0,
        caller_start_ticks: 0,
    };
    let flag = AtomicBool::new(false);
    let mut io = Interaction;
    let command = |action: &str, target: &str| ManagementCommand {
        kind: ManagementKind::Users,
        action: action.into(),
        target: Some(target.into()),
        values: BTreeMap::new(),
        identity: BTreeMap::new(),
    };
    let before = capture().unwrap();
    native(
        "usermod",
        &["--shell", "/bin/bash", "--", "qa_person"],
        None,
    )
    .unwrap();
    let shell = payload(
        &command("user_shell", "qa_person"),
        &before,
        &capture().unwrap(),
    )
    .unwrap();
    native(
        "usermod",
        &["--comment", "Later display name", "--", "qa_person"],
        None,
    )
    .unwrap();
    restore_backup(&shell, &context, &mut io, &flag).unwrap();
    let actual = capture().unwrap();
    assert_eq!(actual["passwd"]["qa_person"][6], "/bin/sh");
    assert_eq!(actual["passwd"]["qa_person"][4], "Later display name");
    let before = capture().unwrap();
    native(
        "chpasswd",
        &["--encrypted"],
        Some(b"qa_person:!new-secret\n"),
    )
    .unwrap();
    let password = payload(
        &command("user_password", "qa_person"),
        &before,
        &capture().unwrap(),
    )
    .unwrap();
    assert!(!describe_backup(&password).contains("new-secret"));
    restore_backup(&password, &context, &mut io, &flag).unwrap();
    assert_eq!(capture().unwrap()["shadow"]["qa_person"][1], "!original");
    let before = capture().unwrap();
    native("usermod", &["--unlock", "--", "qa_person"], None).unwrap();
    let unlocked = payload(
        &command("user_unlock", "qa_person"),
        &before,
        &capture().unwrap(),
    )
    .unwrap();
    native("chage", &["--lastday", "18000", "--", "qa_person"], None).unwrap();
    restore_backup(&unlocked, &context, &mut io, &flag).unwrap();
    let actual = capture().unwrap();
    assert_eq!(actual["shadow"]["qa_person"][1], "!original");
    assert_eq!(actual["shadow"]["qa_person"][2], "18000");
    let before = capture().unwrap();
    native("gpasswd", &["--delete", "qa_person", "qa_team"], None).unwrap();
    let memberships = payload(
        &command("user_groups", "qa_person"),
        &before,
        &capture().unwrap(),
    )
    .unwrap();
    native("gpasswd", &["--add", "qa_other", "qa_team"], None).unwrap();
    restore_backup(&memberships, &context, &mut io, &flag).unwrap();
    let members = capture().unwrap()["group"]["qa_team"][3].clone();
    assert!(members.split(',').any(|n| n == "qa_person"));
    assert!(members.split(',').any(|n| n == "qa_other"));
    let before = capture().unwrap();
    native("userdel", &["--", "qa_person"], None).unwrap();
    let deletion = payload(
        &command("user_delete", "qa_person"),
        &before,
        &capture().unwrap(),
    )
    .unwrap();
    restore_backup(&deletion, &context, &mut io, &flag).unwrap();
    assert_eq!(capture().unwrap()["passwd"]["qa_person"][2], "55001");
    assert_eq!(capture().unwrap()["subuid"]["qa_person"].len(), 6);
    assert_eq!(
        fs::read_to_string("/home/qa_person/keep.txt").unwrap(),
        "retained"
    );
    let before = capture().unwrap();
    native(
        "useradd",
        &[
            "--no-create-home",
            "--no-user-group",
            "--uid",
            "55002",
            "--gid",
            "55000",
            "--home-dir",
            "/home/qa_created",
            "--shell",
            "/bin/sh",
            "--",
            "qa_created",
        ],
        None,
    )
    .unwrap();
    let mut creation = command("user_create", "");
    creation
        .values
        .insert("username".into(), "qa_created".into());
    restore_backup(
        &payload(&creation, &before, &capture().unwrap()).unwrap(),
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    assert!(!capture().unwrap()["passwd"].contains_key("qa_created"));
    assert!(Path::new("/home/qa_created").is_dir());
    let before = capture().unwrap();
    native(
        "groupmod",
        &["--new-name", "qa_renamed", "--", "qa_team"],
        None,
    )
    .unwrap();
    let mut rename = command("group_rename", "group:qa_team");
    rename.values.insert("name".into(), "qa_renamed".into());
    restore_backup(
        &payload(&rename, &before, &capture().unwrap()).unwrap(),
        &context,
        &mut io,
        &flag,
    )
    .unwrap();
    assert!(capture().unwrap()["group"].contains_key("qa_team"));
    let before = capture().unwrap();
    native(
        "usermod",
        &["--shell", "/bin/bash", "--unlock", "--", "qa_person"],
        None,
    )
    .unwrap();
    let after = capture().unwrap();
    let shell = payload(&command("user_shell", "qa_person"), &before, &after).unwrap();
    let unlocked = payload(&command("user_unlock", "qa_person"), &before, &after).unwrap();
    native("userdel", &["--", "qa_person"], None).unwrap();
    native(
        "useradd",
        &[
            "--no-create-home",
            "--no-user-group",
            "--uid",
            "55004",
            "--gid",
            "55000",
            "--home-dir",
            "/home/qa_person",
            "--shell",
            "/bin/bash",
            "--",
            "qa_person",
        ],
        None,
    )
    .unwrap();
    native("chpasswd", &["--encrypted"], Some(b"qa_person:original\n")).unwrap();
    let recreated = capture().unwrap();
    for backup in [shell, unlocked] {
        assert!(matches!(
            restore_backup(&backup, &context, &mut io, &flag),
            Err(ManagementError::Conflict(_))
        ));
    }
    assert_eq!(capture().unwrap(), recreated);
    native("userdel", &["--", "qa_person"], None).unwrap();
    native(
        "useradd",
        &[
            "--no-create-home",
            "--no-user-group",
            "--uid",
            "55001",
            "--gid",
            "55000",
            "--home-dir",
            "/home/qa_created",
            "--shell",
            "/bin/sh",
            "--",
            "qa_collision",
        ],
        None,
    )
    .unwrap();
    assert!(matches!(
        restore_backup(&deletion, &context, &mut io, &flag),
        Err(ManagementError::Conflict(_))
    ));
    let before = capture().unwrap();
    native(
        "usermod",
        &["--shell", "/bin/bash", "--", "qa_collision"],
        None,
    )
    .unwrap();
    let shell = payload(
        &command("user_shell", "qa_collision"),
        &before,
        &capture().unwrap(),
    )
    .unwrap();
    let shadow = fs::read_to_string("/etc/shadow").unwrap();
    fs::write(
        "/etc/shadow",
        shadow
            .lines()
            .filter(|line| !line.starts_with("qa_other:"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    assert!(matches!(
        restore_backup(&shell, &context, &mut io, &flag),
        Err(ManagementError::Failed(_))
    ));
    assert_eq!(capture().unwrap()["passwd"]["qa_collision"][6], "/bin/sh");
    assert!(
        fs::read_dir("/var/lib/tundraux3/config-backups/operations")
            .unwrap()
            .any(|entry| {
                let record: Value =
                    serde_json::from_reader(File::open(entry.unwrap().path()).unwrap()).unwrap();
                record["payload"]["target"] == "qa_collision"
                    && record["result"] == "account fields restored; pwck: failed; grpck: passed"
                    && record["after"].is_array()
            })
    );
}
