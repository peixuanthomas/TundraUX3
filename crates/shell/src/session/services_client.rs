use super::*;
use crate::session_services::{
    SessionProfile, SessionServicesClient, SystemStatusRequests, client::SyncState,
};
use workspace_protocol::{ServiceOperation, WorkerRole};

impl SessionServicesClient {
    pub(crate) fn sync_session(&self, state: &mut ShellSession) {
        let Ok(mut sync) = self.sync.try_lock() else {
            return;
        };
        let result = self.try_sync_session(state, &mut sync);
        match result {
            Ok(()) => sync.failed = false,
            Err(_) if !sync.failed => {
                sync.failed = true;
                state.notify_status(i18n::msg!("workspace-services-unavailable"));
            }
            Err(_) => {}
        }
    }

    fn try_sync_session(&self, state: &mut ShellSession, sync: &mut SyncState) -> io::Result<()> {
        let shell = self.worker().role == WorkerRole::Shell;
        let mut auth: (u64, Option<AuthSession>) =
            self.read(ServiceOperation::ReadAuthorization)?;
        if shell {
            if state.app.auth_session() != auth.1.as_ref() {
                auth = self.read(ServiceOperation::WriteAuthorization {
                    revision: auth.0,
                    policy: Self::encode(&state.app.auth_session())?,
                })?;
            }
        } else if state.app.auth_session() != auth.1.as_ref() {
            let current_session = state.app.auth_session().map(|session| &session.session_id);
            if auth
                .1
                .as_ref()
                .is_some_and(|next| Some(&next.session_id) != current_session)
            {
                auth.1 = None;
            }
            state.app.dispatch_at(
                app::AppCommand::SetAuthSession(auth.1.clone()),
                Instant::now(),
            );
        }
        if sync
            .auth
            .as_ref()
            .and_then(|(_, auth)| auth.as_ref())
            .map(|auth| &auth.session_id)
            != auth.1.as_ref().map(|auth| &auth.session_id)
        {
            sync.profile = None;
        }
        sync.auth = Some(auth);

        let (_, config) = self.read_config()?;
        if state.app.storage_config() != &config {
            if state.language_code() != config.language
                && let Ok((catalog, loaded)) = state.prepare_language(&config.language)
            {
                state.publish_language(catalog, loaded);
            }
            state.replace_storage_config(config);
        }

        let local_profile = SessionProfile {
            appearance: state.app.active_appearance().cloned(),
            dashboard: state.app.active_system_status_dashboard().cloned(),
        };
        let mut profile: (u64, SessionProfile) = self.read(ServiceOperation::ReadProfile)?;
        if shell
            && profile.1 == SessionProfile::default()
            && local_profile.appearance.is_some()
            && local_profile.dashboard.is_some()
        {
            profile = self.read(ServiceOperation::WriteProfile {
                revision: profile.0,
                profile: Self::encode(&local_profile)?,
            })?;
        }
        if local_profile != profile.1 && profile.1 != SessionProfile::default() {
            state.apply_workspace_profile(profile.1.clone());
        }
        sync.profile = Some(profile);

        if shell {
            if let Some(snapshot) = state.app.system_status_snapshot()
                && sync.published_system_revision != Some(snapshot.revision)
            {
                self.request(ServiceOperation::WriteSystemSnapshot {
                    snapshot: Self::encode(snapshot)?,
                })?;
                sync.published_system_revision = Some(snapshot.revision);
            }
            let (_, requests): (_, SystemStatusRequests) =
                self.read(ServiceOperation::ReadSystemStatusRequests)?;
            if requests.active != sync.system_active {
                let _ = state
                    .settings_task_runtime
                    .set_system_status_active(requests.active);
                sync.system_active = requests.active;
            }
            if requests.refresh {
                let _ = state.settings_task_runtime.refresh_system_status();
            }
        } else {
            let (revision, snapshot): (_, Option<app::AppSystemStatusSnapshot>) =
                self.read(ServiceOperation::ReadSystemSnapshot)?;
            if revision != sync.system_revision {
                if let Some(snapshot) = snapshot {
                    state.apply_system_status_snapshot(snapshot);
                }
                sync.system_revision = revision;
            }
        }
        if state.app.explorer_state().is_some() {
            state
                .app
                .refresh_shared_explorer()
                .map_err(io::Error::other)?;
        }
        super::clock_workspace::sync_clock_workspace(self, state)?;
        state.refresh_hit_map();
        Ok(())
    }

    pub(super) fn save_workspace_profile(
        &self,
        state: &mut ShellSession,
        revision: Option<u64>,
        patch: SessionProfile,
    ) -> io::Result<()> {
        let mut sync = self
            .sync
            .lock()
            .map_err(|_| io::Error::other(i18n::tr!("workspace-services-unavailable")))?;
        let revision = revision
            .ok_or_else(|| io::Error::other(i18n::tr!("workspace-services-unavailable")))?;
        let profile: (u64, SessionProfile) = self.read(ServiceOperation::WriteProfile {
            revision,
            profile: Self::encode(&patch)?,
        })?;
        state.apply_workspace_profile(profile.1.clone());
        sync.profile = Some(profile);
        Ok(())
    }
}

impl ShellSession {
    pub(super) fn workspace_profile_revision(&self) -> Option<u64> {
        let services = SessionServicesClient::current()?;
        let sync = services.sync.lock().ok()?;
        sync.profile.as_ref().map(|(revision, _)| *revision)
    }

    fn apply_workspace_profile(&mut self, profile: SessionProfile) {
        let now = Instant::now();
        self.app.dispatch_at(
            app::AppCommand::SetActiveAppearance(profile.appearance),
            now,
        );
        self.app.dispatch_at(
            app::AppCommand::SetActiveSystemStatusDashboard(profile.dashboard),
            now,
        );
    }
}
