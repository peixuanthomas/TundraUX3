use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, CString};
use std::fs;
use std::os::unix::ffi::OsStrExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Provider {
    pub unit: String,
    pub active: bool,
    pub enabled: bool,
    chrony: bool,
}

fn choose(providers: Vec<Provider>) -> Result<Provider, ManagementError> {
    let active: Vec<_> = providers
        .iter()
        .filter(|provider| provider.active)
        .cloned()
        .collect();
    match (active.len(), providers.len()) {
        (1, _) => Ok(active[0].clone()),
        (0, 1) => Ok(providers[0].clone()),
        _ => Err(ManagementError::Unavailable(i18n::tr!(
            "system-settings-provider-ambiguous"
        ))),
    }
}
pub(super) fn detect(
    connection: &Connection,
    preferred: Option<&str>,
) -> Result<Provider, ManagementError> {
    let manager = systemd(connection)?;
    let files: Vec<(String, String)> = manager.call("ListUnitFiles", &()).map_err(dbus_error)?;
    let mut providers = BTreeMap::new();
    let mut names: BTreeSet<String> = [
        "systemd-timesyncd.service",
        "chrony.service",
        "chronyd.service",
        "ntp.service",
        "ntpd.service",
        "ntpsec.service",
        "openntpd.service",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    for directory in [
        "/etc/systemd/ntp-units.d",
        "/run/systemd/ntp-units.d",
        "/usr/local/lib/systemd/ntp-units.d",
        "/usr/lib/systemd/ntp-units.d",
    ] {
        if !Path::new(directory).exists() {
            continue;
        }
        for path in fs::read_dir(directory).map_err(io_error)? {
            let path = path.map_err(io_error)?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "list")
            {
                for name in fs::read_to_string(path)
                    .map_err(io_error)?
                    .lines()
                    .map(str::trim)
                    .filter(|name| !name.is_empty() && !name.starts_with('#'))
                {
                    names.insert(name.into());
                }
            }
        }
    }
    for name in &names {
        if !files.iter().any(|(file, state)| {
            Path::new(file)
                .file_name()
                .is_some_and(|file| file == name.as_str())
                && state != "masked"
        }) {
            continue;
        }
        let path: zbus::zvariant::OwnedObjectPath = manager
            .call("LoadUnit", &(name.as_str(),))
            .map_err(dbus_error)?;
        let unit = Proxy::new(
            connection,
            "org.freedesktop.systemd1",
            path.as_str(),
            "org.freedesktop.systemd1.Unit",
        )
        .map_err(dbus_error)?;
        let id: String = unit.get_property("Id").map_err(dbus_error)?;
        let active: String = unit.get_property("ActiveState").map_err(dbus_error)?;
        let enabled: String = manager
            .call("GetUnitFileState", &(id.as_str(),))
            .map_err(dbus_error)?;
        providers.insert(
            id.clone(),
            Provider {
                chrony: id != "systemd-timesyncd.service",
                unit: id,
                active: matches!(active.as_str(), "active" | "activating" | "reloading"),
                enabled: matches!(enabled.as_str(), "enabled" | "enabled-runtime"),
            },
        );
    }
    let providers: Vec<_> = providers.into_values().collect();
    let any_active = providers.iter().any(|provider| provider.active);
    if !any_active
        && timedate(connection)?
            .get_property::<bool>("NTP")
            .map_err(dbus_error)?
    {
        return Err(ManagementError::Unavailable(i18n::tr!(
            "system-settings-provider-ambiguous"
        )));
    }
    let provider = if !any_active && preferred.is_some() {
        providers
            .iter()
            .find(|provider| Some(provider.unit.as_str()) == preferred)
            .cloned()
            .ok_or_else(invalid)?
    } else {
        choose(providers)?
    };
    if !matches!(
        provider.unit.as_str(),
        "systemd-timesyncd.service" | "chrony.service" | "chronyd.service"
    ) {
        return Err(ManagementError::Unavailable(i18n::tr!(
            "system-settings-provider-ambiguous"
        )));
    }
    Ok(provider)
}

