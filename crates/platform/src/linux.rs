//! Freedesktop/Linux platform integration.
//!
//! This module deliberately never passes a path to a shell.  Desktop helpers
//! are started with `Command`, detached from the TUI, and reaped by a small
//! background waiter so a slow portal/desktop helper cannot freeze the UI or
//! leave a zombie behind.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::{CString, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use arboard::Clipboard;
use freedesktop_desktop_entry::DesktopEntry;
use image::ImageReader;
use watchdog::{
    AppCriticality, AppDescriptor, AppId, AppWatchdog, ProcessWatchdog, RestartPolicy, TaskId,
    TaskSpec,
};

use crate::paths::home_dir_from_env;
use crate::{
    AppPaths, ExecutableKind, FileAttributes, FileOpenPolicy, LocalVolume, NetworkInterface,
    NetworkInterfaceKind, NetworkLinkState, NetworkStatus, Platform, PlatformCapabilities,
    PlatformError, PlatformIcon, PlatformKind, PlatformLifecycleEvent, ProcessExit, ProcessSpec,
    TrashEntry, TrashEntryId, TrashRestoreTarget, TrashStats, UserDirs, VolumeAccess, VolumeKind,
    build_linux_app_paths,
};

const XDG_OPEN: &str = "xdg-open";
const GIO: &str = "gio";
const TRASH_INFO_SUFFIX: &str = ".trashinfo";

static CLIPBOARD: OnceLock<Mutex<Option<Clipboard>>> = OnceLock::new();
static LIFECYCLE_EVENTS: OnceLock<Mutex<LifecycleEvents>> = OnceLock::new();
static DETACHED_TASK_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, Default)]
pub struct LinuxPlatform;

impl Platform for LinuxPlatform {
    fn kind(&self) -> PlatformKind {
        PlatformKind::Linux
    }

    fn capabilities(&self) -> PlatformCapabilities {
        // Runtime services (D-Bus, data-control, xdg-utils) are diagnosed by
        // `tundra-cli doctor`; their absence must not demote Linux itself to an
        // unsupported platform.
        PlatformCapabilities::native_supported()
    }

    fn is_native_backend(&self) -> bool {
        true
    }

    fn user_dirs(&self) -> Result<UserDirs, PlatformError> {
        let home = home_dir_from_env()?;
        let base_dirs = XdgBaseDirs::from_environment(&home);
        let user_dirs = XdgUserDirs::from_file(&base_dirs.config.join("user-dirs.dirs"), &home);

        UserDirs::new(
            user_dirs.desktop.unwrap_or_else(|| home.join("Desktop")),
            user_dirs
                .documents
                .unwrap_or_else(|| home.join("Documents")),
            user_dirs.download.unwrap_or_else(|| home.join("Downloads")),
            user_dirs.pictures.unwrap_or_else(|| home.join("Pictures")),
            user_dirs.videos.unwrap_or_else(|| home.join("Videos")),
            user_dirs.music.unwrap_or_else(|| home.join("Music")),
            base_dirs.data,
        )
        .map_err(Into::into)
    }

    fn app_paths(&self) -> Result<AppPaths, PlatformError> {
        let home = home_dir_from_env()?;
        let base_dirs = XdgBaseDirs::from_environment(&home);
        build_linux_app_paths(
            base_dirs.config,
            base_dirs.data,
            base_dirs.cache,
            base_dirs.state,
            linux_runtime_dir()?,
        )
        .map_err(Into::into)
    }

    fn system_time(&self) -> Result<SystemTime, PlatformError> {
        Ok(SystemTime::now())
    }

    fn file_icon(
        &self,
        path: &Path,
        preferred_size: u32,
    ) -> Result<Option<PlatformIcon>, PlatformError> {
        let icon_name = desktop_icon_name(path).or_else(|| {
            path.extension()
                .and_then(|value| value.to_str())
                .map(|extension| format!("text-x-{}", extension.to_ascii_lowercase()))
        });
        let Some(icon_name) = icon_name else {
            return Ok(None);
        };
        let Some(icon_path) = resolve_icon(&icon_name, preferred_size) else {
            return Ok(None);
        };
        decode_icon(&icon_path, preferred_size).map(Some)
    }

    fn open_path(&self, path: &Path) -> Result<(), PlatformError> {
        let mut command = Command::new(XDG_OPEN);
        command.arg(path);
        run_detached(command, "open with xdg-open")
    }

    fn launch_approved(&self, path: &Path, kind: ExecutableKind) -> Result<(), PlatformError> {
        match kind {
            ExecutableKind::NativeBinary | ExecutableKind::Script => {
                run_detached(Command::new(path), "launch approved Linux executable")
            }
            ExecutableKind::Shortcut => {
                validate_desktop_entry(path)?;
                let mut command = Command::new(GIO);
                command.args(["launch"]).arg(path);
                run_detached(command, "launch approved desktop entry")
            }
            ExecutableKind::Installer | ExecutableKind::ApplicationBundle => self.open_path(path),
        }
    }

    fn open_with(&self, path: &Path, application: &Path) -> Result<(), PlatformError> {
        if application.as_os_str().is_empty() {
            return Err(PlatformError::InvalidInput {
                message: "application path must not be empty".to_string(),
            });
        }
        let mut command = Command::new(application);
        command.arg(path);
        run_detached(command, "open path with Linux application")
    }

    fn open_uri(&self, uri: &str) -> Result<(), PlatformError> {
        if uri.trim().is_empty() {
            return Err(PlatformError::InvalidInput {
                message: "URI must not be empty".to_string(),
            });
        }
        let mut command = Command::new(XDG_OPEN);
        command.arg(uri);
        run_detached(command, "open URI with xdg-open")
    }

    fn spawn_detached(&self, spec: &ProcessSpec) -> Result<(), PlatformError> {
        crate::process::spawn_detached_impl(spec, false)
    }

    fn spawn_wait(&self, spec: &ProcessSpec) -> Result<ProcessExit, PlatformError> {
        crate::process::spawn_wait_impl(spec, false)
    }

    fn read_clipboard_text(&self) -> Result<String, PlatformError> {
        with_clipboard(|clipboard| clipboard.get_text()).map_err(clipboard_error)
    }

    fn write_clipboard_text(&self, text: &str) -> Result<(), PlatformError> {
        with_clipboard(|clipboard| clipboard.set_text(text.to_owned())).map_err(clipboard_error)
    }

    fn local_volumes(&self) -> Result<Vec<LocalVolume>, PlatformError> {
        local_volumes()
    }

    fn network_status(&self) -> Result<NetworkStatus, PlatformError> {
        linux_network_status()
    }

    fn list_trash(&self) -> Result<Vec<TrashEntry>, PlatformError> {
        let mut entries = Vec::new();
        for root in trash_roots()? {
            entries.extend(list_trash_root(&root)?);
        }
        entries.sort_by(|left, right| left.display_name.cmp(&right.display_name));
        Ok(entries)
    }

    fn trash_stats(&self) -> Result<TrashStats, PlatformError> {
        self.list_trash().map(|entries| TrashStats {
            item_count: entries.len() as u64,
            total_bytes: entries.iter().map(|entry| entry.size).sum(),
        })
    }

    fn move_to_trash(&self, paths: &[PathBuf]) -> Result<(), PlatformError> {
        for path in paths {
            move_one_to_trash(path)?;
        }
        Ok(())
    }

    fn empty_trash(&self) -> Result<(), PlatformError> {
        for root in trash_roots()? {
            empty_trash_root(&root)?;
        }
        Ok(())
    }

    fn restore_trash_item(
        &self,
        id: &TrashEntryId,
        target: TrashRestoreTarget,
    ) -> Result<PathBuf, PlatformError> {
        restore_trash_item(id, target)
    }

    fn file_open_policy(&self, path: &Path, attributes: &FileAttributes) -> FileOpenPolicy {
        if attributes.symlink || attributes.junction || attributes.reparse_point {
            return FileOpenPolicy::blocked(
                "symbolic links and reparse points are blocked until safe path traversal is available",
            );
        }
        if attributes.is_file && extension_is(path, "desktop") {
            return FileOpenPolicy::launcher_required(
                ExecutableKind::Shortcut,
                ".desktop launchers must be reviewed through Launcher",
            );
        }
        if attributes.is_file && is_script(path) {
            return FileOpenPolicy::launcher_required(
                ExecutableKind::Script,
                "scripts must be opened through Launcher",
            );
        }
        if attributes.is_file
            && (is_elf(path) || extension_is(path, "appimage") || executable_bit(path))
        {
            return FileOpenPolicy::launcher_required(
                ExecutableKind::NativeBinary,
                "executable files must be opened through Launcher",
            );
        }
        FileOpenPolicy::system_default()
    }

    fn show_critical_error(&self, title: &str, body: &str) -> Result<(), PlatformError> {
        // The notification service is session D-Bus based, so it is available
        // without adding a `notify-send` package dependency. Watchdog/stderr
        // remain the durable error report if the session service is absent.
        let connection = zbus::blocking::Connection::session()
            .map_err(zbus_error("connect to desktop notification D-Bus"))?;
        let proxy = zbus::blocking::Proxy::new(
            &connection,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .map_err(zbus_error("create desktop notification proxy"))?;
        let hints = HashMap::<String, zbus::zvariant::OwnedValue>::new();
        proxy
            .call::<_, _, u32>(
                "Notify",
                &(
                    "TundraUX3",
                    0_u32,
                    "",
                    title,
                    body,
                    Vec::<String>::new(),
                    hints,
                    -1_i32,
                ),
            )
            .map(|_| ())
            .map_err(zbus_error("show Linux desktop notification"))
    }

    fn is_process_alive(&self, pid: u32) -> Result<bool, PlatformError> {
        if pid == 0 {
            return Ok(false);
        }
        let process_path = PathBuf::from("/proc").join(pid.to_string());
        match fs::metadata(&process_path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == ErrorKind::PermissionDenied => Ok(true),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(io_error(
                "check Linux process liveness",
                Some(process_path),
                error,
            )),
        }
    }

    fn poweroff(&self) -> Result<(), PlatformError> {
        with_logind_proxy(|proxy| {
            let availability = logind_can_poweroff(proxy)?;
            if !logind_allows_poweroff(&availability) {
                return Err(PlatformError::Native {
                    operation: "query logind power-off availability",
                    message: format!("logind returned {availability:?}"),
                });
            }
            proxy
                .call::<_, _, ()>("PowerOff", &(true,))
                .map_err(zbus_error("request interactive logind power-off"))
        })
    }

    fn can_poweroff(&self) -> Result<bool, PlatformError> {
        with_logind_proxy(|proxy| Ok(logind_allows_poweroff(&logind_can_poweroff(proxy)?)))
    }

    fn poll_lifecycle_event(&self) -> Result<Option<PlatformLifecycleEvent>, PlatformError> {
        let events = lifecycle_events();
        let mut events = events.lock().map_err(|_| PlatformError::Native {
            operation: "read Linux lifecycle event",
            message: "event receiver is poisoned".to_string(),
        })?;
        events.ensure_listeners();
        match events.receiver.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => Ok(None),
        }
    }

