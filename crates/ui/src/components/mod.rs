//! Reusable component primitives.
//!
//! Interaction contracts shared by all components:
//! - The app/router owns focus order, global shortcuts, and shortcut conflict detection.
//! - Crossterm events are normalized into [`InputEvent`] before components see them.
//! - Component ids are stable across frames and are used by the router for focus and actions.
//! - Hit testing always receives the current render area, so resize handling stays external.

mod button;
mod command_palette;
mod context_menu;
mod dialog;
mod foundation;
mod list;
mod tabs;
mod text_input;

pub use button::Button;
pub use command_palette::{CommandPalette, CommandPaletteCommand};
pub use context_menu::{ContextMenu, ContextMenuItem};
pub use dialog::{Dialog, DialogAction};
pub use foundation::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, KeyInput, KeyModifiers,
    MouseButton, MouseInput, MouseKind, contains_point,
};
pub use list::{List, ListItem};
pub use tabs::{TabItem, Tabs};
pub use text_input::TextInput;

pub(crate) use foundation::{
    byte_index_for_char, char_count, clamp_index, inner_area, interactive_style, item_style,
};
