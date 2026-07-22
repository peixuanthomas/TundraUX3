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
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiIntent {
    App(app::AppCommand),
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
        Self::App(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Key, KeyStroke, ShortcutBinding, ShortcutRegistry};

    #[test]
    fn shortcut_registry_resolves_typed_ui_and_app_intents() {
        let mut registry = ShortcutRegistry::<UiIntent>::new();
        registry
            .register(ShortcutBinding::global(
                KeyStroke::plain(Key::Tab),
                UiIntent::Focus(FocusIntent::Next),
            ))
            .expect("focus shortcut");
        registry
            .register(ShortcutBinding::global(
                KeyStroke::plain(Key::Escape),
                UiIntent::App(app::AppCommand::RequestExit),
            ))
            .expect("app shortcut");

        assert_eq!(
            registry.command_for(&[crate::ShortcutScope::Global], &KeyStroke::plain(Key::Tab)),
            Some(&UiIntent::Focus(FocusIntent::Next))
        );
        assert_eq!(
            registry.command_for(
                &[crate::ShortcutScope::Global],
                &KeyStroke::plain(Key::Escape)
            ),
            Some(&UiIntent::App(app::AppCommand::RequestExit))
        );
    }
}
