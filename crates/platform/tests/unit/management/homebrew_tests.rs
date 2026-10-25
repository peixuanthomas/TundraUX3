use super::*;
use crate::management::OperationEvent;
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::time::{Duration, Instant};

#[test]
fn batch_commands_group_formulae_and_casks_and_validate_before_writing() {
    use crate::management::selection::{self, Target};
    let mut request = command("remove", None);
    let targets = ["formula:demo", "cask:demo", "formula:other"].map(|id| Target {
        id: id.into(),
        identity: BTreeMap::new(),
    });
    request.values.insert(
        selection::KEY.into(),
        serde_json::to_string(&targets).unwrap(),
    );
    let commands = selection::expand(&request).unwrap();
    assert_eq!(
        command_groups(&commands).unwrap(),
        vec![
            vec!["uninstall", "--formula", "--", "demo", "other"],
            vec!["uninstall", "--cask", "--", "demo"],
        ]
    );
    let fixture = Fixture::new("exit 0");
    assert!(
        execute_with_program(
            &fixture.program(),
            &request,
            &mut Interaction::default(),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(!fixture.operation().exists());
}

const INFO: &str = r#"{
    "formulae":[{"full_name":"demo","versions":{"stable":"1.1"},"revision":1,
        "installed":[{"version":"1.0"}],"desc":"命令行工具","tap":"homebrew/core"}],
    "casks":[{"full_token":"demo","version":"2.0","installed":"1.0",
        "desc":"桌面应用","tap":"homebrew/cask","pinned":true}]
}"#;

struct Fixture(tempfile::TempDir);

impl Fixture {
    fn new(operation: &str) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(root.join("info.json"), INFO).unwrap();
        fs::write(root.join("outdated.json"), r#"{
            "formulae":[{"name":"demo","installed_versions":["1.0"],"current_version":"1.1_1"}],
            "casks":[{"name":"demo","installed_versions":["1.0"],"current_version":"2.0","pinned":true}]
        }"#).unwrap();
        let script = format!(
            r#"#!/bin/sh
cd -- "$(dirname -- "$0")" || exit 9
test "$HOMEBREW_NO_AUTO_UPDATE" = 1 || exit 10
test "$HOMEBREW_NO_INSTALL_CLEANUP" = 1 || exit 11
test "$HOMEBREW_NO_AUTOREMOVE" = 1 || exit 12
case "$1" in
    info) /bin/cat info.json ;;
    outdated) /bin/cat outdated.json ;;
    search)
        case "$4" in
            missing) printf 'Error: No formulae or casks found for "missing".\n' >&2; exit 1 ;;
            failure) printf 'Error: Unable to read package cache\n' >&2; exit 1 ;;
            *) printf 'demo\n' ;;
        esac ;;
    *) printf '%s\n' "$@" > operation; {operation} ;;
esac
"#
        );
        fs::write(root.join("brew"), script).unwrap();
        fs::set_permissions(root.join("brew"), fs::Permissions::from_mode(0o755)).unwrap();
        Self(directory)
    }

    fn program(&self) -> PathBuf {
        self.0.path().join("brew")
    }
    fn operation(&self) -> PathBuf {
        self.0.path().join("operation")
    }
}

fn request(scope: &str) -> ManagementQuery {
    ManagementQuery {
        scope: scope.into(),
        ..ManagementQuery::new(ManagementKind::Packages)
    }
}

fn command(action: &str, target: Option<&str>) -> ManagementCommand {
    ManagementCommand {
        kind: ManagementKind::Packages,
        action: action.into(),
        target: target.map(str::to_string),
        values: BTreeMap::new(),
        identity: BTreeMap::new(),
    }
}

#[derive(Default)]
struct Interaction {
    output: Vec<u8>,
    answers: VecDeque<(&'static str, &'static [u8])>,
    stop: u8,
    stop_started: Option<Instant>,
}

