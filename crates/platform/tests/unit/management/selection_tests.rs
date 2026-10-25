use super::*;

fn request() -> ManagementCommand {
    ManagementCommand {
        kind: ManagementKind::Packages,
        action: "remove".into(),
        target: None,
        identity: BTreeMap::new(),
        values: BTreeMap::from([(
            KEY.into(),
            serde_json::to_string(&[
                Target {
                    id: "alpha".into(),
                    identity: BTreeMap::from([("installed_version".into(), "1".into())]),
                },
                Target {
                    id: "omega".into(),
                    identity: BTreeMap::from([("installed_version".into(), "2".into())]),
                },
            ])
            .unwrap(),
        )]),
    }
}

#[test]
fn selection_preserves_each_package_identity_and_operation_options() {
    let mut request = request();
    request.values.insert("yes".into(), "true".into());
    let commands = expand(&request).unwrap();
    assert_eq!(commands.len(), 2);
    for (command, (name, version)) in commands.iter().zip([("alpha", "1"), ("omega", "2")]) {
        assert_eq!(command.action, "remove");
        assert_eq!(command.target.as_deref(), Some(name));
        assert_eq!(command.identity["installed_version"], version);
        assert_eq!(
            command.values,
            BTreeMap::from([("yes".into(), "true".into())])
        );
    }
}

#[test]
fn batches_are_limited_to_the_supported_actions_on_each_page() {
    for (kind, action, unsupported) in [
        (ManagementKind::Services, "restart", "view_logs"),
        (ManagementKind::Processes, "nice", "inspect"),
        (ManagementKind::ScheduledTasks, "pause", "create_task"),
    ] {
        let mut request = request();
        request.kind = kind;
        request.action = action.into();
        assert_eq!(expand(&request).unwrap().len(), 2);
        request.action = unsupported.into();
        assert!(expand(&request).is_err());
    }
}

#[test]
fn ambiguous_empty_duplicate_or_global_selections_are_rejected() {
    for selection in [
        "[]",
        "invalid",
        r#"[{"id":"alpha","identity":{}},{"id":"alpha","identity":{}}]"#,
    ] {
        let mut request = request();
        request.values.insert(KEY.into(), selection.into());
        assert!(expand(&request).is_err());
    }
    for action in ["upgrade_all", "refresh", "source_remove"] {
        let mut request = request();
        request.action = action.into();
        assert!(expand(&request).is_err());
    }
    let mut request = request();
    request.target = Some("other".into());
    assert!(expand(&request).is_err());
}
