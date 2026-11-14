use std::path::PathBuf;

use app::explorer::{
    ExplorerClipboard, ExplorerClipboardMode, ExplorerSharedRequest, ExplorerSharedSnapshot,
};
use identity::{AuthSession, IdentitySource, UserRole};
use platform::AppPaths;
use platform::mock::UnsupportedPlatform;
use serde::de::DeserializeOwned;
use serde_json::json;
use storage::{StorageConfig, StorageManager};
use workspace_protocol::{ProcessGeneration, RunId, SessionId};

use super::*;

#[test]
fn only_current_authorized_configuration_revision_can_be_saved() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let shell = worker(WorkerRole::Shell, 1);
    let guest = worker(WorkerRole::App(SessionId(7)), 2);
    let app = worker(WorkerRole::App(SessionId(8)), 3);
    state.register(shell);
    let (revision, current): (u64, StorageConfig) =
        read(&mut state, shell, ServiceOperation::ReadConfig);
    assert_eq!(revision, 0);

    assert_eq!(
        state.execute(guest, ServiceOperation::ReadConfig, &UnsupportedPlatform),
        Err(ServiceError::Forbidden)
    );
    state.register(guest);
    let (_, guest_config): (u64, StorageConfig) =
        read(&mut state, guest, ServiceOperation::ReadConfig);
    assert_eq!(guest_config, current);
    assert_eq!(
        state.execute(
            guest,
            ServiceOperation::WriteConfig {
                revision,
                config: json!(current),
            },
            &UnsupportedPlatform,
        ),
        Err(ServiceError::Forbidden)
    );
    assert_eq!(
        state.execute(
            guest,
            ServiceOperation::ReadFileOperations,
            &UnsupportedPlatform
        ),
        Err(ServiceError::Forbidden)
    );
    login(&mut state, shell, 0, "first");
    state.register(app);

    let (revision, mut older): (u64, StorageConfig) =
        read(&mut state, app, ServiceOperation::ReadConfig);
    let (same_revision, mut newer): (u64, StorageConfig) =
        read(&mut state, app, ServiceOperation::ReadConfig);
    assert_eq!(revision, same_revision);
    older.language = "en-US".into();
    newer.language = "zh-CN".into();
    let (next_revision, saved): (u64, StorageConfig) = read(
        &mut state,
        app,
        ServiceOperation::WriteConfig {
            revision: same_revision,
            config: json!(newer),
        },
    );
    assert_eq!(next_revision, revision + 1);
    assert_eq!(saved.language, "zh-CN");
    assert_eq!(
        state.execute(
            app,
            ServiceOperation::WriteConfig {
                revision,
                config: json!(older),
            },
            &UnsupportedPlatform,
        ),
        Err(ServiceError::Conflict)
    );
    let (visible_revision, visible): (u64, StorageConfig) =
        read(&mut state, shell, ServiceOperation::ReadConfig);
    assert_eq!(visible_revision, next_revision);
    assert_eq!(visible, saved);
    assert_eq!(fixture.storage.load_config().unwrap(), saved);
}

