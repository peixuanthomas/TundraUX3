use super::super::dbus;
use super::{UpdateCancellation, journal::Record};
use crate::{
    installation::RpmIdentity,
    service::ServiceError,
    updates::{PackageVersion, UpdateProgress, UpdateStage, transaction_error},
};
use futures_lite::StreamExt;
use std::{
    collections::{BTreeSet, HashMap},
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{OwnedObjectPath, OwnedValue},
};

const SERVICE: &str = "org.freedesktop.PackageKit";
const INTERFACE: &str = "org.freedesktop.PackageKit.Transaction";
const ONLY_TRUSTED: u64 = 1 << 1;
const SIMULATE: u64 = 1 << 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Operation {
    Repositories,
    Updates,
    Installed(Vec<String>),
    Simulate(String),
    Execute(String),
    History,
}
#[derive(Debug, Default)]
pub(super) struct Report {
    pub exit: u32,
    pub error: Option<ServiceError>,
    pub packages: Vec<(u32, PackageVersion)>,
    pub repositories: BTreeSet<String>,
    pub history: Vec<History>,
    pub restart: bool,
}
impl Report {
    pub fn require_success(&self) -> Result<(), ServiceError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        match self.exit {
            1 => Ok(()),
            3 => Err(ServiceError::AuthorizationCancelled),
            4 | 5 | 8 => Err(ServiceError::UntrustedTransaction),
            _ => Err(ServiceError::Unknown),
        }
    }
}
#[derive(Debug)]
pub(super) struct History {
    path: String,
    time: String,
    succeeded: bool,
    role: u32,
    data: String,
    uid: u32,
}
impl History {
    pub fn matches(&self, record: &Record) -> bool {
        let Some(hint) = record.transaction.as_deref() else {
            return false;
        };
        let Ok(started_ms) = i64::try_from(record.started_ms) else {
            return false;
        };
        let Ok(timestamp) = chrono::DateTime::parse_from_rfc3339(&self.time) else {
            return false;
        };
        // UpdatePackages is role22 in PackageKit's public enum.
        self.path == hint
            && self.uid == record.uid
            && self.succeeded
            && self.role == 22
            && timestamp.timestamp_millis().abs_diff(started_ms) <= 10_000
            && self
                .data
                .split(['\t', '\n', ' '])
                .any(|field| field == record.expected.id())
    }
}

pub(super) trait Transport {
    fn installed(&mut self) -> Result<RpmIdentity, ServiceError>;
    fn run(
        &mut self,
        operation: Operation,
        cancellation: &UpdateCancellation,
        progress: &mut dyn FnMut(UpdateProgress),
        started: &mut dyn FnMut(&str) -> Result<(), ServiceError>,
    ) -> Result<Report, ServiceError>;
}

