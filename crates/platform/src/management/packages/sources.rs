//! Software source changes are candidates for the controlled editor, never direct writes.
use super::{PackageBackend, detect_backend, status};
use crate::management::{
    ConfigDraft, ManagementAction, ManagementCommand, ManagementError, ManagementField,
    ManagementQuery, ManagementRow, ManagementSnapshot,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_BYTES: usize = 1024 * 1024;
const DISABLED: &str = "# tundra-disabled: ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    AptList,
    Deb822,
    Dnf,
    Pacman,
}

#[derive(Debug, Clone)]
struct Source {
    name: String,
    enabled: bool,
    start: usize,
    end: usize,
    format: Format,
}

fn format(path: &Path) -> Result<Format, ManagementError> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("list") => Ok(Format::AptList),
        Some("sources") => Ok(Format::Deb822),
        Some("repo") => Ok(Format::Dnf),
        _ if path.file_name().is_some_and(|name| name == "pacman.conf")
            || path.starts_with("/etc/pacman.d") =>
        {
            Ok(Format::Pacman)
        }
        _ => Err(ManagementError::InvalidInput(
            "Choose an APT .list/.sources, DNF .repo or pacman configuration file".into(),
        )),
    }
}

fn content_format(path: &Path, text: &str) -> Result<Format, ManagementError> {
    // Include files are allowed outside /etc/pacman.d and may have no extension.
    // Their native Server/Include directives identify the repository format.
    if text.lines().any(|line| {
        let line = line.trim().strip_prefix(DISABLED).unwrap_or(line.trim());
        line.split_once('=')
            .is_some_and(|(key, _)| matches!(key.trim(), "Server" | "Include"))
    }) {
        Ok(Format::Pacman)
    } else {
        format(path)
    }
}

fn read(path: &Path) -> Result<String, ManagementError> {
    let metadata = fs::metadata(path).map_err(|error| source_io(path, error))?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES as u64 {
        return Err(ManagementError::InvalidInput(
            "Source file must be a text file smaller than 1 MiB".into(),
        ));
    }
    fs::read_to_string(path).map_err(|error| source_io(path, error))
}