impl OperationInteraction for Interaction {
    fn emit(&mut self, event: OperationEvent) {
        if let OperationEvent::TerminalOutput { bytes } = event {
            self.output.extend(bytes);
        }
    }
    fn ask(&mut self, _: &str, _: &str, _: &[String], _: bool) -> Result<String, ManagementError> {
        panic!("Homebrew questions must remain in the native terminal")
    }
    fn terminal_input(&mut self, _: Duration) -> Result<Option<Vec<u8>>, ManagementError> {
        if self
            .answers
            .front()
            .is_some_and(|(prompt, _)| String::from_utf8_lossy(&self.output).contains(prompt))
        {
            Ok(self.answers.pop_front().map(|(_, bytes)| bytes.to_vec()))
        } else {
            Ok(None)
        }
    }
    fn stop_requested(&mut self) -> Option<bool> {
        if self.stop == 0 || !String::from_utf8_lossy(&self.output).contains("READY") {
            return None;
        }
        if self.stop_started.is_none() {
            self.stop_started = Some(Instant::now());
            return Some(false);
        }
        if self.stop == 2 && self.stop_started.unwrap().elapsed() > Duration::from_millis(150) {
            self.stop = 0;
            return Some(true);
        }
        None
    }
}

#[test]
fn formula_and_cask_lists_search_details_and_updates_keep_distinct_identities() {
    let fixture = Fixture::new("exit 0");
    let cancelled = AtomicBool::new(false);
    for scope in ["installed", "search", "updates"] {
        let mut query = request(scope);
        if scope == "search" {
            query.filter = "demo".into();
        }
        let snapshot = query_with_program(&fixture.program(), &query, &cancelled).unwrap();
        assert_eq!(snapshot.rows.len(), 2);
        assert_ne!(snapshot.rows[0].id, snapshot.rows[1].id);
        for (kind, candidate) in [("formula", "1.1_1"), ("cask", "2.0")] {
            let row = snapshot
                .rows
                .iter()
                .find(|row| row.id == format!("{kind}:demo"))
                .unwrap();
            assert_eq!(row.identity["installed_version"], "1.0");
            assert_eq!(row.identity["available_version"], candidate);
            assert!(row.actions.iter().any(|action| action.id == "remove"));
            assert!(
                row.actions.iter().all(|action| !action.privileged
                    && action.requires_approval
                    && !action.confirm)
            );
            let mut details = query.clone();
            details.target = Some(row.id.clone());
            let result = query_with_program(&fixture.program(), &details, &cancelled).unwrap();
            assert_eq!(result.rows.len(), 1);
            assert_eq!(result.rows[0].id, row.id);
            assert!(result.details_complete);
        }
        let cask = snapshot
            .rows
            .iter()
            .find(|row| row.id == "cask:demo")
            .unwrap();
        assert!(
            cask.actions
                .iter()
                .find(|action| action.id == "upgrade")
                .unwrap()
                .disabled_reason
                .is_some()
        );
    }
    let empty = query_with_program(&fixture.program(), &request("search"), &cancelled).unwrap();
    assert!(empty.rows.is_empty());
    let mut query = request("installed");
    query.filter = "桌面".into();
    assert_eq!(
        query_with_program(&fixture.program(), &query, &cancelled)
            .unwrap()
            .rows[0]
            .id,
        "cask:demo"
    );
}

#[test]
fn command_mapping_rejects_options_paths_extra_values_and_unsupported_actions() {
    for (action, verb) in [
        ("install", "install"),
        ("remove", "uninstall"),
        ("upgrade", "upgrade"),
    ] {
        for (target, flag, name) in [
            ("formula:demo@2", "--formula", "demo@2"),
            ("cask:owner/tap/demo", "--cask", "owner/tap/demo"),
        ] {
            assert_eq!(
                command_args(&command(action, Some(target))).unwrap(),
                [verb, flag, "--", name]
            );
        }
        for target in [
            "demo",
            "formula:--force",
            "formula:../demo",
            "formula:/tmp/demo.rb",
            "cask:https://example.com/demo",
            "formula:a/b",
            "formula:a/b/../demo",
            "cask:demo;id",
            "formula:*.rb",
            "formula:demo.rb",
            "cask:",
        ] {
            assert!(
                command_args(&command(action, Some(target))).is_err(),
                "{target}"
            );
        }
    }
    for (action, verb) in [("refresh", "update"), ("upgrade_all", "upgrade")] {
        assert_eq!(command_args(&command(action, None)).unwrap(), [verb]);
        assert!(command_args(&command(action, Some("formula:demo"))).is_err());
    }
    for action in [
        "repair_configure",
        "source_add",
        "purge",
        "autoremove",
        "install --force",
    ] {
        assert!(command_args(&command(action, Some("formula:demo"))).is_err());
    }
    let mut extra = command("install", Some("formula:demo"));
    extra.values.insert("flags".into(), "--force".into());
    assert!(command_args(&extra).is_err());
    for metadata in ["{}", "not json", r#"{"formulae":[{}],"casks":[]}"#] {
        assert!(parse_info(metadata).is_err());
    }
}

