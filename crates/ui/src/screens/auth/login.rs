use ratatui::Frame;
use ratatui::layout::{Constraint, HorizontalAlignment, Layout, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Wrap};

use super::{LoginField, LoginViewModel};
use crate::components::{Button, List, ListItem, Surface, TextInput};
use crate::screens::shell::{
    ShellChromeViewModel, ShellLayout, compute_shell_layout, render_compact_home, render_status,
    render_top,
};
use crate::{RenderContext, TundraTheme};

const LOGIN_USER_LIST_WIDTH: u16 = 30;
const LOGIN_USERNAME_FIELD_HEIGHT: u16 = 5;
const LOGIN_PASSWORD_FIELD_HEIGHT: u16 = 3;
const LOGIN_FORM_GAP: u16 = 1;
const LOGIN_PASSWORD_VISIBILITY_WIDTH: u16 = 6;
const LOGIN_CONTROL_GAP: u16 = 1;

/// Shared Login page geometry for rendering and input hit-testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginLayout {
    pub user_list: Rect,
    pub selected_username: Rect,
    pub password: Rect,
    pub password_visibility: Rect,
    pub help: Rect,
}

pub fn render_login(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &LoginViewModel,
    theme: &TundraTheme,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_login_context(frame, area, chrome, model, &context);
}

pub(crate) fn render_login_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &LoginViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    match compute_shell_layout(area) {
        ShellLayout::Compact(compact) => render_compact_home(frame, compact, chrome, theme),
        ShellLayout::Full { top, main, status } => {
            render_top(frame, top, chrome, theme);
            render_login_main(frame, main, model, context);
            render_status(frame, status, chrome, theme);
        }
    }
}

fn render_login_main(
    frame: &mut Frame<'_>,
    main: Rect,
    model: &LoginViewModel,
    context: &RenderContext,
) {
    let theme = &context.compatibility_theme();
    Surface::new()
        .titled("Login")
        .bordered(true)
        .render_frame(frame, main, context);

    let layout = login_layout(main);

    render_login_user_list(frame, layout.user_list, model, context);
    render_login_username_field(frame, layout.selected_username, model, context);
    render_login_password_field(frame, layout.password, model, context);
    render_login_button(
        frame,
        layout.password_visibility,
        if model.password_is_visible() {
            "[Hide]"
        } else {
            "[Show]"
        },
        model.focused_field == LoginField::PasswordVisibility,
        theme,
    );
    if layout.help.height > 0 {
        let mut lines = vec![
            Line::from("Users: Up/Down/Home/End    Tab: password/show"),
            Line::from("Enter: activate    F2: show/hide    Esc: exit"),
        ];
        if let Some(error) = &model.error {
            lines.push(Line::from(""));
            lines.push(Line::styled(error.clone(), theme.error_style()));
        }
        frame.render_widget(
            Paragraph::new(lines)
                .alignment(HorizontalAlignment::Left)
                .style(theme.muted_style())
                .wrap(Wrap { trim: true }),
            layout.help,
        );
    }
}

fn render_login_user_list(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &LoginViewModel,
    context: &RenderContext,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let visible_rows = area.height.saturating_sub(2) as usize;
    let (start, _) = login_user_window_bounds(model, visible_rows);
    let items: Vec<ListItem> = if model.users.is_empty() {
        vec![ListItem::new("login.user.empty", "(no local users)")]
    } else {
        model
            .users
            .iter()
            .enumerate()
            .map(|(index, user)| {
                let mut suffix = String::new();
                if !user.enabled {
                    suffix.push_str(" disabled");
                }
                if user.locked {
                    suffix.push_str(" locked");
                }
                let label = if suffix.is_empty() {
                    format!("{} ({})", user.username, user.role)
                } else {
                    format!("{} ({}) |{}", user.username, user.role, suffix)
                };
                ListItem::new(format!("login.user.{index}"), label).disabled(!user.enabled)
            })
            .collect()
    };

    let mut list = List::new("login.users", items)
        .titled("Users")
        .with_viewport_start(start);
    list.set_focused(model.focused_field == LoginField::UserList);
    list.set_selected((!model.users.is_empty()).then_some(model.selected_index));
    let mut list_theme = context.compatibility_theme();
    if model.focused_field == LoginField::UserList {
        list_theme.border_color = context.theme.focus;
    }
    list.render_frame(frame, area, &list_theme);
}

