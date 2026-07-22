use crate::schema::{SCHEMA_VERSION, StorageFormat, USERS_SCHEMA_VERSION};

pub(crate) const USERS_V1_FILE_NAME: &str = "users.v1.json";
const USERS_V2_FILE_NAME: &str = "users.v2.json";
pub(crate) const TRASH_DIR_NAME: &str = "trash";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageDescriptor {
    pub name: &'static str,
    pub file_name: &'static str,
    pub format: StorageFormat,
    pub schema_version: u32,
}

pub const CONFIG_DESCRIPTOR: StorageDescriptor = StorageDescriptor {
    name: "config",
    file_name: "config.toml",
    format: StorageFormat::Toml,
    schema_version: SCHEMA_VERSION,
};

pub const CLOCK_DESCRIPTOR: StorageDescriptor = StorageDescriptor {
    name: "clock",
    file_name: "clock.v1.json",
    format: StorageFormat::VersionedJson,
    schema_version: SCHEMA_VERSION,
};

pub const VERSIONED_JSON_DESCRIPTORS: &[StorageDescriptor] = &[
    StorageDescriptor {
        name: "users",
        file_name: USERS_V2_FILE_NAME,
        format: StorageFormat::VersionedJson,
        schema_version: USERS_SCHEMA_VERSION,
    },
    StorageDescriptor {
        name: "state",
        file_name: "state.v1.json",
        format: StorageFormat::VersionedJson,
        schema_version: SCHEMA_VERSION,
    },
    StorageDescriptor {
        name: "recent-files",
        file_name: "recent-files.v1.json",
        format: StorageFormat::VersionedJson,
        schema_version: SCHEMA_VERSION,
    },
    StorageDescriptor {
        name: "sessions",
        file_name: "sessions.v1.json",
        format: StorageFormat::VersionedJson,
        schema_version: SCHEMA_VERSION,
    },
    CLOCK_DESCRIPTOR,
    StorageDescriptor {
        name: "trash",
        file_name: "trash.v1.json",
        format: StorageFormat::VersionedJson,
        schema_version: SCHEMA_VERSION,
    },
];