    fn refresh_session(&self) -> Result<(), PlatformError> {
        let mut clipboard = clipboard_store()
            .lock()
            .map_err(|_| PlatformError::Native {
                operation: "refresh Linux clipboard",
                message: "clipboard state is poisoned".to_string(),
            })?;
        *clipboard = None;
        Ok(())
    }
}

fn clipboard_store() -> &'static Mutex<Option<Clipboard>> {
    CLIPBOARD.get_or_init(|| Mutex::new(None))
}

fn with_clipboard<T>(
    operation: impl Fn(&mut Clipboard) -> Result<T, arboard::Error>,
) -> Result<T, arboard::Error> {
    let mut slot = clipboard_store()
        .lock()
        .map_err(|_| arboard::Error::ContentNotAvailable)?;
    if slot.is_none() {
        *slot = Some(Clipboard::new()?);
    }
    match operation(slot.as_mut().expect("clipboard initialized")) {
        Ok(value) => Ok(value),
        Err(first_error) => {
            // Wayland compositors can restart their data-control connection.
            // Reconnect once while keeping a long-lived owner in the normal path.
            *slot = Some(Clipboard::new()?);
            operation(slot.as_mut().expect("clipboard rebuilt")).map_err(|_| first_error)
        }
    }
}

fn clipboard_error(error: arboard::Error) -> PlatformError {
    PlatformError::Native {
        operation: "access Linux clipboard",
        message: error.to_string(),
    }
}

fn zbus_error(operation: &'static str) -> impl FnOnce(zbus::Error) -> PlatformError {
    move |error| PlatformError::Native {
        operation,
        message: error.to_string(),
    }
}

fn with_logind_proxy<T>(
    operation: impl FnOnce(&zbus::blocking::Proxy<'_>) -> Result<T, PlatformError>,
) -> Result<T, PlatformError> {
    let connection =
        zbus::blocking::Connection::system().map_err(zbus_error("connect to system D-Bus"))?;
    let proxy = zbus::blocking::Proxy::new(
        &connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .map_err(zbus_error("create logind proxy"))?;
    operation(&proxy)
}

fn logind_can_poweroff(proxy: &zbus::blocking::Proxy<'_>) -> Result<String, PlatformError> {
    proxy
        .call("CanPowerOff", &())
        .map_err(zbus_error("query logind power-off availability"))
}

fn logind_allows_poweroff(value: &str) -> bool {
    matches!(value, "yes" | "challenge")
}

struct LifecycleEvents {
    sender: mpsc::Sender<PlatformLifecycleEvent>,
    receiver: mpsc::Receiver<PlatformLifecycleEvent>,
    shutdown_listener_started: bool,
    sleep_listener_started: bool,
}

impl LifecycleEvents {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver,
            shutdown_listener_started: false,
            sleep_listener_started: false,
        }
    }

    fn ensure_listeners(&mut self) {
        if !self.shutdown_listener_started {
            self.shutdown_listener_started = start_logind_listener(
                self.sender.clone(),
                "PrepareForShutdown",
                PlatformLifecycleEvent::PrepareForShutdown,
                None,
            )
            .is_ok();
        }
        if !self.sleep_listener_started {
            self.sleep_listener_started = start_logind_listener(
                self.sender.clone(),
                "PrepareForSleep",
                PlatformLifecycleEvent::PrepareForSleep,
                Some(PlatformLifecycleEvent::Resumed),
            )
            .is_ok();
        }
    }
}

fn lifecycle_events() -> &'static Mutex<LifecycleEvents> {
    LIFECYCLE_EVENTS.get_or_init(|| Mutex::new(LifecycleEvents::new()))
}

