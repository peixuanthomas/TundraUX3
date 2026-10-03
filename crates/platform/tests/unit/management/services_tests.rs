use super::*;

fn loaded(name: &str, active: &str) -> LoadedUnit {
    (
        name.into(),
        "Example service".into(),
        "loaded".into(),
        active.into(),
        "running".into(),
        String::new(),
        OwnedObjectPath::try_from("/org/freedesktop/systemd1/unit/example").unwrap(),
        0,
        String::new(),
        OwnedObjectPath::try_from("/").unwrap(),
    )
}

#[test]
fn list_merges_loaded_and_unloaded_service_files_without_losing_states() {
    let records = merge_units(
        vec![
            loaded("example.service", "active"),
            loaded("run-demo.service", "active"),
            loaded("ignored.socket", "active"),
        ],
        vec![
            (
                "/usr/lib/systemd/system/example.service".into(),
                "enabled".into(),
            ),
            (
                "/usr/lib/systemd/system/inactive.service".into(),
                "disabled".into(),
            ),
            (
                "/usr/lib/systemd/system/fixed.service".into(),
                "static".into(),
            ),
        ],
    );
    assert_eq!(records.len(), 4);
    assert_eq!(records["example.service"].active, "active");
    assert_eq!(records["example.service"].file_state, "enabled");
    assert_eq!(records["inactive.service"].load, "unloaded");
    assert_eq!(records["fixed.service"].file_state, "static");
    assert!(records["run-demo.service"].object_path.is_some());
}

#[test]
fn masked_static_and_template_services_have_explicit_disabled_actions() {
    for (name, state, operation) in [
        ("example.service", "masked", "start"),
        ("example.service", "static", "enable"),
        ("example@.service", "disabled", "restart"),
    ] {
        let row = service_row(
            &ServiceRecord {
                name: name.into(),
                file_state: state.into(),
                ..Default::default()
            },
            Scope::System,
        );
        assert!(
            row.actions
                .iter()
                .find(|action| action.id == operation)
                .unwrap()
                .disabled_reason
                .is_some()
        );
        assert!(
            row.actions
                .iter()
                .find(|action| action.id == "view_logs")
                .unwrap()
                .disabled_reason
                .is_none()
        );
    }
    let row = service_row(
        &ServiceRecord {
            name: "example.service".into(),
            file_state: "enabled".into(),
            ..Default::default()
        },
        Scope::User,
    );
    assert!(row.actions.iter().all(|action| !action.privileged));
    assert_eq!(row.identity["scope"], "user");
}

#[test]
fn operations_reject_paths_patterns_shell_text_and_unknown_scopes() {
    for value in [
        "/tmp/example.service",
        "*.service",
        "-example.service",
        "example.service;id",
        "example.service\n",
        ".service",
        "example.socket",
    ] {
        assert!(!valid_unit_name(value), "accepted {value}");
    }
    for value in [
        "example.service",
        "example@instance.service",
        "dev-disk-by\\x2duuid.service",
    ] {
        assert!(valid_unit_name(value));
    }
    assert_eq!(Scope::parse(""), Ok(Scope::System));
    assert_eq!(Scope::parse("user"), Ok(Scope::User));
    assert!(Scope::parse("someone-else").is_err());
}

struct Interaction;
impl OperationInteraction for Interaction {
    fn emit(&mut self, _: OperationEvent) {}
    fn ask(&mut self, _: &str, _: &str, _: &[String], _: bool) -> Result<String, ManagementError> {
        panic!("service backend must not ask for passwords")
    }
}

#[test]
fn cancelled_and_malformed_operations_never_reach_systemctl() {
    let context = ExecutionContext {
        actor_uid: unsafe { libc::getuid() },
        helper_path: PathBuf::new(),
    };
    let mut command = ManagementCommand {
        kind: ManagementKind::Services,
        action: "start".into(),
        target: Some("valid.service".into()),
        values: BTreeMap::new(),
        identity: BTreeMap::new(),
    };
    assert_eq!(
        execute(&command, &context, &mut Interaction, &AtomicBool::new(true)),
        Err(ManagementError::Cancelled)
    );
    assert!(matches!(
        execute(
            &command,
            &context,
            &mut Interaction,
            &AtomicBool::new(false)
        ),
        Err(ManagementError::Conflict(_))
    ));
    command.action = "edit".into();
    assert!(matches!(
        execute(
            &command,
            &context,
            &mut Interaction,
            &AtomicBool::new(false)
        ),
        Err(ManagementError::InvalidInput(_))
    ));
    command.action = "start".into();
    command.target = Some("../valid.service".into());
    assert!(matches!(
        execute(
            &command,
            &context,
            &mut Interaction,
            &AtomicBool::new(false)
        ),
        Err(ManagementError::InvalidInput(_))
    ));
}

#[test]
#[ignore = "read-only live systemd service metadata check; run explicitly on a systemd host"]
fn native_system_manager_lists_loaded_and_installed_services_and_reads_details() {
    let query = ManagementQuery {
        kind: ManagementKind::Services,
        scope: "system".into(),
        filter: "systemd-journald.service".into(),
        target: Some("systemd-journald.service".into()),
        options: BTreeMap::new(),
    };
    let result = super::query(&query, &AtomicBool::new(false)).unwrap();
    let journal = result
        .rows
        .iter()
        .find(|row| row.id == "systemd-journald.service")
        .unwrap();
    assert_eq!(journal.cells.len(), result.columns.len());
    assert_eq!(journal.cells[1], "loaded");
    assert!(journal.detail.iter().any(|(key, _)| key == "Main PID"));
    assert!(journal.detail.iter().any(|(key, _)| key == "UnitFileState"));
    assert_eq!(journal.identity["scope"], "system");
}