#[test]
fn profile_edits_reject_an_old_draft_before_touching_the_account_file() {
    let fixture = Fixture::new();
    let users = UserService::new(fixture.storage.clone());
    let admin = users
        .bootstrap_admin("profile-admin", "example-pass-2026")
        .unwrap();
    let mut sessions = identity::SessionService::new(fixture.storage.clone());
    let administrator = sessions
        .login(&admin.username, "example-pass-2026")
        .unwrap();
    let account = users
        .create_user(
            &administrator,
            "profile-user",
            "Profile User",
            UserRole::User,
            "example-pass-2026",
        )
        .unwrap();
    let auth = sessions
        .login(&account.username, "example-pass-2026")
        .unwrap();
    let mut state = fixture.state();
    let shell = worker(WorkerRole::Shell, 1);
    let settings = worker(WorkerRole::App(SessionId(1)), 2);
    let status = worker(WorkerRole::App(SessionId(2)), 3);
    state.register(shell);
    let _ = read::<Option<AuthSession>>(
        &mut state,
        shell,
        ServiceOperation::WriteAuthorization {
            revision: 0,
            policy: json!(auth),
        },
    );
    let initial = SessionProfile {
        appearance: Some(account.appearance.clone()),
        dashboard: Some(account.system_status_dashboard.clone()),
    };
    let (revision, _): (u64, SessionProfile) =
        read(&mut state, shell, ServiceOperation::ReadProfile);
    let (seeded, profile): (u64, SessionProfile) = read(
        &mut state,
        shell,
        ServiceOperation::WriteProfile {
            revision,
            profile: json!(initial),
        },
    );
    assert_eq!(profile, initial);
    state.register(settings);
    state.register(status);
    let (settings_revision, _): (u64, SessionProfile) =
        read(&mut state, settings, ServiceOperation::ReadProfile);
    let (dashboard_revision, _): (u64, SessionProfile) =
        read(&mut state, status, ServiceOperation::ReadProfile);
    assert_eq!((settings_revision, dashboard_revision), (seeded, seeded));

    let mut appearance = account.appearance;
    appearance.border_shape = match appearance.border_shape {
        storage::BorderShape::Rounded => storage::BorderShape::Square,
        storage::BorderShape::Square => storage::BorderShape::Rounded,
    };
    let (updated, saved): (u64, SessionProfile) = read(
        &mut state,
        settings,
        ServiceOperation::WriteProfile {
            revision: settings_revision,
            profile: json!(SessionProfile {
                appearance: Some(appearance.clone()),
                dashboard: None
            }),
        },
    );
    assert_eq!(updated, seeded + 1);
    assert_eq!(saved.appearance, Some(appearance.clone()));
    assert_eq!(saved.dashboard, initial.dashboard);
    let disk = std::fs::read(&fixture.storage.layout().users_path).unwrap();

    let mut dashboard = account.system_status_dashboard;
    assert!(dashboard.remove_widget(storage::SystemStatusWidgetKind::Network));
    let patch = json!(SessionProfile {
        appearance: None,
        dashboard: Some(dashboard.clone())
    });
    assert_eq!(
        state.execute(
            status,
            ServiceOperation::WriteProfile {
                revision: dashboard_revision,
                profile: patch.clone()
            },
            &UnsupportedPlatform
        ),
        Err(ServiceError::Conflict)
    );
    assert_eq!(
        std::fs::read(&fixture.storage.layout().users_path).unwrap(),
        disk
    );
    let (current, profile): (u64, SessionProfile) =
        read(&mut state, status, ServiceOperation::ReadProfile);
    assert_eq!((current, profile), (updated, saved));

    let (latest, saved): (u64, SessionProfile) = read(
        &mut state,
        status,
        ServiceOperation::WriteProfile {
            revision: current,
            profile: patch,
        },
    );
    assert_eq!(latest, current + 1);
    assert_eq!(saved.appearance, Some(appearance.clone()));
    assert_eq!(saved.dashboard, Some(dashboard.clone()));
    let disk = fixture
        .storage
        .load_users()
        .unwrap()
        .users
        .into_iter()
        .find(|user| user.id == auth.user_id)
        .unwrap();
    assert_eq!(disk.appearance, appearance);
    assert_eq!(disk.system_status_dashboard, dashboard);
}