pub(super) struct DbusTransport {
    connection: Connection,
    pub interaction: Option<std::sync::Arc<dyn super::super::authorization::Interaction>>,
}
impl DbusTransport {
    pub fn new() -> Result<Self, ServiceError> {
        Ok(Self {
            connection: dbus::system()?,
            interaction: None,
        })
    }
    fn owner(&self) -> Result<String, ServiceError> {
        zbus::blocking::fdo::DBusProxy::new(&self.connection)
            .map_err(dbus::map_error)?
            .get_name_owner(SERVICE.try_into().map_err(|_| ServiceError::Unknown)?)
            .map(|name| name.to_string())
            .map_err(|error| dbus::map_error(error.into()))
    }
}
impl Transport for DbusTransport {
    fn installed(&mut self) -> Result<RpmIdentity, ServiceError> {
        super::super::installation::installed_rpm()?.ok_or(ServiceError::Unsupported)
    }
    fn run(
        &mut self,
        operation: Operation,
        cancellation: &UpdateCancellation,
        progress: &mut dyn FnMut(UpdateProgress),
        started: &mut dyn FnMut(&str) -> Result<(), ServiceError>,
    ) -> Result<Report, ServiceError> {
        let mut authorization = if matches!(operation, Operation::Execute(_)) {
            super::super::authorization::prepare(
                &self.connection,
                super::super::authorization::Action::Update,
                self.interaction.clone(),
            )?
        } else {
            None
        };
        let mut awaiting_authorization = false;
        let manager = Proxy::new(
            &self.connection,
            SERVICE,
            "/org/freedesktop/PackageKit",
            SERVICE,
        )
        .map_err(dbus::map_error)?;
        let path: OwnedObjectPath = manager
            .call("CreateTransaction", &())
            .map_err(dbus::map_error)?;
        let owner = self.owner()?;
        let proxy = Proxy::new(&self.connection, owner.as_str(), path.as_str(), INTERFACE)
            .map_err(dbus::map_error)?;
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender(owner.as_str())
            .map_err(|_| ServiceError::Unknown)?
            .path(path.as_str())
            .map_err(|_| ServiceError::Unknown)?
            .build()
            .to_owned();
        let mut signals = futures_lite::future::block_on(zbus::MessageStream::for_match_rule(
            rule,
            self.connection.inner(),
            Some(4096),
        ))
        .map_err(dbus::map_error)?;
        proxy
            .call::<_, _, ()>("SetHints", &(transaction_hints(),))
            .map_err(dbus::map_error)?;
        started(path.as_str())?;
        let executed = matches!(operation, Operation::Execute(_));
        let request: Result<(), zbus::Error> = match &operation {
            Operation::Repositories => proxy.call("GetRepoList", &(2_u64,)),
            Operation::Updates => proxy.call("GetUpdates", &(2_u64,)),
            Operation::Installed(names) => proxy.call("Resolve", &(4_u64, names)),
            Operation::Simulate(id) => proxy.call(
                "UpdatePackages",
                &(ONLY_TRUSTED | SIMULATE, vec![id.as_str()]),
            ),
            Operation::Execute(id) => {
                proxy.call("UpdatePackages", &(ONLY_TRUSTED, vec![id.as_str()]))
            }
            Operation::History => proxy.call("GetOldTransactions", &(20_u32,)),
        };
        if let Err(error) = request {
            let error = dbus::map_error(error);
            return match error {
                ServiceError::PermissionDenied | ServiceError::Unsupported | ServiceError::Busy => {
                    Ok(Report {
                        exit: 2,
                        error: Some(error),
                        ..Default::default()
                    })
                }
                _ => Err(error),
            };
        }
        let deadline = Instant::now()
            + if executed {
                Duration::from_secs(1800)
            } else {
                Duration::from_secs(120)
            };
        let mut owner_check = Instant::now();
        let mut report = Report::default();
        let mut state = UpdateProgress::default();
        let mut cancelled = false;
        loop {
            if Instant::now() >= deadline {
                return Err(ServiceError::Timeout);
            }
            if owner_check.elapsed() >= Duration::from_secs(1) {
                if self.owner()? != owner {
                    return Err(ServiceError::BackendDisconnected);
                }
                owner_check = Instant::now();
            }
            if !cancelled && state.cancellable && cancellation.requested.load(Ordering::Acquire) {
                // This is the only cancellation mechanism. Never kill package processes.
                cancellation.requested.store(false, Ordering::Release);
                match proxy.call::<_, _, ()>("Cancel", &()) {
                    Ok(()) => cancelled = true,
                    Err(zbus::Error::MethodError(_, _, _)) => {
                        // AllowCancel can change between the signal and our request.
                        // A rejected cancellation does not end the update transaction.
                        state.cancellable = false;
                        cancellation.allowed.store(false, Ordering::Release);
                        progress(state.clone());
                    }
                    Err(error) => return Err(dbus::map_error(error)),
                }
            }
            let next = futures_lite::future::block_on(futures_lite::future::race(
                async { Some(signals.next().await) },
                async {
                    async_io::Timer::after(Duration::from_millis(100)).await;
                    None
                },
            ));
            let Some(message) = next else {
                continue;
            };
            let message = message
                .ok_or(ServiceError::BackendDisconnected)?
                .map_err(dbus::map_error)?;
            let header = message.header();
            let member = header.member().map(|member| member.as_str()).unwrap_or("");
            match member {
                "PropertiesChanged" => {
                    let (interface, properties, _): (
                        String,
                        HashMap<String, OwnedValue>,
                        Vec<String>,
                    ) = message
                        .body()
                        .deserialize()
                        .map_err(|_| ServiceError::Unknown)?;
                    if interface != INTERFACE {
                        continue;
                    }
                    if let Some(value) =
                        properties.get("Status").and_then(|v| u32::try_from(v).ok())
                    {
                        if value == 31 {
                            awaiting_authorization = true;
                        } else if awaiting_authorization {
                            // The method reply precedes asynchronous authorization. Release the
                            // terminal only when the service leaves its authorization stage.
                            authorization.take();
                        }
                        state.stage = match value {
                            1 | 30 => UpdateStage::Waiting,
                            31 => UpdateStage::Authorizing,
                            8 | 20..=25 => UpdateStage::Downloading,
                            9..=12 | 16 => UpdateStage::Installing,
                            14 | 15 | 32..=34 => UpdateStage::Verifying,
                            18 => UpdateStage::Finished,
                            _ => UpdateStage::Preparing,
                        };
                    }
                    if let Some(value) = properties
                        .get("Percentage")
                        .and_then(|v| u32::try_from(v).ok())
                    {
                        state.percentage = (value <= 100).then_some(value);
                    }
                    if let Some(value) = properties
                        .get("AllowCancel")
                        .and_then(|v| bool::try_from(v).ok())
                    {
                        state.cancellable = value;
                        cancellation.allowed.store(value, Ordering::Release);
                    }
                    progress(state.clone());
                }
                "Package" => {
                    let (info, id, _): (u32, String, String) = message
                        .body()
                        .deserialize()
                        .map_err(|_| ServiceError::Unknown)?;
                    add_package(&mut report, &mut state, info, &id)?;
                    progress(state.clone());
                }
                "Packages" => {
                    let (packages,): (Vec<(u32, String, String)>,) =
                        message
                            .body()
                            .deserialize()
                            .map_err(|_| ServiceError::Unknown)?;
                    for (info, id, _) in packages {
                        add_package(&mut report, &mut state, info, &id)?;
                    }
                    progress(state.clone());
                }
                "RepoDetail" => {
                    let (id, _, enabled): (String, String, bool) = message
                        .body()
                        .deserialize()
                        .map_err(|_| ServiceError::Unknown)?;
                    if enabled
                        && !id.is_empty()
                        && id.len() <= 512
                        && !id.chars().any(char::is_control)
                    {
                        report.repositories.insert(id);
                    }
                }
                "ErrorCode" => {
                    let (code, _): (u32, String) = message
                        .body()
                        .deserialize()
                        .map_err(|_| ServiceError::Unknown)?;
                    report.error = Some(transaction_error(code));
                }
                "RepoSignatureRequired" | "EulaRequired" | "MediaChangeRequired" => {
                    report.error = Some(ServiceError::UntrustedTransaction)
                }
                "RequireRestart" => {
                    let (kind, _): (u32, String) = message
                        .body()
                        .deserialize()
                        .map_err(|_| ServiceError::Unknown)?;
                    report.restart |= matches!(kind, 4 | 6);
                }
                "Transaction" => {
                    let (path, time, succeeded, role, _, data, uid, _): (
                        OwnedObjectPath,
                        String,
                        bool,
                        u32,
                        u32,
                        String,
                        u32,
                        String,
                    ) = message
                        .body()
                        .deserialize()
                        .map_err(|_| ServiceError::Unknown)?;
                    if report.history.len() >= 20 {
                        return Err(ServiceError::Unknown);
                    }
                    report.history.push(History {
                        path: path.to_string(),
                        time,
                        succeeded,
                        role,
                        data,
                        uid,
                    });
                }
                "Finished" => {
                    let (exit, _): (u32, u32) = message
                        .body()
                        .deserialize()
                        .map_err(|_| ServiceError::Unknown)?;
                    report.exit = exit;
                    cancellation.allowed.store(false, Ordering::Release);
                    state.cancellable = false;
                    progress(state);
                    return Ok(report);
                }
                _ => {}
            }
        }
    }
}
fn transaction_hints() -> Vec<&'static str> {
    vec!["interactive=true", "background=false"]
}

