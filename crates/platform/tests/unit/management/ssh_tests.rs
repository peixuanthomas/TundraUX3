use super::*;
#[test]
fn native_ssh_tool_errors_suggest_authorization_only_for_explicit_permission_denial() {
    for (message, denied) in [
        ("/etc/ssh/sshd_config: Permission denied", true),
        (
            "System has not been booted with systemd as init system",
            false,
        ),
        ("sshd: no hostkeys available -- exiting", false),
    ] {
        let command = format!("printf '%s' '{message}' >&2; exit 1");
        let error = run("sh", &["-c", &command], &AtomicBool::new(false)).unwrap_err();
        assert_eq!(
            matches!(error, ManagementError::PermissionDenied(_)),
            denied
        );
        assert!(error.to_string().contains(message));
    }
}

#[test]
fn editing_globals_keeps_include_match_and_unrelated_content() {
    let text = "# comment\nPort 22\nInclude /etc/ssh/sshd_config.d/*.conf\nPasswordAuthentication yes\nMatch User restricted\n PasswordAuthentication no\n X11Forwarding no\n";
    let changed = rewrite(text, true, 2222, "no", "yes", "prohibit-password");
    assert!(changed.starts_with("Port 2222\nPasswordAuthentication no\n"));
    assert!(!changed.contains("Port 22\n"));
    assert!(changed.contains("Include /etc/ssh/sshd_config.d/*.conf\n"));
    assert!(
        changed.ends_with("Match User restricted\n PasswordAuthentication no\n X11Forwarding no\n")
    );
    assert!(rewrite("Port=2200\n# Port 33\n", false, 22, "yes", "yes", "no").eq("# Port 33\n"));
}
#[test]
fn socket_port_edit_keeps_bound_addresses() {
    assert_eq!(
        socket_content("127.0.0.1:22 (Stream) [::1]:22 (Stream)", 2222).unwrap(),
        "[Socket]\nListenStream=\nListenStream=127.0.0.1:2222\nListenStream=[::1]:2222\n"
    );
    assert!(socket_content("/run/private.sock (Stream)", 2222).is_err());
}
#[test]
fn trace_requires_actual_parse_evidence() {
    let files=traced_files("debug2: /etc/ssh/sshd_config line 1: new include /etc/ssh/sshd_config.d/*.conf\ndebug2: parse_server_config_depth: config /etc/ssh/sshd_config.d/a.conf len 30\n").unwrap();
    assert_eq!(
        files,
        vec![
            PathBuf::from(MAIN),
            PathBuf::from("/etc/ssh/sshd_config.d/a.conf")
        ]
    );
}
#[test]
fn context_rejects_argument_injection_and_invalid_addresses() {
    assert!(
        spec(
            "root",
            &BTreeMap::from([("addr".into(), "127.0.0.1,user=other".into())]),
            22
        )
        .is_err()
    );
    assert!(
        spec(
            "root",
            &BTreeMap::from([("laddr".into(), "localhost".into())]),
            22
        )
        .is_err()
    );
}

#[test]
fn chinese_ssh_labels_leave_actions_and_openssh_directives_unchanged() {
    let language = i18n::LanguageSnapshotData {
        code: "zh-CN".into(),
        generation: 1,
        current: vec![
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../ascii-assets/assets/locales/zh-CN/modules/management-ssh.ftl"
            ))
            .into(),
        ],
        english: Vec::new(),
    }
    .restore()
    .unwrap();
    let _language = i18n::enter_snapshot(std::sync::Arc::new(language));
    let action = action(
        "ssh_configure",
        i18n::tr!("platform-ssh-configure-ssh"),
        Vec::new(),
        None,
    );
    assert_eq!(action.id, "ssh_configure");
    assert_eq!(action.label, "配置 SSH");
    assert!(
        parse_port("0")
            .unwrap_err()
            .to_string()
            .contains("SSH 端口")
    );
    let content = rewrite("Port 22\n", true, 2222, "no", "yes", "prohibit-password");
    assert!(
        content.starts_with("Port 2222\nPasswordAuthentication no\nPubkeyAuthentication yes\n")
    );
    assert_eq!(
        spec("root", &BTreeMap::new(), 2222).unwrap(),
        "user=root,lport=2222,host=localhost,addr=127.0.0.1,laddr=127.0.0.1"
    );
}
