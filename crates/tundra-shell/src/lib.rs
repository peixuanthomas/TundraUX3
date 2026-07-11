mod clock_scheduler;

use clock_scheduler::{
    ClockEntryKind as ScheduledClockEntryKind, ClockScheduler, ClockSchedulerError, DueEvent,
};
use tundra_apps::explorer::{
    ExplorerCommand, ExplorerConflictAction, ExplorerController, ExplorerEffect,
    ExplorerOpenTarget, ExplorerState,
};
use tundra_core::{
    AuditOutcome, AuditService, AuthSession, CoreError, DebugPolicy, PASSWORD_MAX_LEN,
    PASSWORD_MIN_LEN, PermissionAction, PermissionService, SessionService, UserAccount, UserRole,
    UserService,
};
use tundra_storage::{ClockProfile, StorageError, StorageManager};

use chrono::{DateTime, Timelike, Utc};
use crossterm::event;
use ratatui::layout::Rect;
use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tundra_platform::{
    CapabilityStatus, FileAttributes, Platform, PlatformCapabilities, PlatformKind,
    TerminalControlHandler,
};
use tundra_weathr::network_clock::{NetworkClock, TimeSyncResult};

pub use tundra_platform::{ENTER_FULLSCREEN_SEQUENCE, EXIT_FULLSCREEN_SEQUENCE};
pub use tundra_weathr::network_clock::TIME_SYNC_INTERVAL;

pub const BANNER_DISPLAY_DURATION: Duration = Duration::from_secs(2);
pub const LOGIN_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
pub const PASSWORD_REVEAL_DURATION: Duration = Duration::from_secs(5);
const BANNER_ASSET_KEY: &str = "tundraux3";
const DEFAULT_TOAST_DURATION: Duration = Duration::from_secs(4);
const MAX_ACTIVE_ALERTS: usize = 64;
const MAX_NOTIFICATION_RESPONSES: usize = 128;
const MAX_NOTIFICATION_FOLLOW_UP_STEPS: usize = 64;
const DEFAULT_ALERT_KEY: &str = "shell.default";
const NOTIFICATION_FOLLOW_UP_ALERT_KEY: &str = "shell.notification-follow-up";
const EXIT_CONFIRM_NOTIFICATION_KEY: &str = "shell.exit-confirm";
const TIME_SYNC_NOTIFICATION_KEY: &str = "shell.time-sync-failure";
const EXPLORER_DELETE_NOTIFICATION_KEY: &str = "explorer.delete-confirm";
const EXPLORER_CONFLICT_NOTIFICATION_KEY: &str = "explorer.name-conflict";
const EXPLORER_ALERT_KEY: &str = "explorer.operation";
const USER_MANAGEMENT_REFRESH_ALERT_KEY: &str = "user-management.refresh";
const USER_MANAGEMENT_DELETE_NOTIFICATION_KEY: &str = "user-management.delete-confirm";
const CLOCK_STORAGE_ALERT_KEY: &str = "clock.storage";
const CLOCK_MANAGE_NOTIFICATION_KEY_PREFIX: &str = "clock.manage";
const CLOCK_DUE_NOTIFICATION_KEY_PREFIX: &str = "clock.due";

static PANIC_RESTORE_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);

// Public models and low-coupling services live in regular modules. Re-exports
// preserve the crate-root API used by the binary and integration tests.
mod banner;
mod input_events;
mod launch_args;
mod notification_center;
mod shell_commands;
mod shell_components;
mod shortcuts;
mod startup;
mod terminal_events;
mod terminal_session;
mod terminal_size;

pub use banner::*;
pub use input_events::*;
pub use launch_args::*;
pub use notification_center::*;
pub use shell_commands::*;
pub use shell_components::*;
pub use shortcuts::*;
pub use startup::*;
pub use terminal_events::crossterm_event_to_input;
pub use terminal_session::TerminalGuard;
pub use terminal_size::{ShellTerminalSizeError, ShellTerminalSizeRequirement};

pub(crate) use banner::asset_io_error;
pub(crate) use input_events::DOUBLE_CLICK_CELL_TOLERANCE;
pub(crate) use terminal_events::resets_login_idle_timeout;
#[cfg(test)]
pub(crate) use terminal_events::{key_event_to_label, mouse_event_to_input};
pub(crate) use terminal_session::install_panic_restore_hook;
pub(crate) use terminal_size::checked_current_terminal_size;

// Shell state and cohesive business workflows.
include!("shell_state.rs");
include!("state_construction.rs");
include!("view_models.rs");
include!("account_workflows.rs");
include!("clock_workflows.rs");
include!("explorer_workflows.rs");
include!("explorer_task_workflows.rs");
include!("user_management_workflows.rs");
include!("home_navigation.rs");
include!("notification_workflows.rs");
include!("time_sync.rs");
include!("state_queries.rs");

// Input processing, command execution, and presentation geometry.
include!("command_dispatch.rs");
include!("input_routing.rs");
include!("input_diagnostics.rs");
include!("focus_navigation.rs");
include!("hit_testing.rs");
include!("view_helpers.rs");

// Process entry points and runtime integration.
include!("runtime.rs");

include!("shell_unit_tests.rs");
