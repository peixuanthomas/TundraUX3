//! Read-only pacman metadata queries. Every command uses the configured local
//! and sync databases; none refreshes a database or resolves a transaction.

use super::{PackageBackend, PackageRecord, query, validate_target};
use crate::management::ManagementError;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};

const PROGRAM: &str = "/usr/bin/pacman";
const CONFIG_PROGRAM: &str = "/usr/bin/pacman-conf";
const MAX_QUERY_BYTES: usize = 64 * 1024 * 1024;
const MAX_ROWS: usize = 2_000;
const INFO_BATCH_SIZE: usize = 200;

/// Only commands that legitimately return exit 1 without diagnostics may use
/// `allow_empty`. The shared runner still rejects all diagnostic failures.
fn run(
    program: &str,
    args: &[String],
    cancelled: &AtomicBool,
    allow_empty: bool,
) -> Result<String, ManagementError> {
    if program == PROGRAM && args.first().is_some_and(|arg| arg == "-Qi") {
        query::run_read_command_allow_empty(program, args, cancelled)
    } else {
        // Missing higher-priority databases can produce exit 0 and partial
        // results. Every sync/config query therefore rejects stderr as well.
        query::run_pacman_sync_command(program, args, cancelled, allow_empty)
    }
}

pub(super) fn installed(cancelled: &AtomicBool) -> Result<Vec<PackageRecord>, ManagementError> {
    installed_with(cancelled, &mut run)
}

pub(super) fn records(
    scope: &str,
    target: Option<&str>,
    filter: &str,
    cancelled: &AtomicBool,
) -> Result<Vec<PackageRecord>, ManagementError> {
    records_with(scope, target, filter, cancelled, &mut run)
}

// The runner seam is private and cannot become a caller-supplied executable or
// environment override. Tests exercise the real query selection without pacman.
type ReadResult = Result<String, ManagementError>;

fn read_with<F>(
    operation: &str,
    targets: &[String],
    cancelled: &AtomicBool,
    allow_empty: bool,
    read: &mut F,
) -> ReadResult
where
    F: FnMut(&str, &[String], &AtomicBool, bool) -> ReadResult,
{
    check_cancelled(cancelled)?;
    let mut args = vec![operation.into(), "--color".into(), "never".into()];
    if !targets.is_empty() {
        args.push("--".into());
        args.extend_from_slice(targets);
    }
    read_program_with(PROGRAM, &args, cancelled, allow_empty, read)
}

fn read_program_with<F>(
    program: &str,
    args: &[String],
    cancelled: &AtomicBool,
    allow_empty: bool,
    read: &mut F,
) -> ReadResult
where
    F: FnMut(&str, &[String], &AtomicBool, bool) -> ReadResult,
{
    check_cancelled(cancelled)?;
    let output = read(program, args, cancelled, allow_empty)?;
    check_cancelled(cancelled)?;
    check_size(&output)?;
    Ok(output)
}

fn installed_with<F>(
    cancelled: &AtomicBool,
    read: &mut F,
) -> Result<Vec<PackageRecord>, ManagementError>
where
    F: FnMut(&str, &[String], &AtomicBool, bool) -> ReadResult,
{
    let output = read_with("-Qi", &[], cancelled, true, read)?;
    let records = info_records(&output, true)?;
    let mut names = BTreeSet::new();
    for record in &records {
        check_cancelled(cancelled)?;
        if !names.insert(&record.name) {
            return Err(metadata_error("duplicate installed package name"));
        }
    }
    Ok(records)
}

