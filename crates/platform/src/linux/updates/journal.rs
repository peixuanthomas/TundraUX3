//! Private, non-secret recovery metadata. A saved object path is only a hint.
use crate::{
    service::ServiceError,
    updates::{PackageVersion, UpdateResult},
};
use std::{
    io::Read,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::PathBuf,
};

#[derive(Debug, Clone)]
pub(super) struct Record {
    pub expected: PackageVersion,
    pub started_ms: u64,
    pub uid: u32,
    pub transaction: Option<String>,
}

pub(super) struct Journal {
    directory: PathBuf,
}
impl Journal {
    pub fn current() -> Result<Self, ServiceError> {
        let user = super::super::identity::LinuxUserContext::current()
            .map_err(|_| ServiceError::PermissionDenied)?;
        Ok(Self {
            directory: user.state_home.join("TundraUX3").join("package-update"),
        })
    }
    pub fn begin(&self, expected: &PackageVersion) -> Result<(), ServiceError> {
        let uid = super::super::identity::ProcessIdentity::current()
            .validate()
            .map_err(|_| ServiceError::PermissionDenied)?
            .uid;
        let value = serde_json::json!({"format":1,"state":"pending","expected":expected.id(),"started_ms":chrono::Utc::now().timestamp_millis().max(0) as u64,"uid":uid,"transaction":null});
        self.write(&value)
    }
    pub fn record_transaction(&self, path: &str) -> Result<(), ServiceError> {
        let mut value = self.load()?.ok_or(ServiceError::Unknown)?;
        value["transaction"] = path.into();
        self.write(&value)
    }
    pub fn finish(&self, result: &UpdateResult) -> Result<(), ServiceError> {
        let mut value = self.load()?.ok_or(ServiceError::Unknown)?;
        value["state"] = match result {
            UpdateResult::Unknown { .. } => "unknown",
            _ => "finished",
        }
        .into();
        self.write(&value)
    }
    pub fn read(&self) -> Result<Option<Record>, ServiceError> {
        let Some(value) = self.load()? else {
            return Ok(None);
        };
        if value["state"] == "finished" {
            return Ok(None);
        }
        if value["format"] != 1 || !matches!(value["state"].as_str(), Some("pending" | "unknown")) {
            return Err(ServiceError::Unknown);
        }
        let expected =
            PackageVersion::parse(value["expected"].as_str().ok_or(ServiceError::Unknown)?)?;
        if expected.name != super::TARGET {
            return Err(ServiceError::UntrustedTransaction);
        }
        let uid = value["uid"]
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or(ServiceError::Unknown)?;
        if uid != super::super::identity::ProcessIdentity::current().uid {
            return Err(ServiceError::PermissionDenied);
        }
        Ok(Some(Record {
            expected,
            uid,
            started_ms: value["started_ms"].as_u64().ok_or(ServiceError::Unknown)?,
            transaction: value["transaction"].as_str().map(str::to_owned),
        }))
    }
    fn load(&self) -> Result<Option<serde_json::Value>, ServiceError> {
        let path = self.directory.join("transaction.json");
        if !path
            .try_exists()
            .map_err(|_| ServiceError::PermissionDenied)?
        {
            return Ok(None);
        }
        self.validate_directory()?;
        crate::validate_no_follow_path(&path, true).map_err(|_| ServiceError::PermissionDenied)?;
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .map_err(|_| ServiceError::PermissionDenied)?
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| ServiceError::Unknown)?;
        if bytes.len() > 65536 {
            return Err(ServiceError::Unknown);
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| ServiceError::Unknown)
    }
    fn validate_directory(&self) -> Result<(), ServiceError> {
        crate::validate_no_follow_path(&self.directory, true)
            .map_err(|_| ServiceError::PermissionDenied)?;
        let metadata =
            std::fs::metadata(&self.directory).map_err(|_| ServiceError::PermissionDenied)?;
        if !metadata.is_dir()
            || metadata.uid() != super::super::identity::ProcessIdentity::current().uid
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(ServiceError::PermissionDenied);
        }
        Ok(())
    }
    fn write(&self, value: &serde_json::Value) -> Result<(), ServiceError> {
        crate::validate_no_follow_path(&self.directory, false)
            .map_err(|_| ServiceError::PermissionDenied)?;
        if !self.directory.exists() {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&self.directory)
                .map_err(|_| ServiceError::PermissionDenied)?;
        }
        self.validate_directory()?;
        let bytes = serde_json::to_vec(value).map_err(|_| ServiceError::Unknown)?;
        crate::atomic_write_document(&self.directory.join("transaction.json"), &bytes)
            .map_err(|_| ServiceError::Unknown)?;
        Ok(())
    }
    #[cfg(test)]
    pub fn at(directory: PathBuf) -> Self {
        Self { directory }
    }
}
