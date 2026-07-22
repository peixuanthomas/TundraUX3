use crate::clock_scheduler::{
    ClockEntryKind as ScheduledClockEntryKind, ClockScheduler, ClockSchedulerError, DueEvent,
};
use crate::*;
use app::DEFAULT_ALERT_KEY;
use app::editor::{EditorState, is_log_document_path};
use app::explorer::{
    ExplorerCommand, ExplorerConflictAction, ExplorerEffect, ExplorerOpenTarget, ExplorerState,
};
use app::launcher::{
    LauncherAddOutcome, LauncherCommand, LauncherController, LauncherEffect, LauncherItemStatus,
    LauncherState,
};
#[cfg(test)]
use app::{DEFAULT_TOAST_DURATION, MAX_NOTIFICATION_RESPONSES};
use identity::{
    AuthSession, CoreError, DebugPolicy, PASSWORD_MAX_LEN, PASSWORD_MIN_LEN, PermissionAction,
    PermissionService, SessionService, UserAccount, UserRole, UserService,
};
use storage::{ClockProfile, LauncherExecutableKind, StorageError, StorageManager};

use chrono::{DateTime, Timelike, Utc};
use crossterm::event;
use platform::{
    CapabilityStatus, DocumentFingerprint, FileAttributes, Platform, PlatformCapabilities,
    PlatformIcon, PlatformKind, TerminalControlHandler,
};
use ratatui::layout::Rect;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::io::{self, Write};
use std::path::PathBuf;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use time::TimeSyncResult;
use watchdog::{
    AppWatchdog, ManagedTaskGroup, ManagedThreadHandle, ProcessWatchdog, TaskId, TaskSpec,
};
#[cfg(test)]
use watchdog::{IncidentKind, IncidentReceipt, RecoveryOutcome};

const MAX_NOTIFICATION_FOLLOW_UP_STEPS: usize = 64;
const NOTIFICATION_FOLLOW_UP_ALERT_KEY: &str = "shell.notification-follow-up";
const EXIT_CONFIRM_NOTIFICATION_KEY: &str = "shell.exit-confirm";
const TIME_SYNC_NOTIFICATION_KEY: &str = "shell.time-sync-failure";
const EXPLORER_DELETE_NOTIFICATION_KEY: &str = "explorer.delete-confirm";
const EXPLORER_CONFLICT_NOTIFICATION_KEY: &str = "explorer.name-conflict";
const EXPLORER_ALERT_KEY: &str = "explorer.operation";
const EDITOR_CLOSE_NOTIFICATION_KEY: &str = "editor.close-confirm";
const EDITOR_OPEN_NOTIFICATION_KEY: &str = "editor.open-confirm";
const EDITOR_ALERT_KEY: &str = "editor.operation";
const USER_MANAGEMENT_REFRESH_ALERT_KEY: &str = "user-management.refresh";
const USER_MANAGEMENT_DELETE_NOTIFICATION_KEY: &str = "user-management.delete-confirm";
const CLOCK_STORAGE_ALERT_KEY: &str = "clock.storage";
const CLOCK_MANAGE_NOTIFICATION_KEY_PREFIX: &str = "clock.manage";
const CLOCK_DUE_NOTIFICATION_KEY_PREFIX: &str = "clock.due";
mod construction;
mod controller;
mod presentation;
mod queries;
mod runtime;
#[cfg(test)]
mod tests;
mod ui_state;

use controller::*;
use presentation::*;
use runtime::*;
pub use runtime::*;
use ui_state::*;
pub use ui_state::{ShellSession, UiSessionState};