fn records_with<F>(
    scope: &str,
    target: Option<&str>,
    filter: &str,
    cancelled: &AtomicBool,
    read: &mut F,
) -> Result<Vec<PackageRecord>, ManagementError>
where
    F: FnMut(&str, &[String], &AtomicBool, bool) -> ReadResult,
{
    check_cancelled(cancelled)?;
    if !matches!(scope, "installed" | "search" | "updates") {
        return Err(ManagementError::InvalidInput(
            "Package scope must be search, installed or updates".into(),
        ));
    }
    if filter.len() > 512 || filter.chars().any(char::is_control) {
        return Err(ManagementError::InvalidInput(
            "Package search is too long or contains control characters".into(),
        ));
    }
    if let Some(target) = target {
        validate_target(target, PackageBackend::Pacman)?;
    }
    let mut installed = installed_with(cancelled, read)?;

    // No user regular expression is passed to pacman. Query all compact
    // summaries, choose repository priority first, and then apply a literal
    // case-insensitive substring filter. Filtering before choosing would let a
    // matching lower-priority duplicate mask the actual preferred candidate.
    // Repository order is pacman's configured order, never version ordering:
    // https://man.archlinux.org/man/pacman.conf.5.en#REPOSITORY_SECTIONS
    let cache = (|| {
        // pacman-conf resolves Includes and defaults exactly as pacman does.
        // A single fixed query avoids ad-hoc parsing of configuration paths or
        // using transaction resolution to guess repository eligibility.
        // https://man.archlinux.org/man/pacman-conf.8.en
        let configuration = read_program_with(
            CONFIG_PROGRAM,
            &["--verbose".into()],
            cancelled,
            false,
            read,
        )?;
        let repositories = supported_repositories(&configuration)?;
        let output = read_with("-Ss", &[], cancelled, true, read)?;
        let summaries = repository_summaries(&output)?;
        if summaries
            .iter()
            .any(|summary| !repositories.contains(&summary.repository))
        {
            return Err(ManagementError::Conflict(
                "The pacman repository configuration changed; refresh the package list before continuing".into(),
            ));
        }
        Ok(summaries)
    })();
    let summaries = match cache {
        Ok(summaries) => summaries,
        Err(ManagementError::Cancelled) => return Err(ManagementError::Cancelled),
        Err(error) if scope == "installed" => {
            // Local details and removal remain useful even if sync metadata is
            // missing. Do not label these packages foreign or hide the error.
            for record in &mut installed {
                record.details.push((
                    "Repository availability".into(),
                    format!("Cached repository metadata is unavailable: {error}"),
                ));
            }
            retain_matches(&mut installed, target, filter);
            check_cancelled(cancelled)?;
            return Ok(installed);
        }
        Err(error) => return Err(error),
    };
    let mut preferred = BTreeMap::new();
    for summary in summaries {
        check_cancelled(cancelled)?;
        preferred.entry(summary.name.clone()).or_insert(summary);
    }

    if scope == "installed" {
        for record in &mut installed {
            check_cancelled(cancelled)?;
            if let Some(candidate) = preferred.get(&record.name) {
                record.repository = candidate.repository.clone();
                record.details.push((
                    "Cached repository version".into(),
                    candidate.version.clone(),
                ));
                record.details.push((
                    "Repository availability".into(),
                    "Native package: present in the existing repository cache".into(),
                ));
            } else {
                record.details.push((
                    "Repository availability".into(),
                    "Foreign package: no candidate in the existing repository cache; its original source is not recorded by pacman".into(),
                ));
            }
        }
        retain_matches(&mut installed, target, filter);
        check_cancelled(cancelled)?;
        return Ok(installed);
    }

    let installed_by_name: BTreeMap<_, _> = installed
        .iter()
        .map(|record| (record.name.as_str(), record))
        .collect();
    let updates = if scope == "updates" {
        // -Q reads only existing databases; -u performs libalpm's version
        // comparison, preserving epochs and pkgrel without reimplementing it.
        // -q avoids the human-readable "old -> new" format and annotations.
        let output = read_with("-Quq", &[], cancelled, true, read)?;
        let names = query_names(&output)?;
        for name in &names {
            if !installed_by_name.contains_key(name.as_str()) || !preferred.contains_key(name) {
                return Err(ManagementError::Conflict(
                    "Cached update metadata changed or is unavailable; refresh the package list before continuing".into(),
                ));
            }
        }
        Some(names)
    } else {
        None
    };

    let filter = filter.to_lowercase();
    let mut selected: Vec<_> = preferred
        .values()
        .filter(|candidate| {
            target.is_none_or(|name| candidate.name == name)
                && updates
                    .as_ref()
                    .is_none_or(|names| names.contains(&candidate.name))
                && literal_match(&candidate.name, &candidate.description, &filter)
        })
        .collect();
    // The extra candidate tells the shared renderer that the list was limited.
    selected.truncate(MAX_ROWS + 1);
    let mut available = Vec::new();
    for batch in selected.chunks(INFO_BATCH_SIZE) {
        let targets: Vec<_> = batch
            .iter()
            .map(|candidate| format!("{}/{}", candidate.repository, candidate.name))
            .collect();
        // Exact internally-selected repo/name targets cannot select groups,
        // providers or dependencies and are not used for a transaction.
        let output = read_with("-Si", &targets, cancelled, false, read)?;
        let info = info_records(&output, false)?;
        let mut seen = BTreeSet::new();
        for mut record in info {
            check_cancelled(cancelled)?;
            let Some(candidate) = batch.iter().find(|candidate| {
                candidate.name == record.name && candidate.repository == record.repository
            }) else {
                return Err(metadata_error("unexpected repository info target"));
            };
            if !seen.insert(record.name.clone()) {
                return Err(metadata_error("duplicate repository info target"));
            }
            if record.version != candidate.version {
                return Err(ManagementError::Conflict(
                    "The cached package version changed; refresh the package list before continuing".into(),
                ));
            }
            if let Some(local) = installed_by_name.get(record.name.as_str()) {
                record.installed_version = Some(local.version.clone());
                record
                    .details
                    .push(("Installed architecture".into(), local.architecture.clone()));
            }
            available.push(record);
        }
        if seen.len() != batch.len() {
            return Err(metadata_error(
                "repository info is missing a selected package",
            ));
        }
    }
    retain_matches(&mut available, target, &filter);
    check_cancelled(cancelled)?;
    Ok(available)
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), ManagementError> {
    if cancelled.load(Ordering::Acquire) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}

fn check_size(output: &str) -> Result<(), ManagementError> {
    if output.len() > MAX_QUERY_BYTES {
        Err(metadata_error("metadata exceeds the supported query size"))
    } else {
        Ok(())
    }
}

fn metadata_error(message: &str) -> ManagementError {
    ManagementError::Failed(format!("Unrecognized pacman metadata: {message}"))
}

fn package_name(name: &str) -> Result<(), ManagementError> {
    validate_target(name, PackageBackend::Pacman)
        .map_err(|_| metadata_error("invalid package name"))
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1_024
        && !value.chars().any(char::is_whitespace)
        && !value.chars().any(char::is_control)
}

fn repository_name(name: &str) -> bool {
    token(name) && name.len() <= 256 && !name.contains('/') && name != "." && name != ".."
}

