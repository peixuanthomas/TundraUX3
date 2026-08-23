//! Reusable component primitives.
//!
//! Interaction contracts shared by all components:
//! - The app/router owns focus order, global shortcuts, and shortcut conflict detection.
//! - Crossterm events are normalized into [`InputEvent`] before components see them.
//! - Component ids are stable across frames and are used by the router for focus and actions.
//! - Hit testing always receives the current render area, so resize handling stays external.

mod big_text;
mod button;
mod command_palette;
mod context_menu;
mod data_table;
mod dialog;
mod empty_state;
mod foundation;
mod list;
mod nav_rail;
mod picker;
mod scrollbar;
mod skeleton;
mod surface;
mod tabs;
mod text_input;
mod toast;

pub use button::Button;
pub use command_palette::{CommandPalette, CommandPaletteCommand};
pub use context_menu::{ContextMenu, ContextMenuItem};
pub use data_table::DataTable;
pub use dialog::{Dialog, DialogAction};
pub use empty_state::EmptyState;
pub use foundation::{
    ComponentEvent, ComponentId, ComponentState, InputEvent, Key, KeyInput, KeyModifiers,
    MouseButton, MouseInput, MouseKind, contains_point,
};
pub use list::{List, ListItem};
pub use nav_rail::{NavRail, NavRailItem};
pub use picker::Picker;
pub use scrollbar::{Scrollbar, ScrollbarOrientation};
pub use skeleton::Skeleton;
pub use surface::{Panel, Surface};
pub use tabs::{TabItem, Tabs};
pub use text_input::TextInput;
/// Glacier name for the established editable text component.
pub type TextField = TextInput;
pub use toast::{Toast, ToastTone};

pub(crate) use big_text::{BigText, heading_size_ratio};
pub(crate) use foundation::{
    byte_index_for_char, char_count, clamp_index, inner_area, interactive_style, item_style,
    terminal_width, truncate_to_terminal_width,
};
