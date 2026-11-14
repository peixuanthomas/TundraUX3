use super::{Hello, WorkerId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceClientMessage {
    Hello(Hello),
    Request(ServiceRequest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceHostMessage {
    Welcome { worker: WorkerId },
    Reply(ServiceReply),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceRequest {
    pub id: u64,
    pub operation: ServiceOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceOperation {
    ReadConfig,
    WriteConfig { revision: u64, config: Value },
    ReadAuthorization,
    WriteAuthorization { revision: u64, policy: Value },
    ReadProfile,
    WriteProfile { revision: u64, profile: Value },
    ReadClockProfile,
    WriteClockProfile { revision: u64, profile: Value },
    DeleteClockProfile { user_id: String },
    ReadClockTime,
    WriteClockTime { anchor: Value },
    ReadFileOperations,
    UpdateFileOperations { operation: Value },
    ReadSystemSnapshot,
    WriteSystemSnapshot { snapshot: Value },
    RequestSystemStatus { active: Option<bool>, refresh: bool },
    ReadSystemStatusRequests,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceReply {
    pub id: u64,
    pub result: Result<ServiceValue, ServiceError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceValue {
    Versioned { revision: u64, value: Value },
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceError {
    Conflict,
    Forbidden,
    InvalidRequest,
    Unavailable,
    OperationFailed { message: Value },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NativePath(#[serde(with = "native_path")] pub PathBuf);

/// Encodes native paths losslessly between processes on the same operating system.
pub mod native_path {
    #[cfg(unix)]
    use serde::Serialize;
    use serde::{Deserialize, Deserializer, Serializer};
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};

    #[cfg(unix)]
    pub fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().serialize(serializer)
    }

    #[cfg(unix)]
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
        use std::os::unix::ffi::OsStringExt;
        Vec::<u8>::deserialize(deserializer).map(|bytes| PathBuf::from(OsString::from_vec(bytes)))
    }

    #[cfg(windows)]
    pub fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
        use std::os::windows::ffi::OsStrExt;
        serializer.collect_seq(path.as_os_str().encode_wide())
    }

    #[cfg(windows)]
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
        use std::os::windows::ffi::OsStringExt;
        Vec::<u16>::deserialize(deserializer)
            .map(|units| PathBuf::from(OsString::from_wide(&units)))
    }
}

/// Serde adapter for a collection of native paths.
pub mod native_paths {
    use super::{NativePath, native_path};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::path::{Path, PathBuf};

    pub fn serialize<S: Serializer>(paths: &[PathBuf], serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(transparent)]
        struct Borrowed<'a>(#[serde(with = "native_path")] &'a Path);

        serializer.collect_seq(paths.iter().map(|path| Borrowed(path.as_path())))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<PathBuf>, D::Error> {
        Vec::<NativePath>::deserialize(deserializer)
            .map(|paths| paths.into_iter().map(|path| path.0).collect())
    }
}