fn start_logind_listener(
    sender: mpsc::Sender<PlatformLifecycleEvent>,
    signal: &'static str,
    on_true: PlatformLifecycleEvent,
    on_false: Option<PlatformLifecycleEvent>,
) -> Result<(), PlatformError> {
    let app = managed_platform_watchdog()?;
    let id = TaskId::new(format!("logind-{}", signal.to_ascii_lowercase())).map_err(|error| {
        PlatformError::Native {
            operation: "create logind listener task id",
            message: error.to_string(),
        }
    })?;
    let spec = TaskSpec::idempotent_service(
        id,
        RestartPolicy::limited(3, Duration::from_secs(60), vec![Duration::from_secs(1)]),
    );
    app.task_group("platform-linux")
        .spawn_thread(spec, move || {
            // A session/system bus can reconnect after suspend or a desktop
            // service restart. Keep the managed worker alive and resubscribe
            // instead of treating a clean signal-stream end as success.
            loop {
                if let Ok(connection) = zbus::blocking::Connection::system()
                    && let Ok(proxy) = zbus::blocking::Proxy::new(
                        &connection,
                        "org.freedesktop.login1",
                        "/org/freedesktop/login1",
                        "org.freedesktop.login1.Manager",
                    )
                    && let Ok(signals) = proxy.receive_signal(signal)
                {
                    for message in signals {
                        if let Ok(preparing) = message.body().deserialize::<bool>() {
                            let event = if preparing { Some(on_true) } else { on_false };
                            if let Some(event) = event {
                                if event == PlatformLifecycleEvent::PrepareForShutdown {
                                    crate::terminal::request_process_shutdown();
                                }
                                let _ = sender.send(event);
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .map(|_| ())
        .map_err(|error| PlatformError::Native {
            operation: "start logind listener",
            message: error.to_string(),
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct XdgBaseDirs {
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
    state: PathBuf,
}

impl XdgBaseDirs {
    fn from_environment(home: &Path) -> Self {
        Self::resolve(
            home,
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
            std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
        )
    }

    fn resolve(
        home: &Path,
        config: Option<PathBuf>,
        data: Option<PathBuf>,
        cache: Option<PathBuf>,
        state: Option<PathBuf>,
    ) -> Self {
        Self {
            config: absolute_or(config, || home.join(".config")),
            data: absolute_or(data, || home.join(".local").join("share")),
            cache: absolute_or(cache, || home.join(".cache")),
            state: absolute_or(state, || home.join(".local").join("state")),
        }
    }
}

#[derive(Default)]
struct XdgUserDirs {
    desktop: Option<PathBuf>,
    documents: Option<PathBuf>,
    download: Option<PathBuf>,
    pictures: Option<PathBuf>,
    videos: Option<PathBuf>,
    music: Option<PathBuf>,
}

impl XdgUserDirs {
    fn from_file(path: &Path, home: &Path) -> Self {
        let Ok(contents) = fs::read_to_string(path) else {
            return Self::default();
        };
        let mut result = Self::default();
        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, raw_value)) = line.split_once('=') else {
                continue;
            };
            let Some(value) = parse_user_dir_value(raw_value, home) else {
                continue;
            };
            match key.trim() {
                "XDG_DESKTOP_DIR" => result.desktop = Some(value),
                "XDG_DOCUMENTS_DIR" => result.documents = Some(value),
                "XDG_DOWNLOAD_DIR" => result.download = Some(value),
                "XDG_PICTURES_DIR" => result.pictures = Some(value),
                "XDG_VIDEOS_DIR" => result.videos = Some(value),
                "XDG_MUSIC_DIR" => result.music = Some(value),
                _ => {}
            }
        }
        result
    }
}

fn parse_user_dir_value(raw_value: &str, home: &Path) -> Option<PathBuf> {
    let raw_value = raw_value.trim();
    let encoded = raw_value.strip_prefix('"')?.strip_suffix('"')?;
    let mut decoded = String::with_capacity(encoded.len());
    let mut escaped = false;
    for character in encoded.chars() {
        if escaped {
            match character {
                '\\' | '"' | '$' | '`' => decoded.push(character),
                _ => return None,
            }
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            decoded.push(character);
        }
    }
    if escaped {
        return None;
    }

    let value = if decoded == "$HOME" {
        home.to_path_buf()
    } else if let Some(relative) = decoded.strip_prefix("$HOME/") {
        home.join(relative)
    } else {
        PathBuf::from(decoded)
    };
    value.is_absolute().then_some(value)
}

fn absolute_or(candidate: Option<PathBuf>, fallback: impl FnOnce() -> PathBuf) -> PathBuf {
    candidate
        .filter(|path| !path.as_os_str().is_empty() && path.is_absolute())
        .unwrap_or_else(fallback)
}

fn linux_runtime_dir() -> Result<PathBuf, PlatformError> {
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .filter(|path| path_is_private_directory(path).unwrap_or(false))
    {
        return Ok(runtime);
    }
    let fallback = PathBuf::from(format!("/tmp/tundraux3-{}", unsafe { libc::geteuid() }));
    ensure_private_dir(&fallback)?;
    Ok(fallback)
}

fn managed_platform_watchdog() -> Result<AppWatchdog, PlatformError> {
    if let Some(app) = AppWatchdog::current() {
        return Ok(app);
    }
    let process = ProcessWatchdog::global().ok_or_else(|| PlatformError::Native {
        operation: "start managed Linux background task",
        message: "the process watchdog is not active".to_string(),
    })?;
    process
        .register_app(AppDescriptor::new(
            AppId::from_static("platform-linux"),
            "Linux platform integration",
            env!("CARGO_PKG_VERSION"),
            AppCriticality::Optional,
        ))
        .map_err(|error| PlatformError::Native {
            operation: "register Linux platform watchdog",
            message: error.to_string(),
        })
}

fn run_detached(mut command: Command, operation: &'static str) -> Result<(), PlatformError> {
    let app = managed_platform_watchdog()?;
    let program = command.get_program().to_os_string();
    let sequence = DETACHED_TASK_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let id =
        TaskId::new(format!("child-launch-{sequence}")).map_err(|error| PlatformError::Native {
            operation: "create Linux child reaper task id",
            message: error.to_string(),
        })?;
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Process creation stays synchronous so callers can accurately report a
    // missing opener/application. Only the potentially long wait is moved to
    // the watchdog-managed worker.
    let child = spawn_detached_child(&mut command, operation, &program)?;
    let child = Arc::new(Mutex::new(Some(child)));
    let worker_child = Arc::clone(&child);
    let spawned =
        app.task_group("platform-linux")
            .spawn_thread(TaskSpec::one_shot(id), move || {
                if let Some(mut child) = worker_child
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                {
                    let _ = child.wait();
                }
            });
    if let Err(error) = spawned {
        // A worker-creation failure must not leave a child process behind. Do
        // not let it continue: terminate first, then reap the now-dead child.
        if let Some(mut child) = child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        return Err(PlatformError::Native {
            operation: "start Linux child reaper",
            message: error.to_string(),
        });
    }
    Ok(())
}

fn spawn_detached_child(
    command: &mut Command,
    operation: &'static str,
    program: &OsString,
) -> Result<Child, PlatformError> {
    command
        .spawn()
        .map_err(|error| io_error(operation, Some(PathBuf::from(program)), error))
}

#[derive(Debug, Clone)]
struct MountInfo {
    mount_point: PathBuf,
    fs_type: String,
    major_minor: String,
    read_only: bool,
}

fn parse_mountinfo(reader: impl BufRead) -> Vec<MountInfo> {
    reader
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let before: Vec<_> = before.split_whitespace().collect();
            let after: Vec<_> = after.split_whitespace().collect();
            if before.len() < 5 || after.is_empty() {
                return None;
            }
            Some(MountInfo {
                mount_point: unescape_mount_field(before[4]),
                fs_type: after[0].to_string(),
                major_minor: before[2].to_string(),
                read_only: before[5].split(',').any(|option| option == "ro"),
            })
        })
        .collect()
}

fn unescape_mount_field(value: &str) -> PathBuf {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\'
            && index + 3 < bytes.len()
            && bytes[index + 1..index + 4].iter().all(u8::is_ascii_digit)
        {
            let octal = std::str::from_utf8(&bytes[index + 1..index + 4])
                .ok()
                .and_then(|part| u8::from_str_radix(part, 8).ok());
            if let Some(octal) = octal {
                output.push(octal);
                index += 4;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    PathBuf::from(OsString::from_vec(output))
}

fn local_volumes() -> Result<Vec<LocalVolume>, PlatformError> {
    let file = File::open("/proc/self/mountinfo").map_err(|error| {
        io_error(
            "read Linux mountinfo",
            Some(PathBuf::from("/proc/self/mountinfo")),
            error,
        )
    })?;
    let mut roots = BTreeSet::new();
    let mut result = Vec::new();
    for mount in parse_mountinfo(BufReader::new(file)) {
        if !is_local_block_mount(&mount)
            || !mount.mount_point.is_dir()
            || !roots.insert(mount.mount_point.clone())
        {
            continue;
        }
        let capacity = statvfs_bytes(&mount.mount_point);
        let capacity_available = capacity.is_ok();
        let (total_bytes, available_bytes) = capacity.unwrap_or((None, None));
        result.push(LocalVolume {
            label: mount
                .mount_point
                .file_name()
                .map(|part| part.to_string_lossy().into_owned())
                .filter(|label| !label.is_empty()),
            kind: mount_kind(&mount.major_minor),
            root: mount.mount_point,
            total_bytes,
            available_bytes,
            is_system: mount.mount_point == Path::new("/"),
            access: if mount.read_only {
                VolumeAccess::ReadOnly
            } else if capacity_available {
                VolumeAccess::ReadWrite
            } else {
                VolumeAccess::Unavailable
            },
        });
    }
    Ok(result)
}

fn linux_network_status() -> Result<NetworkStatus, PlatformError> {
    let mut head = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Err(io_error(
            "enumerate Linux network interfaces",
            None,
            io::Error::last_os_error(),
        ));
    }
    let mut interfaces: HashMap<String, NetworkInterface> = HashMap::new();
    let mut current = head;
    while !current.is_null() {
        let entry = unsafe { &*current };
        let name = unsafe { std::ffi::CStr::from_ptr(entry.ifa_name) }
            .to_string_lossy()
            .into_owned();
        let loopback = entry.ifa_flags & libc::IFF_LOOPBACK as u32 != 0;
        if !loopback {
            let interface = interfaces
                .entry(name.clone())
                .or_insert_with(|| NetworkInterface {
                    name: name.clone(),
                    display_name: None,
                    kind: linux_interface_kind(&name),
                    link_state: linux_link_state(&name, entry.ifa_flags),
                    addresses: Vec::new(),
                });
            if !entry.ifa_addr.is_null() {
                let family = unsafe { (*entry.ifa_addr).sa_family as i32 };
                let address = match family {
                    libc::AF_INET => {
                        let value = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_in) };
                        Some(std::net::IpAddr::V4(std::net::Ipv4Addr::from(
                            value.sin_addr.s_addr.to_ne_bytes(),
                        )))
                    }
                    libc::AF_INET6 => {
                        let value = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_in6) };
                        Some(std::net::IpAddr::V6(std::net::Ipv6Addr::from(
                            value.sin6_addr.s6_addr,
                        )))
                    }
                    _ => None,
                };
                if let Some(address) = address.filter(|address| !address.is_loopback()) {
                    interface.addresses.push(address);
                }
            }
        }
        current = entry.ifa_next;
    }
    unsafe { libc::freeifaddrs(head) };
    Ok(NetworkStatus::new(interfaces.into_values().collect()))
}

fn linux_interface_kind(name: &str) -> NetworkInterfaceKind {
    linux_interface_kind_with_sysfs(
        name,
        Path::new("/sys/class/net"),
        Path::new("/sys/devices/virtual/net"),
    )
}

fn linux_interface_kind_with_sysfs(
    name: &str,
    sys_class_net: &Path,
    sys_virtual_net: &Path,
) -> NetworkInterfaceKind {
    const VIRTUAL_PREFIXES: &[&str] = &[
        "veth",
        "docker",
        "br-",
        "virbr",
        "tun",
        "tap",
        "wg",
        "tailscale",
        "br0",
        "cni",
        "flannel",
    ];
    let canonical_virtual = sys_class_net
        .join(name)
        .canonicalize()
        .ok()
        .is_some_and(|path| path.starts_with(sys_virtual_net));
    if sys_virtual_net.join(name).exists()
        || canonical_virtual
        || VIRTUAL_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
    {
        NetworkInterfaceKind::Virtual
    } else if sys_class_net.join(name).join("wireless").exists() {
        NetworkInterfaceKind::Wireless
    } else if sys_class_net.join(name).exists() {
        NetworkInterfaceKind::Wired
    } else {
        NetworkInterfaceKind::Unknown
    }
}

fn linux_link_state(name: &str, flags: u32) -> NetworkLinkState {
    match fs::read_to_string(Path::new("/sys/class/net").join(name).join("operstate"))
        .as_deref()
        .map(str::trim)
    {
        Ok("up") => NetworkLinkState::Up,
        Ok("down") => NetworkLinkState::Down,
        _ if flags & libc::IFF_UP as u32 != 0 => NetworkLinkState::Up,
        _ => NetworkLinkState::Unknown,
    }
}

fn mount_kind(major_minor: &str) -> VolumeKind {
    mount_kind_from_sysfs_path(&PathBuf::from("/sys/dev/block").join(major_minor))
}

fn is_local_block_mount(mount: &MountInfo) -> bool {
    is_local_block_mount_with_sysfs(mount, Path::new("/sys/dev/block"))
}

fn is_local_block_mount_with_sysfs(mount: &MountInfo, sys_dev_block: &Path) -> bool {
    const PSEUDO_OR_NETWORK: &[&str] = &[
        "9p",
        "afs",
        "autofs",
        "bpf",
        "ceph",
        "cgroup",
        "cgroup2",
        "cifs",
        "configfs",
        "debugfs",
        "devpts",
        "devtmpfs",
        "efivarfs",
        "fuse.ceph",
        "fuse.sshfs",
        "fusectl",
        "hugetlbfs",
        "mqueue",
        "nfs",
        "nfs4",
        "nsfs",
        "overlay",
        "proc",
        "pstore",
        "ramfs",
        "rpc_pipefs",
        "securityfs",
        "smb3",
        "squashfs",
        "sysfs",
        "tmpfs",
        "tracefs",
    ];
    !PSEUDO_OR_NETWORK.contains(&mount.fs_type.as_str())
        && sys_dev_block.join(&mount.major_minor).exists()
}

fn mount_kind_from_sysfs_path(device_link: &Path) -> VolumeKind {
    let mut current = fs::canonicalize(device_link).unwrap_or_else(|_| device_link.to_path_buf());
    let removable = loop {
        let marker = current.join("removable");
        if let Ok(value) = fs::read_to_string(marker) {
            break value.trim() == "1";
        }
        if !current.pop() {
            break false;
        }
    };
    if removable {
        VolumeKind::Removable
    } else {
        VolumeKind::Fixed
    }
}

fn statvfs_bytes(path: &Path) -> io::Result<(Option<u64>, Option<u64>)> {
    let bytes = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "path contains NUL"))?;
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    if unsafe { libc::statvfs(bytes.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let stats = unsafe { stats.assume_init() };
    let block_size = stats.f_frsize.max(1);
    Ok((
        stats.f_blocks.checked_mul(block_size),
        stats.f_bavail.checked_mul(block_size),
    ))
}

#[derive(Debug, Clone)]
struct TrashRoot {
    root: PathBuf,
    files: PathBuf,
    info: PathBuf,
    /// The mount top directory for a per-volume Trash. Freedesktop requires
    /// `.trashinfo` paths there to be relative to this directory.
    topdir: Option<PathBuf>,
}

fn trash_roots() -> Result<Vec<TrashRoot>, PlatformError> {
    let home_root = home_trash_root()?;
    let home_mount = mount_for_path(&home_root.root).ok();
    let mut roots = vec![home_root];
    for volume in local_volumes()? {
        if home_mount.as_ref() == Some(&volume.root) {
            continue;
        }
        if let Ok(Some(root)) = existing_volume_trash_root(&volume.root) {
            roots.push(root);
        }
    }
    let mut seen = HashSet::new();
    roots.retain(|root| seen.insert(root.root.clone()));
    Ok(roots)
}

fn home_trash_root() -> Result<TrashRoot, PlatformError> {
    let home = home_dir_from_env()?;
    let base = XdgBaseDirs::from_environment(&home);
    private_trash_root(base.data.join("Trash"))
}

fn private_trash_root(root: PathBuf) -> Result<TrashRoot, PlatformError> {
    private_trash_root_with_topdir(root, None)
}

fn private_trash_root_with_topdir(
    root: PathBuf,
    topdir: Option<PathBuf>,
) -> Result<TrashRoot, PlatformError> {
    ensure_private_dir(&root)?;
    let files = root.join("files");
    let info = root.join("info");
    ensure_private_dir(&files)?;
    ensure_private_dir(&info)?;
    Ok(TrashRoot {
        root,
        files,
        info,
        topdir,
    })
}

fn volume_trash_root(mount: &Path) -> Result<TrashRoot, PlatformError> {
    let uid = unsafe { libc::geteuid() };
    let shared = mount.join(".Trash");
    let root = match fs::symlink_metadata(&shared) {
        Ok(metadata) if shared_trash_is_safe(&metadata) => shared.join(uid.to_string()),
        _ => mount.join(format!(".Trash-{uid}")),
    };
    private_trash_root_with_topdir(root, Some(mount.to_path_buf()))
}

fn existing_volume_trash_root(mount: &Path) -> Result<Option<TrashRoot>, PlatformError> {
    let uid = unsafe { libc::geteuid() };
    let shared = mount.join(".Trash");
    let root = match fs::symlink_metadata(&shared) {
        Ok(metadata) if shared_trash_is_safe(&metadata) => shared.join(uid.to_string()),
        _ => mount.join(format!(".Trash-{uid}")),
    };
    existing_private_trash_root(root, Some(mount.to_path_buf()))
}

fn shared_trash_is_safe(metadata: &fs::Metadata) -> bool {
    metadata.is_dir()
        && !metadata.file_type().is_symlink()
        && metadata.uid() == 0
        && metadata.permissions().mode() & 0o1000 != 0
}

fn existing_private_trash_root(
    root: PathBuf,
    topdir: Option<PathBuf>,
) -> Result<Option<TrashRoot>, PlatformError> {
    if !path_is_private_directory(&root)? {
        return Ok(None);
    }
    let files = root.join("files");
    let info = root.join("info");
    if !path_is_private_directory(&files)? || !path_is_private_directory(&info)? {
        return Ok(None);
    }
    Ok(Some(TrashRoot {
        root,
        files,
        info,
        topdir,
    }))
}

fn path_is_private_directory(path: &Path) -> Result<bool, PlatformError> {
    let directory = match open_directory_tree(path, false) {
        Ok(directory) => directory,
        Err(error)
            if error.kind() == ErrorKind::NotFound
                || error.kind() == ErrorKind::NotADirectory
                || error.raw_os_error() == Some(libc::ELOOP) =>
        {
            return Ok(false);
        }
        Err(error) => {
            return Err(io_error(
                "inspect Linux private directory",
                Some(path.to_path_buf()),
                error,
            ));
        }
    };
    let stat = directory_stat(&directory).map_err(|error| {
        io_error(
            "inspect Linux private directory",
            Some(path.to_path_buf()),
            error,
        )
    })?;
    Ok((stat.st_mode & libc::S_IFMT) == libc::S_IFDIR
        && stat.st_uid == unsafe { libc::geteuid() }
        && stat.st_mode & 0o077 == 0)
}

fn ensure_private_dir(path: &Path) -> Result<(), PlatformError> {
    let directory = open_directory_tree(path, true).map_err(|error| {
        io_error(
            "create private Linux directory without following links",
            Some(path.to_path_buf()),
            error,
        )
    })?;
    let mut stat = directory_stat(&directory).map_err(|error| {
        io_error(
            "inspect private Linux directory",
            Some(path.to_path_buf()),
            error,
        )
    })?;
    if (stat.st_mode & libc::S_IFMT) != libc::S_IFDIR || stat.st_uid != unsafe { libc::geteuid() } {
        return Err(PlatformError::Native {
            operation: "validate private Linux directory",
            message: format!(
                "{} is not a private directory owned by this user",
                path.display()
            ),
        });
    }
    if unsafe { libc::fchmod(directory.as_raw_fd(), 0o700) } != 0 {
        return Err(io_error(
            "secure private Linux directory",
            Some(path.to_path_buf()),
            io::Error::last_os_error(),
        ));
    }
    stat = directory_stat(&directory).map_err(|error| {
        io_error(
            "verify private Linux directory permissions",
            Some(path.to_path_buf()),
            error,
        )
    })?;
    if stat.st_mode & 0o077 != 0 {
        return Err(PlatformError::Native {
            operation: "verify private Linux directory permissions",
            message: format!("{} did not retain mode 0700", path.display()),
        });
    }
    Ok(())
}

fn open_directory_tree(path: &Path, create: bool) -> io::Result<OwnedFd> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "private Linux directory path must be absolute",
        ));
    }
    let root = CString::new("/").expect("root path contains no NUL");
    let raw = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut directory = unsafe { OwnedFd::from_raw_fd(raw) };

    for component in path.components() {
        let Component::Normal(part) = component else {
            if matches!(component, Component::RootDir) {
                continue;
            }
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "private Linux directory contains an unsafe component",
            ));
        };
        let part = CString::new(part.as_bytes())
            .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "path contains NUL"))?;
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
        let mut next = unsafe { libc::openat(directory.as_raw_fd(), part.as_ptr(), flags) };
        if next < 0 && create && io::Error::last_os_error().kind() == ErrorKind::NotFound {
            let created = unsafe { libc::mkdirat(directory.as_raw_fd(), part.as_ptr(), 0o700) };
            if created != 0 {
                let error = io::Error::last_os_error();
                if error.kind() != ErrorKind::AlreadyExists {
                    return Err(error);
                }
            }
            next = unsafe { libc::openat(directory.as_raw_fd(), part.as_ptr(), flags) };
        }
        if next < 0 {
            return Err(io::Error::last_os_error());
        }
        directory = unsafe { OwnedFd::from_raw_fd(next) };
    }
    Ok(directory)
}

