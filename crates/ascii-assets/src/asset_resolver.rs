use std::env;
use std::path::{Path, PathBuf};

use crate::asset_error::AssetError;
use crate::asset_manifest::{CANONICAL_ASSETS_DIR, ENV_ASSETS_DIR};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetResolver {
    root: PathBuf,
}

impl AssetResolver {
    pub fn from_env_or_current_exe() -> Result<Self, AssetError> {
        if let Some(root) = env::var_os(ENV_ASSETS_DIR) {
            return Self::new(PathBuf::from(root));
        }

        let exe = env::current_exe().map_err(|source| AssetError::CurrentExe { source })?;
        let Some(parent) = exe.parent() else {
            return Err(AssetError::MissingCurrentExeParent { path: exe });
        };
        let primary = parent.join("assets");
        if primary.exists() {
            return Self::new(primary);
        }
        if parent.file_name().is_some_and(|name| name == "deps")
            && let Some(profile_dir) = parent.parent()
        {
            let profile_assets = profile_dir.join("assets");
            if profile_assets.exists() {
                return Self::new(profile_assets);
            }
        }
        Self::new(primary)
    }

    pub fn canonical() -> Result<Self, AssetError> {
        Self::new(CANONICAL_ASSETS_DIR)
    }

    pub fn new(root: impl Into<PathBuf>) -> Result<Self, AssetError> {
        let root = root.into();
        if !root.exists() {
            return Err(AssetError::MissingRoot { path: root });
        }
        if !root.is_dir() {
            return Err(AssetError::RootNotDirectory { path: root });
        }
        Ok(Self { root })
    }

    pub(crate) fn from_unchecked_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn theme_path(&self, theme_id: &str) -> PathBuf {
        self.root.join("themes").join(theme_id)
    }

    pub fn asset_path(&self, theme_id: &str, relative_path: &str) -> PathBuf {
        self.theme_path(theme_id).join(relative_path)
    }
}

pub fn asset_root_from_env_or_current_exe() -> Result<PathBuf, AssetError> {
    AssetResolver::from_env_or_current_exe().map(|resolver| resolver.root)
}
