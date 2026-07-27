mod model;
mod render;

pub use model::{HomeDisplayMode, HomeViewModel, ShellEntry};
pub use render::{
    HomeIconRenderer, home_entry_icon_area, home_entry_index_at, home_entry_tile_areas,
    home_logout_area, render_home, render_home_with_icons,
};
