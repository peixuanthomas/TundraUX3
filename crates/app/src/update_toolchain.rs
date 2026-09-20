use super::UpdateError;
use platform::ProcessSpec;
use std::path::{Path, PathBuf};

pub(super) struct Toolchain {
    cargo: PathBuf,
    rustc: PathBuf,
}

impl Toolchain {
    pub(super) fn discover() -> Result<Self, UpdateError> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut homes = Vec::new();
        if let Some(home) = std::env::var_os("RUSTUP_HOME") {
            homes.push(PathBuf::from(home));
        }
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            homes.push(PathBuf::from(home).join(".rustup"));
        }
        Self::from_locations(std::env::split_paths(&path), &homes)
    }

    fn from_locations(
        path: impl Iterator<Item = PathBuf>,
        homes: &[PathBuf],
    ) -> Result<Self, UpdateError> {
        // Prefer a complete installed toolchain, not rustup's proxy binaries:
        for dir in path {
            if dir.is_absolute()
                && let Some(pair) = Self::in_dir(&dir)
            {
                return Ok(pair);
            }
        }
        for home in homes {
            let settings = std::fs::read_to_string(home.join("settings.toml")).unwrap_or_default();
            let default = settings
                .parse::<toml::Value>()
                .ok()
                .and_then(|v| v.get("default_toolchain")?.as_str().map(str::to_owned));
            if let Some(default) = default
                && Path::new(&default)
                    .components()
                    .all(|c| matches!(c, std::path::Component::Normal(_)))
                && let Some(pair) = Self::in_dir(&home.join("toolchains").join(default).join("bin"))
            {
                return Ok(pair);
            }
        }
        Err(UpdateError::new(
            "could not find a complete installed Rust toolchain (cargo and rustc)",
        ))
    }

    fn in_dir(dir: &Path) -> Option<Self> {
        let cargo = dir.join(if cfg!(windows) { "cargo.exe" } else { "cargo" });
        let rustc = dir.join(if cfg!(windows) { "rustc.exe" } else { "rustc" });
        if !cargo.is_file() || !rustc.is_file() {
            return None;
        }
        let resolved = cargo.canonicalize().ok()?;
        if resolved.file_stem().is_some_and(|name| name == "rustup") {
            return None;
        }
        Some(Self { cargo, rustc })
    }

    pub(super) fn spec(&self, name: &str) -> ProcessSpec {
        let program = if name == "cargo" {
            &self.cargo
        } else {
            &self.rustc
        };
        let mut paths = vec![self.cargo.parent().unwrap().to_path_buf()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let mut spec = ProcessSpec::new(program).env("RUSTC", self.rustc.to_string_lossy());
        if let Ok(path) = std::env::join_paths(paths) {
            spec = spec.env("PATH", path.to_string_lossy());
        }
        spec
    }
}

#[cfg(test)]
#[path = "../tests/unit/update_toolchain/tests.rs"]
mod tests;