fn directory_stat(directory: &OwnedFd) -> io::Result<libc::stat> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    if unsafe { libc::fstat(directory.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { stat.assume_init() })
}

fn list_trash_root(root: &TrashRoot) -> Result<Vec<TrashEntry>, PlatformError> {
    let mut entries = Vec::new();
    let directory = fs::read_dir(&root.info)
        .map_err(|error| io_error("list Linux Trash metadata", Some(root.info.clone()), error))?;
    for entry in directory {
        let entry = entry.map_err(|error| {
            io_error(
                "read Linux Trash metadata entry",
                Some(root.info.clone()),
                error,
            )
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(item_name) = name
            .strip_suffix(TRASH_INFO_SUFFIX)
            .filter(|name| is_safe_child_name(name))
        else {
            continue;
        };
        let info_path = entry.path();
        if fs::symlink_metadata(&info_path)
            .map(|metadata| metadata.file_type().is_symlink())
            .unwrap_or(true)
        {
            continue;
        }
        let content_path = root.files.join(item_name);
        let Ok(metadata) = fs::symlink_metadata(&content_path) else {
            continue;
        };
        if metadata.file_type().is_symlink() || metadata.uid() != unsafe { libc::geteuid() } {
            continue;
        }
        let info = match parse_trashinfo(&info_path, root.topdir.as_deref()) {
            Ok(info) => info,
            Err(_) => continue,
        };
        entries.push(TrashEntry {
            id: trash_id(root, item_name),
            display_name: item_name.to_string(),
            original_path: Some(info.path),
            deleted_at: Some(info.deleted_at),
            size: path_size(&content_path).unwrap_or(0),
            is_directory: metadata.is_dir(),
        });
    }
    Ok(entries)
}

fn move_one_to_trash(path: &Path) -> Result<(), PlatformError> {
    if !path.is_absolute() {
        return Err(PlatformError::InvalidInput {
            message: "Trash paths must be absolute".to_string(),
        });
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        io_error(
            "inspect path before moving to Trash",
            Some(path.to_path_buf()),
            error,
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(PlatformError::InvalidInput {
            message: "symbolic links are not accepted by the Linux Trash backend".to_string(),
        });
    }
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(PlatformError::InvalidInput {
            message: "Linux Trash only accepts items owned by the current user".to_string(),
        });
    }
    let mount = mount_for_path(path)?;
    let home_root = home_trash_root()?;
    let home_mount = mount_for_path(&home_root.root)?;
    let root = if mount == home_mount {
        home_root
    } else {
        volume_trash_root(&mount)?
    };
    move_one_to_trash_root(path, &root)
}

fn move_one_to_trash_root(path: &Path, root: &TrashRoot) -> Result<(), PlatformError> {
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| is_safe_child_name(value))
        .ok_or_else(|| PlatformError::InvalidInput {
            message: "Trash path must name a normal direct child".to_string(),
        })?;
    let name = unique_trash_name(root, file_name)?;
    let destination = root.files.join(&name);
    let info_path = root.info.join(format!("{name}{TRASH_INFO_SUFFIX}"));
    write_trashinfo(&info_path, path, root.topdir.as_deref())?;
    if let Err(error) = rename_noreplace(path, &destination) {
        let _ = fs::remove_file(&info_path);
        return Err(io_error(
            "move path to Linux Trash",
            Some(path.to_path_buf()),
            error,
        ));
    }
    Ok(())
}

fn empty_trash_root(root: &TrashRoot) -> Result<(), PlatformError> {
    for entry in fs::read_dir(&root.files)
        .map_err(|error| io_error("list Linux Trash files", Some(root.files.clone()), error))?
    {
        let entry = entry
            .map_err(|error| io_error("read Linux Trash file", Some(root.files.clone()), error))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_safe_child_name(&name) {
            continue;
        }
        let content = entry.path();
        let Ok(content_metadata) = fs::symlink_metadata(&content) else {
            continue;
        };
        if content_metadata.file_type().is_symlink()
            || content_metadata.uid() != unsafe { libc::geteuid() }
        {
            continue;
        }
        let info = root.info.join(format!("{name}{TRASH_INFO_SUFFIX}"));
        if !info.is_file()
            || fs::symlink_metadata(&info)
                .map(|metadata| metadata.file_type().is_symlink())
                .unwrap_or(true)
            || parse_trashinfo(&info, root.topdir.as_deref()).is_err()
        {
            continue;
        }
        remove_no_follow(&content)?;
        fs::remove_file(info)
            .map_err(|error| io_error("remove Linux Trash metadata", None, error))?;
    }
    Ok(())
}

fn restore_trash_item(
    id: &TrashEntryId,
    target: TrashRestoreTarget,
) -> Result<PathBuf, PlatformError> {
    let (root, name) = parse_trash_id(id)?;
    let roots = trash_roots()?;
    let root = roots
        .into_iter()
        .find(|candidate| candidate.root == root)
        .ok_or_else(|| PlatformError::InvalidInput {
            message: "Trash entry is not in a trusted Trash root".to_string(),
        })?;
    if !is_safe_child_name(&name) {
        return Err(PlatformError::InvalidInput {
            message: "invalid Trash entry name".to_string(),
        });
    }
    restore_trash_item_from_root(&root, &name, target)
}

fn restore_trash_item_from_root(
    root: &TrashRoot,
    name: &str,
    target: TrashRestoreTarget,
) -> Result<PathBuf, PlatformError> {
    restore_trash_item_from_root_with(root, name, target, rename_noreplace)
}

fn restore_trash_item_from_root_with(
    root: &TrashRoot,
    name: &str,
    target: TrashRestoreTarget,
    rename: impl FnOnce(&Path, &Path) -> io::Result<()>,
) -> Result<PathBuf, PlatformError> {
    let source = root.files.join(name);
    let info_path = root.info.join(format!("{name}{TRASH_INFO_SUFFIX}"));
    let metadata = fs::symlink_metadata(&source)
        .map_err(|error| io_error("inspect Linux Trash item", Some(source.clone()), error))?;
    if metadata.file_type().is_symlink() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(PlatformError::InvalidInput {
            message: "refusing to restore an unsafe or foreign-owned item from Trash".to_string(),
        });
    }
    let info = parse_trashinfo(&info_path, root.topdir.as_deref())?;
    let destination = match target {
        TrashRestoreTarget::OriginalLocation => info.path,
        TrashRestoreTarget::DestinationPath(path) => path,
    };
    if !destination.is_absolute()
        || destination.file_name().is_none()
        || !path_slot_unused(&destination)?
    {
        return Err(PlatformError::InvalidInput {
            message: "restore destination must be an unused absolute path".to_string(),
        });
    }
    let parent = destination
        .parent()
        .ok_or_else(|| PlatformError::InvalidInput {
            message: "restore destination has no parent".to_string(),
        })?;
    ensure_restore_parent(parent)?;
    if !path_slot_unused(&destination)? {
        return Err(PlatformError::InvalidInput {
            message: "restore destination appeared before the restore completed".to_string(),
        });
    }
    match rename(&source, &destination) {
        Ok(()) => {}
        Err(error) if matches!(error.raw_os_error(), Some(libc::EXDEV)) => {
            if let Err(error) = copy_no_follow(&source, &destination) {
                if fs::symlink_metadata(&destination).is_ok() {
                    let _ = remove_no_follow(&destination);
                }
                return Err(error);
            }
            remove_no_follow(&source)?;
        }
        Err(error) => return Err(io_error("restore Linux Trash item", Some(source), error)),
    }
    fs::remove_file(info_path)
        .map_err(|error| io_error("remove restored Linux Trash metadata", None, error))?;
    Ok(destination)
}

