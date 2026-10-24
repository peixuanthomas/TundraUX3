//! Windows Package Manager integration using the community winget source.
use super::{
    ExecutionContext, ManagementAction, ManagementCommand, ManagementError, ManagementKind,
    ManagementQuery, ManagementRow, ManagementSnapshot, OperationEvent, OperationInteraction,
};
use crate::{PlatformError, ProcessExit, ProcessSpec};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

const SOURCE: &str = "winget";
const NO_APPLICATIONS: u32 = 0x8a15_0014;
const NO_UPDATES: u32 = 0x8a15_002b;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    name: String,
    id: String,
    installed: String,
    available: String,
}

fn invalid() -> ManagementError {
    ManagementError::InvalidInput(i18n::tr!("management-winget-invalid"))
}

fn stale() -> ManagementError {
    ManagementError::Conflict(i18n::tr!("management-winget-stale"))
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), ManagementError> {
    if cancelled.load(Ordering::Acquire) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}

fn executable() -> Result<PathBuf, ManagementError> {
    let alias = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("Microsoft/WindowsApps/winget.exe"));
    let paths = std::env::var_os("PATH").unwrap_or_default();
    alias
        .into_iter()
        .chain(std::env::split_paths(&paths).map(|path| path.join("winget.exe")))
        .find(|path| path.is_absolute() && path.is_file())
        .ok_or_else(|| ManagementError::MissingDependency(i18n::tr!("management-winget-missing")))
}

fn spec(program: &Path, verb: &str) -> ProcessSpec {
    ProcessSpec::new(program).args([
        verb,
        "--source",
        SOURCE,
        "--disable-interactivity",
        "--no-vt",
    ])
}

fn validate_id(id: &str) -> Result<(), ManagementError> {
    if id.is_empty()
        || id.len() > 256
        || !id.as_bytes()[0].is_ascii_alphanumeric()
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_' | b'+'))
        || id.contains("...")
    {
        Err(invalid())
    } else {
        Ok(())
    }
}

fn complete_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 256
        && !version.contains(['…', '\r', '\n', '\0'])
        && !version.contains("...")
}

fn read(args: ProcessSpec, cancelled: &AtomicBool) -> Result<ProcessExit, ManagementError> {
    check_cancelled(cancelled)?;
    let started = std::time::Instant::now();
    let result = crate::native_platform().spawn_wait_cancellable(&args, &|| {
        cancelled.load(Ordering::Acquire) || started.elapsed() > std::time::Duration::from_secs(120)
    });
    check_cancelled(cancelled)?;
    result.map_err(|error| match error {
        PlatformError::Interrupted { .. } => {
            ManagementError::Failed(i18n::tr!("management-winget-timeout"))
        }
        _ => ManagementError::Failed(format!(
            "{}\n{error}",
            i18n::tr!("management-winget-failed")
        )),
    })
}

fn output_text(output: &ProcessExit) -> String {
    format!(
        "{}\n{}",
        output.stdout.utf8_lossy(),
        output.stderr.utf8_lossy()
    )
}

