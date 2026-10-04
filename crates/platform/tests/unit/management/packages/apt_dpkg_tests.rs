use super::*;
use crate::management::{ManagementCommand, ManagementKind};
use std::{collections::BTreeMap, fs, path::Path};

const PACKAGE: &str = "tundraux3-pty-regression";

/// Real apt and dpkg operate only on this empty package's private database.
/// No sudo, downloads, host package files, maintainer scripts or host APT hooks.
struct AptFixture {
    directory: tempfile::TempDir,
    spec: PackageCommand,
}

impl AptFixture {
    fn new() -> Option<Self> {
        if ![
            "/usr/bin/apt-get",
            "/usr/bin/dpkg",
            "/usr/bin/cat",
            "/usr/bin/env",
            "/usr/bin/timeout",
        ]
        .iter()
        .all(|program| Path::new(program).is_file())
        {
            eprintln!("Skipping isolated APT/dpkg PTY test: required executables are unavailable");
            return None;
        }
        let directory = tempfile::Builder::new()
            .prefix("tundraux3-apt-pty-")
            .tempdir()
            .unwrap();
        let root = directory.path();
        for relative in [
            "db/info",
            "db/updates",
            "root",
            "state/lists/partial",
            "cache/archives/partial",
            "log",
            "empty-parts",
            "empty-sources",
        ] {
            fs::create_dir_all(root.join(relative)).unwrap();
        }
        fs::write(root.join("sources.list"), "").unwrap();
        fs::write(root.join("main.conf"), "").unwrap();
        fs::write(
            root.join("db/status"),
            format!(
                "Package: {PACKAGE}\nStatus: install ok installed\nPriority: optional\nSection: misc\nInstalled-Size: 0\nMaintainer: Codex <codex@local>\nArchitecture: all\nVersion: 1.0\nDescription: Isolated PTY regression fixture\n\n"
            ),
        )
        .unwrap();
        fs::write(root.join(format!("db/info/{PACKAGE}.list")), "/.\n").unwrap();
        let config = root.join("apt.conf");
        let root = root.display();
        // APT_CONFIG is read before the default config files. Point those files
        // and every writable APT/dpkg path into the private fixture as well.
        fs::write(
            &config,
            format!(
                r#"Dir::Etc::parts "{root}/empty-parts";
Dir::Etc::main "{root}/main.conf";
Dir::Etc::sourcelist "{root}/sources.list";
Dir::Etc::sourceparts "{root}/empty-sources";
Dir::State "{root}/state";
Dir::State::status "{root}/db/status";
Dir::Cache "{root}/cache";
Dir::Log "{root}/log";
DPkg::Options:: "--admindir={root}/db";
DPkg::Options:: "--instdir={root}/root";
DPkg::Options:: "--force-not-root";
DPkg::Options:: "--log={root}/log/dpkg.log";
Debug::NoLocking "true";
"#
            ),
        )
        .unwrap();
        let request = ManagementCommand {
            kind: ManagementKind::Packages,
            action: "remove".into(),
            target: Some(format!("{PACKAGE}:all")),
            values: BTreeMap::new(),
            identity: BTreeMap::new(),
        };
        let mut spec = super::super::super::build_command(&request, PackageBackend::Apt).unwrap();
        let mut args = vec![
            format!("APT_CONFIG={}", config.display()),
            "/usr/bin/timeout".into(),
            "8".into(),
            spec.program.display().to_string(),
        ];
        args.append(&mut spec.args);
        // Only the test wrapper sets APT_CONFIG; the real helper still clears
        // caller-supplied environment variables before launching the backend.
        spec.program = "/usr/bin/env".into();
        spec.args = args;
        Some(Self { directory, spec })
    }

    fn status(&self) -> String {
        fs::read_to_string(self.directory.path().join("db/status")).unwrap()
    }

    fn interaction(answer: &[u8]) -> Interaction {
        Interaction {
            answers: VecDeque::from([("[Y/n]", answer.to_vec())]),
            ..Default::default()
        }
    }
}

#[test]
fn legacy_cat_logger_reproduces_the_dpkg_bad_file_descriptor_failure() {
    let Some(fixture) = AptFixture::new() else {
        return;
    };
    let host_status = fs::read("/var/lib/dpkg/status").unwrap();
    let installed = fixture.status();
    let mut spec = fixture.spec.clone();
    let status_option = spec
        .args
        .iter_mut()
        .find(|arg| arg.as_str() == "Dpkg::Options::=--status-fd=2")
        .expect("the real command should use the terminal's stderr for dpkg status");
    *status_option = "Dpkg::Options::=--status-logger=/usr/bin/cat".into();
    let mut io = AptFixture::interaction(b"y\r");
    let error = execute(spec, PackageBackend::Apt, &mut io, &AtomicBool::new(false))
        .unwrap_err()
        .to_string();
    assert!(error.contains("exit code 100"), "{error}");
    assert!(
        error.contains("standard output: Bad file descriptor"),
        "{error}"
    );
    assert!(error.contains("dpkg exited unexpectedly"), "{error}");
    assert!(
        io.answers.is_empty(),
        "the real confirmation was not reached"
    );
    assert_eq!(fixture.status(), installed);
    assert_eq!(fs::read("/var/lib/dpkg/status").unwrap(), host_status);
}

#[test]
fn real_apt_dpkg_removal_waits_for_confirmation_and_streams_status_to_the_pty() {
    let Some(fixture) = AptFixture::new() else {
        return;
    };
    let host_status = fs::read("/var/lib/dpkg/status").unwrap();
    let installed = fixture.status();
    let mut decline = AptFixture::interaction(b"n\r");
    assert!(
        execute(
            fixture.spec.clone(),
            PackageBackend::Apt,
            &mut decline,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(decline.answers.is_empty());
    assert_eq!(fixture.status(), installed);

    let mut confirm = AptFixture::interaction(b"y\r");
    let result = execute(
        fixture.spec.clone(),
        PackageBackend::Apt,
        &mut confirm,
        &AtomicBool::new(false),
    )
    .unwrap();
    let output = String::from_utf8_lossy(&confirm.output);
    assert!(result.contains("exit code 0"));
    assert!(confirm.answers.is_empty());
    assert!(
        output.contains("The following packages will be REMOVED:"),
        "{output}"
    );
    assert!(output.contains(&format!("Removing {PACKAGE}")), "{output}");
    assert!(
        output.contains(&format!("processing: remove: {PACKAGE}")),
        "{output}"
    );
    assert!(
        confirm
            .progress
            .iter()
            .any(|message| message == &format!("dpkg: remove: {PACKAGE}"))
    );
    assert!(!output.contains("Bad file descriptor"), "{output}");
    assert!(fixture.status().trim().is_empty());
    assert_eq!(fs::read("/var/lib/dpkg/status").unwrap(), host_status);
}