fn mount_for_path(path: &Path) -> Result<PathBuf, PlatformError> {
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mounts = File::open("/proc/self/mountinfo")
        .map(BufReader::new)
        .map(parse_mountinfo)
        .map_err(|error| {
            io_error(
                "read Linux mountinfo",
                Some(PathBuf::from("/proc/self/mountinfo")),
                error,
            )
        })?;
    mounts
        .into_iter()
        .filter(|mount| canonical.starts_with(&mount.mount_point))
        .max_by_key(|mount| mount.mount_point.components().count())
        .map(|mount| mount.mount_point)
        .ok_or_else(|| PlatformError::Native {
            operation: "find Linux mount for Trash path",
            message: format!("no mount found for {}", path.display()),
        })
}

fn unique_trash_name(root: &TrashRoot, base: &str) -> Result<String, PlatformError> {
    for suffix in 0..10_000_u32 {
        let candidate = if suffix == 0 {
            base.to_string()
        } else {
            format!("{base}.{suffix}")
        };
        if path_slot_unused(&root.files.join(&candidate))?
            && path_slot_unused(&root.info.join(format!("{candidate}{TRASH_INFO_SUFFIX}")))?
        {
            return Ok(candidate);
        }
    }
    Err(PlatformError::Native {
        operation: "allocate Linux Trash name",
        message: "too many conflicting names".to_string(),
    })
}

struct TrashInfo {
    path: PathBuf,
    deleted_at: SystemTime,
}

fn write_trashinfo(
    path: &Path,
    original: &Path,
    topdir: Option<&Path>,
) -> Result<(), PlatformError> {
    let deleted = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let stored_path = match topdir {
        Some(topdir) => original
            .strip_prefix(topdir)
            .ok()
            .filter(|relative| is_safe_relative_path(relative))
            .ok_or_else(|| PlatformError::InvalidInput {
                message: "per-volume Trash path is outside its mount top directory".to_string(),
            })?,
        None => original,
    };
    let content = format!(
        "[Trash Info]\nPath={}\nDeletionDate={}\n",
        percent_encode_path(stored_path),
        format_trash_timestamp(deleted)
    );
    let mut file = create_private_file_no_follow(path)?;
    file.write_all(content.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|error| {
            io_error(
                "write Linux Trash metadata",
                Some(path.to_path_buf()),
                error,
            )
        })
}

fn create_private_file_no_follow(path: &Path) -> Result<File, PlatformError> {
    let parent = path.parent().ok_or_else(|| PlatformError::InvalidInput {
        message: "Trash metadata path has no parent".to_string(),
    })?;
    let name = path
        .file_name()
        .ok_or_else(|| PlatformError::InvalidInput {
            message: "Trash metadata path has no file name".to_string(),
        })?;
    let directory = open_directory_tree(parent, false).map_err(|error| {
        io_error(
            "open Linux Trash metadata directory without following links",
            Some(parent.to_path_buf()),
            error,
        )
    })?;
    let name = CString::new(name.as_bytes()).map_err(|_| PlatformError::InvalidInput {
        message: "Trash metadata file name contains NUL".to_string(),
    })?;
    let raw = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o600,
        )
    };
    if raw < 0 {
        return Err(io_error(
            "create Linux Trash metadata without following links",
            Some(path.to_path_buf()),
            io::Error::last_os_error(),
        ));
    }
    Ok(unsafe { File::from_raw_fd(raw) })
}

fn parse_trashinfo(path: &Path, topdir: Option<&Path>) -> Result<TrashInfo, PlatformError> {
    let mut file = open_private_file_no_follow(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)
        .map_err(|error| io_error("read Linux Trash metadata", Some(path.to_path_buf()), error))?;
    let mut lines = content.lines();
    if lines.next() != Some("[Trash Info]") {
        return Err(PlatformError::InvalidInput {
            message: "malformed .trashinfo file".to_string(),
        });
    }
    let mut original = None;
    let mut deleted_at = None;
    for line in lines {
        if let Some(value) = line.strip_prefix("Path=") {
            if original.is_some() {
                return Err(PlatformError::InvalidInput {
                    message: "duplicate Path field in .trashinfo".to_string(),
                });
            }
            original = percent_decode_path(value);
        }
        if let Some(value) = line.strip_prefix("DeletionDate=") {
            if deleted_at.is_some() {
                return Err(PlatformError::InvalidInput {
                    message: "duplicate DeletionDate field in .trashinfo".to_string(),
                });
            }
            deleted_at = parse_trash_timestamp(value);
        }
    }
    let stored_path = original.ok_or_else(|| PlatformError::InvalidInput {
        message: "missing Path field in .trashinfo".to_string(),
    })?;
    let path = match topdir {
        Some(topdir) if is_safe_relative_path(&stored_path) => topdir.join(stored_path),
        Some(_) => {
            return Err(PlatformError::InvalidInput {
                message: "per-volume .trashinfo Path must be a safe relative path".to_string(),
            });
        }
        None if stored_path.is_absolute() => stored_path,
        None => {
            return Err(PlatformError::InvalidInput {
                message: "home .trashinfo Path must be absolute".to_string(),
            });
        }
    };
    let deleted_at = deleted_at.ok_or_else(|| PlatformError::InvalidInput {
        message: "missing or invalid DeletionDate field in .trashinfo".to_string(),
    })?;
    Ok(TrashInfo {
        path,
        deleted_at: UNIX_EPOCH + Duration::from_secs(deleted_at),
    })
}

// Freedesktop Trash uses a local ISO-8601 date without a timezone suffix.
fn format_trash_timestamp(seconds: u64) -> String {
    let Ok(timestamp) = libc::time_t::try_from(seconds) else {
        return "9999-12-31T23:59:59".to_string();
    };
    let mut local = std::mem::MaybeUninit::<libc::tm>::zeroed();
    let local = unsafe { libc::localtime_r(&timestamp, local.as_mut_ptr()) };
    if local.is_null() {
        return "1970-01-01T00:00:00".to_string();
    }
    let local = unsafe { local.read() };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        local.tm_year + 1900,
        local.tm_mon + 1,
        local.tm_mday,
        local.tm_hour,
        local.tm_min,
        local.tm_sec,
    )
}

fn parse_trash_timestamp(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() != 19
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let parse = |range: std::ops::Range<usize>| {
        std::str::from_utf8(&bytes[range]).ok()?.parse::<i64>().ok()
    };
    let (year, month, day, hour, minute, second) = (
        parse(0..4)?,
        parse(5..7)?,
        parse(8..10)?,
        parse(11..13)?,
        parse(14..16)?,
        parse(17..19)?,
    );
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let mut local = unsafe { std::mem::zeroed::<libc::tm>() };
    local.tm_year = i32::try_from(year - 1900).ok()?;
    local.tm_mon = i32::try_from(month - 1).ok()?;
    local.tm_mday = i32::try_from(day).ok()?;
    local.tm_hour = i32::try_from(hour).ok()?;
    local.tm_min = i32::try_from(minute).ok()?;
    local.tm_sec = i32::try_from(second).ok()?;
    local.tm_isdst = -1;
    let timestamp = unsafe { libc::mktime(&mut local) };
    if timestamp < 0
        || local.tm_year != i32::try_from(year - 1900).ok()?
        || local.tm_mon != i32::try_from(month - 1).ok()?
        || local.tm_mday != i32::try_from(day).ok()?
        || local.tm_hour != i32::try_from(hour).ok()?
        || local.tm_min != i32::try_from(minute).ok()?
        || local.tm_sec != i32::try_from(second).ok()?
    {
        return None;
    }
    u64::try_from(timestamp).ok()
}

