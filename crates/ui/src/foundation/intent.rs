//! Typed intents emitted by UI routing and shortcut resolution.

use crate::UiId;

/// Focus operations that affect only the current UI session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusIntent {
    Next,
    Previous,
    Set(UiId),
    Restore,
}

/// High-level UI intents. Application commands stay UI-independent inside
/// [`Self::App`]; all other variants are handled by the UI session or Shell.
#[derive(Debug, Clone, PartialEq)]
pub enum UiIntent {
    App(Box<app::AppCommand>),
    Focus(FocusIntent),
    OpenOverlay(UiId),
    CloseOverlay,
    Activate(UiId),
    Hit(UiId),
    LayoutChanged { width: u16, height: u16 },
    Redraw,
}

impl From<app::AppCommand> for UiIntent {
    fn from(command: app::AppCommand) -> Self {
        Self::App(Box::new(command))
    }
}
