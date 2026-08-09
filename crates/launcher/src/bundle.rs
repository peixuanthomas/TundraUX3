use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

pub const BUNDLE_PROTOCOL_VERSION: &str = "1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleLayout {
    install_root: PathBuf,
    runtime_root: PathBuf,
    pub wezterm_gui: PathBuf,
    pub shell: PathBuf,
    pub cli: PathBuf,
    pub recovery: PathBuf,
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
            recovery: runtime_root.join(executable_name("tundra-recovery")),
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
        require_file("Tundra recovery UI", &self.recovery)?;
        require_dir("asset directory", &self.assets)?;
        require_file("managed WezTerm config", &self.wezterm_config)?;
        let protocol = self.runtime_root.join("launcher-protocol-version");
        let found = fs::read_to_string(&protocol).map_err(|error| BundleError::Io {
            path: protocol.clone(),
            message: error.to_string(),
        })?;
        if found.trim() != BUNDLE_PROTOCOL_VERSION {
            return Err(BundleError::InvalidProtocol {
                path: protocol,
                found: found.trim().to_owned(),
            });
        }
        Ok(())
    }
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
        fs::write(runtime.join(executable_name("tundra-recovery")), b"").unwrap();
        fs::write(runtime.join("wezterm/tundra.lua"), b"").unwrap();
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
        fs::write(root.join("runtime/launcher-protocol-version"), "2").unwrap();
        let error = BundleLayout::from_launcher_executable(&root.join(executable_name("tundra")))
            .unwrap()
            .preflight()
            .unwrap_err();
        assert!(matches!(error, BundleError::InvalidProtocol { .. }));
        let _ = fs::remove_dir_all(root);
    }
}