fn trash_id(root: &TrashRoot, name: &str) -> TrashEntryId {
    TrashEntryId::from_native(format!(
        "{}\n{}",
        percent_encode_path(&root.root),
        percent_encode_path(Path::new(name))
    ))
}
fn parse_trash_id(id: &TrashEntryId) -> Result<(PathBuf, String), PlatformError> {
    let (encoded_root, encoded_name) =
        id.as_str()
            .split_once('\n')
            .ok_or_else(|| PlatformError::InvalidInput {
                message: "invalid Linux Trash entry id".to_string(),
            })?;
    let root = percent_decode_path(encoded_root).ok_or_else(|| PlatformError::InvalidInput {
        message: "invalid Linux Trash root encoding".to_string(),
    })?;
    let name = percent_decode_path(encoded_name)
        .and_then(|path| path.to_str().map(str::to_string))
        .filter(|name| is_safe_child_name(name))
        .ok_or_else(|| PlatformError::InvalidInput {
            message: "invalid Linux Trash entry name encoding".to_string(),
        })?;
    Ok((root, name))
}

fn percent_encode_path(path: &Path) -> String {
    path.as_os_str()
        .as_encoded_bytes()
        .iter()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'-' | b'_' | b'~' => {
                vec![*byte]
            }
            value => format!("%{value:02X}").into_bytes(),
        })
        .map(char::from)
        .collect()
}
fn percent_decode_path(value: &str) -> Option<PathBuf> {
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return None;
            }
            output.push(
                u8::from_str_radix(std::str::from_utf8(&bytes[index + 1..index + 3]).ok()?, 16)
                    .ok()?,
            );
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    Some(PathBuf::from(OsString::from_vec(output)))
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn is_safe_child_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\r')
        && !name.contains('\n')
        && Path::new(name)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn path_slot_unused(path: &Path) -> Result<bool, PlatformError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
        Err(error) => Err(io_error(
            "inspect Linux Trash destination",
            Some(path.to_path_buf()),
            error,
        )),
    }
}

fn open_private_file_no_follow(path: &Path) -> Result<File, PlatformError> {
    let parent = path.parent().ok_or_else(|| PlatformError::InvalidInput {
        message: "Trash metadata path has no parent".to_string(),
    })?;
    let name = path
        .file_name()
        .ok_or_else(|| PlatformError::InvalidInput {
            message: "Trash metadata path has no file name".to_string(),
        })?;
    let directory = open_directory_tree(parent, false).map_err(|error| {
        io_error(
            "open Linux Trash metadata directory without following links",
            Some(parent.to_path_buf()),
            error,
        )
    })?;
    let name = CString::new(name.as_bytes()).map_err(|_| PlatformError::InvalidInput {
        message: "Trash metadata file name contains NUL".to_string(),
    })?;
    let raw = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if raw < 0 {
        return Err(io_error(
            "open Linux Trash metadata without following links",
            Some(path.to_path_buf()),
            io::Error::last_os_error(),
        ));
    }
    let file = unsafe { File::from_raw_fd(raw) };
    let metadata = file.metadata().map_err(|error| {
        io_error(
            "inspect Linux Trash metadata",
            Some(path.to_path_buf()),
            error,
        )
    })?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.len() > 64 * 1024
    {
        return Err(PlatformError::InvalidInput {
            message: format!(
                "{} is not a private regular Trash metadata file owned by this user",
                path.display()
            ),
        });
    }
    Ok(file)
}

fn ensure_restore_parent(parent: &Path) -> Result<(), PlatformError> {
    if !parent.is_absolute() {
        return Err(PlatformError::InvalidInput {
            message: "Trash restore parent must be absolute".to_string(),
        });
    }
    open_directory_tree(parent, true)
        .map(|_| ())
        .map_err(|error| {
            io_error(
                "create Linux Trash restore parent without following links",
                Some(parent.to_path_buf()),
                error,
            )
        })
}

fn rename_noreplace(source: &Path, destination: &Path) -> io::Result<()> {
    let source = std::ffi::CString::new(source.as_os_str().as_encoded_bytes())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "source path contains NUL"))?;
    let destination = std::ffi::CString::new(destination.as_os_str().as_encoded_bytes())
        .map_err(|_| io::Error::new(ErrorKind::InvalidInput, "destination path contains NUL"))?;
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

