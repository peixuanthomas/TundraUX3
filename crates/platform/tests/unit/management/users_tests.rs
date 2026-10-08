use super::*;

fn account(uid: u32, local: bool) -> UserAccount {
    UserAccount {
        username: format!("user{uid}"),
        uid,
        gid: uid,
        display_name: String::new(),
        home: format!("/home/user{uid}").into(),
        shell: "/bin/bash".into(),
        local,
        system: uid < 1000,
        groups: vec!["sudo".into()],
        groups_complete: true,
        locked: None,
        expires: None,
    }
}

#[test]
fn passwd_keeps_root_service_and_directory_accounts() {
    let accounts = parse_passwd(
        "root:x:0:0:Root:/root:/bin/bash\ndaemon:x:1:1:Daemon:/usr/sbin:/usr/sbin/nologin\nalice:x:1000:1000:Alice,Room:/home/alice:/bin/bash\ndirectory:x:5000:5000:Directory:/home/directory:/bin/sh\ninvalid\n",
    );
    assert_eq!(accounts.len(), 4);
    assert_eq!(accounts[0].uid, 0);
    assert_eq!(accounts[1].shell, "/usr/sbin/nologin");
    assert_eq!(accounts[2].display_name, "Alice");
    assert_eq!(
        parse_groups("wheel:x:10:alice,bob\nbad\n")[0].members,
        ["alice", "bob"]
    );
}

#[test]
fn nss_lookup_accepts_directory_names_without_relaxing_local_mutations() {
    for name in ["DOMAIN\\user", "user@domain", "Directory User"] {
        assert!(validate_lookup_name(name).is_ok());
    }
    assert!(validate_name("DOMAIN\\user").is_err());
    assert!(validate_name("Directory User").is_err());
    for name in ["", "--help", "user\0name", "user\nname", "user\rname"] {
        assert!(validate_lookup_name(name).is_err());
    }
}

#[test]
fn selected_account_groups_use_keyed_nss_for_non_enumerating_directories() {
    let mut account = account(1000, false);
    account.username = "DOMAIN\\user".into();
    account.groups_complete = false;
    let mut calls = 0;
    complete_account_groups_with(&mut account, |program, args| {
        calls += 1;
        match program {
            "id" => {
                assert_eq!(args, ["--groups", "--", "DOMAIN\\user"]);
                Ok("1000 5000 5000\n".into())
            }
            "getent" => {
                assert_eq!(args, ["group", "1000", "5000"]);
                Ok("primary:x:1000:\nDOMAIN\\Domain Users:x:5000:\n".into())
            }
            _ => panic!("unexpected system query"),
        }
    });
    assert_eq!(calls, 2);
    assert!(account.groups_complete);
    assert_eq!(account.groups, ["DOMAIN\\Domain Users", "primary"]);
}

#[test]
fn unavailable_or_incomplete_selected_group_lookup_remains_unknown() {
    for mode in ["denied", "empty", "missing-group"] {
        let mut account = account(1000, true);
        let previous = account.groups.clone();
        complete_account_groups_with(&mut account, |program, _| match (mode, program) {
            ("denied", _) => Err(ManagementError::PermissionDenied("denied".into())),
            ("empty", "id") => Ok(String::new()),
            (_, "id") => Ok("1000 5000".into()),
            _ => Ok("primary:x:1000:\n".into()),
        });
        assert!(!account.groups_complete);
        assert_eq!(account.groups, previous);
        let row = account_row(&account, &[], &[]);
        assert!(
            row.detail
                .iter()
                .any(|(key, value)| key == "Groups" && value.starts_with("unknown"))
        );
        assert!(
            row.actions
                .iter()
                .find(|action| action.id == "user_groups")
                .unwrap()
                .disabled_reason
                .is_some()
        );
        assert!(protect_user(&account, 1000, "user_groups", "sudo").is_err());
    }
}
#[test]
fn current_login_root_and_remote_protections() {
    let current = account(1000, true);
    assert!(protect_user(&current, 1000, "user_delete", "").is_err());
    assert!(protect_user(&current, 1000, "user_lock", "").is_err());
    assert!(protect_user(&current, 1000, "user_groups", "audio").is_err());
    assert!(protect_user(&current, 1000, "user_groups", "audio,sudo").is_ok());
    assert!(protect_user(&account(0, true), 1000, "user_delete", "").is_err());
    assert!(protect_user(&account(1, true), 1000, "user_shell", "").is_ok());
    assert!(protect_user(&account(5000, false), 1000, "user_shell", "").is_err());
}

