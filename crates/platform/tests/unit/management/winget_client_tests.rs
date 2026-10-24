use super::*;

fn command(kind: ManagementKind) -> ManagementCommand {
    ManagementCommand {
        kind,
        action: "install".into(),
        target: None,
        values: Default::default(),
        identity: Default::default(),
    }
}

#[test]
fn rejects_other_management_kinds_and_reconnections_before_launch() {
    let (_tx, rx) = mpsc::channel();
    assert!(matches!(
        run(
            Some(command(ManagementKind::Services)),
            true,
            None,
            rx,
            &AtomicBool::new(false),
            &|_| panic!("must not start"),
        ),
        Err(ManagementError::Unavailable(_))
    ));
    let (_tx, rx) = mpsc::channel();
    assert!(matches!(
        run(
            Some(command(ManagementKind::Packages)),
            true,
            Some(PathBuf::from("unused")),
            rx,
            &AtomicBool::new(false),
            &|_| panic!("must not start"),
        ),
        Err(ManagementError::UnsupportedEnvironment(_))
    ));
}

#[test]
fn cancellation_before_start_does_not_launch_an_installer() {
    let (_tx, rx) = mpsc::channel();
    assert!(matches!(
        run(
            Some(command(ManagementKind::Packages)),
            true,
            None,
            rx,
            &AtomicBool::new(true),
            &|_| panic!("must not start"),
        ),
        Err(ManagementError::Cancelled)
    ));
}

#[test]
fn queued_stop_during_preflight_is_seen_before_installation() {
    let (tx, rx) = mpsc::channel();
    let mut interaction = Interaction {
        emit: &|_| {},
        inputs: rx,
    };
    tx.send(OperationInput::Resize {
        columns: 80,
        rows: 24,
    })
    .unwrap();
    assert!(
        interaction
            .terminal_input(std::time::Duration::ZERO)
            .unwrap()
            .is_none()
    );
    tx.send(OperationInput::Cancel).unwrap();
    assert_eq!(
        interaction.terminal_input(std::time::Duration::ZERO),
        Err(ManagementError::Cancelled)
    );
}
