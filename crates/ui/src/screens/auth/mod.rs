mod bootstrap;
mod common;
mod login;
mod model;
mod setup;

pub use bootstrap::render_bootstrap_admin;
pub(crate) use bootstrap::render_bootstrap_admin_context;
pub(crate) use login::render_login_context;
pub use login::{
    LoginLayout, login_layout, login_password_area, login_password_visibility_area,
    login_selected_username_area, login_user_list_area, login_user_list_visible_rows, render_login,
};
pub use model::*;
pub(crate) use setup::render_setup_context;
pub use setup::{
    render_setup, setup_admin_field_area, setup_appearance_field_area,
    setup_appearance_palette_option_areas, setup_appearance_shape_option_areas,
    setup_custom_color_dialog_area, setup_custom_color_input_area, setup_language_list_area,
    setup_timezone_list_area, setup_timezone_visible_rows,
};
