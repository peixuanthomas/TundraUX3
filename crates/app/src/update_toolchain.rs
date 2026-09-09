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
        if let Some(home) = platform::sudo_user_home() {
            homes.push(home.join(".rustup"));
        }
        Self::from_locations(std::env::split_paths(&path), &homes)
    }

    fn from_locations(
        path: impl Iterator<Item = PathBuf>,
        homes: &[PathBuf],
    ) -> Result<Self, UpdateError> {
        // Prefer a complete installed toolchain, not rustup's proxy binaries:
        // sudo changes HOME and the proxies would use root's rustup settings.
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
            "could not find a complete installed Rust toolchain (cargo and rustc), including the sudo user's default toolchain",
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
mod tests {
    use super::*;
    #[test]
    fn update_toolchain_finds_default_rustup_tools_outside_sudo_path() {
        let root = std::env::temp_dir().join(format!("tundra-toolchain-{}", std::process::id()));
        let home = root.join("invoking-user/.rustup");
        let bin = home.join("toolchains/stable-test/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(
            home.join("settings.toml"),
            "default_toolchain = \"stable-test\"\n",
        )
        .unwrap();
        for name in ["cargo", "rustc"] {
            std::fs::write(
                bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
                "tool",
            )
            .unwrap();
        }
        let toolchain = Toolchain::from_locations(std::iter::empty(), &[home]).unwrap();
        assert_eq!(
            toolchain.spec("cargo").program().parent(),
            Some(bin.as_path())
        );
        assert_eq!(
            Path::new(toolchain.spec("cargo").env_map().get("RUSTC").unwrap()),
            bin.join(format!("rustc{}", std::env::consts::EXE_SUFFIX))
        );
        assert!(Toolchain::from_locations(std::iter::empty(), &[]).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