fn remove_no_follow(path: &Path) -> Result<(), PlatformError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        io_error(
            "inspect Linux Trash content",
            Some(path.to_path_buf()),
            error,
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(PlatformError::InvalidInput {
            message: "refusing to remove a symbolic link from Trash".to_string(),
        });
    }
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .map_err(|error| {
        io_error(
            "remove Linux Trash content",
            Some(path.to_path_buf()),
            error,
        )
    })
}
fn path_size(path: &Path) -> io::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Ok(0);
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    let mut size: u64 = 0;
    for entry in fs::read_dir(path)? {
        size = size.saturating_add(path_size(&entry?.path())?);
    }
    Ok(size)
}
fn copy_no_follow(source: &Path, destination: &Path) -> Result<(), PlatformError> {
    let metadata = fs::symlink_metadata(source).map_err(|error| {
        io_error(
            "inspect Linux Trash copy source",
            Some(source.to_path_buf()),
            error,
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(PlatformError::InvalidInput {
            message: "refusing to copy symbolic link from Trash".to_string(),
        });
    }
    if metadata.is_dir() {
        fs::create_dir(destination).map_err(|error| {
            io_error(
                "create Linux Trash restore directory",
                Some(destination.to_path_buf()),
                error,
            )
        })?;
        fs::set_permissions(
            destination,
            fs::Permissions::from_mode(metadata.permissions().mode() & 0o777),
        )
        .map_err(|error| {
            io_error(
                "set Linux Trash restore directory permissions",
                Some(destination.to_path_buf()),
                error,
            )
        })?;
        for entry in fs::read_dir(source).map_err(|error| {
            io_error(
                "read Linux Trash restore directory",
                Some(source.to_path_buf()),
                error,
            )
        })? {
            let entry = entry.map_err(|error| {
                io_error(
                    "read Linux Trash restore entry",
                    Some(source.to_path_buf()),
                    error,
                )
            })?;
            copy_no_follow(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else {
        let mut input = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(source)
            .map_err(|error| {
                io_error(
                    "open Linux Trash restore source without following links",
                    Some(source.to_path_buf()),
                    error,
                )
            })?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(metadata.permissions().mode() & 0o777)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(destination)
            .map_err(|error| {
                io_error(
                    "create Linux Trash restore destination",
                    Some(destination.to_path_buf()),
                    error,
                )
            })?;
        io::copy(&mut input, &mut output)
            .and_then(|_| output.sync_all())
            .map_err(|error| {
                io_error(
                    "copy Linux Trash restore file",
                    Some(source.to_path_buf()),
                    error,
                )
            })?;
    }
    Ok(())
}

fn is_elf(path: &Path) -> bool {
    let mut magic = [0_u8; 4];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut magic))
        .is_ok()
        && magic == [0x7f, b'E', b'L', b'F']
}
fn is_script(path: &Path) -> bool {
    let mut header = [0_u8; 2];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && header == *b"#!"
}
fn executable_bit(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}
fn extension_is(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
}

fn desktop_icon_name(path: &Path) -> Option<String> {
    if !extension_is(path, "desktop") {
        return None;
    }
    DesktopEntry::from_path(path, None::<&[String]>)
        .ok()?
        .icon()
        .filter(|icon| !icon.trim().is_empty())
        .map(str::to_owned)
}
fn validate_desktop_entry(path: &Path) -> Result<(), PlatformError> {
    if !extension_is(path, "desktop") {
        return Err(PlatformError::InvalidInput {
            message: "Shortcut launch requires a .desktop file".to_string(),
        });
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| io_error("inspect desktop entry", Some(path.to_path_buf()), error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(PlatformError::InvalidInput {
            message: "desktop entry must be a regular non-symbolic-link file".to_string(),
        });
    }
    let owner = metadata.uid();
    if (owner != unsafe { libc::geteuid() } && owner != 0)
        || metadata.permissions().mode() & 0o022 != 0
    {
        return Err(PlatformError::InvalidInput {
            message: "desktop entry must be owned by the current user or root and not be group- or world-writable".to_string(),
        });
    }
    // Parse through the freedesktop implementation instead of treating Exec as
    // a shell command. `gio launch` owns the actual field-code expansion.
    let entry = DesktopEntry::from_path(path, None::<&[String]>).map_err(|error| {
        PlatformError::Native {
            operation: "parse desktop entry",
            message: error.to_string(),
        }
    })?;
    if entry.type_() != Some("Application") || entry.exec().is_none_or(str::is_empty) {
        return Err(PlatformError::InvalidInput {
            message: "desktop entry must declare Type=Application and a non-empty Exec".to_string(),
        });
    }
    entry
        .parse_exec()
        .map_err(|error| PlatformError::InvalidInput {
            message: format!("invalid desktop entry Exec: {error}"),
        })?;
    if let Some(try_exec) = entry.try_exec().filter(|value| !value.is_empty())
        && !executable_on_path(try_exec)
    {
        return Err(PlatformError::Native {
            operation: "validate desktop entry TryExec",
            message: format!("{try_exec} is not executable"),
        });
    }
    Ok(())
}
fn executable_on_path(value: &str) -> bool {
    let path = Path::new(value);
    if path.components().count() > 1 {
        return path.is_file() && executable_bit(path);
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|directory| {
            let candidate = directory.join(value);
            candidate.is_file() && executable_bit(&candidate)
        })
    })
}
fn resolve_icon(icon: &str, preferred_size: u32) -> Option<PathBuf> {
    let direct = PathBuf::from(icon);
    if direct.is_absolute() && direct.is_file() {
        return Some(direct);
    }
    let home = home_dir_from_env().ok()?;
    let base = XdgBaseDirs::from_environment(&home);
    let mut roots = vec![
        home.join(".local/share/icons"),
        base.data.join("icons"),
        PathBuf::from("/usr/local/share/icons"),
        PathBuf::from("/usr/share/icons"),
        PathBuf::from("/usr/share/pixmaps"),
    ];
    let data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .filter(|paths| !paths.is_empty())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        });
    roots.extend(data_dirs.into_iter().map(|path| path.join("icons")));
    let sizes = [preferred_size, 128, 96, 64, 48, 32, 24, 16];
    let icon_has_extension = Path::new(icon).extension().is_some();
    for root in roots {
        for theme in ["hicolor", "Adwaita", "breeze"] {
            for size in sizes {
                for directory in [
                    format!("{size}x{size}/apps"),
                    format!("{size}x{size}@2/apps"),
                    "scalable/apps".to_string(),
                ] {
                    for extension in ["png", "svg"] {
                        let file_name = if icon_has_extension {
                            icon.to_string()
                        } else {
                            format!("{icon}.{extension}")
                        };
                        let candidate = root.join(theme).join(&directory).join(file_name);
                        if candidate.is_file() {
                            return Some(candidate);
                        }
                    }
                }
            }
        }
        for extension in ["png", "svg"] {
            let file_name = if icon_has_extension {
                icon.to_string()
            } else {
                format!("{icon}.{extension}")
            };
            let candidate = root.join(file_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
fn decode_icon(path: &Path, preferred_size: u32) -> Result<PlatformIcon, PlatformError> {
    if extension_is(path, "svg") {
        return decode_svg_icon(path, preferred_size);
    }
    let mut reader = ImageReader::open(path).map_err(|error| PlatformError::Native {
        operation: "open Linux icon",
        message: error.to_string(),
    })?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|error| PlatformError::Native {
        operation: "decode Linux icon",
        message: error.to_string(),
    })?;
    let preferred_size = preferred_size.clamp(16, 512);
    let image = image.into_rgba8();
    let image = if image.width() > preferred_size || image.height() > preferred_size {
        image::imageops::thumbnail(&image, preferred_size, preferred_size)
    } else {
        image
    };
    PlatformIcon::new(image.width(), image.height(), image.into_raw())
}
fn decode_svg_icon(path: &Path, preferred_size: u32) -> Result<PlatformIcon, PlatformError> {
    let metadata = fs::metadata(path)
        .map_err(|error| io_error("inspect Linux SVG icon", Some(path.to_path_buf()), error))?;
    if metadata.len() > 4 * 1024 * 1024 {
        return Err(PlatformError::InvalidInput {
            message: "SVG icon exceeds the 4 MiB safety limit".to_string(),
        });
    }
    let options = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(
        &fs::read(path)
            .map_err(|error| io_error("read Linux SVG icon", Some(path.to_path_buf()), error))?,
        &options,
    )
    .map_err(|error| PlatformError::Native {
        operation: "parse Linux SVG icon",
        message: error.to_string(),
    })?;
    let source_size = tree.size().to_int_size();
    let preferred_size = preferred_size.clamp(16, 512);
    let scale = (preferred_size as f32 / source_size.width() as f32)
        .min(preferred_size as f32 / source_size.height() as f32)
        .min(1.0);
    let width = ((source_size.width() as f32 * scale).round() as u32).max(1);
    let height = ((source_size.height() as f32 * scale).round() as u32).max(1);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or_else(|| {
        PlatformError::InvalidInput {
            message: "SVG icon dimensions are invalid".to_string(),
        }
    })?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    PlatformIcon::new(pixmap.width(), pixmap.height(), pixmap.data().to_vec())
}

fn io_error(operation: &'static str, path: Option<PathBuf>, error: io::Error) -> PlatformError {
    PlatformError::Io {
        operation,
        path,
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MountInfo, XdgBaseDirs, XdgUserDirs, ensure_private_dir, format_trash_timestamp,
        is_local_block_mount_with_sysfs, linux_interface_kind_with_sysfs, list_trash_root,
        logind_allows_poweroff, mount_kind_from_sysfs_path, move_one_to_trash_root,
        parse_mountinfo, parse_trash_timestamp, parse_trashinfo, percent_decode_path,
        percent_encode_path, private_trash_root, private_trash_root_with_topdir,
        restore_trash_item_from_root, restore_trash_item_from_root_with, spawn_detached_child,
        validate_desktop_entry,
    };
    use crate::{NetworkInterfaceKind, PlatformError, TrashRestoreTarget, VolumeKind};
    use std::ffi::OsString;
    use std::fs::{self, OpenOptions};
    use std::io::Cursor;
    use std::os::unix::fs::{OpenOptionsExt, symlink};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn test_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "tundra-linux-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn write_private(path: &Path, contents: &[u8]) {
        use std::io::Write;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .unwrap();
        file.write_all(contents).unwrap();
    }

    #[test]
    fn xdg_base_dirs_ignore_relative_values() {
        let home = Path::new("/home/tundra");
        let dirs = XdgBaseDirs::resolve(
            home,
            Some(PathBuf::from("relative")),
            Some(PathBuf::from("/data")),
            None,
            None,
        );
        assert_eq!(dirs.config, home.join(".config"));
        assert_eq!(dirs.data, Path::new("/data"));
    }

    #[test]
    fn detached_spawn_reports_missing_program_before_queuing_a_reaper() {
        let missing = PathBuf::from("/definitely/missing/tundra-open-helper");
        let mut command = Command::new(&missing);
        let error = spawn_detached_child(
            &mut command,
            "open with Linux helper",
            &OsString::from(missing.as_os_str()),
        )
        .expect_err("a missing helper must be reported synchronously");
        assert!(matches!(
            error,
            PlatformError::Io {
                operation: "open with Linux helper",
                path: Some(path),
                ..
            } if path == missing
        ));
    }

    #[test]
    fn detached_spawn_does_not_wait_for_the_child() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 5"]);
        let started = Instant::now();
        let mut child = spawn_detached_child(
            &mut command,
            "start nonblocking Linux helper",
            &OsString::from("/bin/sh"),
        )
        .expect("shell fixture should start");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(child.try_wait().expect("probe child").is_none());
        let _ = child.kill();
        let _ = child.wait();
    }
    #[test]
    fn user_dirs_expand_home_and_ignore_relative_values() {
        let root = test_path("user-dirs");
        let _ = std::fs::create_dir_all(&root);
        let path = root.join("user-dirs.dirs");
        std::fs::write(
            &path,
            "XDG_DESKTOP_DIR=\"$HOME/Desk\"\nXDG_DOWNLOAD_DIR=\"relative\"\n",
        )
        .unwrap();
        let dirs = XdgUserDirs::from_file(&path, Path::new("/home/tundra"));
        assert_eq!(dirs.desktop, Some(PathBuf::from("/home/tundra/Desk")));
        assert_eq!(dirs.download, None);
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn user_dirs_reject_unquoted_values_and_partial_home_expansion() {
        let root = test_path("user-dirs-invalid");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("user-dirs.dirs");
        fs::write(
            &path,
            "XDG_DESKTOP_DIR=$HOME/Desktop\nXDG_DOWNLOAD_DIR=\"$HOMEevil/Downloads\"\n",
        )
        .unwrap();
        let dirs = XdgUserDirs::from_file(&path, Path::new("/home/tundra"));
        assert_eq!(dirs.desktop, None);
        assert_eq!(dirs.download, None);
        let _ = fs::remove_dir_all(root);
    }
    #[test]
    fn mountinfo_parser_unescapes_mount_points() {
        let mounts = parse_mountinfo(Cursor::new(
            "24 22 8:1 / /media/tundra/My\\040Disk rw - ext4 /dev/sda1 rw\n",
        ));
        assert_eq!(mounts[0].mount_point, Path::new("/media/tundra/My Disk"));
        assert_eq!(mounts[0].fs_type, "ext4");
        assert!(!mounts[0].read_only);
        let mounts = parse_mountinfo(Cursor::new("24 22 8:1 / / ro - ext4 /dev/sda1 ro\n"));
        assert!(mounts[0].read_only);
    }

    #[test]
    fn network_kind_uses_virtual_prefixes_and_wireless_sysfs_marker() {
        let sysfs = test_path("sys-class-net");
        fs::create_dir_all(sysfs.join("wlan0/wireless")).unwrap();
        fs::create_dir_all(sysfs.join("eno1")).unwrap();
        assert_eq!(
            linux_interface_kind_with_sysfs("wlan0", &sysfs, &sysfs.join("virtual")),
            NetworkInterfaceKind::Wireless
        );
        assert_eq!(
            linux_interface_kind_with_sysfs("eno1", &sysfs, &sysfs.join("virtual")),
            NetworkInterfaceKind::Wired
        );
        assert_eq!(
            linux_interface_kind_with_sysfs("veth123", &sysfs, &sysfs.join("virtual")),
            NetworkInterfaceKind::Virtual
        );
        assert_eq!(
            linux_interface_kind_with_sysfs("mystery", &sysfs, &sysfs.join("virtual")),
            NetworkInterfaceKind::Unknown
        );
        let _ = fs::remove_dir_all(sysfs);
    }

    #[test]
    fn network_kind_detects_virtual_devices_without_known_names() {
        let sysfs = test_path("sys-virtual-net");
        let class = sysfs.join("class");
        let virtual_net = sysfs.join("devices/virtual/net");
        fs::create_dir_all(virtual_net.join("unexpected0")).unwrap();
        fs::create_dir_all(&class).unwrap();
        symlink(virtual_net.join("unexpected0"), class.join("unexpected0")).unwrap();
        assert_eq!(
            linux_interface_kind_with_sysfs("unexpected0", &class, &virtual_net),
            NetworkInterfaceKind::Virtual
        );
        for name in ["br0", "cni0", "flannel.1"] {
            assert_eq!(
                linux_interface_kind_with_sysfs(name, &class, &virtual_net),
                NetworkInterfaceKind::Virtual
            );
        }
        let _ = fs::remove_dir_all(sysfs);
    }
    #[test]
    fn trash_path_encoding_round_trips() {
        let path = Path::new("/tmp/space % 汉字");
        assert_eq!(
            percent_decode_path(&percent_encode_path(path)),
            Some(path.to_path_buf())
        );
    }
    #[test]
    fn trash_timestamp_is_freedesktop_iso_and_round_trips() {
        let timestamp = format_trash_timestamp(1_700_000_000);
        assert_eq!(timestamp.len(), 19);
        assert_eq!(parse_trash_timestamp(&timestamp), Some(1_700_000_000));
        assert_eq!(parse_trash_timestamp("not-a-date"), None);
        assert_eq!(parse_trash_timestamp("2025-02-31T12:00:00"), None);
    }

    #[test]
    fn local_mount_filter_requires_a_real_block_device_and_rejects_network_filesystems() {
        let sysfs = test_path("sys-dev-block");
        fs::create_dir_all(sysfs.join("8:1")).unwrap();
        let local = MountInfo {
            mount_point: PathBuf::from("/media/local"),
            fs_type: "ext4".to_string(),
            major_minor: "8:1".to_string(),
            read_only: false,
        };
        let network = MountInfo {
            fs_type: "nfs4".to_string(),
            ..local.clone()
        };
        let missing = MountInfo {
            major_minor: "8:2".to_string(),
            ..local.clone()
        };
        assert!(is_local_block_mount_with_sysfs(&local, &sysfs));
        assert!(!is_local_block_mount_with_sysfs(&network, &sysfs));
        assert!(!is_local_block_mount_with_sysfs(&missing, &sysfs));
        let _ = fs::remove_dir_all(sysfs);
    }

    #[test]
    fn removable_marker_is_discovered_on_the_parent_block_device() {
        let sysfs = test_path("sys-removable");
        let partition = sysfs.join("devices/block/sdb/sdb1");
        fs::create_dir_all(&partition).unwrap();
        fs::write(sysfs.join("devices/block/sdb/removable"), "1\n").unwrap();
        assert_eq!(
            mount_kind_from_sysfs_path(&partition),
            VolumeKind::Removable
        );
        fs::write(sysfs.join("devices/block/sdb/removable"), "0\n").unwrap();
        assert_eq!(mount_kind_from_sysfs_path(&partition), VolumeKind::Fixed);
        let _ = fs::remove_dir_all(sysfs);
    }

    #[test]
    fn private_directory_creation_rejects_symlinked_ancestors() {
        let fixture = test_path("private-dir-symlink");
        let real = fixture.join("real");
        fs::create_dir_all(&real).unwrap();
        let linked = fixture.join("linked");
        symlink(&real, &linked).unwrap();

        let error = ensure_private_dir(&linked.join("Trash"))
            .expect_err("a private directory must never follow an ancestor symlink");

        assert!(error.to_string().contains("without following links"));
        assert!(!real.join("Trash").exists());
        let _ = fs::remove_dir_all(fixture);
    }

    #[test]
    fn home_trash_round_trips_files_and_directories_without_touching_real_trash() {
        let fixture = test_path("trash-roundtrip");
        let root = private_trash_root(fixture.join("Trash")).unwrap();
        let source_dir = fixture.join("source");
        fs::create_dir_all(source_dir.join("folder")).unwrap();
        let file = source_dir.join("hello.txt");
        let directory = source_dir.join("folder");
        fs::write(&file, "hello").unwrap();
        fs::write(directory.join("nested.txt"), "nested").unwrap();

        move_one_to_trash_root(&file, &root).unwrap();
        move_one_to_trash_root(&directory, &root).unwrap();
        assert!(!file.exists());
        assert!(!directory.exists());
        let entries = list_trash_root(&root).unwrap();
        assert_eq!(entries.len(), 2);

        restore_trash_item_from_root(&root, "hello.txt", TrashRestoreTarget::OriginalLocation)
            .unwrap();
        restore_trash_item_from_root(&root, "folder", TrashRestoreTarget::OriginalLocation)
            .unwrap();
        assert_eq!(fs::read_to_string(file).unwrap(), "hello");
        assert_eq!(
            fs::read_to_string(directory.join("nested.txt")).unwrap(),
            "nested"
        );
        assert!(list_trash_root(&root).unwrap().is_empty());
        let _ = fs::remove_dir_all(fixture);
    }

    #[test]
    fn per_volume_trash_stores_relative_paths_and_resolves_them_safely() {
        let fixture = test_path("trash-volume");
        let topdir = fixture.join("volume");
        fs::create_dir_all(topdir.join("docs")).unwrap();
        let root =
            private_trash_root_with_topdir(fixture.join("volume-trash"), Some(topdir.clone()))
                .unwrap();
        let source = topdir.join("docs/report.txt");
        fs::write(&source, "report").unwrap();

        move_one_to_trash_root(&source, &root).unwrap();
        let metadata = fs::read_to_string(root.info.join("report.txt.trashinfo")).unwrap();
        assert!(metadata.contains("\nPath=docs/report.txt\n"));
        let parsed = parse_trashinfo(
            &root.info.join("report.txt.trashinfo"),
            root.topdir.as_deref(),
        )
        .unwrap();
        assert_eq!(parsed.path, source);
        restore_trash_item_from_root(&root, "report.txt", TrashRestoreTarget::OriginalLocation)
            .unwrap();
        assert_eq!(fs::read_to_string(source).unwrap(), "report");
        let _ = fs::remove_dir_all(fixture);
    }

    #[test]
    fn cross_volume_restore_uses_safe_copy_then_removes_trash_source() {
        let fixture = test_path("trash-cross-volume");
        let root = private_trash_root(fixture.join("Trash")).unwrap();
        let source = fixture.join("source.txt");
        fs::write(&source, "cross-volume").unwrap();
        move_one_to_trash_root(&source, &root).unwrap();
        let destination = fixture.join("restored/copy.txt");

        restore_trash_item_from_root_with(
            &root,
            "source.txt",
            TrashRestoreTarget::DestinationPath(destination.clone()),
            |_, _| Err(std::io::Error::from_raw_os_error(libc::EXDEV)),
        )
        .unwrap();

        assert_eq!(fs::read_to_string(destination).unwrap(), "cross-volume");
        assert!(!root.files.join("source.txt").exists());
        assert!(!root.info.join("source.txt.trashinfo").exists());
        let _ = fs::remove_dir_all(fixture);
    }

    #[test]
    fn malformed_metadata_and_symbolic_link_entries_are_never_listed() {
        let fixture = test_path("trash-malformed");
        let root = private_trash_root(fixture.join("Trash")).unwrap();
        fs::write(root.files.join("bad"), "data").unwrap();
        write_private(
            &root.info.join("bad.trashinfo"),
            b"[Trash Info]\nPath=%ZZ\nDeletionDate=2025-01-01T00:00:00\n",
        );
        fs::write(root.files.join("linked"), "data").unwrap();
        symlink(
            root.info.join("bad.trashinfo"),
            root.info.join("linked.trashinfo"),
        )
        .unwrap();
        symlink(root.files.join("bad"), root.files.join("content-symlink")).unwrap();
        write_private(
            &root.info.join("content-symlink.trashinfo"),
            b"[Trash Info]\nPath=/tmp/content\nDeletionDate=2025-01-01T00:00:00\n",
        );

        assert!(list_trash_root(&root).unwrap().is_empty());
        let _ = fs::remove_dir_all(fixture);
    }

    #[test]
    fn stale_metadata_and_existing_names_allocate_a_unique_trash_name() {
        let fixture = test_path("trash-conflict");
        let root = private_trash_root(fixture.join("Trash")).unwrap();
        write_private(
            &root.info.join("same.txt.trashinfo"),
            b"[Trash Info]\nPath=/tmp/stale\nDeletionDate=2025-01-01T00:00:00\n",
        );
        let source = fixture.join("same.txt");
        fs::write(&source, "new").unwrap();

        move_one_to_trash_root(&source, &root).unwrap();

        assert_eq!(
            fs::read_to_string(root.files.join("same.txt.1")).unwrap(),
            "new"
        );
        assert!(root.info.join("same.txt.1.trashinfo").is_file());
        let _ = fs::remove_dir_all(fixture);
    }

    #[test]
    fn restore_rejects_symlinked_parent_and_preserves_trash_item() {
        let fixture = test_path("trash-restore-symlink");
        let root = private_trash_root(fixture.join("Trash")).unwrap();
        let source = fixture.join("source.txt");
        fs::write(&source, "keep").unwrap();
        move_one_to_trash_root(&source, &root).unwrap();
        let real_parent = fixture.join("real-parent");
        fs::create_dir(&real_parent).unwrap();
        let linked_parent = fixture.join("linked-parent");
        symlink(&real_parent, &linked_parent).unwrap();

        let error = restore_trash_item_from_root(
            &root,
            "source.txt",
            TrashRestoreTarget::DestinationPath(linked_parent.join("restored.txt")),
        )
        .expect_err("symlink parent must be rejected");

        assert!(error.to_string().contains("without following links"));
        assert!(root.files.join("source.txt").is_file());
        assert!(root.info.join("source.txt.trashinfo").is_file());
        assert!(fs::read_dir(real_parent).unwrap().next().is_none());
        let _ = fs::remove_dir_all(fixture);
    }

    #[test]
    fn desktop_entry_validation_covers_type_exec_try_exec_permissions_and_links() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = test_path("desktop-entry");
        fs::create_dir_all(&fixture).unwrap();
        let valid = fixture.join("valid.desktop");
        write_private(
            &valid,
            b"[Desktop Entry]\nType=Application\nName=Valid\nExec=/bin/true\nTryExec=/bin/true\n",
        );
        assert!(validate_desktop_entry(&valid).is_ok());

        let wrong_type = fixture.join("link.desktop");
        write_private(
            &wrong_type,
            b"[Desktop Entry]\nType=Link\nName=Link\nExec=/bin/true\n",
        );
        assert!(validate_desktop_entry(&wrong_type).is_err());

        let missing_exec = fixture.join("missing.desktop");
        write_private(
            &missing_exec,
            b"[Desktop Entry]\nType=Application\nName=Missing\n",
        );
        assert!(validate_desktop_entry(&missing_exec).is_err());

        let missing_try_exec = fixture.join("try.desktop");
        write_private(
            &missing_try_exec,
            b"[Desktop Entry]\nType=Application\nName=Missing\nExec=/bin/true\nTryExec=/definitely/missing\n",
        );
        assert!(validate_desktop_entry(&missing_try_exec).is_err());

        let writable = fixture.join("writable.desktop");
        write_private(
            &writable,
            b"[Desktop Entry]\nType=Application\nName=Writable\nExec=/bin/true\n",
        );
        fs::set_permissions(&writable, fs::Permissions::from_mode(0o666)).unwrap();
        assert!(validate_desktop_entry(&writable).is_err());

        let linked = fixture.join("linked.desktop");
        symlink(&valid, &linked).unwrap();
        assert!(validate_desktop_entry(&linked).is_err());
        let _ = fs::remove_dir_all(fixture);
    }

    #[test]
    fn logind_poweroff_accepts_yes_and_challenge_only() {
        assert!(logind_allows_poweroff("yes"));
        assert!(logind_allows_poweroff("challenge"));
        assert!(!logind_allows_poweroff("no"));
        assert!(!logind_allows_poweroff("na"));
        assert!(!logind_allows_poweroff(""));
    }
}
