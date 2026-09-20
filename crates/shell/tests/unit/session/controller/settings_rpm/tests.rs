use super::*;
fn rpm_state() -> SettingsUpdateState {
    SettingsUpdateState {
        rpm: Some(RpmSettingsState {
            installation: Some(Installation {
                backend: UpdateBackend::SystemRpm,
                directory: None,
                rpm: Some(platform::installation::RpmIdentity {
                    name: "tundraux3".into(),
                    version: "1-1".into(),
                    architecture: "x86_64".into(),
                }),
                reason: None,
            }),
            check: Some(UpdateCheck {
                installed_version: "1-1".into(),
                candidate: Some(PackageVersion {
                    name: "tundraux3".into(),
                    version: "2-1".into(),
                    architecture: "x86_64".into(),
                    repository: "test-updates".into(),
                }),
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}
fn enabled(state: &SettingsUpdateState, field: ui::SettingsField) -> bool {
    rpm_settings_cards(state)
        .iter()
        .flat_map(|card| &card.items)
        .find(|item| item.field == field)
        .unwrap()
        .enabled
}
#[test]
fn unknown_result_exposes_query_without_reexecution_or_successful_restart() {
    let mut state = rpm_state();
    assert!(enabled(&state, ui::SettingsField::StartUpdate));
    state.rpm.as_mut().unwrap().result = Some(UpdateResult::Unknown {
        expected_version: "2-1".into(),
    });
    assert!(!enabled(&state, ui::SettingsField::StartUpdate));
    assert!(!enabled(&state, ui::SettingsField::CheckUpdates));
    assert!(!enabled(&state, ui::SettingsField::RestartAfterRpmUpdate));
    assert!(enabled(&state, ui::SettingsField::QueryRpmUpdate));
}
#[test]
fn cancellation_is_available_only_during_a_backend_cancellable_phase() {
    let mut state = rpm_state();
    state.busy = true;
    assert!(!enabled(&state, ui::SettingsField::CancelRpmUpdate));
    state.rpm.as_mut().unwrap().progress.cancellable = true;
    assert!(enabled(&state, ui::SettingsField::CancelRpmUpdate));
    state.busy = false;
    assert!(!enabled(&state, ui::SettingsField::CancelRpmUpdate));
}
#[test]
fn completed_installation_requires_explicit_restart_and_cancellation_never_claims_rollback() {
    let mut session = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    session.settings_update_state = rpm_state();
    session.apply_rpm_event(RpmTaskEvent::Completed(Ok(RpmTaskOutcome::Result(Some(
        UpdateResult::Installed {
            version: "2-1".into(),
            system_restart_recommended: true,
        },
    )))));
    assert!(!session.restart_requested());
    assert!(!session.shutdown_requested());
    assert!(enabled(
        &session.settings_update_state,
        ui::SettingsField::RestartAfterRpmUpdate
    ));
    assert!(!enabled(
        &session.settings_update_state,
        ui::SettingsField::StartUpdate
    ));
    session.apply_rpm_event(RpmTaskEvent::Completed(Ok(RpmTaskOutcome::Result(Some(
        UpdateResult::Cancelled,
    )))));
    assert!(!enabled(
        &session.settings_update_state,
        ui::SettingsField::RestartAfterRpmUpdate
    ));
    assert!(
        session
            .settings_update_state
            .rpm
            .as_ref()
            .unwrap()
            .check
            .is_none()
    );
}
#[test]
fn cancelling_the_scrollable_preview_invalidates_its_confirmation() {
    let mut session = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    session.settings_update_state = rpm_state();
    let changes = (0..80)
        .map(|index| PackageChange {
            package: PackageVersion {
                name: format!("dependency-{index}"),
                version: "1-1".into(),
                architecture: "noarch".into(),
                repository: "test-updates".into(),
            },
            old_version: None,
        })
        .collect();
    session.apply_rpm_event(RpmTaskEvent::Completed(Ok(RpmTaskOutcome::Preview(
        UpdatePreview { changes },
    ))));
    assert!(session.to_notification_view_model().is_some());
    assert!(
        session
            .settings_update_state
            .rpm
            .as_ref()
            .unwrap()
            .confirmation_pending
    );
    session.cancel_rpm_confirmation();
    session.confirm_rpm_update();
    assert!(!session.settings_update_state.busy);
    assert!(
        session
            .settings_update_state
            .rpm
            .as_ref()
            .unwrap()
            .preview
            .is_none()
    );
}