#[derive(Debug, Clone)]
struct Source {
    path: PathBuf,
    line: usize,
    kind: String,
    address: String,
    tokens: Vec<String>,
    dynamic: bool,
}
#[derive(Debug, Clone)]
pub(super) struct Configuration {
    main: PathBuf,
    documents: BTreeMap<PathBuf, ConfigDocument>,
    sources: Vec<Source>,
    fallback: Vec<String>,
    fallback_explicit: bool,
    selected: String,
    chrony: bool,
}
impl Configuration {
    pub fn versions(&self) -> BTreeMap<PathBuf, String> {
        self.documents
            .iter()
            .map(|(path, file)| (path.clone(), file.version.clone()))
            .collect()
    }
    pub fn fields(&self) -> Vec<ManagementField> {
        let mut fields: Vec<_> = self
            .sources
            .iter()
            .filter(|source| !source.dynamic)
            .enumerate()
            .map(|(index, source)| ManagementField {
                id: format!("source_{index}"),
                label: format!(
                    "{} {}",
                    i18n::tr!("system-settings-server-address"),
                    source.kind
                ),
                value: source.address.clone(),
                ..Default::default()
            })
            .collect();
        fields.push(ManagementField {
            id: "add_servers".into(),
            label: i18n::tr!("system-settings-add-servers"),
            ..Default::default()
        });
        if !self.chrony {
            fields.push(ManagementField {
                id: "fallback_servers".into(),
                label: i18n::tr!("system-settings-fallback-servers"),
                value: self.fallback.join(" "),
                ..Default::default()
            });
        }
        fields
    }
    pub fn details(&self) -> Vec<(String, String)> {
        vec![
            (
                i18n::tr!("system-settings-static-servers"),
                self.sources
                    .iter()
                    .filter(|source| !source.dynamic)
                    .map(|source| source.address.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            (
                i18n::tr!("system-settings-dynamic-servers"),
                self.sources
                    .iter()
                    .filter(|source| source.dynamic)
                    .map(|source| source.address.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            (
                i18n::tr!("system-settings-current-server"),
                if self.selected.is_empty() {
                    i18n::tr!("system-settings-unknown")
                } else {
                    self.selected.clone()
                },
            ),
            (
                i18n::tr!("system-settings-fallback-servers"),
                self.fallback.join(", "),
            ),
        ]
    }
    fn candidates(
        &self,
        values: &BTreeMap<String, String>,
    ) -> Result<Vec<ConfigCandidate>, ManagementError> {
        let mut documents: BTreeMap<_, _> = self
            .documents
            .iter()
            .map(|(path, document)| (path.clone(), document.content.clone()))
            .collect();
        let mut addresses = Vec::new();
        let mut edits: BTreeMap<PathBuf, BTreeMap<usize, String>> = BTreeMap::new();
        for (index, source) in self
            .sources
            .iter()
            .filter(|source| !source.dynamic)
            .enumerate()
        {
            let address = values
                .get(&format!("source_{index}"))
                .ok_or_else(invalid)?
                .trim();
            if !address.is_empty() {
                validate_address(address)?;
                addresses.push(address.to_string());
            }
            if self.chrony && address != source.address {
                let text = if address.is_empty() {
                    String::new()
                } else {
                    let mut tokens = source.tokens.clone();
                    tokens[1] = address.into();
                    tokens.join(" ")
                };
                edits
                    .entry(source.path.clone())
                    .or_default()
                    .insert(source.line, text);
            }
        }
        let added = parse_addresses(values.get("add_servers").map(String::as_str).unwrap_or(""))?;
        if self.chrony {
            for (path, lines) in edits {
                let original = documents.get(&path).ok_or_else(invalid)?;
                let mut updated = String::new();
                for (index, line) in original.lines().enumerate() {
                    let line = lines.get(&index).map(String::as_str).unwrap_or(line);
                    if !line.is_empty() || !lines.contains_key(&index) {
                        updated.push_str(line);
                        updated.push('\n');
                    }
                }
                documents.insert(path, updated);
            }
            if !added.is_empty() {
                let main = documents.get_mut(&self.main).ok_or_else(invalid)?;
                if !main.ends_with('\n') {
                    main.push('\n');
                }
                for address in added {
                    main.push_str(&format!("server {address}\n"));
                }
            }
        } else {
            addresses.extend(added);
            let fallback = parse_addresses(
                values
                    .get("fallback_servers")
                    .map(String::as_str)
                    .unwrap_or(""),
            )?;
            let mut content = format!("[Time]\nNTP=\nNTP={}\n", addresses.join(" "));
            if self.fallback_explicit || fallback != self.fallback {
                content.push_str(&format!(
                    "FallbackNTP=\nFallbackNTP={}\n",
                    fallback.join(" ")
                ));
            }
            documents.insert(self.main.clone(), content);
        }
        Ok(documents
            .into_iter()
            .map(|(path, content)| ConfigCandidate {
                expected_version: self.documents[&path].version.clone(),
                path,
                content,
            })
            .collect())
    }
}
fn validate_address(value: &str) -> Result<(), ManagementError> {
    if value.parse::<std::net::IpAddr>().is_ok() {
        return Ok(());
    }
    if value.len() > 253
        || value.is_empty()
        || !value.trim_end_matches('.').split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
    {
        return Err(ManagementError::InvalidInput(i18n::tr!(
            "system-settings-address-invalid"
        )));
    }
    Ok(())
}
fn parse_addresses(value: &str) -> Result<Vec<String>, ManagementError> {
    value
        .split_whitespace()
        .map(|value| {
            validate_address(value)?;
            Ok(value.into())
        })
        .collect()
}
fn glob(pattern: &Path) -> Result<Vec<PathBuf>, ManagementError> {
    let pattern = CString::new(pattern.as_os_str().as_bytes()).map_err(error)?;
    let mut found: libc::glob_t = unsafe { std::mem::zeroed() };
    let status = unsafe { libc::glob(pattern.as_ptr(), libc::GLOB_ERR, None, &mut found) };
    if status != 0 {
        unsafe { libc::globfree(&mut found) };
        return Err(ManagementError::Unavailable(i18n::tr!(
            "system-settings-config-missing"
        )));
    }
    let paths = (0..found.gl_pathc)
        .map(|index| {
            PathBuf::from(std::ffi::OsStr::from_bytes(
                unsafe { CStr::from_ptr(*found.gl_pathv.add(index)) }.to_bytes(),
            ))
        })
        .collect();
    unsafe { libc::globfree(&mut found) };
    Ok(paths)
}
fn source_paths(tokens: &[String]) -> Result<Vec<PathBuf>, ManagementError> {
    if tokens[0] == "include" {
        let mut paths = Vec::new();
        for pattern in &tokens[1..] {
            paths.extend(glob(Path::new(pattern))?);
        }
        return Ok(paths);
    }
    let suffix = if tokens[0] == "sourcedir" {
        ".sources"
    } else {
        ".conf"
    };
    let mut paths = BTreeMap::new();
    for directory in &tokens[1..] {
        let directory = directory
            .strip_prefix("/var/run/")
            .map(|tail| format!("/run/{tail}"))
            .unwrap_or_else(|| directory.clone());
        if !Path::new(&directory).exists() {
            continue;
        }
        for file in fs::read_dir(&directory).map_err(io_error)? {
            let file = file.map_err(io_error)?;
            if file.file_name().to_string_lossy().ends_with(suffix) {
                paths.entry(file.file_name()).or_insert(file.path());
            }
        }
    }
    Ok(paths.into_values().collect())
}
fn read_chrony(
    path: &Path,
    configuration: &mut Configuration,
    visiting: &mut BTreeSet<PathBuf>,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    check(flag)?;
    if !visiting.insert(path.into()) {
        return Err(ManagementError::Unavailable(i18n::tr!(
            "system-settings-config-cycle"
        )));
    }
    if configuration.documents.contains_key(path) {
        visiting.remove(path);
        return Ok(());
    }
    let file = super::super::system_config::read(path)?;
    for (index, line) in file.content.lines().enumerate() {
        let text = line.trim();
        if text.is_empty() || text.starts_with(['#', '!', ';', '%']) {
            continue;
        }
        let tokens = shlex::split(text).ok_or_else(invalid)?;
        let Some(kind) = tokens.first() else {
            continue;
        };
        if matches!(kind.as_str(), "server" | "pool" | "peer") {
            let address = tokens.get(1).ok_or_else(invalid)?.clone();
            let dynamic = path.starts_with("/run") || path.starts_with("/var/run");
            configuration.sources.push(Source {
                path: path.into(),
                line: index,
                kind: kind.clone(),
                address,
                tokens,
                dynamic,
            });
        } else if matches!(kind.as_str(), "include" | "confdir" | "sourcedir") {
            for child in source_paths(&tokens)? {
                read_chrony(&child, configuration, visiting, flag)?;
            }
        }
    }
    configuration.documents.insert(path.into(), file);
    visiting.remove(path);
    Ok(())
}
fn chrony_main(provider: &Provider, connection: &Connection) -> Result<PathBuf, ManagementError> {
    let manager = systemd(connection)?;
    let path: zbus::zvariant::OwnedObjectPath = manager
        .call("LoadUnit", &(provider.unit.as_str(),))
        .map_err(dbus_error)?;
    let service = Proxy::new(
        connection,
        "org.freedesktop.systemd1",
        path.as_str(),
        "org.freedesktop.systemd1.Service",
    )
    .map_err(dbus_error)?;
    let pid: u32 = service.get_property("MainPID").map_err(dbus_error)?;
    let args = if pid != 0 {
        let bytes = fs::read(format!("/proc/{pid}/cmdline")).map_err(io_error)?;
        bytes
            .split(|b| *b == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect::<Vec<_>>()
    } else {
        type StartCommand = (String, Vec<String>, bool, u64, u64, u64, u64, u32, i32, i32);
        let commands: Vec<StartCommand> = service.get_property("ExecStart").map_err(dbus_error)?;
        if commands.len() != 1 || commands[0].1.iter().any(|argument| argument.contains('$')) {
            return Err(ManagementError::Unavailable(i18n::tr!(
                "system-settings-config-missing"
            )));
        }
        commands[0].1.clone()
    };
    if args.iter().any(|arg| arg == "-x") {
        return Err(ManagementError::Unavailable(i18n::tr!(
            "system-settings-clock-disabled"
        )));
    }
    if let Some(index) = args.iter().position(|arg| arg == "-f") {
        return args.get(index + 1).map(PathBuf::from).ok_or_else(invalid);
    }
    let main: Vec<_> = ["/etc/chrony/chrony.conf", "/etc/chrony.conf"]
        .into_iter()
        .filter(|path| Path::new(path).is_file())
        .collect();
    if main.len() != 1 {
        return Err(ManagementError::Unavailable(i18n::tr!(
            "system-settings-config-missing"
        )));
    }
    Ok(main[0].into())
}
fn timesync_servers(output: &str) -> Result<(Vec<String>, Vec<String>), ManagementError> {
    let mut ntp = Vec::new();
    let mut fallback = Vec::new();
    let mut time = false;
    for line in output.lines().map(str::trim) {
        if line.starts_with('#') || line.starts_with(';') || line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            time = line == "[Time]";
            continue;
        }
        if !time {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let list = match key.trim() {
                "NTP" => &mut ntp,
                "FallbackNTP" => &mut fallback,
                _ => continue,
            };
            if value.trim().is_empty() {
                list.clear();
            } else {
                list.extend(parse_addresses(value.trim())?);
            }
        }
    }
    Ok((ntp, fallback))
}
pub(super) fn configuration(
    provider: &Provider,
    connection: &Connection,
    flag: &AtomicBool,
) -> Result<Configuration, ManagementError> {
    if provider.chrony {
        let main = chrony_main(provider, connection)?;
        if !main.starts_with("/etc") {
            return Err(ManagementError::Unavailable(i18n::tr!(
                "system-settings-config-external"
            )));
        }
        let mut config = Configuration {
            main: main.clone(),
            documents: BTreeMap::new(),
            sources: vec![],
            fallback: vec![],
            fallback_explicit: false,
            selected: String::new(),
            chrony: true,
        };
        read_chrony(&main, &mut config, &mut BTreeSet::new(), flag)?;
        if config
            .sources
            .iter()
            .any(|source| !source.dynamic && !source.path.starts_with("/etc"))
        {
            return Err(ManagementError::Unavailable(i18n::tr!(
                "system-settings-config-external"
            )));
        }
        if provider.active {
            if let Ok(output) = native(
                if Path::new("/usr/bin/chronyc").is_file() {
                    "/usr/bin/chronyc"
                } else {
                    "/usr/sbin/chronyc"
                },
                &["-c", "-n", "sources"],
                flag,
            ) {
                config.selected = output
                    .lines()
                    .find_map(|line| {
                        let fields: Vec<_> = line.split(',').collect();
                        (fields.get(1) == Some(&"*"))
                            .then(|| fields.get(2).copied().unwrap_or("").to_string())
                    })
                    .unwrap_or_default();
            }
        }
        Ok(config)
    } else {
        let main = PathBuf::from("/etc/systemd/timesyncd.conf.d/90-tundraux3.conf");
        let output = native(
            "/usr/bin/systemd-analyze",
            &["cat-config", "systemd/timesyncd.conf"],
            flag,
        )?;
        let (servers, mut fallback) = timesync_servers(&output)?;
        let mut sources: Vec<_> = servers
            .into_iter()
            .enumerate()
            .map(|(line, address)| Source {
                path: main.clone(),
                line,
                kind: "NTP".into(),
                address,
                tokens: vec![],
                dynamic: false,
            })
            .collect();
        let mut selected = String::new();
        if provider.active {
            let timesync = proxy(
                connection,
                "org.freedesktop.timesync1",
                "/org/freedesktop/timesync1",
                "org.freedesktop.timesync1.Manager",
            )?;
            let links: Vec<String> = timesync
                .get_property("LinkNTPServers")
                .map_err(dbus_error)?;
            sources.extend(links.into_iter().map(|address| Source {
                path: PathBuf::new(),
                line: 0,
                kind: "NTP".into(),
                address,
                tokens: vec![],
                dynamic: true,
            }));
            selected = timesync.get_property("ServerName").unwrap_or_default();
            if fallback.is_empty() {
                fallback = timesync
                    .get_property("FallbackNTPServers")
                    .unwrap_or_default();
            }
        }
        let mut documents = BTreeMap::new();
        for path in output.lines().filter_map(|line| line.strip_prefix("# /")) {
            let path = PathBuf::from(format!("/{path}"));
            if path.is_file() {
                documents.insert(path.clone(), super::super::system_config::read(&path)?);
            }
        }
        documents.insert(main.clone(), super::super::system_config::read(&main)?);
        Ok(Configuration {
            main,
            documents,
            sources,
            fallback,
            fallback_explicit: output.lines().any(|line| {
                line.split_once('=')
                    .is_some_and(|(key, _)| key.trim() == "FallbackNTP")
            }),
            selected,
            chrony: false,
        })
    }
}
pub(super) fn set_enabled(
    command: &ManagementCommand,
    connection: &Connection,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    let before = detect(connection, command.values.get("unit").map(String::as_str))?;
    if command.values.get("unit") != Some(&before.unit)
        || command.values.get("was_enabled") != Some(&before.enabled.to_string())
        || command.values.get("was_active") != Some(&before.active.to_string())
    {
        return Err(ManagementError::Conflict(i18n::tr!(
            "platform-management-review-again"
        )));
    }
    let enabled = match command.values.get("enabled").map(String::as_str) {
        Some("true") => true,
        Some("false") => false,
        _ => return Err(invalid()),
    };
    super::super::configuration::confirm_preview(
        "system-settings-ntp",
        &i18n::tr!("system-settings-review"),
        &[format!(
            "{}: {} → {} ({})",
            i18n::tr!("system-settings-automatic"),
            bool_label(before.active || before.enabled),
            bool_label(enabled),
            before.unit
        )],
        io,
    )?;
    if detect(connection, command.values.get("unit").map(String::as_str))? != before {
        return Err(ManagementError::Conflict(i18n::tr!(
            "platform-management-review-again"
        )));
    }
    native(
        "/usr/bin/systemctl",
        &[
            if enabled { "enable" } else { "disable" },
            "--now",
            "--",
            &before.unit,
        ],
        flag,
    )?;
    let after = detect(connection, command.values.get("unit").map(String::as_str))?;
    if after.unit != before.unit || after.active != enabled || after.enabled != enabled {
        return Err(error(i18n::tr!("system-settings-verify-failed")));
    }
    Ok(())
}
pub(super) fn save_servers(
    command: &ManagementCommand,
    connection: &Connection,
    io: &mut dyn OperationInteraction,
    flag: &AtomicBool,
) -> Result<(), ManagementError> {
    let before = detect(connection, command.values.get("unit").map(String::as_str))?;
    if command.values.get("unit") != Some(&before.unit) {
        return Err(ManagementError::Conflict(i18n::tr!(
            "platform-management-review-again"
        )));
    }
    let config = configuration(&before, connection, flag)?;
    let versions: BTreeMap<PathBuf, String> =
        serde_json::from_str(command.values.get("versions").ok_or_else(invalid)?).map_err(error)?;
    if versions != config.versions() {
        return Err(ManagementError::Conflict(i18n::tr!(
            "platform-management-review-again"
        )));
    }
    let candidates = config.candidates(&command.values)?;
    let mut preview = vec![i18n::tr!("system-settings-servers-help")];
    for candidate in &candidates {
        let original = &config.documents[&candidate.path];
        if candidate.content != original.content {
            preview.push(format!(
                "{}\n{}\n{}\n{}\n{}",
                candidate.path.display(),
                i18n::tr!("system-settings-before"),
                original.content,
                i18n::tr!("system-settings-after"),
                candidate.content
            ));
        }
    }
    super::super::configuration::confirm_preview(
        "system-settings-servers",
        &i18n::tr!("system-settings-review"),
        &preview,
        io,
    )?;
    if detect(connection, command.values.get("unit").map(String::as_str))? != before
        || configuration(&before, connection, flag)?.versions() != versions
    {
        return Err(ManagementError::Conflict(i18n::tr!(
            "platform-management-review-again"
        )));
    }
    let check_files: Vec<_> = candidates
        .iter()
        .filter(|file| file.path.starts_with("/etc"))
        .cloned()
        .collect();
    let (checked, output) = super::super::system_config::check_batch(
        &config.main,
        &check_files,
        if config.chrony { "chrony" } else { "timesyncd" },
        flag,
    )?;
    if checked != ConfigCheck::Passed {
        return Err(error(i18n::tr!(
            "system-settings-check-failed",
            reason = format!("{checked:?}")
        )));
    }
    if !config.chrony {
        let expected = timesync_servers(
            &candidates
                .iter()
                .find(|file| file.path == config.main)
                .ok_or_else(invalid)?
                .content,
        )?;
        if timesync_servers(&output)? != expected {
            return Err(ManagementError::Conflict(i18n::tr!(
                "system-settings-override-conflict"
            )));
        }
    }
    let backup = super::super::system_config::install_batch_reviewed(&candidates, flag)?;
    io.emit(OperationEvent::Output {
        text: i18n::tr!("system-settings-config-saved", backup = &backup),
    });
    if before.active && !backup.is_empty() {
        if let Err(reason) = native("/usr/bin/systemctl", &["restart", "--", &before.unit], flag) {
            return Err(error(i18n::tr!(
                "system-settings-reload-failed",
                reason = reason.to_string(),
                backup = &backup
            )));
        }
        let loaded = detect(connection, Some(&before.unit))?;
        if loaded.unit != before.unit || !loaded.active {
            return Err(error(i18n::tr!(
                "system-settings-reload-failed",
                reason = i18n::tr!("system-settings-service-inactive"),
                backup = &backup
            )));
        }
        io.emit(OperationEvent::Output {
            text: i18n::tr!("system-settings-service-loaded"),
        });
    }
    let after = state(connection)?;
    io.emit(OperationEvent::Output {
        text: i18n::tr!(
            "system-settings-sync-result",
            synchronized = bool_label(after.synchronized)
        ),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chrony_includes_and_directory_precedence_are_read_from_real_files() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        fs::write(first.join("servers.conf"), "server first.example iburst\n").unwrap();
        fs::write(second.join("servers.conf"), "server ignored.example\n").unwrap();
        fs::write(second.join("extra.conf"), "pool extra.example\n").unwrap();
        let main = temp.path().join("chrony.conf");
        fs::write(
            &main,
            format!("confdir {} {}\n", first.display(), second.display()),
        )
        .unwrap();
        let mut config = Configuration {
            main: main.clone(),
            documents: BTreeMap::new(),
            sources: vec![],
            fallback: vec![],
            fallback_explicit: false,
            selected: String::new(),
            chrony: true,
        };
        read_chrony(
            &main,
            &mut config,
            &mut BTreeSet::new(),
            &AtomicBool::new(false),
        )
        .unwrap();
        let addresses: BTreeSet<_> = config
            .sources
            .iter()
            .map(|source| source.address.as_str())
            .collect();
        assert_eq!(
            addresses,
            BTreeSet::from(["first.example", "extra.example"])
        );
        assert_eq!(config.documents.len(), 3);
        fs::write(&main, format!("include {}\n", main.display())).unwrap();
        config.documents.clear();
        assert!(
            read_chrony(
                &main,
                &mut config,
                &mut BTreeSet::new(),
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
    #[test]
    fn source_addresses_reject_configuration_injection() {
        for address in ["pool.ntp.org", "192.0.2.1", "2001:db8::1"] {
            assert!(validate_address(address).is_ok());
        }
        for address in [
            "https://pool.ntp.org",
            "host\nserver evil",
            ";server",
            "-option",
            "host/path",
        ] {
            assert!(validate_address(address).is_err());
        }
    }
    #[test]
    fn provider_selection_never_switches_an_active_daemon_or_picks_a_conflict() {
        let timesync = Provider {
            unit: "systemd-timesyncd.service".into(),
            active: false,
            enabled: false,
            chrony: false,
        };
        let chrony = Provider {
            unit: "chrony.service".into(),
            active: true,
            enabled: true,
            chrony: true,
        };
        assert_eq!(
            choose(vec![timesync.clone(), chrony.clone()]).unwrap(),
            chrony
        );
        let mut active = timesync.clone();
        active.active = true;
        assert!(choose(vec![active, chrony]).is_err());
        assert_eq!(choose(vec![timesync.clone()]).unwrap(), timesync);
        assert!(choose(vec![]).is_err());
    }
    #[test]
    fn timesync_list_reset_and_configuration_order_are_respected() {
        let text = "[Time]\nNTP=old.example\nNTP=\nNTP=new.example\nFallbackNTP=fallback.example\n[Other]\nNTP=ignored.example\n";
        assert_eq!(
            timesync_servers(text).unwrap(),
            (vec!["new.example".into()], vec!["fallback.example".into()])
        );
    }
    #[test]
    fn chrony_edits_preserve_source_types_options_and_unrelated_lines() {
        let temp = tempfile::tempdir().unwrap();
        let main = temp.path().join("chrony.conf");
        fs::write(&main, "pool old.example iburst nts\nmakestep 1 3\n").unwrap();
        let document = super::super::super::system_config::read(&main).unwrap();
        let config = Configuration {
            main: main.clone(),
            documents: BTreeMap::from([(main.clone(), document)]),
            sources: vec![Source {
                path: main,
                line: 0,
                kind: "pool".into(),
                address: "old.example".into(),
                tokens: vec![
                    "pool".into(),
                    "old.example".into(),
                    "iburst".into(),
                    "nts".into(),
                ],
                dynamic: false,
            }],
            fallback: vec![],
            fallback_explicit: false,
            selected: String::new(),
            chrony: true,
        };
        let candidates = config
            .candidates(&BTreeMap::from([
                ("source_0".into(), "new.example".into()),
                ("add_servers".into(), "extra.example".into()),
            ]))
            .unwrap();
        assert_eq!(
            candidates[0].content,
            "pool new.example iburst nts\nmakestep 1 3\nserver extra.example\n"
        );
    }
}