#[test]
fn windows_share_explorer_state_and_unregister_releases_activity() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let shell = worker(WorkerRole::Shell, 1);
    let first = worker(WorkerRole::App(SessionId(7)), 2);
    let second = worker(WorkerRole::App(SessionId(8)), 3);
    state.register(shell);
    login(&mut state, shell, 0, "first");
    state.register(first);
    state.register(second);
    let clipboard = ExplorerClipboard {
        paths: vec![fixture.root.join("测试文件")],
        mode: ExplorerClipboardMode::Cut,
    };
    update_files(
        &mut state,
        first,
        ExplorerSharedRequest::SetClipboard(Some(clipboard.clone())),
    );
    let (_, shared): (u64, ExplorerSharedSnapshot) =
        read(&mut state, second, ServiceOperation::ReadFileOperations);
    assert_eq!(shared.clipboard, Some(clipboard));
    assert!(update_files(&mut state, first, ExplorerSharedRequest::SetBusy(true)).busy);
    assert!(update_files(&mut state, second, ExplorerSharedRequest::SetBusy(true)).busy);

    assert_eq!(
        state.execute(
            first,
            ServiceOperation::RequestSystemStatus {
                active: Some(true),
                refresh: true,
            },
            &UnsupportedPlatform,
        ),
        Ok(ServiceValue::Empty)
    );
    let (_, requests): (u64, SystemStatusRequests) = read(
        &mut state,
        shell,
        ServiceOperation::ReadSystemStatusRequests,
    );
    assert!(requests.active && requests.refresh);
    state.unregister(first);
    let (_, requests): (u64, SystemStatusRequests) = read(
        &mut state,
        shell,
        ServiceOperation::ReadSystemStatusRequests,
    );
    assert!(!requests.active && !requests.refresh);
    let (_, shared): (u64, ExplorerSharedSnapshot) =
        read(&mut state, second, ServiceOperation::ReadFileOperations);
    assert!(shared.busy);
    state.unregister(second);
    let (_, shared): (u64, ExplorerSharedSnapshot) =
        read(&mut state, shell, ServiceOperation::ReadFileOperations);
    assert!(!shared.busy);
}

#[test]
fn relogin_does_not_reauthorize_a_previous_window() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let shell = worker(WorkerRole::Shell, 1);
    let stale = worker(WorkerRole::App(SessionId(7)), 2);
    let next = worker(WorkerRole::App(SessionId(8)), 3);
    state.register(shell);
    login(&mut state, shell, 0, "first");
    state.register(stale);
    update_files(
        &mut state,
        stale,
        ExplorerSharedRequest::SetClipboard(Some(ExplorerClipboard {
            paths: vec![fixture.root.join("saved")],
            mode: ExplorerClipboardMode::Copy,
        })),
    );
    let (_, logout): (u64, Option<AuthSession>) = read(
        &mut state,
        shell,
        ServiceOperation::WriteAuthorization {
            revision: 1,
            policy: Value::Null,
        },
    );
    assert!(logout.is_none());
    login(&mut state, shell, 2, "second");
    state.register(stale);
    state.register(next);

    let (_, old_policy): (u64, Option<AuthSession>) =
        read(&mut state, stale, ServiceOperation::ReadAuthorization);
    assert!(old_policy.is_none());
    assert_eq!(
        state.execute(
            stale,
            ServiceOperation::ReadFileOperations,
            &UnsupportedPlatform
        ),
        Err(ServiceError::Forbidden)
    );
    let (_, shared): (u64, ExplorerSharedSnapshot) =
        read(&mut state, next, ServiceOperation::ReadFileOperations);
    assert!(shared.clipboard.is_none());
}

