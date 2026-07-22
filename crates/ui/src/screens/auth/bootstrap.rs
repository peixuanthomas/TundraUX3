use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::common::{focus_marker, render_auth_screen};
use super::{AuthField, BootstrapAdminViewModel};
use crate::TundraTheme;
use crate::screens::shell::ShellChromeViewModel;

pub fn render_bootstrap_admin(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &BootstrapAdminViewModel,
    theme: &TundraTheme,
) {
    render_auth_screen(
        frame,
        area,
        chrome,
        "Create Admin",
        bootstrap_lines(model),
        theme,
    );
}

fn bootstrap_lines(model: &BootstrapAdminViewModel) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from("Tab / Down: password    Enter on password: create admin    Esc: exit"),
        Line::from(""),
        Line::from(format!(
            "{}Admin username: {}",
            focus_marker(model.focused_field == AuthField::Username),
            model.username
        )),
        Line::from(format!(
            "{}Admin password: {}",
            focus_marker(model.focused_field == AuthField::Password),
            "*".repeat(model.password_len)
        )),
    ];
    if let Some(error) = &model.error {
        lines.push(Line::from(""));
        lines.push(Line::from(error.clone()));
    }
    lines
}
