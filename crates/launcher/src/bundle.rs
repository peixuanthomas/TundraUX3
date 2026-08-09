use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// Version 2 requires a kiosk-enabled WezTerm that implements its private
/// no-PTY `tundra-recovery` command. Version 1 bundles used the now-retired
/// `tundra-recovery` foreground helper instead.
pub const BUNDLE_PROTOCOL_VERSION: &str = "2";
pub const WEZTERM_HOST_PROTOCOL_MARKER: &str = "tundra-host-protocol";
pub const WEZTERM_MANIFEST_FILE: &str = "tundra-wezterm-manifest-v1";
pub const WEZTERM_EXPECTED_GIT_SHA: &str = "e378176fd3aa8204ace298157599b5a3b8496ca4";
pub const WEZTERM_EXPECTED_PATCH_SHA256: &str = env!("TUNDRA_WEZTERM_PATCH_SHA256");
const WEZTERM_MANIFEST_HEADER: &str = "TUNDRA_WEZTERM_MANIFEST_V1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleLayout {
    install_root: PathBuf,
    runtime_root: PathBuf,
    pub wezterm_gui: PathBuf,
    pub shell: PathBuf,
    pub cli: PathBuf,
    pub assets: PathBuf,
    pub wezterm_config: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleError {
    CurrentExecutable(String),
    MissingParent(PathBuf),
    MissingRuntime(PathBuf),
    MissingComponent { name: &'static str, path: PathBuf },
    InvalidProtocol { path: PathBuf, found: String },
    InvalidManifest { path: PathBuf, message: String },
    Io { path: PathBuf, message: String },
}

impl fmt::Display for BundleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrentExecutable(message) => {
                write!(formatter, "cannot resolve launcher executable: {message}")
            }
            Self::MissingParent(path) => write!(
                formatter,
                "launcher executable has no parent: {}",
                path.display()
            ),
            Self::MissingRuntime(path) => {
                write!(formatter, "bundled runtime is missing: {}", path.display())
            }
            Self::MissingComponent { name, path } => {
                write!(formatter, "bundled {name} is missing: {}", path.display())
            }
            Self::InvalidProtocol { path, found } => write!(
                formatter,
                "unsupported launcher protocol in {}: {found}",
                path.display()
            ),
            Self::InvalidManifest { path, message } => write!(
                formatter,
                "invalid bundled WezTerm manifest {}: {message}",
                path.display()
            ),
            Self::Io { path, message } => {
                write!(formatter, "cannot read {}: {message}", path.display())
            }
        }
    }
}

impl std::error::Error for BundleError {}

impl BundleLayout {
    pub fn from_current_exe() -> Result<Self, BundleError> {
        let executable = std::env::current_exe()
            .map_err(|error| BundleError::CurrentExecutable(error.to_string()))?;
        Self::from_launcher_executable(&executable)
    }

    /// Resolves only deterministic, installation-relative locations.  This is
    /// deliberately independent of CWD, PATH, HOME and WEZTERM_* settings.
    pub fn from_launcher_executable(executable: &Path) -> Result<Self, BundleError> {
        let executable = executable.canonicalize().unwrap_or_else(|_| {
            if executable.is_absolute() {
                executable.to_path_buf()
            } else {
                // This fallback is useful for packaging smoke tests where the
                // binary is represented by a not-yet-existing fixture.  Real
                // `current_exe` values are already absolute.
                std::env::current_dir()
                    .map(|directory| directory.join(executable))
                    .unwrap_or_else(|_| executable.to_path_buf())
            }
        });
        let executable_dir = executable
            .parent()
            .ok_or_else(|| BundleError::MissingParent(executable.clone()))?;
        let candidates = runtime_candidates(executable_dir);
        let runtime_root = candidates
            .into_iter()
            .find(|candidate| candidate.is_dir())
            .ok_or_else(|| BundleError::MissingRuntime(executable_dir.join("runtime")))?;
        let runtime_root = runtime_root.canonicalize().unwrap_or(runtime_root);
        let install_root = runtime_root
            .parent()
            .unwrap_or(executable_dir)
            .to_path_buf();
        Ok(Self {
            install_root,
            wezterm_gui: runtime_root
                .join("wezterm")
                .join(executable_name("wezterm-gui")),
            shell: runtime_root.join(executable_name("tundra-shell")),
            cli: runtime_root.join(executable_name("tundra-cli")),
            assets: runtime_root.join("assets"),
            wezterm_config: runtime_root.join("wezterm").join("tundra.lua"),
            runtime_root,
        })
    }