#[test]
fn clock_profile_rejects_stale_revisions_and_deleted_accounts() {
    let fixture = Fixture::new();
    let users = identity::UserService::new(fixture.storage.clone());
    let account = users
        .bootstrap_admin("clock-admin", "example-pass-2026")
        .unwrap();
    let auth = AuthSession {
        source: IdentitySource::LocalAccount,
        session_id: "clock-session".into(),
        user_id: account.id,
        username: account.username,
        role: UserRole::Admin,
        started_at_epoch_ms: 1,
    };
    let mut state = fixture.state();
    let shell = worker(WorkerRole::Shell, 1);
    let first = worker(WorkerRole::App(SessionId(1)), 2);
    let second = worker(WorkerRole::App(SessionId(2)), 3);
    state.register(shell);
    let _ = read::<Option<AuthSession>>(
        &mut state,
        shell,
        ServiceOperation::WriteAuthorization {
            revision: 0,
            policy: json!(auth),
        },
    );
    state.register(first);
    state.register(second);
    let (revision, mut profile) =
        read::<storage::ClockProfile>(&mut state, first, ServiceOperation::ReadClockProfile);
    let (other_revision, _) =
        read::<storage::ClockProfile>(&mut state, second, ServiceOperation::ReadClockProfile);
    assert_eq!(revision, other_revision);
    profile.next_id = 2;
    profile.entries.push(storage::ClockEntryRecord::Countdown {
        id: 1,
        deadline_epoch_ms: 100_000,
        strong: false,
    });
    let (updated, ()) = read(
        &mut state,
        first,
        ServiceOperation::WriteClockProfile {
            revision,
            profile: json!(profile),
        },
    );
    assert_eq!(
        state.execute(
            second,
            ServiceOperation::WriteClockProfile {
                revision,
                profile: json!(profile)
            },
            &UnsupportedPlatform,
        ),
        Err(ServiceError::Conflict),
    );
    assert_eq!(
        read::<storage::ClockProfile>(&mut state, shell, ServiceOperation::ReadClockProfile),
        (updated, profile.clone()),
    );
    let mut users = fixture.storage.load_users().unwrap();
    users.users.clear();
    fixture.storage.save_users(&users).unwrap();
    assert_eq!(
        state.execute(
            first,
            ServiceOperation::DeleteClockProfile {
                user_id: auth.user_id.clone()
            },
            &UnsupportedPlatform,
        ),
        Ok(ServiceValue::Empty),
    );
    assert_eq!(
        state.execute(
            second,
            ServiceOperation::WriteClockProfile {
                revision: updated + 1,
                profile: json!(profile)
            },
            &UnsupportedPlatform,
        ),
        Err(ServiceError::Forbidden),
    );
    assert!(fixture.storage.load_clock().unwrap().profiles.is_empty());
}

#[test]
fn only_shell_can_publish_the_shared_clock_time() {
    let fixture = Fixture::new();
    let mut state = fixture.state();
    let shell = worker(WorkerRole::Shell, 1);
    let guest = worker(WorkerRole::App(SessionId(1)), 2);
    state.register(shell);
    state.register(guest);
    let now = chrono::Utc::now();
    let anchor = crate::session_services::clock::ClockAnchor {
        utc: Some(now + chrono::TimeDelta::hours(1)),
        observed_at: now,
    };
    let update = ServiceOperation::WriteClockTime {
        anchor: json!(anchor),
    };
    assert_eq!(
        state.execute(guest, update.clone(), &UnsupportedPlatform),
        Err(ServiceError::Forbidden),
    );
    let (revision, _) =
        read::<crate::session_services::clock::ClockAnchor>(&mut state, shell, update);
    let (received, observed) = read::<crate::session_services::clock::ClockAnchor>(
        &mut state,
        guest,
        ServiceOperation::ReadClockTime,
    );
    assert_eq!(received, revision);
    assert_eq!(observed, anchor);
}

