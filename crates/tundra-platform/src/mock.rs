use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::{
    AppPaths, FileAttributes, Platform, PlatformCapabilities, PlatformError, PlatformKind,
    ProcessExit, ProcessSpec, ProcessStream, UserDirs,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockCall {
    OpenPath(PathBuf),
    OpenWith { path: PathBuf, application: PathBuf },
    OpenUri(String),
    SpawnDetached(ProcessSpec),
    SpawnWait(ProcessSpec),
    ReadClipboardText,
    WriteClipboardText(String),
}

#[derive(Debug)]
pub struct MockPlatform {
    kind: PlatformKind,
    capabilities: PlatformCapabilities,
    user_dirs: UserDirs,
    app_paths: AppPaths,
    clipboard_text: Mutex<String>,
    calls: Mutex<Vec<MockCall>>,
    file_attributes: Mutex<BTreeMap<PathBuf, FileAttributes>>,
}

impl MockPlatform {
    pub fn new(user_dirs: UserDirs, app_paths: AppPaths) -> Self {
        Self {
            kind: PlatformKind::Unsupported,
            capabilities: PlatformCapabilities::native_supported(),
            user_dirs,
            app_paths,
            clipboard_text: Mutex::new(String::new()),
            calls: Mutex::new(Vec::new()),
            file_attributes: Mutex::new(BTreeMap::new()),
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

    pub fn set_file_attributes(&self, path: PathBuf, attributes: FileAttributes) {
        self.file_attributes
            .lock()
            .expect("file attributes lock poisoned")
            .insert(path, attributes);
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

    fn user_dirs(&self) -> Result<UserDirs, PlatformError> {
        Ok(self.user_dirs.clone())
    }

    fn app_paths(&self) -> Result<AppPaths, PlatformError> {
        Ok(self.app_paths.clone())
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

    fn file_attributes(&self, path: &Path) -> Result<FileAttributes, PlatformError> {
        if let Some(attributes) = self
            .file_attributes
            .lock()
            .expect("file attributes lock poisoned")
            .get(path)
            .cloned()
        {
            Ok(attributes)
        } else {
            crate::default_file_attributes(path)
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
