use super::{PackageBackend, PackageRecord, detect_backend, parsing, validate_target};
use crate::management::{
    ManagementAction, ManagementError, ManagementQuery, ManagementRow, ManagementSnapshot,
};
use std::collections::BTreeMap;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const MAX_QUERY_BYTES: usize = 64 * 1024 * 1024;
const MAX_ROWS: usize = 2_000;

/// Bounded, cancellable read-only subprocess. Cancellation never touches a
/// package installation; this function is not used for package changes.
pub(crate) fn run_read_command(
    program: &str,
    args: &[String],
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(ManagementError::Cancelled);
    }
    let home = crate::linux::identity::LinuxUserContext::current()
        .map_err(|error| ManagementError::Unavailable(error.to_string()))?
        .home;
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("HOME", home)
        .env("LC_ALL", "C.UTF-8")
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            ManagementError::Unavailable(format!("Could not run {program}: {error}"))
        })?;
    let result = (|| {
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| ManagementError::Failed("Missing package query stdout".into()))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| ManagementError::Failed("Missing package query stderr".into()))?;
        for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                return Err(ManagementError::Failed(
                    std::io::Error::last_os_error().to_string(),
                ));
            }
        }
        let deadline = Instant::now() + Duration::from_secs(45);
        let mut out = Vec::new();
        let mut err = Vec::new();
        loop {
            drain(&mut stdout, &mut out)?;
            drain(&mut stderr, &mut err)?;
            if cancelled.load(Ordering::Acquire) {
                return Err(ManagementError::Cancelled);
            }
            if Instant::now() >= deadline {
                return Err(ManagementError::Failed(
                    "Package query timed out; repositories were not modified".into(),
                ));
            }
            if let Some(status) = child.try_wait().map_err(io_error)? {
                drain(&mut stdout, &mut out)?;
                drain(&mut stderr, &mut err)?;
                if !status.success() {
                    return Err(ManagementError::Failed(format!(
                        "{program} failed ({status}): {}",
                        runtime_log::sanitize_text(&String::from_utf8_lossy(&err))
                    )));
                }
                return Ok(String::from_utf8_lossy(&out).into_owned());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    if result.is_err() {
        // Only these fixed read-only queries can be killed on timeout/cancellation.
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn io_error(error: std::io::Error) -> ManagementError {
    ManagementError::Failed(error.to_string())
}

fn drain(reader: &mut dyn Read, destination: &mut Vec<u8>) -> Result<(), ManagementError> {
    let mut bytes = [0_u8; 16_384];
    loop {
        match reader.read(&mut bytes) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                if destination.len().saturating_add(count) > MAX_QUERY_BYTES {
                    return Err(ManagementError::Failed(
                        "Package metadata exceeds the supported query size".into(),
                    ));
                }
                destination.extend_from_slice(&bytes[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_error(error)),
        }
    }
}

pub(super) fn query(
    request: &ManagementQuery,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    if request.kind != crate::management::ManagementKind::Packages {
        return Err(ManagementError::InvalidInput(
            "This query is not a package query".into(),
        ));
    }
    if request.filter.len() > 512 || request.filter.chars().any(char::is_control) {
        return Err(ManagementError::InvalidInput(
            "Package search is too long or contains control characters".into(),
        ));
    }
    if !request.options.is_empty() {
        return Err(ManagementError::InvalidInput(
            "Package queries do not accept additional command options".into(),
        ));
    }
    let backend = detect_backend(cancelled)?;
    let scope = if request.scope.is_empty() {
        "installed"
    } else {
        request.scope.as_str()
    };
    if !matches!(scope, "installed" | "search" | "updates") {
        return Err(ManagementError::InvalidInput(
            "Package scope must be search, installed or updates".into(),
        ));
    }
    if let Some(target) = request.target.as_deref() {
        validate_target(target, backend)?;
    }
    let mut records = match backend {
        PackageBackend::Apt => {
            apt_records(scope, request.target.as_deref(), &request.filter, cancelled)?
        }
        PackageBackend::Dnf4 | PackageBackend::Dnf5 => {
            rpm_records(backend, scope, request.target.as_deref(), cancelled)?
        }
    };
    let filter = request.filter.to_lowercase();
    records.retain(|record| {
        request
            .target
            .as_ref()
            .is_none_or(|id| record.id(backend) == *id || record.name == *id)
            && (filter.is_empty()
                || format!("{} {}", record.name, record.summary)
                    .to_lowercase()
                    .contains(&filter))
    });
    records.sort_by(|left, right| {
        (&left.name, &left.architecture).cmp(&(&right.name, &right.architecture))
    });
    records.dedup_by(|left, right| {
        left.name == right.name
            && left.architecture == right.architecture
            && left.version == right.version
    });
    let count = records.len();
    records.truncate(MAX_ROWS);
    let mut notices = vec![
        "Lists use the existing repository cache. Refresh sources to download new metadata.".into(),
    ];
    if count > MAX_ROWS {
        notices.push(format!(
            "Showing the first {MAX_ROWS} matching packages; enter a more specific search."
        ));
    }
    if backend != PackageBackend::Apt {
        notices.push("RPM may preserve changed configuration files as .rpmnew or .rpmsave; review package output after changes.".into());
    }
    let mut snapshot = ManagementSnapshot {
        columns: vec![
            "Package".into(),
            "Installed".into(),
            "Available".into(),
            "Description".into(),
        ],
        rows: records
            .into_iter()
            .map(|record| to_row(record, backend, scope))
            .collect(),
        actions: vec![
            action("scope_search", "Search packages", false),
            action("scope_installed", "Installed packages", false),
            action("scope_updates", "Available updates", false),
            action("refresh", "Refresh sources", true),
            action("upgrade_all", "Upgrade all packages", true),
        ],
        notices,
        backend: backend.id().into(),
    };
    // Scope selectors are query actions, never privileged commands.
    for action in &mut snapshot.actions[..3] {
        action.confirm = false;
    }
    Ok(snapshot)
}

fn action(id: &str, label: &str, privileged: bool) -> ManagementAction {
    ManagementAction {
        id: id.into(),
        label: label.into(),
        confirm: privileged,
        privileged,
        ..Default::default()
    }
}

fn to_row(record: PackageRecord, backend: PackageBackend, scope: &str) -> ManagementRow {
    let id = record.id(backend);
    let installed = record.installed_version.as_deref().unwrap_or("");
    let mut identity = BTreeMap::from([
        ("backend".into(), backend.id().into()),
        ("name".into(), record.name.clone()),
        ("architecture".into(), record.architecture.clone()),
        ("installed_version".into(), installed.into()),
    ]);
    if scope != "installed" && !record.version.is_empty() {
        identity.insert("available_version".into(), record.version.clone());
    }
    let mut actions = Vec::new();
    if record.installed_version.is_some() {
        actions.push(action("remove", "Remove package", true));
        actions.push(action("upgrade", "Upgrade package", true));
    } else {
        actions.push(action("install", "Install package", true));
    }
    let mut detail = record.details;
    if !record.description.is_empty() {
        detail.push(("Description".into(), record.description));
    }
    if !record.repository.is_empty() {
        detail.push(("Repository".into(), record.repository));
    }
    ManagementRow {
        id,
        cells: vec![
            record.name,
            installed.into(),
            if scope == "installed" {
                String::new()
            } else {
                record.version
            },
            record.summary,
        ],
        detail,
        actions,
        identity,
    }
}

fn apt_installed(cancelled: &AtomicBool) -> Result<Vec<PackageRecord>, ManagementError> {
    let output = run_read_command("/usr/bin/dpkg-query", &["-W".into(),
        "-f=${binary:Package}\t${Version}\t${Architecture}\t${db:Status-Abbrev}\t${binary:Summary}\n".into()], cancelled)?;
    parsing::dpkg_records(&output)
}

fn apt_records(
    scope: &str,
    target: Option<&str>,
    filter: &str,
    cancelled: &AtomicBool,
) -> Result<Vec<PackageRecord>, ManagementError> {
    let installed = apt_installed(cancelled)?;
    if scope == "installed" && target.is_none() {
        return Ok(installed);
    }
    // Reading dumpavail loads every version's full description and dependencies;
    // ordinary Ubuntu repositories can exceed hundreds of MiB. Obtain a compact
    // name list first, then ask only for the displayed candidates in small batches.
    let updates = if scope == "updates" {
        let preview = run_read_command(
            "/usr/bin/apt-get",
            &[
                "-s".into(),
                "-o".into(),
                "APT::Get::Show-User-Simulation-Note=0".into(),
                "--with-new-pkgs".into(),
                "upgrade".into(),
            ],
            cancelled,
        )?;
        Some(parsing::apt_upgrades(&preview)?)
    } else {
        None
    };
    let names = if let Some(target) = target {
        vec![target.to_owned()]
    } else if let Some(updates) = &updates {
        installed
            .iter()
            .filter(|record| updates.iter().any(|(name, _)| *name == record.name))
            .map(|record| record.id(PackageBackend::Apt))
            .collect()
    } else {
        let text = run_read_command(
            "/usr/bin/apt-cache",
            &["search".into(), "--".into(), apt_search_pattern(filter)],
            cancelled,
        )?;
        apt_search_names(&text, filter)?
    };
    let mut names = names;
    names.sort();
    names.dedup();
    // The extra row lets the shared renderer report that results were limited.
    names.truncate(MAX_ROWS + 1);
    let mut available = Vec::new();
    for batch in names.chunks(200) {
        let mut args = vec!["show".into(), "--no-all-versions".into(), "--".into()];
        args.extend_from_slice(batch);
        let metadata = run_read_command("/usr/bin/apt-cache", &args, cancelled)?;
        available.extend(parsing::apt_records(&metadata)?);
    }
    let installed_versions: BTreeMap<_, _> = installed
        .iter()
        .map(|record| ((&record.name, &record.architecture), &record.version))
        .collect();
    for record in &mut available {
        record.installed_version = installed_versions
            .get(&(&record.name, &record.architecture))
            .map(|version| (*version).clone());
    }
    if let Some(updates) = updates {
        available.retain(|record| {
            record.installed_version.is_some()
                && updates
                    .iter()
                    .any(|(name, version)| *name == record.name && *version == record.version)
        });
    } else if scope == "installed" {
        available.retain(|record| record.installed_version.is_some());
    }
    Ok(available)
}

fn apt_search_pattern(filter: &str) -> String {
    if filter.is_empty() {
        return ".".into();
    }
    let mut pattern = String::new();
    for character in filter.chars() {
        if ".^$[]()*+?{}|\\".contains(character) {
            pattern.push('\\');
        }
        pattern.push(character);
    }
    pattern
}

fn apt_search_names(text: &str, filter: &str) -> Result<Vec<String>, ManagementError> {
    let filter = filter.to_lowercase();
    let mut names = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let (name, summary) = line.split_once(" - ").ok_or_else(|| {
            ManagementError::Failed("Unrecognized APT package search output".into())
        })?;
        validate_target(name, PackageBackend::Apt)?;
        if filter.is_empty() || format!("{name} {summary}").to_lowercase().contains(&filter) {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

fn rpm_installed(cancelled: &AtomicBool) -> Result<Vec<PackageRecord>, ManagementError> {
    let text = run_read_command(
        "/usr/bin/rpm",
        &[
            "-qa".into(),
            "--qf".into(),
            "%{NAME}\t%{ARCH}\t%{EPOCHNUM}:%{VERSION}-%{RELEASE}\t%{SUMMARY}\n".into(),
        ],
        cancelled,
    )?;
    // Install-only RPMs (for example kernels) can have several versions. A
    // name.arch removal addresses all of them, so expose one row with the full
    // version set instead of several misleading rows sharing a target.
    let mut groups = BTreeMap::<(String, String), Vec<PackageRecord>>::new();
    for record in parsing::rpm_records(&text)? {
        groups
            .entry((record.name.clone(), record.architecture.clone()))
            .or_default()
            .push(record);
    }
    let mut records = Vec::new();
    for mut versions in groups.into_values() {
        versions.sort_by(|left, right| left.version.cmp(&right.version));
        versions.dedup_by(|left, right| left.version == right.version);
        let all = versions
            .iter()
            .map(|record| record.version.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let mut record = versions.pop().expect("nonempty RPM version group");
        record.version = all.clone();
        record.installed_version = Some(all.clone());
        record.details.push(("Installed versions".into(), all));
        if !versions.is_empty() {
            record.details.push(("Removal scope".into(), "Removing this package targets all installed versions with this name and architecture; review DNF's actual plan".into()));
        }
        records.push(record);
    }
    Ok(records)
}

fn rpm_records(
    backend: PackageBackend,
    scope: &str,
    target: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Vec<PackageRecord>, ManagementError> {
    let installed = rpm_installed(cancelled)?;
    if scope == "installed" && target.is_none() {
        return Ok(installed);
    }
    let mut args = vec![
        "-C".into(),
        "--quiet".into(),
        "repoquery".into(),
        "--latest-limit=1".into(),
        "--qf".into(),
        "%{name}\t%{arch}\t%{evr}\t%{summary}\n".into(),
    ];
    if scope == "updates" {
        args.push("--upgrades".into());
    } else if scope == "installed" {
        args.push("--installed".into());
    }
    if let Some(target) = target {
        args.extend(["--".into(), target.into()]);
    }
    let text = run_read_command(backend.program(), &args, cancelled)?;
    let mut records = parsing::rpm_records(&text)?;
    for record in &mut records {
        record.installed_version = installed
            .iter()
            .find(|old| old.name == record.name && old.architecture == record.architecture)
            .map(|old| old.version.clone());
    }
    if let Some(target) = target {
        let text = run_read_command(
            backend.program(),
            &[
                "-C".into(),
                "--quiet".into(),
                "repoquery".into(),
                "--info".into(),
                "--".into(),
                target.into(),
            ],
            cancelled,
        )?;
        for record in &mut records {
            record.description = text.clone();
        }
    }
    Ok(records)
}

pub(super) fn validate_identity(
    target: &str,
    backend: PackageBackend,
    action: &str,
    expected: &BTreeMap<String, String>,
    cancelled: &AtomicBool,
) -> Result<(), ManagementError> {
    let installed = if backend == PackageBackend::Apt {
        apt_installed(cancelled)?
    } else {
        rpm_installed(cancelled)?
    };
    let actual = installed
        .iter()
        .find(|record| record.id(backend) == target || record.name == target);
    if let Some(version) = expected.get("installed_version") {
        if actual.map(|record| record.version.as_str()).unwrap_or("") != version {
            return Err(ManagementError::Conflict(
                "The installed package changed; refresh the package list before continuing".into(),
            ));
        }
    }
    if let Some(name) = expected.get("name") {
        let matches = match backend {
            PackageBackend::Apt => target.split(':').next() == Some(name),
            _ => {
                target == name
                    || expected
                        .get("architecture")
                        .is_some_and(|arch| target == format!("{name}.{arch}"))
            }
        };
        if !matches {
            return Err(ManagementError::InvalidInput(
                "Package target and identity do not match".into(),
            ));
        }
    }
    // A displayed repository version is part of the user's selected operation.
    // An already-refreshed cache must not silently turn a confirmed single-package
    // operation into installation of another version. The running manager still
    // resolves its own transaction and owns its actual confirmation afterwards.
    if matches!(action, "install" | "upgrade") {
        if let Some(version) = expected.get("available_version") {
            let candidates = if backend == PackageBackend::Apt {
                apt_records("search", Some(target), "", cancelled)?
            } else {
                rpm_records(backend, "search", Some(target), cancelled)?
            };
            if !candidates.iter().any(|record| {
                (record.id(backend) == target || record.name == target)
                    && record.version == *version
            }) {
                return Err(ManagementError::Conflict(
                    "The available package version changed; refresh the list and confirm again"
                        .into(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::management::ManagementKind;

    #[test]
    fn cancelled_queries_never_start_a_child_process() {
        assert_eq!(
            run_read_command("/program/that/does/not/exist", &[], &AtomicBool::new(true)),
            Err(ManagementError::Cancelled)
        );
    }

    #[test]
    fn failed_metadata_queries_are_not_reported_as_empty_lists() {
        let error = run_read_command(
            "/bin/sh",
            &[
                "-c".into(),
                "printf 'metadata unavailable' >&2; exit 19".into(),
            ],
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(error.to_string().contains("metadata unavailable"));
    }

    #[test]
    fn apt_search_uses_literal_text_and_compact_summaries() {
        assert_eq!(apt_search_pattern("c++ [demo].*"), r"c\+\+ \[demo\]\.\*");
        assert_eq!(apt_search_pattern(""), ".");
        assert_eq!(
            apt_search_names("bash - GNU Bourne shell\nexample - unrelated\n", "SHELL").unwrap(),
            ["bash"]
        );
        assert!(apt_search_names("unexpected metadata", "").is_err());
    }

    #[test]
    fn metadata_identity_cannot_be_substituted_for_another_target() {
        let expected = BTreeMap::from([("name".into(), "some-other-package".into())]);
        // Identity equality is checked after a fresh installed-version read by
        // production code; here a real read uses bash if APT is present.
        if !super::super::apt_available() {
            return;
        }
        assert!(
            validate_identity(
                "bash",
                PackageBackend::Apt,
                "remove",
                &expected,
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }

    #[test]
    fn changed_available_version_requires_a_fresh_confirmation() {
        if !super::super::apt_available() {
            return;
        }
        let expected = BTreeMap::from([
            ("name".into(), "bash".into()),
            (
                "available_version".into(),
                "unavailable-stale-version".into(),
            ),
        ]);
        let result = validate_identity(
            "bash",
            PackageBackend::Apt,
            "install",
            &expected,
            &AtomicBool::new(false),
        );
        assert!(matches!(result, Err(ManagementError::Conflict(_))));
        // Removing a package does not install a repository candidate.
        assert!(
            validate_identity(
                "bash",
                PackageBackend::Apt,
                "remove",
                &expected,
                &AtomicBool::new(false),
            )
            .is_ok()
        );
    }

    #[test]
    #[ignore = "read-only live APT metadata check; run explicitly on a Debian/Ubuntu host"]
    fn live_apt_installed_search_updates_and_details() {
        let cancelled = AtomicBool::new(false);
        assert_eq!(detect_backend(&cancelled).unwrap(), PackageBackend::Apt);
        let mut request = ManagementQuery::new(ManagementKind::Packages);
        request.scope = "installed".into();
        request.filter = "bash".into();
        let snapshot = query(&request, &cancelled).unwrap();
        let bash = snapshot
            .rows
            .iter()
            .find(|row| row.cells[0] == "bash")
            .unwrap();
        assert!(!bash.cells[1].is_empty());
        let target = bash.id.clone();
        request.scope = "search".into();
        request.target = Some(target);
        let details = query(&request, &cancelled).unwrap();
        assert!(details.rows.iter().any(|row| {
            row.detail
                .iter()
                .any(|(key, text)| key == "Description" && text.contains("shell"))
        }));
        request.target = None;
        let search = query(&request, &cancelled).unwrap();
        assert!(search.rows.iter().any(|row| row.cells[0] == "bash"));
        request.filter.clear();
        let all = query(&request, &cancelled).unwrap();
        assert!(!all.rows.is_empty() && all.rows.len() <= MAX_ROWS);
        request.scope = "updates".into();
        let updates = query(&request, &cancelled).unwrap();
        assert!(
            updates
                .rows
                .iter()
                .all(|row| !row.cells[1].is_empty() && !row.cells[2].is_empty())
        );
        assert!(
            updates
                .actions
                .iter()
                .any(|action| action.id == "upgrade_all")
        );
    }
}