#[test]
fn shared_clock_keeps_new_alarm_dates_until_shell_advances_them() {
    use crate::session_services::clock::SharedClockProfile;
    let fixture = Fixture::new();
    let users = identity::UserService::new(fixture.storage.clone());
    let account = users
        .bootstrap_admin("alarm-admin", "example-pass-2026")
        .unwrap();
    let auth = AuthSession {
        source: IdentitySource::LocalAccount,
        session_id: "clock-session".into(),
        user_id: account.id,
        username: account.username,
        role: UserRole::Admin,
        started_at_epoch_ms: 1,
    };
    let mut state = fixture.state();
    let shell = worker(WorkerRole::Shell, 1);
    let clock = worker(WorkerRole::App(SessionId(1)), 2);
    state.register(shell);
    let _ = read::<Option<AuthSession>>(
        &mut state,
        shell,
        ServiceOperation::WriteAuthorization {
            revision: 0,
            policy: json!(auth),
        },
    );
    state.register(clock);
    let (revision, mut shared): (_, SharedClockProfile) =
        read(&mut state, clock, ServiceOperation::ReadClockProfile);
    let today = chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap();
    let tomorrow = today.succ_opt().unwrap();
    shared.profile.next_id = 2;
    shared
        .profile
        .entries
        .push(storage::ClockEntryRecord::DailyAlarm {
            id: 1,
            hour: 10,
            minute: 0,
            second: 0,
            strong: false,
            snooze_deadline_epoch_ms: None,
        });
    shared.alarm_dates.insert(1, today);
    let (revision, ()) = read(
        &mut state,
        clock,
        ServiceOperation::WriteClockProfile {
            revision,
            profile: json!(shared),
        },
    );
    assert_eq!(
        read::<SharedClockProfile>(&mut state, shell, ServiceOperation::ReadClockProfile).1,
        shared
    );

    shared.alarm_dates.insert(1, tomorrow);
    let (unchanged, ()) = read(
        &mut state,
        clock,
        ServiceOperation::WriteClockProfile {
            revision,
            profile: json!(shared),
        },
    );
    assert_eq!(unchanged, revision);
    let (advanced, ()) = read(
        &mut state,
        shell,
        ServiceOperation::WriteClockProfile {
            revision,
            profile: json!(shared),
        },
    );
    assert_eq!(advanced, revision + 1);
    assert_eq!(
        read::<SharedClockProfile>(&mut state, shell, ServiceOperation::ReadClockProfile).1,
        shared
    );
    assert_eq!(
        fixture.storage.load_clock().unwrap().profiles[&auth.user_id],
        shared.profile
    );
}

fn worker(role: WorkerRole, generation: u64) -> WorkerId {
    WorkerId {
        run: RunId(1),
        role,
        generation: ProcessGeneration(generation),
    }
}

fn login(state: &mut ServiceState, shell: WorkerId, revision: u64, session_id: &str) {
    let auth = AuthSession {
        source: IdentitySource::LocalAccount,
        session_id: session_id.into(),
        user_id: "test-user".into(),
        username: "test-user".into(),
        role: UserRole::Admin,
        started_at_epoch_ms: 1,
    };
    let (next, assigned): (u64, Option<AuthSession>) = read(
        state,
        shell,
        ServiceOperation::WriteAuthorization {
            revision,
            policy: json!(auth),
        },
    );
    assert_eq!(next, revision + 1);
    assert_eq!(assigned, Some(auth));
}

fn read<T: DeserializeOwned>(
    state: &mut ServiceState,
    worker: WorkerId,
    operation: ServiceOperation,
) -> (u64, T) {
    let ServiceValue::Versioned { revision, value } = state
        .execute(worker, operation, &UnsupportedPlatform)
        .unwrap()
    else {
        panic!("expected a versioned state")
    };
    (revision, serde_json::from_value(value).unwrap())
}

fn update_files(
    state: &mut ServiceState,
    worker: WorkerId,
    operation: ExplorerSharedRequest,
) -> ExplorerSharedSnapshot {
    read(
        state,
        worker,
        ServiceOperation::UpdateFileOperations {
            operation: json!(operation),
        },
    )
    .1
}

struct Fixture {
    root: PathBuf,
    storage: StorageManager,
}

impl Fixture {
    fn new() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "tundra-services-{}-{}",
            std::process::id(),
            now.as_nanos()
        ));
        let paths = AppPaths::from_parts(
            root.join("config.toml"),
            root.join("data"),
            root.join("cache"),
            root.join("logs"),
            root.join("tmp"),
        )
        .unwrap();
        let storage = StorageManager::open(paths).unwrap().manager;
        Self { root, storage }
    }

    fn state(&self) -> ServiceState {
        ServiceState::new(self.storage.clone()).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
