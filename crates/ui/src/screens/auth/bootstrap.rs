use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::common::render_auth_screen;
use super::{AuthField, BootstrapAdminViewModel};
use crate::components::{Surface, TextInput};
use crate::screens::shell::{ShellChromeViewModel, ShellLayout, compute_shell_layout};
use crate::{RenderContext, TundraTheme};

pub fn render_bootstrap_admin(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &BootstrapAdminViewModel,
    theme: &TundraTheme,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_bootstrap_admin_context(frame, area, chrome, model, &context);
}

pub(crate) fn render_bootstrap_admin_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &BootstrapAdminViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    render_auth_screen(
        frame,
        area,
        chrome,
        "Create Admin",
        bootstrap_lines(model),
        theme,
    );

    let ShellLayout::Full { main, .. } = compute_shell_layout(area) else {
        return;
    };
    let inner = Surface::new().bordered(true).inner(main);
    render_bootstrap_input(
        frame,
        Rect::new(
            inner.x,
            inner.y.saturating_add(2),
            inner.width,
            u16::from(inner.height > 2),
        ),
        "bootstrap.username",
        "Admin username: ",
        &model.username,
        model.focused_field == AuthField::Username,
        theme,
    );
    render_bootstrap_input(
        frame,
        Rect::new(
            inner.x,
            inner.y.saturating_add(3),
            inner.width,
            u16::from(inner.height > 3),
        ),
        "bootstrap.password",
        "Admin password: ",
        &"*".repeat(model.password_len),
        model.focused_field == AuthField::Password,
        theme,
    );
}

#[allow(clippy::too_many_arguments)]
fn render_bootstrap_input(
    frame: &mut Frame<'_>,
    area: Rect,
    id: &'static str,
    label: &'static str,
    value: &str,
    focused: bool,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let prefix = label.to_string();
    let mut input = TextInput::new(id).with_cursor_symbol("_");
    input.set_value(value);
    input.set_focused(focused);
    input.state.hovered = focused;
    input.render_borderless_frame_with_prefix(frame, area, theme, &prefix);
}

fn bootstrap_lines(model: &BootstrapAdminViewModel) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from("Tab / Down: password    Enter on password: create admin    Esc: exit"),
        Line::from(""),
        Line::from(""),
        Line::from(""),
    ];
    if let Some(error) = &model.error {
        lines.push(Line::from(""));
        lines.push(Line::from(error.clone()));
    }
    lines
}
