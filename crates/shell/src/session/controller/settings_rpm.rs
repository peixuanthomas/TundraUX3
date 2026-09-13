use super::super::*;
use platform::{
    installation::{Installation, UpdateBackend},
    updates::*,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(in crate::session) struct RpmSettingsState {
    pub installation: Option<Installation>,
    pub check: Option<UpdateCheck>,
    pub preview: Option<UpdatePreview>,
    pub progress: UpdateProgress,
    pub result: Option<UpdateResult>,
    pub confirmation_pending: bool,
    pub active_task: Option<RpmTask>,
}

pub(in crate::session) const RPM_SETTINGS_FIELDS: &[ui::SettingsField] = &[
    ui::SettingsField::InstalledVersion,
    ui::SettingsField::RemoteVersion,
    ui::SettingsField::CheckUpdates,
    ui::SettingsField::StartUpdate,
    ui::SettingsField::CancelRpmUpdate,
    ui::SettingsField::QueryRpmUpdate,
    ui::SettingsField::RestartAfterRpmUpdate,
];

impl ShellSession {
    pub(in crate::session) fn uses_native_linux_updates(&self) -> bool {
        self.app
            .auth_session()
            .is_some_and(|session| session.source == identity::IdentitySource::LinuxCurrentProcess)
    }
    pub(in crate::session) fn uses_rpm_settings_cards(&self) -> bool {
        self.settings_update_state.rpm.as_ref().is_some_and(|rpm| {
            rpm.installation
                .as_ref()
                .is_none_or(|installation| installation.backend != UpdateBackend::PortableUser)
        })
    }
    pub(in crate::session) fn start_rpm_task(&mut self, task: RpmTask) {
        if !self.uses_native_linux_updates() || self.settings_update_state.busy {
            return;
        }
        if !matches!(task, RpmTask::Query)
            && self
                .settings_update_state
                .rpm
                .as_ref()
                .is_some_and(|rpm| matches!(rpm.result, Some(UpdateResult::Unknown { .. })))
        {
            return;
        }
        self.settings_update_state
            .rpm
            .get_or_insert_with(Default::default)
            .active_task = Some(task);
        #[cfg(target_os = "linux")]
        let result = self.settings_task_runtime.submit_rpm_task(task);
        #[cfg(not(target_os = "linux"))]
        let result: Result<(), i18n::LocalizedText> = {
            let _ = task;
            Err("Unsupported".into())
        };
        match result {
            Ok(()) => {
                self.settings_update_state.busy = true;
                self.settings_update_state.error = None;
                self.settings_update_state.status = i18n::msg!("settings-rpm-working").into();
            }
            Err(error) => self.set_update_error(error),
        }
    }
    pub(in crate::session) fn confirm_rpm_update(&mut self) {
        let Some(rpm) = self.settings_update_state.rpm.as_mut() else {
            return;
        };
        if !rpm.confirmation_pending || rpm.preview.is_none() {
            return;
        }
        rpm.confirmation_pending = false;
        self.start_rpm_task(RpmTask::Execute);
    }
    pub(in crate::session) fn cancel_rpm_confirmation(&mut self) {
        if let Some(rpm) = self.settings_update_state.rpm.as_mut() {
            rpm.confirmation_pending = false;
            rpm.preview = None;
        }
    }
    pub(in crate::session) fn request_rpm_cancellation(&mut self) {
        #[cfg(target_os = "linux")]
        if self.uses_native_linux_updates() && self.settings_task_runtime.cancel_rpm_task() {
            self.settings_update_state.status = i18n::msg!("settings-rpm-cancelling").into();
        }
    }
    pub(in crate::session) fn apply_rpm_event(&mut self, event: RpmTaskEvent) {
        match event {
            RpmTaskEvent::Installation(installation) => {
                self.settings_update_state
                    .rpm
                    .get_or_insert_with(Default::default)
                    .installation = Some(installation);
            }
            RpmTaskEvent::Progress(progress) => {
                self.settings_update_state.status = i18n::msg!(
                    "settings-rpm-progress",
                    stage = rpm_stage_text(progress.stage),
                    package = progress.package.clone().unwrap_or_default(),
                    percent = progress
                        .percentage
                        .map(|value| format!("{value}%"))
                        .unwrap_or_default()
                )
                .into();
                self.settings_update_state
                    .rpm
                    .get_or_insert_with(Default::default)
                    .progress = progress;
            }
            RpmTaskEvent::Completed(result) => {
                self.settings_update_state.busy = false;
                match result {
                    Err(error) => {
                        let rpm = self
                            .settings_update_state
                            .rpm
                            .get_or_insert_with(Default::default);
                        if rpm.active_task == Some(RpmTask::Execute)
                            && error == platform::service::ServiceError::Unknown
                        {
                            rpm.result = Some(UpdateResult::Unknown {
                                expected_version: rpm
                                    .check
                                    .as_ref()
                                    .and_then(|check| check.candidate.as_ref())
                                    .map(|candidate| candidate.version.clone())
                                    .unwrap_or_default(),
                            });
                            self.settings_update_state.status =
                                i18n::msg!("settings-rpm-unknown").into();
                        } else {
                            self.set_update_error(error.to_string());
                        }
                    }
                    Ok(RpmTaskOutcome::Check(check)) => {
                        self.settings_update_state.status = if check.candidate.is_some() {
                            i18n::msg!("settings-rpm-candidate").into()
                        } else {
                            i18n::msg!("settings-rpm-no-candidate").into()
                        };
                        let rpm = self
                            .settings_update_state
                            .rpm
                            .get_or_insert_with(Default::default);
                        rpm.check = Some(check);
                        rpm.preview = None;
                    }
                    Ok(RpmTaskOutcome::Preview(preview)) => {
                        let body = preview
                            .changes
                            .iter()
                            .map(|change| {
                                let transition = change
                                    .old_version
                                    .as_ref()
                                    .map(|old| format!("{old} → {}", change.package.version))
                                    .unwrap_or_else(|| format!("+ {}", change.package.version));
                                format!(
                                    "{} ({})\n{}\n{}",
                                    change.package.name,
                                    change.package.architecture,
                                    transition,
                                    change.package.repository
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n\n");
                        let rpm = self
                            .settings_update_state
                            .rpm
                            .get_or_insert_with(Default::default);
                        rpm.preview = Some(preview);
                        rpm.confirmation_pending = true;
                        self.notify_modal(
                            i18n::msg!("settings-rpm-preview-title"),
                            body,
                            ui::NotificationTone::Warning,
                            vec![
                                ShellNotificationAction::new(
                                    "install",
                                    i18n::msg!("settings-rpm-install"),
                                )
                                .with_follow_up(ShellCommand::SettingsRpmUpdateConfirmed),
                                ShellNotificationAction::new(
                                    "cancel",
                                    i18n::msg!("settings-cancel"),
                                )
                                .cancel()
                                .with_follow_up(ShellCommand::SettingsRpmUpdateCancelled),
                            ],
                        );
                    }
                    Ok(RpmTaskOutcome::Result(result)) => {
                        self.settings_update_state.status = match &result {
                            Some(UpdateResult::Installed {
                                version,
                                system_restart_recommended,
                            }) => i18n::msg!(
                                "settings-rpm-installed",
                                version = version.clone(),
                                restart = if *system_restart_recommended {
                                    i18n::tr!("settings-rpm-system-restart")
                                } else {
                                    String::new()
                                }
                            )
                            .into(),
                            Some(UpdateResult::Cancelled) => {
                                i18n::msg!("settings-rpm-cancelled").into()
                            }
                            Some(UpdateResult::Unknown { .. }) => {
                                i18n::msg!("settings-rpm-unknown").into()
                            }
                            Some(UpdateResult::Failed(error)) => error.to_string().into(),
                            None => i18n::msg!("settings-rpm-no-pending").into(),
                        };
                        let rpm = self
                            .settings_update_state
                            .rpm
                            .get_or_insert_with(Default::default);
                        if matches!(
                            result,
                            Some(UpdateResult::Installed { .. } | UpdateResult::Cancelled)
                        ) {
                            rpm.check = None;
                        }
                        rpm.result = result;
                        rpm.preview = None;
                        rpm.confirmation_pending = false;
                        rpm.progress.cancellable = false;
                    }
                }
            }
        }
    }
}

pub(in crate::session) fn rpm_settings_cards(
    update: &SettingsUpdateState,
) -> Vec<ui::SettingsCardViewModel> {
    use ui::{
        SettingsCardViewModel as Card, SettingsControlKind as Kind, SettingsField as Field,
        SettingsItemViewModel as Item,
    };
    let Some(rpm) = update.rpm.as_ref() else {
        return Vec::new();
    };
    let installation = rpm.installation.as_ref();
    let supported = installation.is_some_and(|value| value.backend == UpdateBackend::SystemRpm);
    let unknown = matches!(rpm.result, Some(UpdateResult::Unknown { .. }));
    let installed = match &rpm.result {
        Some(UpdateResult::Installed { version, .. }) => version.clone(),
        Some(UpdateResult::Cancelled | UpdateResult::Unknown { .. }) => {
            i18n::tr!("settings-rpm-recheck-version")
        }
        _ => rpm
            .check
            .as_ref()
            .map(|value| value.installed_version.clone())
            .or_else(|| {
                installation
                    .and_then(|value| value.rpm.as_ref())
                    .map(|value| value.version.clone())
            })
            .unwrap_or_default(),
    };
    let candidate = rpm
        .check
        .as_ref()
        .and_then(|value| value.candidate.as_ref());
    vec![
        Card::new(
            i18n::tr!("settings-rpm-backend"),
            vec![
                Item::new(
                    Field::InstalledVersion,
                    i18n::tr!("settings-installed-build"),
                    installed,
                    installation
                        .and_then(|value| value.reason.clone())
                        .unwrap_or_else(|| {
                            if supported {
                                "SystemRpm · Fedora · PackageKit / RPM".into()
                            } else {
                                i18n::tr!("settings-rpm-working")
                            }
                        }),
                    Kind::ReadOnly,
                ),
                Item::new(
                    Field::RemoteVersion,
                    i18n::tr!("settings-rpm-candidate-label"),
                    candidate
                        .map(|value| value.version.clone())
                        .unwrap_or_default(),
                    candidate
                        .map(|value| value.repository.clone())
                        .unwrap_or_default(),
                    Kind::ReadOnly,
                ),
            ],
        ),
        Card::new(
            i18n::tr!("settings-actions"),
            vec![
                Item::new(
                    Field::CheckUpdates,
                    i18n::tr!("settings-check-again"),
                    "",
                    "",
                    Kind::Action,
                )
                .enabled(!update.busy && !unknown),
                Item::new(
                    Field::StartUpdate,
                    i18n::tr!("settings-rpm-preview-title"),
                    "",
                    "",
                    Kind::Action,
                )
                .enabled(supported && !update.busy && candidate.is_some() && !unknown),
                Item::new(
                    Field::CancelRpmUpdate,
                    i18n::tr!("settings-rpm-cancel-action"),
                    "",
                    i18n::tr!("settings-rpm-cancel-help"),
                    Kind::Action,
                )
                .enabled(update.busy && rpm.progress.cancellable),
                Item::new(
                    Field::QueryRpmUpdate,
                    i18n::tr!("settings-rpm-query"),
                    "",
                    "",
                    Kind::Action,
                )
                .enabled(supported && !update.busy),
                Item::new(
                    Field::RestartAfterRpmUpdate,
                    i18n::tr!("settings-rpm-restart"),
                    "",
                    "",
                    Kind::Action,
                )
                .enabled(
                    !update.busy && matches!(rpm.result, Some(UpdateResult::Installed { .. })),
                ),
            ],
        ),
    ]
}

fn rpm_stage_text(stage: UpdateStage) -> i18n::LocalizedMessage {
    match stage {
        UpdateStage::Preparing => i18n::msg!("settings-rpm-stage-preparing"),
        UpdateStage::StartingTransaction => i18n::msg!("settings-rpm-stage-starting"),
        UpdateStage::Waiting => i18n::msg!("settings-rpm-stage-waiting"),
        UpdateStage::Authorizing => i18n::msg!("settings-rpm-stage-authorizing"),
        UpdateStage::Downloading => i18n::msg!("settings-rpm-stage-downloading"),
        UpdateStage::Installing => i18n::msg!("settings-rpm-stage-installing"),
        UpdateStage::Verifying => i18n::msg!("settings-rpm-stage-verifying"),
        UpdateStage::Finished => i18n::msg!("settings-rpm-stage-finished"),
    }
}

#[cfg(test)]
mod tests {
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
}