#[test]
fn empty_search_is_not_a_failure_but_tool_errors_are_preserved() {
    let fixture = Fixture::new("exit 0");
    let mut query = request("search");
    query.filter = "missing".into();
    assert!(
        query_with_program(&fixture.program(), &query, &AtomicBool::new(false))
            .unwrap()
            .rows
            .is_empty()
    );
    query.filter = "failure".into();
    let error =
        query_with_program(&fixture.program(), &query, &AtomicBool::new(false)).unwrap_err();
    assert!(error.to_string().contains("Unable to read package cache"));
    assert_eq!(
        query_with_program(&fixture.program(), &query, &AtomicBool::new(true)),
        Err(ManagementError::Cancelled)
    );
}

#[test]
fn native_confirmation_and_secret_input_run_in_a_managed_pty() {
    let fixture = Fixture::new(
        r#"
        printf 'Apply changes? [Y/n] '; read answer; test "$answer" = y || exit 23
        stty -echo; printf 'Password: '; read password; stty echo
        test "$password" = private-test-password || exit 24
        printf '\nCOMPLETED\n'
    "#,
    );
    let mut interaction = Interaction {
        answers: VecDeque::from([
            ("[Y/n]", b"y\r".as_slice()),
            ("Password:", b"private-test-password\r".as_slice()),
        ]),
        ..Default::default()
    };
    execute_with_program(
        &fixture.program(),
        &command("remove", Some("cask:demo")),
        &mut interaction,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(fixture.operation()).unwrap(),
        "uninstall\n--cask\n--\ndemo\n"
    );
    let output = String::from_utf8_lossy(&interaction.output);
    assert!(output.contains("COMPLETED"));
    assert!(!output.contains("private-test-password"));
    assert!(interaction.answers.is_empty());
}

#[test]
fn stale_selection_and_declined_native_plan_never_apply_changes() {
    let fixture = Fixture::new(
        "printf '[y/N] '; read answer; test \"$answer\" = y || exit 23; printf APPLIED",
    );
    let mut request = command("upgrade", Some("formula:demo"));
    request
        .identity
        .insert("installed_version".into(), "old-version".into());
    assert!(matches!(
        execute_with_program(
            &fixture.program(),
            &request,
            &mut Interaction::default(),
            &AtomicBool::new(false)
        ),
        Err(ManagementError::Conflict(_))
    ));
    assert!(!fixture.operation().exists());
    let mut interaction = Interaction {
        answers: VecDeque::from([("[y/N]", b"n\r".as_slice())]),
        ..Default::default()
    };
    let error = execute_with_program(
        &fixture.program(),
        &command("install", Some("formula:demo")),
        &mut interaction,
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(error.to_string().contains("23"));
    assert!(!String::from_utf8_lossy(&interaction.output).contains("APPLIED"));
}

#[test]
fn explicit_stop_and_force_stop_finish_and_do_not_report_success() {
    for (mode, script) in [
        (
            1,
            "trap 'exit 0' TERM; printf READY; while :; do sleep 1; done",
        ),
        (2, "trap '' TERM; printf READY; while :; do sleep 1; done"),
    ] {
        let fixture = Fixture::new(script);
        let mut interaction = Interaction {
            stop: mode,
            ..Default::default()
        };
        assert!(
            execute_with_program(
                &fixture.program(),
                &command("refresh", None),
                &mut interaction,
                &AtomicBool::new(false)
            )
            .is_err()
        );
        let stopping = interaction.stop_started.unwrap().elapsed();
        assert!(stopping < Duration::from_secs(3), "stop took {stopping:?}");
    }
}