fn source_io(path: &Path, error: io::Error) -> ManagementError {
    let message = format!("Cannot read {}: {error}", path.display());
    if error.kind() == io::ErrorKind::PermissionDenied {
        ManagementError::PermissionDenied(message)
    } else {
        ManagementError::Unavailable(message)
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), ManagementError> {
    if cancelled.load(Ordering::Acquire) {
        Err(ManagementError::Cancelled)
    } else {
        Ok(())
    }
}

fn source_paths(
    backend: PackageBackend,
    cancelled: &AtomicBool,
) -> Result<(Vec<PathBuf>, Vec<String>), ManagementError> {
    let mut paths = Vec::new();
    let mut notices = Vec::new();
    match backend {
        PackageBackend::Apt => {
            let main = Path::new("/etc/apt/sources.list");
            if main.is_file() {
                paths.push(main.to_path_buf());
            }
            collect_directory(
                Path::new("/etc/apt/sources.list.d"),
                &["list", "sources"],
                &mut paths,
                &mut notices,
            )?;
        }
        PackageBackend::Dnf4 | PackageBackend::Dnf5 => collect_directory(
            Path::new("/etc/yum.repos.d"),
            &["repo"],
            &mut paths,
            &mut notices,
        )?,
        PackageBackend::Pacman => {
            let mut pending = vec![PathBuf::from("/etc/pacman.conf")];
            let mut visited = BTreeSet::new();
            while let Some(path) = pending.pop() {
                check_cancelled(cancelled)?;
                if !visited.insert(path.clone()) {
                    continue;
                }
                if visited.len() > 256 {
                    notices.push(
                        "Include limit reached; inspect the remaining files in the editor".into(),
                    );
                    break;
                }
                match read(&path) {
                    Ok(text) => {
                        for line in text.lines() {
                            let Some((key, value)) = line.split_once('=') else {
                                continue;
                            };
                            if key.trim() != "Include" {
                                continue;
                            }
                            let pattern = value.trim();
                            let mut includes = expand_include(pattern)?;
                            includes.reverse();
                            pending.extend(includes);
                        }
                        paths.push(path);
                    }
                    Err(error) => notices.push(error.to_string()),
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    Ok((paths, notices))
}

fn collect_directory(
    directory: &Path,
    extensions: &[&str],
    paths: &mut Vec<PathBuf>,
    notices: &mut Vec<String>,
) -> Result<(), ManagementError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            notices.push(source_io(directory, error).to_string());
            return Ok(());
        }
    };
    for entry in entries {
        let entry = entry.map_err(|error| source_io(directory, error))?;
        if extensions.contains(
            &entry
                .path()
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or(""),
        ) {
            paths.push(entry.path());
        }
    }
    Ok(())
}

fn expand_include(pattern: &str) -> Result<Vec<PathBuf>, ManagementError> {
    let path = Path::new(pattern);
    if !path.is_absolute() || pattern.chars().any(char::is_control) {
        return Err(ManagementError::InvalidInput(
            "pacman Include must use an absolute path".into(),
        ));
    }
    let file = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| ManagementError::InvalidInput("pacman Include needs a file name".into()))?;
    if !file.contains(['*', '?']) {
        return Ok(if path.is_file() {
            vec![path.to_path_buf()]
        } else {
            Vec::new()
        });
    }
    let directory = path.parent().unwrap();
    if directory.to_string_lossy().contains(['*', '?']) {
        return Err(ManagementError::InvalidInput(
            "Include patterns in directory names need manual review".into(),
        ));
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| source_io(directory, error))? {
        let entry = entry.map_err(|error| source_io(directory, error))?;
        if wildcard(file.as_bytes(), entry.file_name().as_encoded_bytes()) && entry.path().is_file()
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn wildcard(pattern: &[u8], value: &[u8]) -> bool {
    let (mut p, mut v, mut star, mut retry) = (0, 0, None, 0);
    while v < value.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == value[v]) {
            p += 1;
            v += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            p += 1;
            retry = v;
        } else if let Some(index) = star {
            p = index + 1;
            retry += 1;
            v = retry;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

fn lines_with_offsets(text: &str) -> Vec<(usize, &str)> {
    let mut offset = 0;
    text.split_inclusive('\n')
        .map(|line| {
            let result = (offset, line);
            offset += line.len();
            result
        })
        .collect()
}

fn parse_sources(path: &Path, text: &str) -> Result<Vec<Source>, ManagementError> {
    let format = content_format(path, text)?;
    let lines = lines_with_offsets(text);
    let mut sources = Vec::new();
    match format {
        Format::AptList => {
            for (start, line) in lines {
                let trimmed = line.trim();
                let (enabled, entry) = if let Some(rest) = trimmed.strip_prefix('#') {
                    (false, rest.trim_start())
                } else {
                    (true, trimmed)
                };
                if entry.starts_with("deb ") || entry.starts_with("deb-src ") {
                    sources.push(Source {
                        name: entry.into(),
                        enabled,
                        start,
                        end: start + line.len(),
                        format,
                    });
                }
            }
        }
        Format::Deb822 => {
            let mut start = None;
            let mut fields = BTreeMap::new();
            for (offset, line) in lines
                .iter()
                .copied()
                .chain(std::iter::once((text.len(), "\n")))
            {
                if line.trim().is_empty() {
                    if let Some(start) = start.take() {
                        if fields.contains_key("types") || fields.contains_key("uris") {
                            sources.push(Source {
                                name: format!(
                                    "{} {}",
                                    fields.get("uris").copied().unwrap_or(""),
                                    fields.get("suites").copied().unwrap_or("")
                                ),
                                enabled: fields
                                    .get("enabled")
                                    .is_none_or(|value| !value.eq_ignore_ascii_case("no")),
                                start,
                                end: offset,
                                format,
                            });
                        }
                        fields.clear();
                    }
                } else if !line.trim_start().starts_with('#') {
                    if start.is_none() {
                        start = Some(offset);
                    }
                    if let Some((key, value)) = line.trim_end().split_once(':') {
                        fields.insert(key.trim().to_ascii_lowercase(), value.trim());
                    }
                }
            }
        }
        Format::Dnf | Format::Pacman => {
            let mut current: Option<Source> = None;
            for (offset, line) in lines
                .iter()
                .copied()
                .chain(std::iter::once((text.len(), "")))
            {
                let (disabled, body) = if let Some(body) = line.trim_start().strip_prefix(DISABLED)
                {
                    (true, body.trim())
                } else {
                    (false, line.trim())
                };
                let section = body
                    .strip_prefix('[')
                    .and_then(|body| body.strip_suffix(']'));
                if section.is_some() || offset == text.len() {
                    if let Some(mut source) = current.take() {
                        source.end = offset;
                        if source.name != "options" {
                            sources.push(source);
                        }
                    }
                    if let Some(name) = section {
                        current = Some(Source {
                            name: name.into(),
                            enabled: !disabled,
                            start: offset,
                            end: text.len(),
                            format,
                        });
                    }
                } else if format == Format::Dnf {
                    if let Some(source) = current.as_mut() {
                        if let Some((key, value)) = body.split_once('=') {
                            if key.trim() == "enabled" {
                                source.enabled =
                                    !matches!(value.trim(), "0" | "false" | "False" | "no");
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(sources)
}

pub(super) fn query(
    request: &ManagementQuery,
    backend: PackageBackend,
    cancelled: &AtomicBool,
) -> Result<ManagementSnapshot, ManagementError> {
    let (paths, mut notices) = source_paths(backend, cancelled)?;
    let mut rows = Vec::new();
    let filter = request.filter.to_ascii_lowercase();
    for path in paths {
        check_cancelled(cancelled)?;
        let text = match read(&path) {
            Ok(text) => text,
            Err(error) => {
                notices.push(error.to_string());
                continue;
            }
        };
        let sources = match parse_sources(&path, &text) {
            Ok(sources) => sources,
            Err(error) => {
                notices.push(error.to_string());
                continue;
            }
        };
        // Mirror Include files can inherit the repository section from pacman.conf.
        // Keep an editor entry even when they contain no standalone repository.
        if sources.is_empty() && backend == PackageBackend::Pacman {
            rows.push(ManagementRow {
                id: path.to_string_lossy().into_owned(),
                cells: vec![
                    "Included source file".into(),
                    "inherited".into(),
                    path.to_string_lossy().into_owned(),
                ],
                detail: vec![(
                    "Configuration file".into(),
                    path.to_string_lossy().into_owned(),
                )],
                actions: vec![edit_action(&path)],
                ..Default::default()
            });
        }
        for source in sources {
            if !filter.is_empty()
                && !format!("{} {}", source.name, path.display())
                    .to_ascii_lowercase()
                    .contains(&filter)
            {
                continue;
            }
            let mut actions = vec![edit_action(&path)];
            for (id, label) in [
                (
                    if source.enabled {
                        "source_disable"
                    } else {
                        "source_enable"
                    },
                    if source.enabled {
                        "Disable source"
                    } else {
                        "Enable source"
                    },
                ),
                ("source_remove", "Remove source"),
            ] {
                actions.push(ManagementAction {
                    id: id.into(),
                    label: label.into(),
                    group: "configuration".into(),
                    confirm: false,
                    ..Default::default()
                });
            }
            rows.push(ManagementRow {
                id: format!("{}#{}", path.display(), source.start),
                cells: vec![
                    runtime_log::sanitize_text(&source.name),
                    if source.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                    .into(),
                    path.to_string_lossy().into_owned(),
                ],
                detail: vec![
                    ("Source".into(), runtime_log::sanitize_text(&source.name)),
                    (
                        "State".into(),
                        if source.enabled {
                            "enabled"
                        } else {
                            "disabled"
                        }
                        .into(),
                    ),
                    (
                        "Configuration file".into(),
                        path.to_string_lossy().into_owned(),
                    ),
                ],
                actions,
                identity: BTreeMap::from([
                    ("source_path".into(), path.to_string_lossy().into_owned()),
                    ("source_start".into(), source.start.to_string()),
                    ("source_digest".into(), fingerprint(&text)),
                    ("backend".into(), backend.id().into()),
                ]),
                ..Default::default()
            });
        }
    }
    let mut actions = status::view_actions();
    actions.insert(0, add_action(backend));
    Ok(ManagementSnapshot {
        columns: vec!["Source".into(), "State".into(), "Configuration file".into()],
        rows,
        actions,
        notices,
        backend: backend.id().into(),
    })
}

fn edit_action(path: &Path) -> ManagementAction {
    ManagementAction {
        id: "edit_package_source".into(),
        label: "Edit source file".into(),
        primary: true,
        group: "configuration".into(),
        values: BTreeMap::from([
            ("path".into(), path.to_string_lossy().into_owned()),
            ("validator".into(), "sources".into()),
        ]),
        ..Default::default()
    }
}

fn field(id: &str, label: &str, value: &str, required: bool) -> ManagementField {
    ManagementField {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        required,
        ..Default::default()
    }
}

fn add_action(backend: PackageBackend) -> ManagementAction {
    let mut fields = vec![
        field("source_name", "Source name", "", true),
        field("url", "Repository URL", "https://", true),
    ];
    if backend == PackageBackend::Apt {
        fields.push(field("suite", "Suite", "", true));
        fields.push(field("components", "Components", "main", true));
        fields.push(field("signed_by", "Signing key file", "", true));
    } else if matches!(backend, PackageBackend::Dnf4 | PackageBackend::Dnf5) {
        fields.push(field("gpgkey", "Signing key URL", "https://", true));
    }
    ManagementAction {
        id: "source_add".into(),
        label: "Add software source".into(),
        primary: true,
        fields,
        group: "configuration".into(),
        values: BTreeMap::from([("backend".into(), backend.id().into())]),
        ..Default::default()
    }
}

fn fingerprint(text: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

pub fn prepare_config_draft(
    command: &ManagementCommand,
    cancelled: &AtomicBool,
) -> Result<ConfigDraft, ManagementError> {
    check_cancelled(cancelled)?;
    let backend = detect_backend(cancelled)?;
    if command
        .identity
        .get("backend")
        .or_else(|| command.values.get("backend"))
        .is_some_and(|expected| expected != backend.id())
    {
        return Err(ManagementError::Conflict(
            "The package backend changed; refresh and try again".into(),
        ));
    }
    let (path, content, expected_content) = if command.action == "source_add" {
        new_source(command, backend)?
    } else {
        if !matches!(
            command.action.as_str(),
            "source_enable" | "source_disable" | "source_remove"
        ) {
            return Err(ManagementError::InvalidInput(
                "Unknown source draft operation".into(),
            ));
        }
        let path = command
            .identity
            .get("source_path")
            .map(PathBuf::from)
            .ok_or_else(|| {
                ManagementError::InvalidInput("Choose a software source first".into())
            })?;
        let (paths, _) = source_paths(backend, cancelled)?;
        if !paths.contains(&path) {
            return Err(ManagementError::Conflict(
                "The source file is no longer configured; refresh".into(),
            ));
        }
        let text = read(&path)?;
        if command.identity.get("source_digest") != Some(&fingerprint(&text)) {
            return Err(ManagementError::Conflict(
                "The source file changed; refresh before editing".into(),
            ));
        }
        let start = command
            .identity
            .get("source_start")
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| ManagementError::InvalidInput("Source identity is incomplete".into()))?;
        let source = parse_sources(&path, &text)?
            .into_iter()
            .find(|source| source.start == start)
            .ok_or_else(|| ManagementError::Conflict("The source entry changed; refresh".into()))?;
        let content = transform(&text, &source, &command.action)?;
        (path, content, Some(text))
    };
    validate_source_config(&path, &content)?;
    check_cancelled(cancelled)?;
    Ok(ConfigDraft {
        path,
        content,
        validator: "sources".into(),
        service: None,
        scope: "system".into(),
        expected_content,
    })
}

fn transform(text: &str, source: &Source, action: &str) -> Result<String, ManagementError> {
    let old = text
        .get(source.start..source.end)
        .ok_or_else(|| ManagementError::Conflict("Source range changed".into()))?;
    let new = if action == "source_remove" {
        // Comments are not source properties; retain them even when an entry is removed.
        old.split_inclusive('\n')
            .filter(|line| {
                line.trim_start().starts_with('#') && !line.trim_start().starts_with(DISABLED)
            })
            .collect::<String>()
    } else {
        let enabled = action == "source_enable";
        match source.format {
            Format::AptList => {
                if enabled {
                    let indent = old.len() - old.trim_start().len();
                    format!(
                        "{}{}",
                        &old[..indent],
                        old[indent..]
                            .strip_prefix('#')
                            .unwrap_or(&old[indent..])
                            .trim_start()
                    )
                } else {
                    format!("# {old}")
                }
            }
            Format::Deb822 => {
                replace_property(old, ':', "Enabled", if enabled { "yes" } else { "no" })
            }
            Format::Dnf => replace_property(old, '=', "enabled", if enabled { "1" } else { "0" }),
            Format::Pacman => old
                .split_inclusive('\n')
                .map(|line| {
                    if enabled {
                        line.strip_prefix(DISABLED).unwrap_or(line).to_string()
                    } else {
                        format!("{DISABLED}{line}")
                    }
                })
                .collect(),
        }
    };
    let mut content = String::with_capacity(text.len() + new.len());
    content.push_str(&text[..source.start]);
    content.push_str(&new);
    content.push_str(&text[source.end..]);
    Ok(content)
}

fn replace_property(text: &str, delimiter: char, key: &str, value: &str) -> String {
    let mut replaced = false;
    let mut result = String::new();
    for line in text.split_inclusive('\n') {
        if line
            .split_once(delimiter)
            .is_some_and(|(name, _)| name.trim().eq_ignore_ascii_case(key))
        {
            if !replaced {
                result.push_str(&format!("{key}{delimiter} {value}\n"));
                replaced = true;
            }
        } else {
            result.push_str(line);
        }
    }
    if !replaced {
        if !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(&format!("{key}{delimiter} {value}\n"));
    }
    result
}

fn new_source(
    command: &ManagementCommand,
    backend: PackageBackend,
) -> Result<(PathBuf, String, Option<String>), ManagementError> {
    let value = |key: &str| command.values.get(key).map(String::as_str).unwrap_or("");
    let name = command
        .values
        .get("source_name")
        .or_else(|| command.values.get("name"))
        .map(String::as_str)
        .unwrap_or("");
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        || name.starts_with(['-', '.'])
    {
        return Err(ManagementError::InvalidInput(
            "Source name must contain letters, numbers, dots, hyphens or underscores".into(),
        ));
    }
    let url = value("url");
    validate_url(url)?;
    match backend {
        PackageBackend::Apt => {
            let suite = value("suite");
            let components = value("components");
            let key = value("signed_by");
            if suite.is_empty()
                || components.is_empty()
                || suite.split_whitespace().count() != 1
                || [suite, components]
                    .iter()
                    .any(|value| value.chars().any(char::is_control))
                || !Path::new(key).is_absolute()
                || key.chars().any(char::is_control)
            {
                return Err(ManagementError::InvalidInput(
                    "Fill in the suite, components and absolute signing-key path".into(),
                ));
            }
            let path = PathBuf::from(format!("/etc/apt/sources.list.d/{name}.sources"));
            if path.exists() {
                return Err(ManagementError::Conflict(
                    "This source file exists; edit it instead".into(),
                ));
            }
            Ok((
                path,
                format!(
                    "Types: deb\nURIs: {url}\nSuites: {suite}\nComponents: {components}\nSigned-By: {key}\nEnabled: yes\n"
                ),
                None,
            ))
        }
        PackageBackend::Dnf4 | PackageBackend::Dnf5 => {
            let gpgkey = value("gpgkey");
            validate_url(gpgkey)?;
            let path = PathBuf::from(format!("/etc/yum.repos.d/{name}.repo"));
            if path.exists() {
                return Err(ManagementError::Conflict(
                    "This source file exists; edit it instead".into(),
                ));
            }
            Ok((
                path,
                format!(
                    "[{name}]\nname={name}\nbaseurl={url}\nenabled=1\ngpgcheck=1\ngpgkey={gpgkey}\n"
                ),
                None,
            ))
        }
        PackageBackend::Pacman => {
            let path = PathBuf::from("/etc/pacman.conf");
            let original = read(&path)?;
            if parse_sources(&path, &original)?
                .iter()
                .any(|source| source.name == name)
            {
                return Err(ManagementError::Conflict(
                    "This repository exists; edit it instead".into(),
                ));
            }
            let mut content = original.clone();
            if !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(&format!(
                "\n[{name}]\nSigLevel = Required DatabaseOptional\nServer = {url}\n"
            ));
            Ok((path, content, Some(original)))
        }
    }
}

fn validate_url(url: &str) -> Result<(), ManagementError> {
    if (!url.starts_with("https://") && !url.starts_with("http://") && !url.starts_with("file:///"))
        || url.split_whitespace().count() != 1
        || url.chars().any(char::is_control)
        || url.len() > 2048
    {
        return Err(ManagementError::InvalidInput(
            "Use one http, https or local file URL without spaces".into(),
        ));
    }
    let authority = url
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or(""));
    if authority.is_some_and(|authority| authority.contains('@')) {
        return Err(ManagementError::InvalidInput(
            "Keep credentials out of the repository URL".into(),
        ));
    }
    Ok(())
}

/// Checks syntax only. It does not refresh databases, download keys or change trust.
pub fn validate_source_config(path: &Path, content: &str) -> Result<(), ManagementError> {
    if content.len() > MAX_BYTES || content.contains('\0') {
        return Err(ManagementError::InvalidInput(
            "Source file is too large or contains NUL bytes".into(),
        ));
    }
    let format = content_format(path, content)?;
    match format {
        Format::AptList => {
            for (index, line) in content.lines().enumerate() {
                let entry = line.trim();
                if entry.is_empty() || entry.starts_with('#') {
                    continue;
                }
                let tokens = apt_tokens(entry).ok_or_else(|| {
                    ManagementError::InvalidInput(format!(
                        "APT line {} has incomplete options",
                        index + 1
                    ))
                })?;
                if tokens.len() < 3
                    || !matches!(tokens[0].as_str(), "deb" | "deb-src")
                    || !tokens[1].contains(':')
                {
                    return Err(ManagementError::InvalidInput(format!(
                        "APT line {} needs a type, URI and suite",
                        index + 1
                    )));
                }
                if !tokens[2].ends_with('/') && tokens.len() < 4 {
                    return Err(ManagementError::InvalidInput(format!(
                        "APT line {} needs components",
                        index + 1
                    )));
                }
            }
        }
        Format::Deb822 => {
            let mut fields = BTreeMap::<String, String>::new();
            let mut previous = String::new();
            for line in content.lines().chain(std::iter::once("")) {
                if line.trim().is_empty() {
                    if !fields.is_empty() {
                        for key in ["types", "uris", "suites"] {
                            if fields.get(key).is_none_or(|value| value.trim().is_empty()) {
                                return Err(ManagementError::InvalidInput(format!(
                                    "APT source stanza needs {key}"
                                )));
                            }
                        }
                        if !fields["types"]
                            .split_whitespace()
                            .all(|kind| matches!(kind, "deb" | "deb-src"))
                        {
                            return Err(ManagementError::InvalidInput(
                                "APT Types must be deb or deb-src".into(),
                            ));
                        }
                        if fields["suites"]
                            .split_whitespace()
                            .any(|suite| !suite.ends_with('/'))
                            && fields
                                .get("components")
                                .is_none_or(|value| value.is_empty())
                        {
                            return Err(ManagementError::InvalidInput(
                                "APT source stanza needs Components".into(),
                            ));
                        }
                    }
                    fields.clear();
                    previous.clear();
                } else if line.trim_start().starts_with('#') {
                    continue;
                } else if line.starts_with([' ', '\t']) {
                    if previous.is_empty() {
                        return Err(ManagementError::InvalidInput(
                            "APT continuation needs a preceding field".into(),
                        ));
                    }
                    fields.get_mut(&previous).unwrap().push_str(line);
                } else {
                    let (key, value) = line.split_once(':').ok_or_else(|| {
                        ManagementError::InvalidInput("APT source fields need a colon".into())
                    })?;
                    let key = key.to_ascii_lowercase();
                    if !key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                        || key.is_empty()
                        || fields.contains_key(&key)
                    {
                        return Err(ManagementError::InvalidInput(
                            "APT source has an invalid or repeated field".into(),
                        ));
                    }
                    fields.insert(key.clone(), value.trim().into());
                    previous = key;
                }
            }
        }
        Format::Dnf | Format::Pacman => {
            // pacman Include inherits the surrounding repository section.
            let mut section = format == Format::Pacman
                && path.file_name().is_some_and(|name| name != "pacman.conf");
            let mut sections = BTreeSet::new();
            for (index, line) in content.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() || line.starts_with(['#', ';']) {
                    continue;
                }
                if line.starts_with('[') {
                    let name = line
                        .strip_prefix('[')
                        .and_then(|line| line.strip_suffix(']'))
                        .filter(|name| !name.is_empty())
                        .ok_or_else(|| {
                            ManagementError::InvalidInput(format!(
                                "Source line {} has an invalid section",
                                index + 1
                            ))
                        })?;
                    if !sections.insert(name) {
                        return Err(ManagementError::InvalidInput(format!(
                            "Duplicate source section: {name}"
                        )));
                    }
                    section = true;
                } else if !section && format == Format::Dnf {
                    return Err(ManagementError::InvalidInput(
                        "DNF properties must appear inside a repository section".into(),
                    ));
                } else if !line.contains('=')
                    && !(format == Format::Pacman
                        && line.bytes().all(|byte| byte.is_ascii_alphanumeric()))
                {
                    return Err(ManagementError::InvalidInput(format!(
                        "Source line {} needs a property assignment",
                        index + 1
                    )));
                }
            }
        }
    }
    Ok(())
}

fn apt_tokens(entry: &str) -> Option<Vec<String>> {
    let mut tokens = entry.split_whitespace();
    let kind = tokens.next()?;
    let mut token = tokens.next()?;
    if token.starts_with('[') {
        while !token.ends_with(']') {
            token = tokens.next()?;
        }
        token = tokens.next()?;
    }
    let mut result = vec![kind.into(), token.into()];
    result.extend(
        tokens
            .take_while(|token| !token.starts_with('#'))
            .map(String::from),
    );
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_changes_preserve_other_sections_comments_and_security_options() {
        for (path, text, name) in [
            (
                "/etc/apt/sources.list",
                "# note\ndeb [signed-by=/key trusted=no] https://a stable main\ndeb https://b stable main\n",
                "deb [signed-by=/key trusted=no] https://a stable main",
            ),
            (
                "/etc/apt/a.sources",
                "# note\nTypes: deb\nURIs: https://a\nSuites: stable\nComponents: main\nSigned-By: /key\n\nTypes: deb\nURIs: https://b\nSuites: stable\nComponents: main\n",
                "https://a stable",
            ),
            (
                "/etc/yum.repos.d/a.repo",
                "# note\n[a]\nbaseurl=https://a\ngpgcheck=1\n[b]\nbaseurl=https://b\n",
                "a",
            ),
            (
                "/etc/pacman.conf",
                "[options]\nSigLevel = Required\n[a]\nInclude = /etc/pacman.d/mirrorlist\n[b]\nServer = https://b/$arch\n",
                "a",
            ),
        ] {
            let path = Path::new(path);
            let sources = parse_sources(path, text).unwrap();
            let source = sources.iter().find(|source| source.name == name).unwrap();
            let disabled = transform(text, source, "source_disable").unwrap();
            assert!(disabled.contains("https://b"));
            assert!(disabled.contains("# note") || disabled.contains("SigLevel = Required"));
            validate_source_config(path, &disabled).unwrap();
            let source = parse_sources(path, &disabled)
                .unwrap()
                .into_iter()
                .find(|source| source.name == name)
                .unwrap();
            assert!(!source.enabled);
            let enabled = transform(&disabled, &source, "source_enable").unwrap();
            assert!(
                parse_sources(path, &enabled)
                    .unwrap()
                    .iter()
                    .find(|source| source.name == name)
                    .unwrap()
                    .enabled
            );
            if text.contains("gpgcheck=1") {
                assert!(enabled.contains("gpgcheck=1"));
            }
            if text.contains("trusted=no") {
                assert!(enabled.contains("trusted=no"));
            }
        }
    }
    #[test]
    fn validators_detect_broken_files_and_accept_native_formats() {
        assert!(
            validate_source_config(
                Path::new("/etc/apt/sources.list"),
                "deb [signed-by=/key https://a stable main"
            )
            .is_err()
        );
        assert!(
            validate_source_config(
                Path::new("/etc/apt/a.sources"),
                "URIs: https://a\nSuites: stable\n"
            )
            .is_err()
        );
        assert!(validate_source_config(Path::new("/etc/apt/a.sources"), "Types: deb deb-src\nURIs: https://a https://b\nSuites: stable\nComponents: main\nSigned-By: -----BEGIN PGP PUBLIC KEY BLOCK-----\n .\n key\n").is_ok());
        assert!(
            validate_source_config(Path::new("/etc/yum.repos.d/a.repo"), "[repo\nenabled=1")
                .is_err()
        );
        assert!(validate_source_config(Path::new("/etc/pacman.conf"), "[options]\nCheckSpace\nSigLevel = Required DatabaseOptional\n[core]\nInclude = /etc/pacman.d/mirrorlist\n").is_ok());
    }
    #[test]
    fn include_globs_and_candidate_versions_are_checked() {
        assert!(wildcard(b"mirror*", b"mirrorlist"));
        assert!(!wildcard(b"mirror?", b"mirrorlist"));
        assert_ne!(fingerprint("old"), fingerprint("new"));
        assert!(validate_url("https://user:secret@example.org/repo").is_err());
    }
    #[test]
    fn deb822_fields_accept_case_variants_but_reject_duplicate_names() {
        let path = Path::new("/etc/apt/example.sources");
        let content = "types: deb\nURIS: https://example.org/repo\nsuites: stable\ncomponents: main\nenabled: NO\n";
        validate_source_config(path, content).unwrap();
        let source = parse_sources(path, content).unwrap().remove(0);
        assert!(!source.enabled);
        let enabled = transform(content, &source, "source_enable").unwrap();
        assert!(parse_sources(path, &enabled).unwrap()[0].enabled);
        assert!(validate_source_config(path, &format!("{content}Types: deb-src\n")).is_err());
    }
    #[test]
    fn pacman_include_files_outside_default_directory_are_editable() {
        let path = Path::new("/usr/local/share/mirrors/custom-list");
        let text = "Server = https://mirror.example/$repo/os/$arch\n";
        assert_eq!(content_format(path, text).unwrap(), Format::Pacman);
        validate_source_config(path, text).unwrap();
    }
}
