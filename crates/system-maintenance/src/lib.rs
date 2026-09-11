//! Fixed-policy system maintenance. No caller-selected executable or root destination.
use serde::{Deserialize, Serialize};
use std::{
    io,
    path::{Component, Path},
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const REPOSITORY: &str = "peixuanthomas/TundraUX3";
pub const WORKFLOW: &str = "peixuanthomas/TundraUX3/.github/workflows/linux-system-release.yml";
pub const PROTOCOL: u32 = 1;
pub const RUNTIME_BINARIES: [&str; 6] = [
    "tundra-shell",
    "tundra-cli",
    "tundra-sessiond",
    "tundra-greeter",
    "tundra-privileged",
    "kmscon",
];
pub const MAX_BUNDLE: u64 = 512 * 1024 * 1024;
pub const MAX_EXPANDED: u64 = 1024 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    pub version: String,
    pub source_sha: String,
    pub architecture: String,
    pub protocol: u32,
    pub runtime_sha256: String,
}
pub fn invalid(message: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}
pub fn release_version(id: &str) -> Result<semver::Version> {
    if id.len() > 64 || !id.starts_with('v') {
        return Err(invalid("release must be vMAJOR.MINOR.PATCH"));
    }
    let version = semver::Version::parse(&id[1..])?;
    if !version.pre.is_empty() || !version.build.is_empty() || format!("v{version}") != id {
        return Err(invalid("only canonical stable releases are accepted"));
    }
    Ok(version)
}
impl ReleaseManifest {
    /// Bind display/authorization metadata to the manifest inside the attested runtime.
    pub fn validate_attested_metadata(&self, embedded: &Self) -> Result<()> {
        let mut expected = self.clone();
        // A digest cannot cover a ZIP containing itself; only this self-reference differs.
        expected.runtime_sha256 = "0".repeat(64);
        if embedded != &expected {
            return Err(invalid("attested release metadata mismatch"));
        }
        Ok(())
    }
    pub fn validate(&self, id: &str, current: Option<&str>) -> Result<()> {
        let version = release_version(id)?;
        if self.version != id
            || self.architecture != "x86_64-unknown-linux-gnu"
            || self.protocol != PROTOCOL
        {
            return Err(invalid(
                "release identity, architecture or protocol mismatch",
            ));
        }
        for (value, len) in [(&self.source_sha, 40), (&self.runtime_sha256, 64)] {
            if value.len() != len
                || !value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(invalid("invalid release digest"));
            }
        }
        if let Some(current) = current {
            if version <= release_version(current)? {
                return Err(invalid("release replay or downgrade refused"));
            }
        }
        Ok(())
    }
}
fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}
/// Extract only regular files/directories; never delegates extraction to an installer.
pub fn extract_runtime<R: io::Read + io::Seek>(reader: R, destination: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(reader)?;
    if archive.len() > 4096 {
        return Err(invalid("too many runtime entries"));
    }
    let mut size = 0_u64;
    let mut seen = std::collections::HashSet::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().trim_end_matches('/').to_owned();
        let path = Path::new(&name);
        let mode = entry.unix_mode().unwrap_or(0o100644);
        let kind = mode & 0o170000;
        if !safe_relative(path)
            || name.contains('\\')
            || !seen.insert(name.to_owned())
            || !(kind == 0 || kind == 0o100000 || kind == 0o040000)
            || (mode & 0o7000) != 0
        {
            return Err(invalid("unsafe runtime entry"));
        }
        // The release may contain binaries and read-only data, never system configuration.
        if path
            .components()
            .next()
            .is_none_or(|c| c.as_os_str() != "bin" && c.as_os_str() != "share")
        {
            return Err(invalid("runtime entry outside bin/share"));
        }
        size = size
            .checked_add(entry.size())
            .ok_or_else(|| invalid("runtime size overflow"))?;
        if size > MAX_EXPANDED {
            return Err(invalid("runtime too large"));
        }
        let target = destination.join(path);
        if entry.is_dir() {
            std::fs::create_dir_all(target)?;
            continue;
        }
        std::fs::create_dir_all(target.parent().unwrap())?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)?;
        let expected_size = entry.size();
        if io::copy(
            &mut io::Read::take(&mut entry, expected_size.saturating_add(1)),
            &mut file,
        )? != expected_size
        {
            return Err(invalid("truncated runtime"));
        }
        file.sync_all()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(
                if path.parent() == Some(Path::new("bin"))
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| RUNTIME_BINARIES.contains(&name))
                {
                    0o755
                } else {
                    0o644
                },
            ))?;
        }
    }
    for name in RUNTIME_BINARIES {
        if !destination.join("bin").join(name).is_file() {
            return Err(invalid(format!("missing runtime binary {name}")));
        }
    }
    for required in [
        "bin/kmscon-capabilities.json",
        "share/tundra/kmscon-modules/mod-pango.so",
        "share/tundra/licenses/LICENSE.kmscon",
        "share/tundra/licenses/LICENSE.libtsm",
    ] {
        if !destination.join(required).is_file() {
            return Err(invalid(format!(
                "missing private terminal resource {required}"
            )));
        }
    }
    Ok(())
}
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "linux")]
pub mod migration;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_ids_are_not_paths_or_options() {
        for id in [
            "../x",
            "--help",
            "v1.2.3/../x",
            "v1.2.3-beta",
            "v01.2.3",
            "v1.2.3+build",
        ] {
            assert!(release_version(id).is_err());
        }
        assert!(release_version("v1.2.3").is_ok());
    }
    #[test]
    fn manifest_rejects_replay_architecture_and_protocol() {
        let mut m = ReleaseManifest {
            version: "v2.0.0".into(),
            source_sha: "a".repeat(40),
            architecture: "x86_64-unknown-linux-gnu".into(),
            protocol: 1,
            runtime_sha256: "b".repeat(64),
        };
        assert!(m.validate("v2.0.0", Some("v1.0.0")).is_ok());
        assert!(m.validate("v2.0.0", Some("v2.0.0")).is_err());
        m.protocol = 2;
        assert!(m.validate("v2.0.0", None).is_err());
        m.protocol = 1;
        m.architecture = "aarch64".into();
        assert!(m.validate("v2.0.0", None).is_err());
    }
    #[test]
    fn extraction_refuses_traversal_and_links() {
        use std::io::{Cursor, Write};
        for name in ["../escape", "/escape", "bin/../../escape", "bin\\evil"] {
            let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
            out.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            out.write_all(b"bad").unwrap();
            assert!(
                extract_runtime(
                    out.finish().unwrap(),
                    Path::new("/unreachable-test-destination")
                )
                .is_err()
            );
        }
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        out.add_symlink(
            "bin/link",
            "/etc/passwd",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        assert!(
            extract_runtime(
                out.finish().unwrap(),
                Path::new("/unreachable-test-destination")
            )
            .is_err()
        );
    }
    #[test]
    fn complete_private_runtime_extracts_with_limited_execute_modes() {
        use std::io::{Cursor, Write};
        let destination =
            std::env::temp_dir().join(format!("tundra-runtime-extract-{}", std::process::id()));
        std::fs::create_dir(&destination).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for name in RUNTIME_BINARIES {
            out.start_file(
                format!("bin/{name}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            out.write_all(b"fixture").unwrap();
        }
        for name in [
            "bin/kmscon-capabilities.json",
            "share/tundra/kmscon-modules/mod-pango.so",
            "share/tundra/licenses/LICENSE.kmscon",
            "share/tundra/licenses/LICENSE.libtsm",
        ] {
            out.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            out.write_all(b"fixture").unwrap();
        }
        extract_runtime(out.finish().unwrap(), &destination).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(destination.join("bin/kmscon"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o755
            );
            assert_eq!(
                std::fs::metadata(destination.join("bin/kmscon-capabilities.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o644
            );
        }
        std::fs::remove_dir_all(destination).unwrap();
    }
    #[test]
    fn unsigned_envelope_cannot_replace_attested_release_metadata() {
        let outer = ReleaseManifest {
            version: "v2.0.0".into(),
            source_sha: "a".repeat(40),
            architecture: "x86_64-unknown-linux-gnu".into(),
            protocol: 1,
            runtime_sha256: "b".repeat(64),
        };
        let mut inner = outer.clone();
        inner.runtime_sha256 = "0".repeat(64);
        assert!(outer.validate_attested_metadata(&inner).is_ok());
        let mut tampered = outer.clone();
        tampered.version = "v99.0.0".into();
        assert!(tampered.validate_attested_metadata(&inner).is_err());
        tampered = outer.clone();
        tampered.source_sha = "c".repeat(40);
        assert!(tampered.validate_attested_metadata(&inner).is_err());
        tampered = outer.clone();
        tampered.protocol = 2;
        assert!(tampered.validate_attested_metadata(&inner).is_err());
        tampered = outer.clone();
        tampered.architecture = "aarch64".into();
        assert!(tampered.validate_attested_metadata(&inner).is_err());
        inner.runtime_sha256 = outer.runtime_sha256.clone();
        assert!(outer.validate_attested_metadata(&inner).is_err());
    }
}
