//! Linux release downloads use the same staging, readiness and rollback as source builds.
use super::*;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    body: Option<String>,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    state: String,
}

pub(super) fn check(
    identity: &BuildIdentity,
    api_root: &str,
) -> Result<UpdateCheckResult, UpdateError> {
    if std::env::consts::ARCH != "x86_64" {
        return Err(UpdateError::new(
            "No Linux release archive is available for this architecture",
        ));
    }
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| UpdateError::new(e.to_string()))?;
    let base = format!("{api_root}/repos/{GITHUB_OWNER}/{GITHUB_REPO}");
    let release: Release = get_json(&client, &format!("{base}/releases/latest"))?;
    let payload = select_payload(&release)?;
    // target_commitish can name a moving branch. Resolve the actual release tag instead.
    let mut url = reqwest::Url::parse(&format!("{base}/commits/"))
        .map_err(|e| UpdateError::new(e.to_string()))?;
    url.path_segments_mut()
        .map_err(|_| UpdateError::new("Invalid GitHub URL"))?
        .pop_if_empty()
        .push(&release.tag_name);
    let commit: ApiCommitRef = get_json(&client, url.as_str())?;
    if !git::is_commit_sha(&commit.sha) {
        return Err(UpdateError::new("Release tag did not resolve to a commit"));
    }
    let remote = parse_version(&payload.version).map_err(|e| UpdateError::new(e.to_string()))?;
    let local =
        parse_version(&identity.package_version).map_err(|e| UpdateError::new(e.to_string()))?;
    let relation = release_relation(identity, &local, &remote, &commit.sha);
    Ok(UpdateCheckResult {
        default_branch: release.tag_name,
        head_sha: commit.sha.clone(),
        relation,
        commits: vec![UpdateCommit {
            sha: commit.sha,
            message: release.body.unwrap_or_default(),
        }],
        release: Some(payload),
    })
}

fn release_relation(
    identity: &BuildIdentity,
    local: &Version,
    remote: &Version,
    sha: &str,
) -> UpdateRelation {
    if remote > local {
        UpdateRelation::Behind { remote_ahead: 1 }
    } else if remote < local {
        UpdateRelation::Ahead { local_ahead: 1 }
    } else if identity.commit_sha.as_deref() == Some(sha) && !identity.dirty {
        UpdateRelation::Identical
    } else {
        // A beta build with the same package version can explicitly return to the release.
        UpdateRelation::Unknown
    }
}

fn select_payload(release: &Release) -> Result<ReleaseUpdate, UpdateError> {
    if release.draft || release.prerelease {
        return Err(UpdateError::new(
            "The release channel requires a published stable release",
        ));
    }
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    let parsed = parse_version(version).map_err(|_| UpdateError::new("Invalid release version"))?;
    if !parsed.pre.is_empty() || !parsed.build.is_empty() {
        return Err(UpdateError::new("Invalid stable release version"));
    }
    let names = [
        format!("TundraUX3-{}-linux-x86_64.tar.gz", release.tag_name),
        format!("tundraux3-{version}-linux-x86_64.tar.gz"),
    ];
    let archives: Vec<_> = release
        .assets
        .iter()
        .filter(|a| a.state == "uploaded" && names.contains(&a.name))
        .collect();
    if archives.len() != 1 {
        return Err(UpdateError::new(
            "Release must contain exactly one matching Linux portable archive",
        ));
    }
    let archive = archives[0];
    let checksum_names = [
        format!("{}.sha256", archive.name),
        archive.name.replace(".tar.gz", ".sha256"),
        "SHA256SUMS".into(),
    ];
    let checksum = checksum_names
        .iter()
        .find_map(|name| {
            release
                .assets
                .iter()
                .find(|a| a.state == "uploaded" && &a.name == name)
        })
        .ok_or_else(|| UpdateError::new("Release has no SHA-256 checksum asset"))?;
    validate_asset_url(&archive.browser_download_url)?;
    validate_asset_url(&checksum.browser_download_url)?;
    Ok(ReleaseUpdate {
        version: parsed.to_string(),
        archive_name: archive.name.clone(),
        archive_url: archive.browser_download_url.clone(),
        checksum_url: checksum.browser_download_url.clone(),
    })
}

fn validate_asset_url(value: &str) -> Result<(), UpdateError> {
    let url = reqwest::Url::parse(value).map_err(|e| UpdateError::new(e.to_string()))?;
    if url.scheme() != "https"
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !url
            .path()
            .starts_with(&format!("/{GITHUB_OWNER}/{GITHUB_REPO}/releases/download/"))
    {
        return Err(UpdateError::new(
            "Release asset URL is outside the project releases",
        ));
    }
    Ok(())
}

pub(super) fn prepare(
    platform: &dyn Platform,
    check: &UpdateCheckResult,
    progress: &mut dyn FnMut(UpdateProgress),
) -> Result<PreparedUpdate, UpdateError> {
    let paths = platform
        .app_paths()
        .map_err(|e| UpdateError::new(e.to_string()))?;
    let work = platform::create_temp_dir(&paths.cache_path().join("updates"), "release")
        .map_err(|e| UpdateError::new(e.to_string()))?;
    let result = prepare_in(check, &work, progress);
    if let Err(error) = &result {
        notify(progress, UpdatePhase::Failed, &error.to_string());
        let _ = platform.cleanup_temp_path(&work);
    }
    result
}