#[test]
fn custom_administrator_groups_in_system_policy_are_protected() {
    let groups = policy_groups(
        "# %commented ALL=(ALL) ALL\n%operators ALL=(ALL:ALL) ALL\npolkit.addAdminRule(function() { return ['unix-group:maintainers']; });\nif (subject.isInGroup(\"desktop-admin\")) { return polkit.Result.YES; }\n",
    );
    assert_eq!(
        groups.into_iter().collect::<Vec<_>>(),
        ["desktop-admin", "maintainers", "operators"]
    );
}
#[test]
fn expiry_and_tool_input_reject_ambiguous_or_destructive_values() {
    for value in ["", "2026-02-30", "2026-2-1", "2026-01-01\n"] {
        assert!(validate_expiry(value, false).is_err());
    }
    assert!(validate_expiry("never", true).is_ok());
    assert!(validate_expiry("2000-01-01", true).is_err());
    assert!(validate_expiry("2000-01-01", false).is_ok());
    for value in ["--root", "alice:bob", "../alice", "alice\nbob"] {
        assert!(validate_name(value).is_err());
    }
    for value in ["", "secret\nroot:secret", "secret\0"] {
        assert!(password_text(value).is_err());
    }
}
#[test]
fn fallback_only_for_missing_or_unsupported_service() {
    for name in [
        "org.freedesktop.DBus.Error.ServiceUnknown",
        "org.freedesktop.DBus.Error.UnknownMethod",
    ] {
        let error = zbus::Error::MethodError(
            name.try_into().unwrap(),
            None,
            zbus::Message::method_call("/", "Test")
                .unwrap()
                .destination("org.test")
                .unwrap()
                .build(&())
                .unwrap(),
        );
        assert!(fallback_error(&error));
    }
    for name in [
        "org.freedesktop.Accounts.Error.PermissionDenied",
        "org.freedesktop.DBus.Error.Timeout",
        "org.freedesktop.DBus.Error.NoReply",
        "org.freedesktop.DBus.Error.Disconnected",
    ] {
        let error = zbus::Error::MethodError(
            name.try_into().unwrap(),
            None,
            zbus::Message::method_call("/", "Test")
                .unwrap()
                .destination("org.test")
                .unwrap()
                .build(&())
                .unwrap(),
        );
        assert!(!fallback_error(&error));
    }
}
#[test]
fn key_files_refuse_symlink_replacement_and_preserve_other_lines() {
    let root = tempfile::tempdir().unwrap();
    let account = UserAccount {
        home: root.path().into(),
        uid: unsafe { libc::getuid() },
        gid: unsafe { libc::getgid() },
        ..account(1000, true)
    };
    let directory = File::open(root.path()).unwrap();
    fs::write(root.path().join("keys"), "# retained\nother\n").unwrap();
    let (text, identity) = read_key_file(&directory, "keys").unwrap();
    write_key_file(
        &directory,
        "keys",
        &text,
        identity,
        "# retained\nother\nnew\n",
        &account,
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(root.path().join("keys")).unwrap(),
        "# retained\nother\nnew\n"
    );
    fs::rename(root.path().join("keys"), root.path().join("original")).unwrap();
    std::os::unix::fs::symlink("original", root.path().join("keys")).unwrap();
    assert!(read_key_file(&directory, "keys").is_err());
    assert!(write_key_file(&directory, "keys", &text, identity, "replaced", &account).is_err());
    assert_eq!(
        fs::read_to_string(root.path().join("original")).unwrap(),
        "# retained\nother\nnew\n"
    );
}