fn render_login_username_field(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &LoginViewModel,
    context: &RenderContext,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let theme = &context.compatibility_theme();
    let selected = model.selected_user();
    let username = selected
        .map(|user| user.username.clone())
        .unwrap_or_else(|| "No user selected".to_string());
    let display = selected
        .map(|user| user.display_name.clone())
        .unwrap_or_else(|| "Choose a local account".to_string());
    let role = selected
        .map(|user| user.role.clone())
        .unwrap_or_else(|| "Unavailable".to_string());

    let lines = vec![
        Line::styled(username, theme.title_style()),
        Line::from(display),
        Line::styled(role, theme.muted_style()),
    ];
    let surface = Surface::new().titled("Selected User").bordered(true);
    let inner = surface.inner(area);
    surface.render_frame(frame, area, context);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(HorizontalAlignment::Center)
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn render_login_password_field(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &LoginViewModel,
    context: &RenderContext,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let theme = &context.compatibility_theme();
    let focused = model.focused_field == LoginField::Password;
    let surface = Surface::new().titled("Password").bordered(true);
    let inner = surface.inner(area);
    surface.render_frame(
        frame,
        area,
        &RenderContext {
            theme: if focused {
                crate::ThemeTokens {
                    border: context.theme.focus,
                    ..context.theme
                }
            } else {
                context.theme
            },
            ..*context
        },
    );
    frame.render_widget(
        Paragraph::new("Password").style(if focused {
            theme.title_style()
        } else {
            theme.body_style()
        }),
        Rect::new(
            area.x.saturating_add(1),
            area.y,
            area.width.saturating_sub(2),
            u16::from(area.height > 0),
        ),
    );

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let (password, placeholder) = if let Some(visible) = model.visible_password() {
        (visible.to_string(), "")
    } else {
        ("*".repeat(model.password_len), "Enter password")
    };
    let mut input = TextInput::new("login.password")
        .with_placeholder(placeholder)
        .with_cursor_symbol("");
    input.set_value(password);
    // The controller owns the password and the outer block owns focus. Keeping
    // the borderless content renderer unfocused preserves the prior placeholder
    // styling and deliberately avoids introducing a visible component cursor.
    let mut input_theme = *theme;
    if focused {
        input_theme.foreground = theme.accent_color;
    }
    input.render_borderless_frame(
        frame,
        Rect::new(inner.x, inner.y, inner.width, 1),
        &input_theme,
    );
}

fn render_login_button(
    frame: &mut Frame<'_>,
    area: Rect,
    label: &'static str,
    selected: bool,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let line = Rect::new(
        area.x,
        area.y.saturating_add(area.height / 2),
        area.width,
        1,
    );
    let mut button = Button::new("login.password-visibility", label);
    button.set_focused(selected);
    // Inline focused actions historically use the title style. Combining the
    // component's focused and hover affordances preserves that visual contract.
    button.state.hovered = selected;
    button.render_borderless_frame(frame, line, theme);
}

pub fn login_layout(main: Rect) -> LoginLayout {
    let (user_list, form) = login_columns(main);
    let reserved_password_height = LOGIN_PASSWORD_FIELD_HEIGHT.saturating_add(LOGIN_FORM_GAP);
    let height = if form.height > reserved_password_height {
        form.height
            .saturating_sub(reserved_password_height)
            .min(LOGIN_USERNAME_FIELD_HEIGHT)
    } else {
        0
    };
    let selected_username = Rect::new(form.x, form.y, form.width, height);
    let gap = if selected_username.height > 0 {
        LOGIN_FORM_GAP.min(form.height.saturating_sub(selected_username.height))
    } else {
        0
    };
    let password_y = selected_username
        .y
        .saturating_add(selected_username.height)
        .saturating_add(gap);
    let max_height = form
        .y
        .saturating_add(form.height)
        .saturating_sub(password_y);
    let control_height = max_height.min(LOGIN_PASSWORD_FIELD_HEIGHT);

    let (password_width, control_gap, visibility_width) = if form.width
        >= 3_u16
            .saturating_add(LOGIN_CONTROL_GAP)
            .saturating_add(LOGIN_PASSWORD_VISIBILITY_WIDTH)
    {
        (
            form.width
                .saturating_sub(LOGIN_CONTROL_GAP.saturating_add(LOGIN_PASSWORD_VISIBILITY_WIDTH)),
            LOGIN_CONTROL_GAP,
            LOGIN_PASSWORD_VISIBILITY_WIDTH,
        )
    } else {
        let password_width = form.width.div_ceil(2);
        (password_width, 0, form.width.saturating_sub(password_width))
    };
    let password = Rect::new(form.x, password_y, password_width, control_height);
    let visibility_x = password
        .x
        .saturating_add(password.width)
        .saturating_add(control_gap);
    let password_visibility = Rect::new(visibility_x, password_y, visibility_width, control_height);

    let help_y = password_y
        .saturating_add(control_height)
        .saturating_add(LOGIN_FORM_GAP);
    let help_height = form.y.saturating_add(form.height).saturating_sub(help_y);
    let help = Rect::new(form.x, help_y, form.width, help_height);

    LoginLayout {
        user_list,
        selected_username,
        password,
        password_visibility,
        help,
    }
}

pub fn login_user_list_area(main: Rect) -> Rect {
    login_layout(main).user_list
}

pub fn login_selected_username_area(main: Rect) -> Rect {
    login_layout(main).selected_username
}

pub fn login_password_area(main: Rect) -> Rect {
    login_layout(main).password
}

pub fn login_password_visibility_area(main: Rect) -> Rect {
    login_layout(main).password_visibility
}

pub fn login_user_list_visible_rows(main: Rect) -> usize {
    login_user_list_area(main).height.saturating_sub(2) as usize
}

fn login_columns(main: Rect) -> (Rect, Rect) {
    let inner = login_inner_area(main);
    if inner.width <= LOGIN_USER_LIST_WIDTH.saturating_add(10) {
        let [list, form] =
            Layout::vertical([Constraint::Percentage(45), Constraint::Percentage(55)]).areas(inner);
        return (list, form);
    }

    let [list, form] = Layout::horizontal([
        Constraint::Length(LOGIN_USER_LIST_WIDTH),
        Constraint::Min(30),
    ])
    .areas(inner);
    (list, form)
}

fn login_inner_area(main: Rect) -> Rect {
    Surface::new().bordered(true).inner(main)
}

fn login_user_window_bounds(model: &LoginViewModel, visible_rows: usize) -> (usize, usize) {
    if model.users.is_empty() || visible_rows == 0 {
        return (0, 0);
    }

    let selected = model.selected_index.min(model.users.len() - 1);
    let max_start = model.users.len().saturating_sub(visible_rows);
    let mut start = model.user_window_start.min(max_start);
    if selected < start {
        start = selected;
    } else if selected >= start.saturating_add(visible_rows) {
        start = selected.saturating_add(1).saturating_sub(visible_rows);
    }
    start = start.min(max_start);

    let end = start.saturating_add(visible_rows).min(model.users.len());
    (start, end)
}
