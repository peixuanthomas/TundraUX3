use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::{
    AppPaths, DirectoryListing, FileAttributes, FileOpenPolicy, LocalVolume, Platform,
    PlatformCapabilities, PlatformError, PlatformIcon, PlatformKind, ProcessExit, ProcessSpec,
    ProcessStream, StartupPermissionStatus, TrashEntry, TrashEntryId, TrashRestoreTarget,
    TrashStats, UserDirs,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockCall {
    StartupPermissionStatus,
    RequestStartupPermissions,
    FileIcon {
        path: PathBuf,
        preferred_size: u32,
    },
    OpenPath(PathBuf),
    OpenWith {
        path: PathBuf,
        application: PathBuf,
    },
    OpenUri(String),
    SpawnDetached(ProcessSpec),
    SpawnWait(ProcessSpec),
    ReadClipboardText,
    WriteClipboardText(String),
    ShowCriticalError {
        title: String,
        body: String,
    },
    IsProcessAlive(u32),
    ReadDirectory(PathBuf),
    RenamePath {
        source: PathBuf,
        target: PathBuf,
    },
    LocalVolumes,
    ListTrash,
    TrashStats,
    MoveToTrash(Vec<PathBuf>),
    EmptyTrash,
    RestoreTrashItem {
        id: TrashEntryId,
        target: TrashRestoreTarget,
    },
}

#[derive(Debug)]
pub struct MockPlatform {
    kind: PlatformKind,
    capabilities: PlatformCapabilities,
    user_dirs: UserDirs,
    app_paths: AppPaths,
    clipboard_text: Mutex<String>,
    startup_permission_status: Mutex<Result<StartupPermissionStatus, PlatformError>>,
    request_startup_permissions_result: Mutex<Result<(), PlatformError>>,
    critical_error_result: Mutex<Result<(), PlatformError>>,
    process_alive_results: Mutex<BTreeMap<u32, Result<bool, PlatformError>>>,
    calls: Mutex<Vec<MockCall>>,
    file_attributes: Mutex<BTreeMap<PathBuf, Result<FileAttributes, PlatformError>>>,
    directory_listings: Mutex<BTreeMap<PathBuf, Result<DirectoryListing, PlatformError>>>,
    file_open_policies: Mutex<BTreeMap<PathBuf, FileOpenPolicy>>,
    file_icons: Mutex<BTreeMap<PathBuf, Result<Option<PlatformIcon>, PlatformError>>>,
    rename_results: Mutex<BTreeMap<(PathBuf, PathBuf), Result<(), PlatformError>>>,
    local_volumes: Mutex<Result<Vec<LocalVolume>, PlatformError>>,
    trash_entries: Mutex<Result<Vec<TrashEntry>, PlatformError>>,
    trash_stats: Mutex<Result<TrashStats, PlatformError>>,
    move_to_trash_result: Mutex<Result<(), PlatformError>>,
    empty_trash_result: Mutex<Result<(), PlatformError>>,
    restore_results:
        Mutex<BTreeMap<(TrashEntryId, TrashRestoreTarget), Result<PathBuf, PlatformError>>>,
}

impl MockPlatform {
    pub fn new(user_dirs: UserDirs, app_paths: AppPaths) -> Self {
        Self {
            kind: PlatformKind::Unsupported,
            capabilities: PlatformCapabilities::native_supported(),
            user_dirs,
            app_paths,
            clipboard_text: Mutex::new(String::new()),
            startup_permission_status: Mutex::new(Ok(StartupPermissionStatus::Ready)),
            request_startup_permissions_result: Mutex::new(Ok(())),
            critical_error_result: Mutex::new(Ok(())),
            process_alive_results: Mutex::new(BTreeMap::new()),
            calls: Mutex::new(Vec::new()),
            file_attributes: Mutex::new(BTreeMap::new()),
            directory_listings: Mutex::new(BTreeMap::new()),
            file_open_policies: Mutex::new(BTreeMap::new()),
            file_icons: Mutex::new(BTreeMap::new()),
            rename_results: Mutex::new(BTreeMap::new()),
            local_volumes: Mutex::new(Ok(Vec::new())),
            trash_entries: Mutex::new(Ok(Vec::new())),
            trash_stats: Mutex::new(Ok(TrashStats::default())),
            move_to_trash_result: Mutex::new(Ok(())),
            empty_trash_result: Mutex::new(Ok(())),
            restore_results: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn with_kind(mut self, kind: PlatformKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_capabilities(mut self, capabilities: PlatformCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn set_clipboard_text(&self, text: impl Into<String>) {
        *self.clipboard_text.lock().expect("clipboard lock poisoned") = text.into();
    }

    pub fn set_startup_permission_status(
        &self,
        status: Result<StartupPermissionStatus, PlatformError>,
    ) {
        *self
            .startup_permission_status
            .lock()
            .expect("startup permission status lock poisoned") = status;
    }

    pub fn set_request_startup_permissions_result(&self, result: Result<(), PlatformError>) {
        *self
            .request_startup_permissions_result
            .lock()
            .expect("startup permission request lock poisoned") = result;
    }

    pub fn set_critical_error_result(&self, result: Result<(), PlatformError>) {
        *self
            .critical_error_result
            .lock()
            .expect("critical error result lock poisoned") = result;
    }

    pub fn set_process_alive_result(&self, pid: u32, result: Result<bool, PlatformError>) {
        self.process_alive_results
            .lock()
            .expect("process liveness lock poisoned")
            .insert(pid, result);
    }

    pub fn set_file_attributes(&self, path: PathBuf, attributes: FileAttributes) {
        self.file_attributes
            .lock()
            .expect("file attributes lock poisoned")
            .insert(path, Ok(attributes));
    }

    pub fn set_file_attributes_error(&self, path: PathBuf, error: PlatformError) {
        self.file_attributes
            .lock()
            .expect("file attributes lock poisoned")
            .insert(path, Err(error));
    }

    pub fn set_directory_listing(&self, path: PathBuf, listing: DirectoryListing) {
        self.directory_listings
            .lock()
            .expect("directory listings lock poisoned")
            .insert(path, Ok(listing));
    }

    pub fn set_directory_error(&self, path: PathBuf, error: PlatformError) {
        self.directory_listings
            .lock()
            .expect("directory listings lock poisoned")
            .insert(path, Err(error));
    }

    pub fn set_file_open_policy(&self, path: PathBuf, policy: FileOpenPolicy) {
        self.file_open_policies
            .lock()
            .expect("file open policies lock poisoned")
            .insert(path, policy);
    }

    pub fn set_file_icon(&self, path: PathBuf, icon: Option<PlatformIcon>) {
        self.set_file_icon_result(path, Ok(icon));
    }

    pub fn set_file_icon_result(
        &self,
        path: PathBuf,
        result: Result<Option<PlatformIcon>, PlatformError>,
    ) {
        self.file_icons
            .lock()
            .expect("file icons lock poisoned")
            .insert(path, result);
    }

    pub fn set_rename_result(
        &self,
        source: PathBuf,
        target: PathBuf,
        result: Result<(), PlatformError>,
    ) {
        self.rename_results
            .lock()
            .expect("rename results lock poisoned")
            .insert((source, target), result);
    }

    pub fn set_cross_device_rename(
        &self,
        source: PathBuf,
        target: PathBuf,
        message: impl Into<String>,
    ) {
        let error = PlatformError::CrossDevice {
            source: source.clone(),
            target: target.clone(),
            message: message.into(),
        };
        self.set_rename_result(source, target, Err(error));
    }

    pub fn trash_entry_id(value: impl Into<String>) -> TrashEntryId {
        TrashEntryId::from_native(value)
    }

    pub fn set_local_volumes_result(&self, result: Result<Vec<LocalVolume>, PlatformError>) {
        *self
            .local_volumes
            .lock()
            .expect("local volumes lock poisoned") = result;
    }

    pub fn set_trash_entries_result(&self, result: Result<Vec<TrashEntry>, PlatformError>) {
        *self
            .trash_entries
            .lock()
            .expect("trash entries lock poisoned") = result;
    }

    pub fn set_trash_stats_result(&self, result: Result<TrashStats, PlatformError>) {
        *self.trash_stats.lock().expect("trash stats lock poisoned") = result;
    }

    pub fn set_move_to_trash_result(&self, result: Result<(), PlatformError>) {
        *self
            .move_to_trash_result
            .lock()
            .expect("move to trash lock poisoned") = result;
    }

    pub fn set_empty_trash_result(&self, result: Result<(), PlatformError>) {
        *self
            .empty_trash_result
            .lock()
            .expect("empty trash lock poisoned") = result;
    }

    pub fn set_restore_result(
        &self,
        id: TrashEntryId,
        target: TrashRestoreTarget,
        result: Result<PathBuf, PlatformError>,
    ) {
        self.restore_results
            .lock()
            .expect("restore results lock poisoned")
            .insert((id, target), result);
    }

    pub fn calls(&self) -> Vec<MockCall> {
        self.calls.lock().expect("calls lock poisoned").clone()
    }

    fn record(&self, call: MockCall) {
        self.calls.lock().expect("calls lock poisoned").push(call);
    }
}

impl Platform for MockPlatform {
    fn kind(&self) -> PlatformKind {
        self.kind
    }

    fn capabilities(&self) -> PlatformCapabilities {
        self.capabilities.clone()
    }

    fn startup_permission_status(&self) -> Result<StartupPermissionStatus, PlatformError> {
        self.record(MockCall::StartupPermissionStatus);
        self.startup_permission_status
            .lock()
            .expect("startup permission status lock poisoned")
            .clone()
    }

    fn request_startup_permissions(&self) -> Result<(), PlatformError> {
        self.record(MockCall::RequestStartupPermissions);
        self.request_startup_permissions_result
            .lock()
            .expect("startup permission request lock poisoned")
            .clone()
    }

    fn user_dirs(&self) -> Result<UserDirs, PlatformError> {
        Ok(self.user_dirs.clone())
    }

    fn app_paths(&self) -> Result<AppPaths, PlatformError> {
        Ok(self.app_paths.clone())
    }

    fn file_icon(
        &self,
        path: &Path,
        preferred_size: u32,
    ) -> Result<Option<PlatformIcon>, PlatformError> {
        self.record(MockCall::FileIcon {
            path: path.to_path_buf(),
            preferred_size,
        });
        self.file_icons
            .lock()
            .expect("file icons lock poisoned")
            .get(path)
            .cloned()
            .unwrap_or(Ok(None))
    }

    fn open_path(&self, path: &Path) -> Result<(), PlatformError> {
        self.record(MockCall::OpenPath(path.to_path_buf()));
        Ok(())
    }

    fn open_with(&self, path: &Path, application: &Path) -> Result<(), PlatformError> {
        self.record(MockCall::OpenWith {
            path: path.to_path_buf(),
            application: application.to_path_buf(),
        });
        Ok(())
    }

    fn open_uri(&self, uri: &str) -> Result<(), PlatformError> {
        self.record(MockCall::OpenUri(uri.to_string()));
        Ok(())
    }

    fn spawn_detached(&self, spec: &ProcessSpec) -> Result<(), PlatformError> {
        self.record(MockCall::SpawnDetached(spec.clone()));
        Ok(())
    }

    fn spawn_wait(&self, spec: &ProcessSpec) -> Result<ProcessExit, PlatformError> {
        self.record(MockCall::SpawnWait(spec.clone()));
        Ok(ProcessExit {
            code: Some(0),
            stdout: ProcessStream::from_bytes(Vec::new()),
            stderr: ProcessStream::from_bytes(Vec::new()),
        })
    }

    fn read_clipboard_text(&self) -> Result<String, PlatformError> {
        self.record(MockCall::ReadClipboardText);
        Ok(self
            .clipboard_text
            .lock()
            .expect("clipboard lock poisoned")
            .clone())
    }

    fn write_clipboard_text(&self, text: &str) -> Result<(), PlatformError> {
        self.record(MockCall::WriteClipboardText(text.to_string()));
        *self.clipboard_text.lock().expect("clipboard lock poisoned") = text.to_string();
        Ok(())
    }

    fn show_critical_error(&self, title: &str, body: &str) -> Result<(), PlatformError> {
        self.record(MockCall::ShowCriticalError {
            title: title.to_string(),
            body: body.to_string(),
        });
        self.critical_error_result
            .lock()
            .expect("critical error result lock poisoned")
            .clone()
    }

    fn is_process_alive(&self, pid: u32) -> Result<bool, PlatformError> {
        self.record(MockCall::IsProcessAlive(pid));
        self.process_alive_results
            .lock()
            .expect("process liveness lock poisoned")
            .get(&pid)
            .cloned()
            .unwrap_or(Ok(false))
    }

    fn local_volumes(&self) -> Result<Vec<LocalVolume>, PlatformError> {
        self.record(MockCall::LocalVolumes);
        self.local_volumes
            .lock()
            .expect("local volumes lock poisoned")
            .clone()
    }

    fn list_trash(&self) -> Result<Vec<TrashEntry>, PlatformError> {
        self.record(MockCall::ListTrash);
        self.trash_entries
            .lock()
            .expect("trash entries lock poisoned")
            .clone()
    }

    fn trash_stats(&self) -> Result<TrashStats, PlatformError> {
        self.record(MockCall::TrashStats);
        self.trash_stats
            .lock()
            .expect("trash stats lock poisoned")
            .clone()
    }

    fn move_to_trash(&self, paths: &[PathBuf]) -> Result<(), PlatformError> {
        self.record(MockCall::MoveToTrash(paths.to_vec()));
        self.move_to_trash_result
            .lock()
            .expect("move to trash lock poisoned")
            .clone()
    }

    fn empty_trash(&self) -> Result<(), PlatformError> {
        self.record(MockCall::EmptyTrash);
        self.empty_trash_result
            .lock()
            .expect("empty trash lock poisoned")
            .clone()
    }

    fn restore_trash_item(
        &self,
        id: &TrashEntryId,
        target: TrashRestoreTarget,
    ) -> Result<PathBuf, PlatformError> {
        self.record(MockCall::RestoreTrashItem {
            id: id.clone(),
            target: target.clone(),
        });
        self.restore_results
            .lock()
            .expect("restore results lock poisoned")
            .get(&(id.clone(), target))
            .cloned()
            .unwrap_or_else(|| {
                Err(PlatformError::InvalidInput {
                    message: format!("no mock restore result for trash entry {}", id.as_str()),
                })
            })
    }

    fn file_attributes(&self, path: &Path) -> Result<FileAttributes, PlatformError> {
        if let Some(attributes) = self
            .file_attributes
            .lock()
            .expect("file attributes lock poisoned")
            .get(path)
            .cloned()
        {
            attributes
        } else {
            crate::default_file_attributes(path)
        }
    }

    fn read_directory(&self, path: &Path) -> Result<DirectoryListing, PlatformError> {
        self.record(MockCall::ReadDirectory(path.to_path_buf()));
        if let Some(listing) = self
            .directory_listings
            .lock()
            .expect("directory listings lock poisoned")
            .get(path)
            .cloned()
        {
            listing
        } else {
            crate::default_read_directory(self, path)
        }
    }

    fn external_open_policy(
        &self,
        path: &Path,
        attributes: &FileAttributes,
    ) -> crate::ExternalOpenPolicy {
        crate::ExternalOpenPolicy::from_file_open_policy(self.file_open_policy(path, attributes))
    }

    fn file_open_policy(&self, path: &Path, attributes: &FileAttributes) -> FileOpenPolicy {
        self.file_open_policies
            .lock()
            .expect("file open policies lock poisoned")
            .get(path)
            .cloned()
            .unwrap_or_else(|| crate::default_file_open_policy(self.kind, path, attributes))
    }

    fn rename_path(&self, source: &Path, target: &Path) -> Result<(), PlatformError> {
        self.record(MockCall::RenamePath {
            source: source.to_path_buf(),
            target: target.to_path_buf(),
        });
        if let Some(result) = self
            .rename_results
            .lock()
            .expect("rename results lock poisoned")
            .get(&(source.to_path_buf(), target.to_path_buf()))
            .cloned()
        {
            result
        } else {
            crate::default_rename_path(source, target)
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedPlatform;

impl Platform for UnsupportedPlatform {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Unsupported
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities::unsupported()
    }

    fn user_dirs(&self) -> Result<UserDirs, PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "user_dirs",
        })
    }

    fn app_paths(&self) -> Result<AppPaths, PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "app_paths",
        })
    }

    fn open_path(&self, _path: &Path) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "open_path",
        })
    }

    fn open_with(&self, _path: &Path, _application: &Path) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "open_with",
        })
    }

    fn open_uri(&self, _uri: &str) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "open_uri",
        })
    }

    fn spawn_detached(&self, _spec: &ProcessSpec) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "spawn_detached",
        })
    }

    fn spawn_wait(&self, _spec: &ProcessSpec) -> Result<ProcessExit, PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "spawn_wait",
        })
    }

    fn read_clipboard_text(&self) -> Result<String, PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "clipboard_text",
        })
    }

    fn write_clipboard_text(&self, _text: &str) -> Result<(), PlatformError> {
        Err(PlatformError::Unsupported {
            capability: "clipboard_text",
        })
    }
}