/// Parse only the sections and canonical Usage values in pacman-conf's fully
/// resolved, verbose dump. Other named directives are irrelevant to this guard.
/// -Quq does not honor repository Upgrade eligibility and even an unfiltered
/// -Ss can include Search-disabled repositories. Trust shared candidates only
/// when Search, Install and Upgrade are all enabled in every configured repo.
fn supported_repositories(output: &str) -> Result<BTreeSet<String>, ManagementError> {
    check_size(output)?;
    let mut options = false;
    let mut current: Option<String> = None;
    let mut repositories = BTreeMap::<String, BTreeSet<String>>::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        if line.starts_with('[') {
            let section = line
                .strip_prefix('[')
                .and_then(|name| name.strip_suffix(']'))
                .ok_or_else(|| metadata_error("invalid pacman-conf section"))?;
            if section == "options" && !options && repositories.is_empty() {
                options = true;
                current = None;
            } else {
                if !options
                    || section == "options"
                    || section == "local"
                    || !repository_name(section)
                {
                    return Err(metadata_error("invalid pacman-conf repository section"));
                }
                if repositories
                    .insert(section.into(), BTreeSet::new())
                    .is_some()
                {
                    return Err(metadata_error("duplicate pacman-conf repository"));
                }
                current = Some(section.into());
            }
        } else if !options {
            return Err(metadata_error("pacman-conf is missing its options section"));
        } else if line.starts_with("Usage") {
            let role = line
                .strip_prefix("Usage = ")
                .filter(|role| matches!(*role, "All" | "Sync" | "Search" | "Install" | "Upgrade"))
                .ok_or_else(|| metadata_error("invalid pacman-conf Usage value"))?;
            let roles = current
                .as_ref()
                .and_then(|name| repositories.get_mut(name))
                .ok_or_else(|| metadata_error("pacman-conf Usage without a repository"))?;
            if !roles.insert(role.into()) {
                return Err(metadata_error("duplicate pacman-conf Usage role"));
            }
        }
    }
    if !options {
        return Err(metadata_error("pacman-conf is missing its options section"));
    }
    for (name, roles) in &repositories {
        if roles.is_empty() {
            return Err(metadata_error("pacman-conf repository is missing Usage"));
        }
        if !(roles.contains("All")
            || ["Search", "Install", "Upgrade"]
                .iter()
                .all(|role| roles.contains(*role)))
        {
            return Err(ManagementError::Unavailable(format!(
                "Unsupported pacman Usage for repository '{name}': cached package previews require Search, Install and Upgrade to all be enabled"
            )));
        }
        if roles.contains("All") && roles.len() != 1 {
            return Err(metadata_error("conflicting pacman-conf Usage roles"));
        }
    }
    Ok(repositories.into_keys().collect())
}

fn retain_matches(records: &mut Vec<PackageRecord>, target: Option<&str>, filter: &str) {
    let filter = filter.to_lowercase();
    records.retain(|record| {
        target.is_none_or(|name| record.name == name)
            && literal_match(&record.name, &record.description, &filter)
    });
}

fn literal_match(name: &str, description: &str, filter: &str) -> bool {
    filter.is_empty()
        || format!("{name} {description}")
            .to_lowercase()
            .contains(filter)
}

#[derive(Debug, PartialEq, Eq)]
struct RepositorySummary {
    name: String,
    repository: String,
    version: String,
    description: String,
}

/// -Ss emits repo/name version [optional groups/installed marker], followed by
/// an indented description. Preserve emission order for repository priority.
fn repository_summaries(output: &str) -> Result<Vec<RepositorySummary>, ManagementError> {
    check_size(output)?;
    let mut summaries = Vec::new();
    let mut current: Option<RepositorySummary> = None;
    let mut has_description = false;
    for line in output.lines() {
        if line.is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            let record = current
                .as_mut()
                .ok_or_else(|| metadata_error("search description without a package"))?;
            if has_description && !record.description.is_empty() {
                record.description.push(' ');
            }
            record.description.push_str(line.trim());
            has_description = true;
            continue;
        }
        if let Some(record) = current.take() {
            if !has_description {
                return Err(metadata_error("search package is missing its description"));
            }
            summaries.push(record);
        }
        let mut fields = line.split_whitespace();
        let (repository, name) = fields
            .next()
            .and_then(|target| target.split_once('/'))
            .ok_or_else(|| metadata_error("invalid repository search header"))?;
        if !repository_name(repository) {
            return Err(metadata_error("invalid search repository name"));
        }
        package_name(name)?;
        let version = fields
            .next()
            .filter(|version| token(version))
            .ok_or_else(|| metadata_error("search package is missing its version"))?;
        if fields
            .next()
            .is_some_and(|field| !field.starts_with(['(', '[']))
        {
            return Err(metadata_error("invalid repository search annotation"));
        }
        current = Some(RepositorySummary {
            name: name.into(),
            repository: repository.into(),
            version: version.into(),
            description: String::new(),
        });
        has_description = false;
    }
    if let Some(record) = current {
        if !has_description {
            return Err(metadata_error("search package is missing its description"));
        }
        summaries.push(record);
    }
    Ok(summaries)
}

/// -Quq emits just package names, one per line. Never treat an error message or
/// a changed output format as a successful empty updates list.
fn query_names(output: &str) -> Result<BTreeSet<String>, ManagementError> {
    check_size(output)?;
    let mut names = BTreeSet::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        package_name(line)?;
        if !names.insert(line.into()) {
            return Err(metadata_error("duplicate package in updates query"));
        }
    }
    Ok(names)
}