    pub fn install_root(&self) -> &Path {
        &self.install_root
    }
    pub fn runtime_root(&self) -> &Path {
        &self.runtime_root
    }

    pub fn preflight(&self) -> Result<(), BundleError> {
        require_file("WezTerm GUI", &self.wezterm_gui)?;
        require_file("Tundra Shell", &self.shell)?;
        require_file("Tundra CLI", &self.cli)?;
        require_dir("asset directory", &self.assets)?;
        require_file("managed WezTerm config", &self.wezterm_config)?;
        let protocol = self.runtime_root.join("launcher-protocol-version");
        require_protocol(&protocol)?;
        let wezterm_protocol = self
            .runtime_root
            .join("wezterm")
            .join(WEZTERM_HOST_PROTOCOL_MARKER);
        require_file("WezTerm native recovery capability", &wezterm_protocol)?;
        require_protocol(&wezterm_protocol)?;
        let wezterm_manifest = self
            .runtime_root
            .join("wezterm")
            .join(WEZTERM_MANIFEST_FILE);
        require_file("WezTerm supply-chain manifest", &wezterm_manifest)?;
        require_wezterm_manifest(&wezterm_manifest, &self.wezterm_gui)?;
        Ok(())
    }
}

fn require_wezterm_manifest(manifest_path: &Path, binary_path: &Path) -> Result<(), BundleError> {
    let manifest = fs::read_to_string(manifest_path).map_err(|error| BundleError::Io {
        path: manifest_path.to_path_buf(),
        message: error.to_string(),
    })?;
    let manifest = parse_wezterm_manifest(manifest_path, &manifest)?;
    if manifest.protocol != BUNDLE_PROTOCOL_VERSION {
        return Err(BundleError::InvalidManifest {
            path: manifest_path.to_path_buf(),
            message: format!(
                "protocol is {}; expected {BUNDLE_PROTOCOL_VERSION}",
                manifest.protocol
            ),
        });
    }
    if manifest.git_sha != WEZTERM_EXPECTED_GIT_SHA {
        return Err(BundleError::InvalidManifest {
            path: manifest_path.to_path_buf(),
            message: format!(
                "git_sha is {}; expected {WEZTERM_EXPECTED_GIT_SHA}",
                manifest.git_sha
            ),
        });
    }
    if manifest.patch_sha256 != WEZTERM_EXPECTED_PATCH_SHA256 {
        return Err(BundleError::InvalidManifest {
            path: manifest_path.to_path_buf(),
            message: format!(
                "patch_sha256 is {}; expected {WEZTERM_EXPECTED_PATCH_SHA256}",
                manifest.patch_sha256
            ),
        });
    }
    let actual_binary_hash = sha256_file(binary_path)?;
    if manifest.binary_sha256 != actual_binary_hash {
        return Err(BundleError::InvalidManifest {
            path: manifest_path.to_path_buf(),
            message: "binary_sha256 does not match wezterm-gui".to_owned(),
        });
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct WezTermManifest<'a> {
    protocol: &'a str,
    git_sha: &'a str,
    patch_sha256: &'a str,
    binary_sha256: &'a str,
}

fn parse_wezterm_manifest<'a>(
    path: &Path,
    contents: &'a str,
) -> Result<WezTermManifest<'a>, BundleError> {
    // Build scripts emit a single final LF. Reject CRLF, extra fields and
    // whitespace so a manifest is a fixed integrity record, not a config file.
    if contents.contains('\r') || !contents.ends_with('\n') {
        return invalid_manifest(path, "must use exactly one final LF and no CR characters");
    }
    let lines: Vec<_> = contents.strip_suffix('\n').unwrap().split('\n').collect();
    if lines.len() != 5 || lines.iter().any(|line| line.is_empty()) {
        return invalid_manifest(path, "must contain exactly five non-empty lines");
    }
    if lines[0] != WEZTERM_MANIFEST_HEADER {
        return invalid_manifest(path, "header is not TUNDRA_WEZTERM_MANIFEST_V1");
    }
    let protocol = manifest_value(path, lines[1], "protocol")?;
    let git_sha = manifest_value(path, lines[2], "git_sha")?;
    let patch_sha256 = manifest_value(path, lines[3], "patch_sha256")?;
    let binary_sha256 = manifest_value(path, lines[4], "binary_sha256")?;
    if !is_lower_hex(git_sha, 40)
        || !is_lower_hex(patch_sha256, 64)
        || !is_lower_hex(binary_sha256, 64)
    {
        return invalid_manifest(
            path,
            "hash fields must be lowercase hexadecimal with fixed lengths",
        );
    }
    Ok(WezTermManifest {
        protocol,
        git_sha,
        patch_sha256,
        binary_sha256,
    })
}

