use super::*;
use std::collections::VecDeque;

struct MockTransport {
    responses: VecDeque<Result<Report, ServiceError>>,
    installed: RpmIdentity,
    after_execute: Option<RpmIdentity>,
    calls: Vec<Operation>,
}
impl Transport for MockTransport {
    fn installed(&mut self) -> Result<RpmIdentity, ServiceError> {
        Ok(self.installed.clone())
    }
    fn run(
        &mut self,
        operation: Operation,
        _: &UpdateCancellation,
        _: &mut dyn FnMut(UpdateProgress),
        started: &mut dyn FnMut(&str) -> Result<(), ServiceError>,
    ) -> Result<Report, ServiceError> {
        if matches!(operation, Operation::Execute(_)) {
            started("/123_tundra")?;
            if let Some(installed) = self.after_execute.take() {
                self.installed = installed;
            }
        }
        self.calls.push(operation);
        self.responses
            .pop_front()
            .expect("unexpected backend request")
    }
}
fn installed(version: &str) -> RpmIdentity {
    RpmIdentity {
        name: TARGET.into(),
        version: version.into(),
        architecture: "x86_64".into(),
    }
}
fn package(name: &str, version: &str) -> PackageVersion {
    PackageVersion::parse(&format!("{name};{version};x86_64;test-updates")).unwrap()
}
fn report(packages: Vec<(u32, PackageVersion)>) -> Report {
    Report {
        exit: 1,
        packages,
        ..Default::default()
    }
}
fn repositories() -> Report {
    Report {
        exit: 1,
        repositories: BTreeSet::from(["test-updates".into()]),
        ..Default::default()
    }
}
fn preview_reports(with_dependency: bool) -> Vec<Result<Report, ServiceError>> {
    let mut packages = vec![(11, package(TARGET, "2-1"))];
    if with_dependency {
        packages.push((12, package("tundra-runtime", "1-1")));
    }
    vec![
        Ok(repositories()),
        Ok(report(packages)),
        Ok(report(vec![(1, package(TARGET, "1-1"))])),
    ]
}
struct Fixture {
    controller: Controller<MockTransport>,
    root: std::path::PathBuf,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn fixture(responses: Vec<Result<Report, ServiceError>>) -> Fixture {
    let base = std::env::temp_dir().join(format!("tundra-packagekit-tests-{}", std::process::id()));
    let root = crate::create_temp_dir(&base, "case").unwrap();
    Fixture {
        controller: Controller {
            transport: MockTransport {
                responses: responses.into(),
                installed: installed("1-1"),
                after_execute: None,
                calls: Vec::new(),
            },
            installed: installed("1-1"),
            candidate: Some(package(TARGET, "2-1")),
            preview: None,
            cancellation: UpdateCancellation::default(),
            journal: Journal::at(root.join("journal")),
        },
        root,
    }
}
#[test]
fn source_or_portable_test_executable_cannot_open_the_rpm_client() {
    assert!(matches!(
        RpmUpdates::current(),
        Err(ServiceError::Unsupported)
    ));
}
#[test]
fn check_accepts_no_candidate_and_filters_other_system_updates() {
    for packages in [
        vec![],
        vec![(5, package("unrelated", "9-1"))],
        vec![(5, package(TARGET, "2-1"))],
        vec![(2, package(TARGET, "2-1"))],
    ] {
        let expected = packages
            .iter()
            .find(|(_, p)| p.name == TARGET)
            .map(|(_, p)| p.clone());
        let mut f = fixture(vec![Ok(repositories()), Ok(report(packages))]);
        let check = f.controller.check(&mut |_| {}).unwrap();
        assert_eq!(check.installed_version, "1-1");
        assert_eq!(check.candidate, expected);
        assert!(
            !f.controller
                .transport
                .calls
                .iter()
                .any(|c| matches!(c, Operation::Execute(_)))
        );
    }
}
#[test]
fn dependency_preview_includes_old_versions_and_new_installations() {
    let mut f = fixture(preview_reports(true));
    let preview = f.controller.preview(&mut |_| {}).unwrap();
    assert_eq!(preview.changes.len(), 2);
    assert!(
        preview
            .changes
            .iter()
            .any(|c| c.package.name == TARGET && c.old_version.as_deref() == Some("1-1"))
    );
    assert!(
        preview
            .changes
            .iter()
            .any(|c| c.package.name == "tundra-runtime" && c.old_version.is_none())
    );
}
#[test]
fn removals_downgrades_unknown_repositories_and_trust_requests_are_refused() {
    for info in [0, 13, 15, 20, 23, 28, 29, 30] {
        let mut f = fixture(vec![
            Ok(repositories()),
            Ok(report(vec![
                (11, package(TARGET, "2-1")),
                (info, package("dependency", "1-1")),
            ])),
        ]);
        assert_eq!(
            f.controller.preview(&mut |_| {}),
            Err(ServiceError::UntrustedTransaction)
        );
    }
    let mut unknown = package(TARGET, "2-1");
    unknown.repository = "unknown-repository".into();
    let mut f = fixture(vec![Ok(repositories()), Ok(report(vec![(11, unknown)]))]);
    assert_eq!(
        f.controller.preview(&mut |_| {}),
        Err(ServiceError::UntrustedTransaction)
    );
    let mut trust = report(vec![(11, package(TARGET, "2-1"))]);
    trust.error = Some(ServiceError::UntrustedTransaction);
    let mut f = fixture(vec![Ok(repositories()), Ok(trust)]);
    assert_eq!(
        f.controller.preview(&mut |_| {}),
        Err(ServiceError::UntrustedTransaction)
    );
}
#[test]
fn preview_changes_require_another_confirmation_before_execution() {
    let mut responses = preview_reports(false);
    responses.extend(preview_reports(true));
    let mut f = fixture(responses);
    f.controller.preview(&mut |_| {}).unwrap();
    assert_eq!(
        f.controller.execute(&mut |_| {}),
        Err(ServiceError::UntrustedTransaction)
    );
    assert!(
        !f.controller
            .transport
            .calls
            .iter()
            .any(|c| matches!(c, Operation::Execute(_)))
    );
    assert!(f.controller.journal.read().unwrap().is_none());
}
#[test]
fn success_requires_finished_success_and_the_expected_installed_rpm() {
    for actual in ["1-1", "2-1"] {
        let mut responses = preview_reports(false);
        responses.extend(preview_reports(false));
        responses.push(Ok(report(vec![])));
        let mut f = fixture(responses);
        f.controller.transport.after_execute = Some(installed(actual));
        f.controller.preview(&mut |_| {}).unwrap();
        let result = f.controller.execute(&mut |_| {}).unwrap();
        if actual == "2-1" {
            assert!(matches!(result, UpdateResult::Installed { version, .. } if version == actual));
        } else {
            assert_eq!(
                result,
                UpdateResult::Unknown {
                    expected_version: "2-1".into()
                }
            );
        }
        assert_eq!(
            f.controller.transport.calls.last(),
            Some(&Operation::Execute(package(TARGET, "2-1").id()))
        );
    }
}
#[test]
fn backend_failures_and_cancellation_are_not_reported_as_success() {
    for error in [
        ServiceError::PermissionDenied,
        ServiceError::AuthorizationCancelled,
        ServiceError::Busy,
        ServiceError::NetworkError,
        ServiceError::UntrustedTransaction,
    ] {
        let mut responses = preview_reports(false);
        responses.extend(preview_reports(false));
        responses.push(Ok(Report {
            exit: 2,
            error: Some(error),
            ..Default::default()
        }));
        let mut f = fixture(responses);
        f.controller.preview(&mut |_| {}).unwrap();
        assert_eq!(
            f.controller.execute(&mut |_| {}).unwrap(),
            UpdateResult::Failed(error)
        );
    }
    let mut responses = preview_reports(false);
    responses.extend(preview_reports(false));
    responses.push(Ok(Report {
        exit: 3,
        ..Default::default()
    }));
    let mut f = fixture(responses);
    f.controller.preview(&mut |_| {}).unwrap();
    assert_eq!(
        f.controller.execute(&mut |_| {}).unwrap(),
        UpdateResult::Cancelled
    );
    assert!(!f.controller.cancellation.request());
}
#[test]
fn disconnected_execution_is_persisted_and_requery_never_reexecutes() {
    let mut responses = preview_reports(false);
    responses.extend(preview_reports(false));
    responses.push(Err(ServiceError::BackendDisconnected));
    responses.push(Ok(report(vec![])));
    let mut f = fixture(responses);
    f.controller.preview(&mut |_| {}).unwrap();
    assert_eq!(
        f.controller.execute(&mut |_| {}).unwrap(),
        UpdateResult::Unknown {
            expected_version: "2-1".into()
        }
    );
    let record = f.controller.journal.read().unwrap().unwrap();
    assert_eq!(record.transaction.as_deref(), Some("/123_tundra"));
    f.controller.transport.installed = installed("2-1");
    // An externally changed RPM version without a matching successful history is insufficient.
    assert_eq!(
        f.controller.query(&mut |_| {}).unwrap(),
        Some(UpdateResult::Unknown {
            expected_version: "2-1".into()
        })
    );
    assert_eq!(
        f.controller
            .transport
            .calls
            .iter()
            .filter(|c| matches!(c, Operation::Execute(_)))
            .count(),
        1
    );
}

#[test]
fn recovery_verifies_matching_successful_history_without_reusing_the_old_object() {
    let mut responses = preview_reports(false);
    responses.extend(preview_reports(false));
    responses.push(Err(ServiceError::BackendDisconnected));
    let mut f = fixture(responses);
    f.controller.preview(&mut |_| {}).unwrap();
    f.controller.execute(&mut |_| {}).unwrap();
    let record = f.controller.journal.read().unwrap().unwrap();
    let history = transport::History::successful(&record);
    f.controller.transport.responses.push_back(Ok(Report {
        exit: 1,
        history: vec![history],
        ..Default::default()
    }));
    f.controller.transport.installed = installed("2-1");
    assert!(
        matches!(f.controller.query(&mut |_| {}).unwrap(), Some(UpdateResult::Installed { version, .. }) if version == "2-1")
    );
    assert!(f.controller.journal.read().unwrap().is_none());
    assert_eq!(
        f.controller.transport.calls.last(),
        Some(&Operation::History)
    );
    assert_eq!(
        f.controller
            .transport
            .calls
            .iter()
            .filter(|c| matches!(c, Operation::Execute(_)))
            .count(),
        1
    );
}
