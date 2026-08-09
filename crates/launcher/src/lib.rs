//! The GUI entry point for a bundled TundraUX3 installation.
//!
//! The launcher intentionally does not search PATH or read a user's WezTerm
//! configuration.  It owns one bundled WezTerm process at a time and passes a
//! small, versioned managed-session contract to it.

mod bundle;
mod supervisor;

pub use bundle::{BUNDLE_PROTOCOL_VERSION, BundleError, BundleLayout};
pub use supervisor::{
    ACTIVATED_EXISTING_EXIT, BUNDLED_WEZTERM_REVISION, BundledWezTerm, ChildFactory, ChildStatus,
    Clock, LauncherError, LauncherSupervisor, NoopReset, Outcome, ProductStorageReset,
    RecoveryAction, RecoveryPolicy, ResetCallback, SessionChild, SessionSpec, SessionWait,
    SystemClock, production_supervisor, show_critical_fallback,
};