fn add_package(
    report: &mut Report,
    progress: &mut UpdateProgress,
    info: u32,
    id: &str,
) -> Result<(), ServiceError> {
    if report.packages.len() >= 20_000 {
        return Err(ServiceError::UntrustedTransaction);
    }
    let package = PackageVersion::parse(id)?;
    progress.package = Some(package.name.clone());
    report.packages.push((info, package));
    Ok(())
}

#[cfg(test)]
impl History {
    pub(super) fn successful(record: &Record) -> Self {
        Self {
            path: record.transaction.clone().unwrap(),
            time: chrono::DateTime::from_timestamp_millis(record.started_ms as i64)
                .unwrap()
                .to_rfc3339(),
            succeeded: true,
            role: 22,
            data: format!("updating\t{}\tTundraUX3", record.expected.id()),
            uid: record.uid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transaction_hints_use_the_packagekit_string_array_wire_contract() {
        let message = zbus::Message::method_call("/test", "SetHints")
            .unwrap()
            .build(&(transaction_hints(),))
            .unwrap();
        let (hints,): (Vec<String>,) = message.body().deserialize().unwrap();
        assert_eq!(hints, ["interactive=true", "background=false"]);
        assert_eq!(message.body().signature().to_string(), "as");
    }

    #[test]
    fn history_requires_target_time_user_and_transaction_hint_together() {
        let record = Record {
            expected: PackageVersion::parse("tundraux3;2-1;x86_64;updates").unwrap(),
            started_ms: 1_700_000_000_000,
            uid: 1000,
            transaction: Some("/123_test".into()),
        };
        assert!(History::successful(&record).matches(&record));
        let mut history = History::successful(&record);
        history.uid += 1;
        assert!(!history.matches(&record));
        let mut history = History::successful(&record);
        history.path = "/other".into();
        assert!(!history.matches(&record));
        let mut history = History::successful(&record);
        history.time = "2020-01-01T00:00:00Z".into();
        assert!(!history.matches(&record));
        let mut history = History::successful(&record);
        history.succeeded = false;
        assert!(!history.matches(&record));
        let mut history = History::successful(&record);
        history.data = "tundraux3;3-1;x86_64;updates".into();
        assert!(!history.matches(&record));
    }
}
