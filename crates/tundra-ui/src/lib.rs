#[cfg(not(any(windows, target_os = "macos")))]
compile_error!("TundraUX3 phase 1 supports Windows and macOS only; Linux is unsupported.");

mod command;
mod focus;
mod hit_test;
mod input;
mod layout;
mod render;
mod theme;
mod view_model;

pub mod components;

pub use command::{
    Command, KeyChord, ShortcutBinding, ShortcutConflict, ShortcutConflictError,
    ShortcutConflictKind, ShortcutRegistry, ShortcutScope,
};
pub use focus::{
    ComponentId, FocusChange, FocusDirection, FocusError, FocusManager, FocusNode, FocusScope,
    ModalTrap,
};
pub use hit_test::{HitKind, HitMap, HitTarget, HitTargetKind};
pub use input::{
    InputEvent, Key, KeyCode, KeyEvent, KeyModifiers, KeyStroke, MouseAction, MouseButton,
    MouseEvent, MouseEventKind, Point, RouteTarget, RoutedEvent, RoutedTarget, ScrollDirection,
    UiId,
};
pub use layout::{ShellLayout, compute_shell_layout};
pub use render::{render_exit_confirmation, render_home};
pub use theme::TundraTheme;
pub use view_model::{
    DebugDiagnosticsViewModel, ExitConfirmViewModel, HomeDisplayMode, HomeViewModel,
    ShellChromeViewModel, ShellEntry, StatusViewModel,
};
