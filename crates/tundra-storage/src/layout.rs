use std::path::PathBuf;

use tundra_platform::AppPaths;

use crate::descriptors::{
    CLOCK_DESCRIPTOR, TRASH_DIR_NAME, USERS_V1_FILE_NAME, VERSIONED_JSON_DESCRIPTORS,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageLayout {
    pub config_path: PathBuf,
    pub data_path: PathBuf,
    pub cache_path: PathBuf,
    pub logs_path: PathBuf,
    pub temp_path: PathBuf,
    pub users_path: PathBuf,
    pub legacy_users_path: PathBuf,
    pub state_path: PathBuf,
    pub recent_files_path: PathBuf,
    pub sessions_path: PathBuf,
    pub clock_path: PathBuf,
    pub trash_path: PathBuf,
    pub trash_manifest_path: PathBuf,
}

impl StorageLayout {
    pub fn from_app_paths(app_paths: &AppPaths) -> Self {
        let data_path = app_paths.data_path().to_path_buf();
        let trash_path = data_path.join(TRASH_DIR_NAME);

        Self {
            config_path: app_paths.config_path().to_path_buf(),
            data_path: data_path.clone(),
            cache_path: app_paths.cache_path().to_path_buf(),
            logs_path: app_paths.logs_path().to_path_buf(),
            temp_path: app_paths.temp_path().to_path_buf(),
            users_path: data_path.join(VERSIONED_JSON_DESCRIPTORS[0].file_name),
            legacy_users_path: data_path.join(USERS_V1_FILE_NAME),
            state_path: data_path.join(VERSIONED_JSON_DESCRIPTORS[1].file_name),
            recent_files_path: data_path.join(VERSIONED_JSON_DESCRIPTORS[2].file_name),
            sessions_path: data_path.join(VERSIONED_JSON_DESCRIPTORS[3].file_name),
            clock_path: data_path.join(CLOCK_DESCRIPTOR.file_name),
            trash_manifest_path: trash_path.join(VERSIONED_JSON_DESCRIPTORS[5].file_name),
            trash_path,
        }
    }
}