fn verified_output(output: &ProcessExit, scope: &str) -> Result<Option<String>, ManagementError> {
    let code = output.code.map(|code| code as u32);
    if (matches!(scope, "installed" | "search" | "updates") && code == Some(NO_APPLICATIONS))
        || (scope == "updates" && code == Some(NO_UPDATES))
    {
        return Ok(None);
    }
    if code != Some(0) {
        return Err(ManagementError::Failed(format!(
            "{} ({})\n{}",
            i18n::tr!("management-winget-failed"),
            code.map(|code| format!("0x{code:08X}"))
                .unwrap_or_else(|| "?".into()),
            output_text(output).trim()
        )));
    }
    Ok(Some(output.stdout.utf8_lossy()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Name,
    Id,
    Version,
    Available,
    Match,
    Source,
}

fn column(label: &str) -> Option<Column> {
    Some(match label {
        "Name" | "名称" => Column::Name,
        "Id" | "ID" => Column::Id,
        "Version" | "版本" => Column::Version,
        "Available" | "可用" => Column::Available,
        "Match" | "匹配" => Column::Match,
        "Source" | "源" => Column::Source,
        _ => return None,
    })
}

fn parse_error() -> ManagementError {
    ManagementError::Failed(i18n::tr!("management-winget-output"))
}

fn header(line: &str) -> Option<Vec<(Column, usize)>> {
    let mut columns = Vec::new();
    let mut offset = 0;
    for token in line.split_whitespace() {
        let index = line[offset..].find(token)? + offset;
        columns.push((column(token)?, display_width(&line[..index])));
        offset = index + token.len();
    }
    let kinds: Vec<_> = columns.iter().map(|(kind, _)| *kind).collect();
    (matches!(
        kinds.as_slice(),
        [Column::Name, Column::Id, Column::Version]
            | [
                Column::Name,
                Column::Id,
                Column::Version,
                Column::Available | Column::Match | Column::Source
            ]
            | [
                Column::Name,
                Column::Id,
                Column::Version,
                Column::Available | Column::Match,
                Column::Source
            ]
    ))
    .then_some(columns)
}

fn grapheme_width(value: &str) -> usize {
    value
        .chars()
        .next()
        .and_then(UnicodeWidthChar::width)
        .unwrap_or(1)
        .max(1)
}

fn display_width(value: &str) -> usize {
    value.graphemes(true).map(grapheme_width).sum()
}

fn cell(line: &str, start: usize, end: Option<usize>) -> Result<String, ManagementError> {
    let mut width = 0;
    let mut value = String::new();
    for grapheme in line.graphemes(true) {
        let next = width + grapheme_width(grapheme);
        if (width < start && next > start) || end.is_some_and(|end| width < end && next > end) {
            return Err(parse_error());
        }
        if width >= start && end.is_none_or(|end| width < end) {
            value.push_str(grapheme);
        }
        width = next;
    }
    Ok(value.trim().to_string())
}

fn no_results(text: &str) -> bool {
    text.lines().any(|line| {
        matches!(
            line.trim(),
            "No installed package found matching input criteria."
                | "No package found matching input criteria."
                | "No applicable upgrade found."
                | "找不到与输入条件匹配的已安装程序包。"
                | "找不到与输入条件匹配的程序包。"
                | "找不到适用的升级。"
        )
    })
}

fn table_notice(line: &str) -> bool {
    let line = line.trim();
    if matches!(
        line,
        "The following packages have an upgrade available, but require explicit targeting for upgrade:"
            | "以下程序包有可用的升级，但需要显式目标才能进行升级："
    ) {
        return true;
    }
    let count_end = line
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(0);
    count_end > 0
        && matches!(
            &line[count_end..],
            " upgrades available."
                | " 升级可用。"
                | " package(s) have version numbers that cannot be determined. Use --include-unknown to see all results."
                | " 程序包的版本号无法确定。使用 --include-unknown 查看所有结果。"
        )
}

fn parse_table(text: &str, scope: &str) -> Result<Vec<Record>, ManagementError> {
    let lines: Vec<_> = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .collect();
    let table = lines
        .windows(2)
        .position(|pair| pair[1].len() >= 3 && pair[1].bytes().all(|byte| byte == b'-'));
    let Some(table) = table else {
        return if no_results(text) {
            Ok(Vec::new())
        } else {
            Err(parse_error())
        };
    };
    // Redirected WinGet progress can overwrite its spinner with the table header.
    let heading = lines[table].rsplit('\r').next().unwrap_or_default();
    let mut columns = header(heading).ok_or_else(parse_error)?;
    let mut records = Vec::new();
    let mut ids = BTreeSet::new();
    for line in &lines[table + 2..] {
        if line.contains('\r') {
            return Err(parse_error());
        }
        if line.trim().is_empty() {
            continue;
        }
        if let Some(next_columns) = header(line) {
            columns = next_columns;
            continue;
        }
        if line.bytes().all(|byte| byte == b'-') || table_notice(line) {
            continue;
        }
        let mut record = Record {
            name: String::new(),
            id: String::new(),
            installed: String::new(),
            available: String::new(),
        };
        for (index, (kind, start)) in columns.iter().enumerate() {
            let value = cell(
                line,
                *start,
                columns.get(index + 1).map(|(_, start)| *start),
            )?;
            match kind {
                Column::Name => record.name = value,
                Column::Id => record.id = value,
                Column::Version if scope == "search" => record.available = value,
                Column::Version => record.installed = value,
                Column::Available => record.available = value,
                Column::Source if !value.is_empty() && value != SOURCE => return Err(parse_error()),
                _ => {}
            }
        }
        if record.id.is_empty() {
            // WinGet adds upgrade counts and pin notices after the table.
            if record.installed.is_empty() && record.available.is_empty() {
                continue;
            }
            return Err(parse_error());
        }
        if record.name.is_empty() || !ids.insert(record.id.to_ascii_lowercase()) {
            return Err(parse_error());
        }
        records.push(record);
    }
    Ok(records)
}

fn records(
    program: &Path,
    scope: &str,
    filter: &str,
    target: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Vec<Record>, ManagementError> {
    let mut args = spec(program, if scope == "search" { "search" } else { "list" });
    if scope == "updates" {
        args = args.arg("--upgrade-available");
    }
    if let Some(target) = target {
        validate_id(target)?;
        args = args.args(["--id", target, "--exact"]);
    } else if !filter.is_empty() {
        args = args.args(["--query", filter]);
    }
    let output = read(args, cancelled)?;
    let Some(text) = verified_output(&output, scope)? else {
        return Ok(Vec::new());
    };
    let records = parse_table(&text, scope)?;
    if target.is_some_and(|target| {
        records
            .iter()
            .any(|record| !record.id.eq_ignore_ascii_case(target))
    }) {
        return Err(parse_error());
    }
    Ok(records)
}

fn action(id: &str, key: &str, write: bool) -> ManagementAction {
    ManagementAction {
        id: id.into(),
        label: i18n::tr!(key),
        confirm: write,
        privileged: write,
        primary: matches!(id, "install" | "remove" | "upgrade"),
        group: if write { "package" } else { "view" }.into(),
        ..Default::default()
    }
}

fn row(record: Record, scope: &str) -> ManagementRow {
    let safe = validate_id(&record.id).is_ok()
        && (record.installed.is_empty() || complete_version(&record.installed))
        && (record.available.is_empty() || complete_version(&record.available));
    let mut actions = if scope == "search" {
        vec![action("install", "management-winget-install", true)]
    } else {
        let mut actions = vec![action("remove", "management-winget-remove", true)];
        if !record.available.is_empty() {
            actions.push(action("upgrade", "management-winget-upgrade", true));
        }
        actions
    };
    if !safe {
        for action in &mut actions {
            action.disabled_reason = Some(i18n::tr!("management-winget-truncated"));
        }
    }
    let identity = BTreeMap::from([
        ("backend".into(), SOURCE.into()),
        ("source".into(), SOURCE.into()),
        ("name".into(), record.id.clone()),
        ("installed_version".into(), record.installed.clone()),
        ("available_version".into(), record.available.clone()),
    ]);
    ManagementRow {
        id: record.id.clone(),
        cells: vec![
            record.name.clone(),
            record.installed.clone(),
            record.available.clone(),
            record.id.clone(),
        ],
        detail: vec![
            (i18n::tr!("management-winget-name"), record.name),
            (i18n::tr!("management-winget-id"), record.id),
            (i18n::tr!("management-winget-installed"), record.installed),
            (i18n::tr!("management-winget-available"), record.available),
        ],
        actions,
        identity,
    }
}

fn update_plan(records: &[Record]) -> String {
    let mut records: Vec<_> = records.iter().collect();
    records.sort_by(|left, right| left.id.cmp(&right.id));
    let mut digest = Sha256::new();
    for record in records {
        for value in [&record.id, &record.installed, &record.available] {
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
    }
    format!("{:x}", digest.finalize())
}

pub fn query(
    request: &ManagementQuery,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    if request.kind != ManagementKind::Packages
        || !request.options.is_empty()
        || request.filter.len() > 512
        || request.filter.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    check_cancelled(cancelled)?;
    let scope = if request.scope.is_empty() {
        "installed"
    } else {
        &request.scope
    };
    if !matches!(scope, "installed" | "updates" | "search") {
        return Err(invalid());
    }
    let program = executable()?;
    let mut snapshot = ManagementSnapshot {
        backend: SOURCE.into(),
        columns: vec![
            i18n::tr!("management-winget-name"),
            i18n::tr!("management-winget-installed"),
            i18n::tr!("management-winget-available"),
            i18n::tr!("management-winget-id"),
        ],
        actions: vec![
            action("scope_installed", "management-winget-view-installed", false),
            action("scope_updates", "management-winget-view-updates", false),
            action("scope_search", "management-winget-view-search", false),
        ],
        notices: vec![i18n::tr!("management-winget-notice")],
        ..Default::default()
    };
    if scope == "search" && request.filter.is_empty() && request.target.is_none() {
        snapshot
            .notices
            .push(i18n::tr!("management-winget-search-prompt"));
        return Ok(snapshot);
    }
    let packages = records(
        &program,
        scope,
        &request.filter,
        request.target.as_deref(),
        cancelled,
    )?;
    let plan = update_plan(&packages);
    snapshot.rows = packages
        .into_iter()
        .map(|record| row(record, scope))
        .collect();
    if scope == "updates"
        && request.filter.is_empty()
        && request.target.is_none()
        && !snapshot.rows.is_empty()
    {
        let mut upgrade = action("upgrade_all", "management-winget-upgrade-all", true);
        upgrade.values.insert("winget_plan".into(), plan);
        if snapshot
            .rows
            .iter()
            .flat_map(|row| &row.actions)
            .any(|action| action.disabled_reason.is_some())
        {
            upgrade.disabled_reason = Some(i18n::tr!("management-winget-truncated"));
        }
        snapshot.actions.push(upgrade);
    }
    if let Some(target) = request.target.as_deref() {
        if !snapshot.rows.is_empty() {
            let output = read(
                spec(&program, "show").args(["--id", target, "--exact"]),
                cancelled,
            )?;
            match verified_output(&output, "details") {
                Ok(Some(text)) => snapshot.rows[0]
                    .detail
                    .push((i18n::tr!("management-winget-details"), text)),
                Ok(None) => {}
                Err(error) => snapshot.rows[0]
                    .detail
                    .push((i18n::tr!("management-winget-details"), error.to_string())),
            }
        }
        snapshot.details_complete = true;
    }
    Ok(snapshot)
}

fn validate_identity(command: &ManagementCommand, current: &Record) -> Result<(), ManagementError> {
    if validate_id(&current.id).is_err()
        || (!current.installed.is_empty() && !complete_version(&current.installed))
        || (!current.available.is_empty() && !complete_version(&current.available))
        || command.identity.get("backend").map(String::as_str) != Some(SOURCE)
        || command.identity.get("source").map(String::as_str) != Some(SOURCE)
        || command.identity.get("name") != Some(&current.id)
        || command.identity.get("installed_version") != Some(&current.installed)
        || command.identity.get("available_version") != Some(&current.available)
    {
        return Err(stale());
    }
    Ok(())
}

fn build_command(
    program: &Path,
    command: &ManagementCommand,
) -> Result<ProcessSpec, ManagementError> {
    if command.kind != ManagementKind::Packages {
        return Err(invalid());
    }
    if command.action == "upgrade_all" {
        if command.target.is_some()
            || !command.identity.is_empty()
            || command.values.len() != 1
            || !command.values.get("winget_plan").is_some_and(|plan| {
                plan.len() == 64 && plan.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err(invalid());
        }
        return Ok(spec(program, "upgrade").arg("--all"));
    }
    if !command.values.is_empty() {
        return Err(invalid());
    }
    let verb = match command.action.as_str() {
        "install" => "install",
        "remove" => "uninstall",
        "upgrade" => "upgrade",
        _ => return Err(invalid()),
    };
    let target = command.target.as_deref().ok_or_else(invalid)?;
    validate_id(target)?;
    let mut args = spec(program, verb).args(["--id", target, "--exact"]);
    if command.action != "remove" {
        let version = command
            .identity
            .get("available_version")
            .ok_or_else(stale)?;
        if !complete_version(version) {
            return Err(stale());
        }
        args = args.args(["--version", version]);
    }
    Ok(args)
}

pub fn execute(
    command: &ManagementCommand,
    _context: &ExecutionContext,
    interaction: &mut dyn OperationInteraction,
    cancelled: &AtomicBool,
) -> Result<String, ManagementError> {
    check_cancelled(cancelled)?;
    let program = executable()?;
    let args = build_command(&program, command)?;
    if let Some(target) = command.target.as_deref() {
        let scope = match command.action.as_str() {
            "install" => "search",
            "upgrade" => "updates",
            _ => "installed",
        };
        let current = records(&program, scope, "", Some(target), cancelled)?;
        if current.len() != 1 {
            return Err(stale());
        }
        validate_identity(command, &current[0])?;
        if command.action == "install"
            && !records(&program, "installed", "", Some(target), cancelled)?.is_empty()
        {
            return Err(stale());
        }
    } else if command.action == "upgrade_all" {
        let current = records(&program, "updates", "", None, cancelled)?;
        if current.iter().any(|record| {
            validate_id(&record.id).is_err()
                || !complete_version(&record.installed)
                || !complete_version(&record.available)
        }) || command.values.get("winget_plan") != Some(&update_plan(&current))
        {
            return Err(stale());
        }
    }
    check_cancelled(cancelled)?;
    interaction.terminal_input(std::time::Duration::ZERO)?;
    interaction.emit(OperationEvent::Output {
        text: i18n::tr!("management-winget-running"),
    });
    // An AA cancellation must not interrupt an installer that is already modifying packages.
    let output = crate::native_platform()
        .spawn_streaming(&args, &mut |output| {
            interaction.emit(OperationEvent::Output { text: output.text });
        })
        .map_err(|error| {
            ManagementError::Failed(format!(
                "{}\n{error}",
                i18n::tr!("management-winget-failed")
            ))
        })?;
    verified_output(&output, "write")?;
    Ok(i18n::tr!("management-winget-completed"))
}

#[cfg(test)]
#[path = "../../tests/unit/management/winget_tests.rs"]
mod tests;