fn prepare_in(
    check: &UpdateCheckResult,
    work: &Path,
    progress: &mut dyn FnMut(UpdateProgress),
) -> Result<PreparedUpdate, UpdateError> {
    let release = check
        .release
        .as_ref()
        .ok_or_else(|| UpdateError::new("Missing release target"))?;
    validate_asset_url(&release.archive_url)?;
    validate_asset_url(&release.checksum_url)?;
    let client = Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|e| UpdateError::new(e.to_string()))?;
    let mut checksum = String::new();
    checked(
        client
            .get(&release.checksum_url)
            .send()
            .map_err(|e| UpdateError::new(e.to_string()))?,
    )?
    .take(64 * 1024)
    .read_to_string(&mut checksum)?;
    let expected = checksum_for(&checksum, &release.archive_name)?;
    notify(
        progress,
        UpdatePhase::Downloading,
        "Downloading Linux release archive",
    );
    let response = checked(
        client
            .get(&release.archive_url)
            .send()
            .map_err(|e| UpdateError::new(e.to_string()))?,
    )?;
    let total = response.content_length();
    let archive = work.join("release.tar.gz");
    download_archive(
        response,
        File::create(&archive)?,
        total,
        &expected,
        progress,
    )?;
    notify(
        progress,
        UpdatePhase::Staging,
        "Verifying release executables",
    );
    extract_executables(File::open(archive)?, work)?;
    let prepared = PreparedUpdate {
        work_dir: work.to_owned(),
        target_sha: check.head_sha.clone(),
        shell_exe: work.join(SHELL_FILE),
        cli_exe: work.join(CLI_FILE),
    };
    for path in [&prepared.shell_exe, &prepared.cli_exe] {
        validate_update_probe(path, &check.head_sha)?;
    }
    Ok(prepared)
}

fn checksum_for(text: &str, name: &str) -> Result<String, UpdateError> {
    let mut matches = text.lines().filter_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then_some(hash)
    });
    let hash = matches
        .next()
        .ok_or_else(|| UpdateError::new("Archive is missing from release checksums"))?;
    if matches.next().is_some() || hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(UpdateError::new("Invalid or duplicate release checksum"));
    }
    Ok(hash.to_ascii_lowercase())
}

fn download_archive(
    mut input: impl Read,
    mut output: impl Write,
    total: Option<u64>,
    expected: &str,
    progress: &mut dyn FnMut(UpdateProgress),
) -> Result<(), UpdateError> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut received = 0;
    loop {
        let size = input.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        received += size as u64;
        if received > 2 * 1024 * 1024 * 1024 {
            return Err(UpdateError::new("Release archive exceeds the size limit"));
        }
        output.write_all(&buffer[..size])?;
        hash.update(&buffer[..size]);
        progress(UpdateProgress {
            phase: UpdatePhase::Downloading,
            message: "Downloading Linux release archive".into(),
            detail: UpdateProgressDetail::Download {
                received,
                total,
                finished: false,
            },
        });
    }
    if total.is_some_and(|value| value != received) || format!("{:x}", hash.finalize()) != expected
    {
        return Err(UpdateError::new(
            "Release archive SHA-256 verification failed",
        ));
    }
    output.flush()?;
    progress(UpdateProgress {
        phase: UpdatePhase::Downloading,
        message: "Release download verified".into(),
        detail: UpdateProgressDetail::Download {
            received,
            total,
            finished: true,
        },
    });
    Ok(())
}

fn extract_executables(input: impl Read, destination: &Path) -> Result<(), UpdateError> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(input));
    let mut found = std::collections::BTreeSet::new();
    let mut root = None;
    let mut expanded = 0u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        if path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(UpdateError::new("Unsafe release archive path"));
        }
        expanded = expanded.saturating_add(entry.size());
        if expanded > 4 * 1024 * 1024 * 1024 {
            return Err(UpdateError::new(
                "Expanded release archive exceeds the size limit",
            ));
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if ![SHELL_FILE, CLI_FILE].contains(&name) {
            continue;
        }
        if path.components().count() != 2
            || !entry.header().entry_type().is_file()
            || !found.insert(name.to_owned())
        {
            return Err(UpdateError::new("Invalid or duplicate release executable"));
        }
        let parent = path.parent().unwrap().to_owned();
        if root.as_ref().is_some_and(|value| value != &parent) {
            return Err(UpdateError::new(
                "Release executables have different archive roots",
            ));
        }
        root = Some(parent);
        let output = destination.join(name);
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)?;
        io::copy(&mut entry, &mut file)?;
        file.sync_all()?;
        fs::set_permissions(output, fs::Permissions::from_mode(0o755))?;
    }
    if found.len() != 2 {
        return Err(UpdateError::new("Release is missing Shell or CLI"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/update_release.rs"]
mod tests;