/// -Qi/-Si use named fields and indent wrapped continuations. A continuation
/// can contain a colon (notably an optional dependency), so it is handled
/// before looking for a field separator. Required identity fields are strict.
fn info_records(output: &str, local: bool) -> Result<Vec<PackageRecord>, ManagementError> {
    check_size(output)?;
    let mut records = Vec::new();
    let mut fields = Vec::<(String, String)>::new();
    for line in output.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if !fields.is_empty() {
                records.push(info_record(&fields, local)?);
                fields.clear();
            }
        } else if line.starts_with([' ', '\t']) {
            let (key, value) = fields
                .last_mut()
                .ok_or_else(|| metadata_error("info continuation without a field"))?;
            value.push(if key == "Description" { ' ' } else { '\n' });
            value.push_str(line.trim());
        } else {
            let (key, value) = line
                .split_once(':')
                .ok_or_else(|| metadata_error("invalid info field"))?;
            let key = key.trim_end();
            if key.is_empty()
                || !key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-'))
                || fields.iter().any(|(old, _)| old == key)
            {
                return Err(metadata_error("invalid or duplicate info field"));
            }
            fields.push((key.into(), value.trim().into()));
        }
    }
    Ok(records)
}

fn info_record(fields: &[(String, String)], local: bool) -> Result<PackageRecord, ManagementError> {
    let field = |key: &str| {
        fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
            .ok_or_else(|| metadata_error(&format!("info is missing {key}")))
    };
    let name = field("Name")?;
    package_name(&name)?;
    let version = field("Version")?;
    let architecture = field("Architecture")?;
    if !token(&version) || !token(&architecture) {
        return Err(metadata_error("invalid info version or architecture"));
    }
    let description = field("Description")?;
    let repository = if local {
        String::new()
    } else {
        let repository = field("Repository")?;
        if !repository_name(&repository) {
            return Err(metadata_error("invalid info repository name"));
        }
        repository
    };
    Ok(PackageRecord {
        name,
        architecture,
        version: version.clone(),
        summary: description.lines().next().unwrap_or("").into(),
        description,
        repository,
        installed_version: local.then_some(version),
        details: fields
            .iter()
            .filter(|(key, _)| {
                !matches!(
                    key.as_str(),
                    "Name" | "Version" | "Description" | "Repository"
                )
            })
            .cloned()
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    const INSTALLED: &str = include_str!("../../../tests/fixtures/pacman/installed-info.txt");
    const SEARCH: &str = include_str!("../../../tests/fixtures/pacman/repository-search.txt");
    const BASH_INFO: &str = include_str!("../../../tests/fixtures/pacman/bash-sync-info.txt");
    const DUPLICATE_INFO: &str =
        include_str!("../../../tests/fixtures/pacman/duplicate-sync-info.txt");
    const LITERAL_INFO: &str = include_str!("../../../tests/fixtures/pacman/literal-sync-info.txt");
    const REAL_INSTALLED: &str =
        include_str!("../../../tests/fixtures/pacman/real-installed-info.txt");
    const REAL_SEARCH: &str =
        include_str!("../../../tests/fixtures/pacman/real-repository-search.txt");
    const REAL_INFO: &str = include_str!("../../../tests/fixtures/pacman/real-sync-info.txt");
    const CONFIG: &str = include_str!("../../../tests/fixtures/pacman/resolved-config.txt");
    const REAL_CONFIG: &str =
        include_str!("../../../tests/fixtures/pacman/real-resolved-config.txt");
    const REAL_INCLUDED_CONFIG: &str =
        include_str!("../../../tests/fixtures/pacman/real-included-config.txt");

    struct Expected {
        program: String,
        args: Vec<String>,
        allow_empty: bool,
        output: ReadResult,
    }

    struct FixtureReader {
        expected: VecDeque<Expected>,
    }

    impl FixtureReader {
        fn new(expected: Vec<Expected>) -> Self {
            Self {
                expected: expected.into(),
            }
        }

        fn read(
            &mut self,
            program: &str,
            args: &[String],
            _: &AtomicBool,
            allow_empty: bool,
        ) -> ReadResult {
            let expected = self
                .expected
                .pop_front()
                .expect("unexpected pacman command");
            assert!(
                matches!(program, PROGRAM | CONFIG_PROGRAM),
                "queries must use a fixed official executable"
            );
            assert_eq!(program, expected.program);
            assert_eq!(args, expected.args);
            assert_eq!(allow_empty, expected.allow_empty);
            expected.output
        }

        fn finish(self) {
            assert!(
                self.expected.is_empty(),
                "an expected pacman command was not run"
            );
        }
    }

    fn expected(operation: &str, targets: &[&str], output: &str) -> Expected {
        let mut args = vec![operation.into(), "--color".into(), "never".into()];
        if !targets.is_empty() {
            args.push("--".into());
            args.extend(targets.iter().map(|value| (*value).into()));
        }
        Expected {
            program: PROGRAM.into(),
            args,
            allow_empty: operation != "-Si",
            output: Ok(output.into()),
        }
    }

    fn expected_config(output: &str) -> Expected {
        Expected {
            program: CONFIG_PROGRAM.into(),
            args: vec!["--verbose".into()],
            allow_empty: false,
            output: Ok(output.into()),
        }
    }

    fn records_from_fixture(
        scope: &str,
        target: Option<&str>,
        filter: &str,
        mut expected: Vec<Expected>,
    ) -> Result<Vec<PackageRecord>, ManagementError> {
        if expected.len() > 1 && expected[1].program != CONFIG_PROGRAM {
            expected.insert(1, expected_config(CONFIG));
        }
        let mut reader = FixtureReader::new(expected);
        let result = records_with(
            scope,
            target,
            filter,
            &AtomicBool::new(false),
            &mut |program, args, cancelled, allow_empty| {
                reader.read(program, args, cancelled, allow_empty)
            },
        );
        reader.finish();
        result
    }

    #[test]
    fn installed_info_preserves_epoch_pkgrel_architecture_and_dependency_details() {
        let items = info_records(INSTALLED, true).unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].name, "bash");
        assert_eq!(items[0].version, "2:5.2.026-3");
        assert_eq!(items[0].installed_version.as_deref(), Some("2:5.2.026-3"));
        assert_eq!(items[0].architecture, "x86_64");
        assert!(items[0].description.contains("command language"));
        assert!(items[0].details.contains(&("Optional Deps".into(), "bash-completion: completion for interactive shells [installed]\nreadline: editing support".into())));
        assert!(
            items[0]
                .details
                .contains(&("Install Reason".into(), "Explicitly installed".into()))
        );
        assert_eq!(items[1].architecture, "any");
        assert_eq!(items[2].architecture, "aarch64");
        assert!(items.iter().all(|item| item.repository.is_empty()));
    }

    #[test]
    fn real_pacman_captures_preserve_utf8_wrapping_and_colons() {
        let local = info_records(REAL_INSTALLED, true).unwrap();
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].name, "dupe");
        assert_eq!(local[0].version, "1:1.0-2");
        assert!(local[0].description.contains("UTF-8 café; detailed"));
        assert!(local[0].description.contains("punctuation [.]+$"));
        assert!(
            local[0]
                .details
                .iter()
                .any(|(key, value)| key == "Optional Deps"
                    && value.contains("optional-one: supports the first extra\ncapability")
                    && value.contains("optional-two: supports a long optional feature"))
        );
        assert!(local[0].details.iter().any(|(key, value)| key == "Packager"
            && value.contains("Isolated Test Builder\n<builder@example.invalid>")));
        let candidates = repository_summaries(REAL_SEARCH).unwrap();
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].repository, "first");
        assert_eq!(candidates[1].repository, "second");
        assert!(candidates[0].description.contains("UTF-8 café; detailed"));
        let items = records_from_fixture(
            "search",
            Some("dupe"),
            "UTF-8 CAFÉ",
            vec![
                expected("-Qi", &[], REAL_INSTALLED),
                expected("-Ss", &[], REAL_SEARCH),
                expected("-Si", &["first/dupe"], REAL_INFO),
            ],
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].version, "1:1.1-1");
        assert_eq!(items[0].installed_version.as_deref(), Some("1:1.0-2"));
        assert_eq!(items[0].repository, "first");
    }

    #[test]
    fn resolved_config_accepts_default_all_and_matching_explicit_roles() {
        assert_eq!(supported_repositories(CONFIG).unwrap().len(), 5);
        assert_eq!(
            supported_repositories(REAL_CONFIG).unwrap(),
            BTreeSet::from(["first".into(), "second".into()])
        );
        assert!(matches!(
            supported_repositories(REAL_INCLUDED_CONFIG),
            Err(ManagementError::Unavailable(_))
        ));
        let config = "[options]\nArchitecture = aarch64\n[custom]\nUsage = Search\nUsage = Install\nUsage = Upgrade\nServer = https://example.invalid/custom\n";
        assert_eq!(
            supported_repositories(config).unwrap(),
            BTreeSet::from(["custom".into()])
        );
        assert!(
            supported_repositories("[options]\nArchitecture = x86_64\n")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn restricted_usage_blocks_previews_but_preserves_local_details_and_removal() {
        for roles in [
            "Sync\nUsage = Search",
            "Sync",
            "Search",
            "Install\nUsage = Upgrade",
            "Sync\nUsage = Search\nUsage = Install",
            "Upgrade",
        ] {
            let config = format!("[options]\n[first]\nUsage = {roles}\n[second]\nUsage = All\n");
            assert!(matches!(
                supported_repositories(&config),
                Err(ManagementError::Unavailable(_))
            ));
            for scope in ["search", "updates"] {
                let error = records_from_fixture(
                    scope,
                    None,
                    "",
                    vec![expected("-Qi", &[], INSTALLED), expected_config(&config)],
                )
                .unwrap_err();
                assert!(error.to_string().contains("Unsupported pacman Usage"));
            }
            let local = records_from_fixture(
                "installed",
                Some("bash"),
                "",
                vec![expected("-Qi", &[], INSTALLED), expected_config(&config)],
            )
            .unwrap();
            assert_eq!(local.len(), 1);
            assert!(local[0].repository.is_empty());
            assert_eq!(local[0].installed_version.as_deref(), Some("2:5.2.026-3"));
            assert!(
                local[0]
                    .details
                    .iter()
                    .any(|(key, value)| key == "Repository availability"
                        && value.contains("Unsupported pacman Usage"))
            );
        }
    }

    #[test]
    fn missing_configuration_helper_does_not_guess_repository_eligibility() {
        let mut config = expected_config("");
        config.output = Err(ManagementError::Unavailable(
            "Could not run /usr/bin/pacman-conf".into(),
        ));
        assert!(matches!(
            records_from_fixture(
                "search",
                None,
                "",
                vec![expected("-Qi", &[], INSTALLED), config]
            ),
            Err(ManagementError::Unavailable(_))
        ));
        let mut config = expected_config("");
        config.output = Err(ManagementError::Unavailable(
            "Could not run /usr/bin/pacman-conf".into(),
        ));
        let local = records_from_fixture(
            "installed",
            None,
            "",
            vec![expected("-Qi", &[], INSTALLED), config],
        )
        .unwrap();
        assert_eq!(local.len(), 3);
        assert!(local.iter().all(|item| item.repository.is_empty()));
    }

    #[test]
    fn malformed_resolved_usage_is_not_a_supported_configuration() {
        for config in [
            "",
            "unexpected output",
            "[first]\nUsage = All\n",
            "[options]\n[first]\n",
            "[options]\n[first]\nUsage = Unknown\n",
            "[options]\nUsage = All\n",
            "[options]\n[first]\nUsage = All\nUsage = Install\n",
            "[options]\n[first]\nUsage = All\n[first]\nUsage = All\n",
            "[options]\n[../first]\nUsage = All\n",
        ] {
            assert!(
                supported_repositories(config).is_err(),
                "accepted {config:?}"
            );
        }
        let changed = "[options]\n[other]\nUsage = All\n";
        assert!(matches!(
            records_from_fixture(
                "search",
                None,
                "",
                vec![
                    expected("-Qi", &[], INSTALLED),
                    expected_config(changed),
                    expected("-Ss", &[], SEARCH)
                ]
            ),
            Err(ManagementError::Conflict(_))
        ));
    }

    #[test]
    fn installed_identity_reads_need_only_the_local_database() {
        let mut reader = FixtureReader::new(vec![expected("-Qi", &[], INSTALLED)]);
        let items = installed_with(
            &AtomicBool::new(false),
            &mut |program, args, cancelled, allow_empty| {
                reader.read(program, args, cancelled, allow_empty)
            },
        )
        .unwrap();
        assert_eq!(items.len(), 3);
        reader.finish();
    }

    #[test]
    fn native_and_foreign_installed_details_survive_missing_repository_candidates() {
        let items = records_from_fixture(
            "installed",
            None,
            "",
            vec![
                expected("-Qi", &[], INSTALLED),
                expected("-Ss", &[], SEARCH),
            ],
        )
        .unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].repository, "core");
        assert_eq!(items[0].version, "2:5.2.026-3");
        assert!(
            items[0]
                .details
                .contains(&("Cached repository version".into(), "2:5.2.037-1".into()))
        );
        assert_eq!(items[1].repository, "custom-first");
        assert!(items[2].repository.is_empty());
        assert!(items[2].details.iter().any(
            |(key, text)| key == "Repository availability" && text.contains("Foreign package")
        ));
        let target = records_from_fixture(
            "installed",
            Some("local-only"),
            "",
            vec![
                expected("-Qi", &[], INSTALLED),
                expected("-Ss", &[], SEARCH),
            ],
        )
        .unwrap();
        assert_eq!(target.len(), 1);
        assert_eq!(target[0].name, "local-only");
        assert!(
            target[0]
                .description
                .contains("outside configured repositories")
        );
    }

    #[test]
    fn repository_priority_wins_over_newer_duplicate_versions() {
        let items = records_from_fixture(
            "search",
            Some("duplicate"),
            "",
            vec![
                expected("-Qi", &[], INSTALLED),
                expected("-Ss", &[], SEARCH),
                expected("-Si", &["custom-first/duplicate"], DUPLICATE_INFO),
            ],
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].repository, "custom-first");
        assert_eq!(items[0].version, "1.0-2");
        assert_eq!(items[0].architecture, "any");
        assert_eq!(items[0].installed_version.as_deref(), Some("1.0-1"));
    }

    #[test]
    fn lower_priority_duplicate_description_cannot_select_another_candidate() {
        let items = records_from_fixture(
            "search",
            None,
            "only in lower priority",
            vec![
                expected("-Qi", &[], INSTALLED),
                expected("-Ss", &[], SEARCH),
            ],
        )
        .unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn search_is_literal_case_insensitive_and_includes_wrapped_description() {
        let items = records_from_fixture(
            "search",
            None,
            "C++ [DEMO].*",
            vec![
                expected("-Qi", &[], INSTALLED),
                expected("-Ss", &[], SEARCH),
                expected("-Si", &["extra/search-literal"], LITERAL_INFO),
            ],
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "search-literal");
        assert_eq!(items[0].architecture, "any");
        assert_eq!(items[0].installed_version, None);
        let items = records_from_fixture(
            "installed",
            None,
            "COMMAND LANGUAGE",
            vec![
                expected("-Qi", &[], INSTALLED),
                expected("-Ss", &[], SEARCH),
            ],
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "bash");
        let items = records_from_fixture(
            "search",
            None,
            "shell",
            vec![
                expected("-Qi", &[], INSTALLED),
                expected("-Ss", &[], SEARCH),
                expected("-Si", &["core/bash"], BASH_INFO),
            ],
        )
        .unwrap();
        assert_eq!(items[0].name, "bash");
    }

    #[test]
    fn updates_use_cached_pacman_version_comparison_and_exact_repository_metadata() {
        let items = records_from_fixture(
            "updates",
            None,
            "",
            vec![
                expected("-Qi", &[], INSTALLED),
                expected("-Ss", &[], SEARCH),
                expected("-Quq", &[], "bash\n"),
                expected("-Si", &["core/bash"], BASH_INFO),
            ],
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].version, "2:5.2.037-1");
        assert_eq!(items[0].installed_version.as_deref(), Some("2:5.2.026-3"));
        assert_eq!(items[0].repository, "core");
    }

    #[test]
    fn empty_updates_do_not_request_repository_details() {
        let items = records_from_fixture(
            "updates",
            None,
            "",
            vec![
                expected("-Qi", &[], INSTALLED),
                expected("-Ss", &[], SEARCH),
                expected("-Quq", &[], ""),
            ],
        )
        .unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn empty_database_and_no_search_matches_are_valid() {
        let items = records_from_fixture(
            "search",
            None,
            "not-present",
            vec![expected("-Qi", &[], ""), expected("-Ss", &[], SEARCH)],
        )
        .unwrap();
        assert!(items.is_empty());
        let items = records_from_fixture(
            "installed",
            None,
            "",
            vec![expected("-Qi", &[], ""), expected("-Ss", &[], "")],
        )
        .unwrap();
        assert!(items.is_empty());
        let items = records_from_fixture(
            "search",
            Some("not-present"),
            "",
            vec![
                expected("-Qi", &[], INSTALLED),
                expected("-Ss", &[], SEARCH),
            ],
        )
        .unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn target_and_filter_are_checked_before_queries() {
        for target in [
            "--help",
            "/tmp/package",
            "core/bash",
            "bash*",
            "bash>=1",
            "bash:x86_64",
        ] {
            assert!(matches!(
                records_from_fixture("search", Some(target), "", vec![]),
                Err(ManagementError::InvalidInput(_))
            ));
        }
        for filter in ["\n".into(), "x".repeat(513)] {
            assert!(matches!(
                records_from_fixture("search", None, &filter, vec![]),
                Err(ManagementError::InvalidInput(_))
            ));
        }
        assert!(matches!(
            records_from_fixture("unsupported", None, "", vec![]),
            Err(ManagementError::InvalidInput(_))
        ));
    }

    #[test]
    fn malformed_info_never_becomes_an_empty_or_incomplete_record() {
        for output in [
            "error: package database unavailable",
            "Name : foo\nVersion : 1\nDescription : missing architecture\n",
            "Name : foo\nName : bar\n",
            "   continuation without field\n",
            "Name : /tmp/package\nVersion : 1\nArchitecture : any\nDescription : invalid target\n",
            "Name : foo\nVersion : 1 2\nArchitecture : any\nDescription : invalid version\n",
        ] {
            assert!(info_records(output, true).is_err(), "accepted {output:?}");
        }
        assert!(info_records(INSTALLED, false).is_err());
        assert!(info_records(&format!("{INSTALLED}\n{INSTALLED}"), true).is_ok());
        let result = records_from_fixture(
            "installed",
            None,
            "",
            vec![expected("-Qi", &[], &format!("{INSTALLED}\n{INSTALLED}"))],
        );
        assert!(
            result.is_err(),
            "duplicate local identities must be rejected"
        );
    }

    #[test]
    fn malformed_search_and_update_output_are_failures() {
        for output in [
            "error: repository unavailable",
            "    description without package\n",
            "core/bash\n    missing version\n",
            "core/bash 1.0\n",
            "core/--help 1.0\n    option\n",
            "core/bash 1.0 unexpected\n    invalid annotation\n",
        ] {
            assert!(repository_summaries(output).is_err(), "accepted {output:?}");
        }
        assert!(query_names("bash 1 -> 2\n").is_err());
        assert!(query_names("bash\nbash\n").is_err());
        assert!(query_names("error: unavailable\n").is_err());
    }

    #[test]
    fn unavailable_cache_preserves_local_details_without_claiming_foreign_source() {
        let mut cache = expected("-Ss", &[], "");
        cache.output = Err(ManagementError::Failed("sync database is missing".into()));
        let items = records_from_fixture(
            "installed",
            Some("bash"),
            "",
            vec![expected("-Qi", &[], INSTALLED), cache],
        )
        .unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].repository.is_empty());
        assert!(
            items[0]
                .details
                .iter()
                .any(|(key, value)| key == "Repository availability"
                    && value.contains("sync database is missing"))
        );
        assert!(
            !items[0]
                .details
                .iter()
                .any(|(_, value)| value.contains("Foreign package"))
        );
    }

    #[test]
    fn repository_failures_and_cancellation_are_not_empty_search_results() {
        for error in [
            ManagementError::Failed("metadata unavailable".into()),
            ManagementError::Cancelled,
        ] {
            let mut cache = expected("-Ss", &[], "");
            cache.output = Err(error.clone());
            assert_eq!(
                records_from_fixture(
                    "search",
                    None,
                    "",
                    vec![expected("-Qi", &[], INSTALLED), cache]
                ),
                Err(error)
            );
        }
        let mut cache = expected("-Ss", &[], "");
        cache.output = Err(ManagementError::Cancelled);
        assert_eq!(
            records_from_fixture(
                "installed",
                None,
                "",
                vec![expected("-Qi", &[], INSTALLED), cache]
            ),
            Err(ManagementError::Cancelled)
        );
        let mut calls = 0;
        let result = installed_with(&AtomicBool::new(true), &mut |_, _, _, _| {
            calls += 1;
            Ok(String::new())
        });
        assert_eq!(result, Err(ManagementError::Cancelled));
        assert_eq!(calls, 0);
    }

    #[test]
    fn cancellation_after_a_read_stops_subsequent_queries() {
        let cancelled = AtomicBool::new(false);
        let mut calls = 0;
        let result = records_with("search", None, "", &cancelled, &mut |_, _, _, _| {
            calls += 1;
            cancelled.store(true, Ordering::Release);
            Ok(INSTALLED.into())
        });
        assert_eq!(result, Err(ManagementError::Cancelled));
        assert_eq!(calls, 1);
    }

    #[test]
    fn stale_missing_or_unexpected_candidates_require_a_fresh_list() {
        let stale = BASH_INFO.replace("2:5.2.037-1", "3:5.3-1");
        assert!(matches!(
            records_from_fixture(
                "search",
                Some("bash"),
                "",
                vec![
                    expected("-Qi", &[], INSTALLED),
                    expected("-Ss", &[], SEARCH),
                    expected("-Si", &["core/bash"], &stale)
                ]
            ),
            Err(ManagementError::Conflict(_))
        ));
        assert!(
            records_from_fixture(
                "search",
                Some("bash"),
                "",
                vec![
                    expected("-Qi", &[], INSTALLED),
                    expected("-Ss", &[], SEARCH),
                    expected("-Si", &["core/bash"], "")
                ]
            )
            .is_err()
        );
        assert!(
            records_from_fixture(
                "search",
                Some("bash"),
                "",
                vec![
                    expected("-Qi", &[], INSTALLED),
                    expected("-Ss", &[], SEARCH),
                    expected("-Si", &["core/bash"], DUPLICATE_INFO)
                ]
            )
            .is_err()
        );
        assert!(matches!(
            records_from_fixture(
                "updates",
                None,
                "",
                vec![
                    expected("-Qi", &[], INSTALLED),
                    expected("-Ss", &[], SEARCH),
                    expected("-Quq", &[], "local-only\n")
                ]
            ),
            Err(ManagementError::Conflict(_))
        ));
    }

    #[test]
    fn oversized_metadata_is_rejected_even_by_an_injected_reader() {
        let oversized = "x".repeat(MAX_QUERY_BYTES + 1);
        let mut reader = FixtureReader::new(vec![expected("-Qi", &[], &oversized)]);
        assert!(
            installed_with(
                &AtomicBool::new(false),
                &mut |program, args, cancelled, allow_empty| reader.read(
                    program,
                    args,
                    cancelled,
                    allow_empty
                )
            )
            .is_err()
        );
        reader.finish();
    }

    #[test]
    fn large_search_limits_detail_queries_to_two_thousand_and_one_rows_in_batches() {
        let mut search = String::new();
        for index in 0..2_005 {
            search.push_str(&format!(
                "extra/pkg{index:04} 1.0-1\n    Description {index}\n"
            ));
        }
        let mut expected_reads = vec![expected("-Qi", &[], ""), expected("-Ss", &[], &search)];
        for start in (0..MAX_ROWS + 1).step_by(INFO_BATCH_SIZE) {
            let end = (start + INFO_BATCH_SIZE).min(MAX_ROWS + 1);
            let targets: Vec<_> = (start..end)
                .map(|index| format!("extra/pkg{index:04}"))
                .collect();
            let mut info = String::new();
            for index in start..end {
                info.push_str(&format!("Repository : extra\nName : pkg{index:04}\nVersion : 1.0-1\nArchitecture : any\nDescription : Description {index}\n\n"));
            }
            expected_reads.push(expected(
                "-Si",
                &targets.iter().map(String::as_str).collect::<Vec<_>>(),
                &info,
            ));
        }
        let items = records_from_fixture("search", None, "", expected_reads).unwrap();
        assert_eq!(items.len(), MAX_ROWS + 1);
        assert_eq!(items.last().unwrap().name, "pkg2000");
    }

    #[test]
    #[ignore = "read-only live pacman metadata check; run explicitly on an Arch-family host"]
    fn live_pacman_installed_search_updates_and_details() {
        let cancelled = AtomicBool::new(false);
        let local = installed(&cancelled).unwrap();
        assert!(!local.is_empty());
        assert!(
            local
                .iter()
                .all(|item| item.installed_version.is_some() && !item.architecture.is_empty())
        );
        let items = records("installed", None, "", &cancelled).unwrap();
        assert_eq!(items.len(), local.len());
        let native = items
            .iter()
            .find(|item| !item.repository.is_empty())
            .expect("live fixture needs a package in the sync cache");
        let details = records("installed", Some(&native.name), "", &cancelled).unwrap();
        assert_eq!(details.len(), 1);
        assert!(!details[0].description.is_empty());
        assert!(
            details[0]
                .details
                .iter()
                .any(|(key, _)| key == "Architecture")
        );
        let search = records("search", Some(&native.name), "", &cancelled).unwrap();
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].name, native.name);
        assert!(!search[0].version.is_empty());
        assert_eq!(search[0].installed_version, native.installed_version);
        let all = records("search", None, "", &cancelled).unwrap();
        assert!(!all.is_empty() && all.len() <= MAX_ROWS + 1);
        let updates = records("updates", None, "", &cancelled).unwrap();
        assert!(updates.iter().all(|item| item.installed_version.is_some()
            && !item.version.is_empty()
            && !item.repository.is_empty()));
    }
}
