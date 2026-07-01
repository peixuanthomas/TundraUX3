#[cfg(not(windows))]
compile_error!("TundraUX3 phase 0 supports Windows 11 only.");

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageFormat {
    Toml,
    VersionedJson,
}

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

pub const VERSIONED_JSON_DESCRIPTORS: &[StorageDescriptor] = &[
    StorageDescriptor {
        name: "users",
        file_name: "users.v1.json",
        format: StorageFormat::VersionedJson,
        schema_version: SCHEMA_VERSION,
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
];
