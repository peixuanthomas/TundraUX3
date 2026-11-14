use crate::{StorageConfig, StorageError};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

pub trait SessionConfigStore: Send + Sync {
    fn load_config(&self) -> Result<StorageConfig, StorageError>;
    fn save_config(&self, config: &StorageConfig) -> Result<(), StorageError>;
}

static SESSION_CONFIG: OnceLock<(PathBuf, Arc<dyn SessionConfigStore>)> = OnceLock::new();

/// Installs the run's configuration authority before starting a Shell or app worker.
pub fn install_session_config_store(path: PathBuf, store: Arc<dyn SessionConfigStore>) {
    let _ = SESSION_CONFIG.set((path, store));
}

pub(crate) fn session_config_store(path: &Path) -> Option<&'static dyn SessionConfigStore> {
    SESSION_CONFIG
        .get()
        .and_then(|(owned_path, store)| (owned_path == path).then_some(store.as_ref()))
}
