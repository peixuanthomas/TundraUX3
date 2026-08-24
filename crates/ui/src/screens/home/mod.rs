mod model;
mod render;

pub use model::{HomeDisplayMode, HomeViewModel, ShellEntry};
pub(crate) use render::render_home_context;
pub use render::{
    home_entry_icon_area, home_entry_index_at, home_entry_tile_areas, home_logout_area, render_home,
};