fn manifest_value<'a>(path: &Path, line: &'a str, key: &str) -> Result<&'a str, BundleError> {
    line.strip_prefix(&format!("{key}="))
        .ok_or_else(|| BundleError::InvalidManifest {
            path: path.to_path_buf(),
            message: format!("expected {key}= field"),
        })
}

fn is_lower_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn invalid_manifest<T>(path: &Path, message: &str) -> Result<T, BundleError> {
    Err(BundleError::InvalidManifest {
        path: path.to_path_buf(),
        message: message.to_owned(),
    })
}

fn sha256_file(path: &Path) -> Result<String, BundleError> {
    let mut file = fs::File::open(path).map_err(|error| BundleError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 32 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| BundleError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn require_protocol(path: &Path) -> Result<(), BundleError> {
    let found = fs::read_to_string(path).map_err(|error| BundleError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if found.trim() != BUNDLE_PROTOCOL_VERSION {
        return Err(BundleError::InvalidProtocol {
            path: path.to_path_buf(),
            found: found.trim().to_owned(),
        });
    }
    Ok(())
}

fn runtime_candidates(executable_dir: &Path) -> Vec<PathBuf> {
    vec![
        // Windows portable distributions and Linux tarballs.
        executable_dir.join("runtime"),
        // Linux packages: /usr/bin/tundra + /usr/lib/tundra/runtime.
        executable_dir
            .join("..")
            .join("lib")
            .join("tundra")
            .join("runtime"),
        // macOS: TundraUX3.app/Contents/MacOS/tundra + Contents/Resources/runtime.
        executable_dir.join("..").join("Resources").join("runtime"),
    ]
}

fn executable_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_owned()
    }
}

fn require_file(name: &'static str, path: &Path) -> Result<(), BundleError> {
    if path.is_file() {
        Ok(())
    } else {
        Err(BundleError::MissingComponent {
            name,
            path: path.to_path_buf(),
        })
    }
}

fn require_dir(name: &'static str, path: &Path) -> Result<(), BundleError> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(BundleError::MissingComponent {
            name,
            path: path.to_path_buf(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tundra-launcher-bundle-{}",
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let runtime = root.join("runtime");
        fs::create_dir_all(runtime.join("wezterm")).unwrap();
        fs::create_dir_all(runtime.join("assets")).unwrap();
        fs::write(root.join(executable_name("tundra")), b"").unwrap();
        fs::write(
            runtime.join("wezterm").join(executable_name("wezterm-gui")),
            b"",
        )
        .unwrap();
        fs::write(runtime.join(executable_name("tundra-shell")), b"").unwrap();
        fs::write(runtime.join(executable_name("tundra-cli")), b"").unwrap();
        fs::write(runtime.join("wezterm/tundra.lua"), b"").unwrap();
        fs::write(
            runtime.join("wezterm").join(WEZTERM_HOST_PROTOCOL_MARKER),
            format!("{BUNDLE_PROTOCOL_VERSION}\n"),
        )
        .unwrap();
        let wezterm_gui = runtime.join("wezterm").join(executable_name("wezterm-gui"));
        let binary_sha256 = sha256_file(&wezterm_gui).unwrap();
        fs::write(
            runtime.join("wezterm").join(WEZTERM_MANIFEST_FILE),
            format!(
                "{WEZTERM_MANIFEST_HEADER}\nprotocol={BUNDLE_PROTOCOL_VERSION}\ngit_sha={WEZTERM_EXPECTED_GIT_SHA}\npatch_sha256={WEZTERM_EXPECTED_PATCH_SHA256}\nbinary_sha256={binary_sha256}\n",
            ),
        )
        .unwrap();
        fs::write(
            runtime.join("launcher-protocol-version"),
            BUNDLE_PROTOCOL_VERSION,
        )
        .unwrap();
        root
    }

    #[test]
    fn portable_bundle_needs_no_path_lookup() {
        let root = fixture();
        let layout =
            BundleLayout::from_launcher_executable(&root.join(executable_name("tundra"))).unwrap();
        layout.preflight().unwrap();
        assert!(layout.wezterm_gui.is_absolute());
        assert_eq!(
            layout.install_root(),
            root.canonicalize().unwrap().as_path()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn incompatible_protocol_is_rejected_before_spawning() {
        let root = fixture();
        fs::write(root.join("runtime/launcher-protocol-version"), "1").unwrap();
        let error = BundleLayout::from_launcher_executable(&root.join(executable_name("tundra")))
            .unwrap()
            .preflight()
            .unwrap_err();
        assert!(matches!(error, BundleError::InvalidProtocol { .. }));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn wezterm_without_native_recovery_capability_is_rejected() {
        let root = fixture();
        fs::remove_file(
            root.join("runtime/wezterm")
                .join(WEZTERM_HOST_PROTOCOL_MARKER),
        )
        .unwrap();
        let error = BundleLayout::from_launcher_executable(&root.join(executable_name("tundra")))
            .unwrap()
            .preflight()
            .unwrap_err();
        assert!(matches!(error, BundleError::MissingComponent { .. }));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_wezterm_manifest_is_rejected() {
        let root = fixture();
        fs::remove_file(root.join("runtime/wezterm").join(WEZTERM_MANIFEST_FILE)).unwrap();
        let error = BundleLayout::from_launcher_executable(&root.join(executable_name("tundra")))
            .unwrap()
            .preflight()
            .unwrap_err();
        assert!(matches!(error, BundleError::MissingComponent { .. }));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tampered_wezterm_binary_is_rejected() {
        let root = fixture();
        fs::write(
            root.join("runtime/wezterm")
                .join(executable_name("wezterm-gui")),
            b"tampered",
        )
        .unwrap();
        let error = BundleLayout::from_launcher_executable(&root.join(executable_name("tundra")))
            .unwrap()
            .preflight()
            .unwrap_err();
        assert!(matches!(error, BundleError::InvalidManifest { .. }));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_with_wrong_wezterm_pin_is_rejected() {
        let root = fixture();
        let manifest = root.join("runtime/wezterm").join(WEZTERM_MANIFEST_FILE);
        let contents = fs::read_to_string(&manifest).unwrap();
        fs::write(
            &manifest,
            contents.replace(WEZTERM_EXPECTED_GIT_SHA, &"0".repeat(40)),
        )
        .unwrap();
        let error = BundleLayout::from_launcher_executable(&root.join(executable_name("tundra")))
            .unwrap()
            .preflight()
            .unwrap_err();
        assert!(matches!(error, BundleError::InvalidManifest { .. }));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_with_wrong_managed_patch_is_rejected() {
        let root = fixture();
        let manifest = root.join("runtime/wezterm").join(WEZTERM_MANIFEST_FILE);
        let contents = fs::read_to_string(&manifest).unwrap();
        fs::write(
            &manifest,
            contents.replace(WEZTERM_EXPECTED_PATCH_SHA256, &"0".repeat(64)),
        )
        .unwrap();
        let error = BundleLayout::from_launcher_executable(&root.join(executable_name("tundra")))
            .unwrap()
            .preflight()
            .unwrap_err();
        assert!(matches!(error, BundleError::InvalidManifest { .. }));
        let _ = fs::remove_dir_all(root);
    }
}
