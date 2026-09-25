//! Composition harness for existing page fixtures. Production frames use ScreenCompositor.
#![allow(dead_code, unused_imports)]
pub use ::ui::*;
use ratatui::{Frame, layout::Rect};

fn compose(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    content: ScreenContent<'_>,
    context: &RenderContext,
    home_icons: Option<&dyn HomeIconRenderer>,
    launcher_icons: Option<&dyn LauncherIconRenderer>,
) -> ShellFrameLayout {
    let context = content.render_context(context);
    let layout = ShellFrameLayout::new(area, chrome.status.time_button_label.as_deref(), &context);
    components::Surface::new().render_frame(frame, area, &context);
    if layout.is_compact() && !content.renders_in_compact() {
        render_compact_home(frame, area, chrome, &context.compatibility_theme());
    } else {
        content.render_content(frame, &layout, &context, home_icons, launcher_icons);
        content.render_overlay(frame, &layout, &context);
        render_shell_chrome(frame, &layout, chrome, &context);
    }
    layout
}

pub fn render_home(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &HomeViewModel,
    theme: &TundraTheme,
) {
    let context = &RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::Home(model),
        context,
        None,
        None,
    );
}

pub fn render_home_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &HomeViewModel,
    context: &RenderContext,
    icons: Option<&dyn HomeIconRenderer>,
) {
    compose(
        frame,
        area,
        chrome,
        ScreenContent::Home(model),
        context,
        icons,
        None,
    );
}

pub fn render_home_with_icons(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &HomeViewModel,
    theme: &TundraTheme,
    icons: Option<&dyn HomeIconRenderer>,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::Home(model),
        &context,
        icons,
        None,
    );
}

pub fn render_setup(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SetupViewModel,
    theme: &TundraTheme,
) {
    let context = &RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::Setup(model),
        context,
        None,
        None,
    );
}

pub fn render_login(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &LoginViewModel,
    theme: &TundraTheme,
) {
    let context = &RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::Login(model),
        context,
        None,
        None,
    );
}

pub fn render_bootstrap_admin(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &BootstrapAdminViewModel,
    theme: &TundraTheme,
) {
    let context = &RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::BootstrapAdmin(model),
        context,
        None,
        None,
    );
}

pub fn render_user_management(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &UserManagementViewModel,
    theme: &TundraTheme,
) {
    let context = &RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::UserManagement(model),
        context,
        None,
        None,
    );
}

pub fn render_explorer(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &ExplorerViewModel,
    theme: &TundraTheme,
) {
    let context = &RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::Explorer(model),
        context,
        None,
        None,
    );
}

pub fn render_explorer_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &ExplorerViewModel,
    context: &RenderContext,
) {
    compose(
        frame,
        area,
        chrome,
        ScreenContent::Explorer(model),
        context,
        None,
        None,
    );
}

pub fn render_launcher(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &LauncherViewModel,
    theme: &TundraTheme,
) {
    let context = &RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::Launcher(model),
        context,
        None,
        None,
    );
}

pub fn render_launcher_with_icons(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &LauncherViewModel,
    theme: &TundraTheme,
    icons: Option<&dyn LauncherIconRenderer>,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::Launcher(model),
        &context,
        None,
        icons,
    );
}

pub fn render_command_line(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &CommandLineViewModel,
    theme: &TundraTheme,
) {
    let context = &RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::CommandLine(model),
        context,
        None,
        None,
    );
}

pub fn render_settings(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SettingsViewModel,
    theme: &TundraTheme,
) -> SettingsLayout {
    let context = &RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::Settings(model),
        context,
        None,
        None,
    );
    settings_layout(
        ShellFrameLayout::new(area, chrome.status.time_button_label.as_deref(), context).main,
        model,
    )
}

pub fn render_logs_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &LogsViewModel,
    context: &RenderContext,
) {
    compose(
        frame,
        area,
        chrome,
        ScreenContent::Logs(model),
        context,
        None,
        None,
    );
}

pub fn render_diagnostics(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &DiagnosticsViewModel,
    theme: &TundraTheme,
) {
    let context = &RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::Diagnostics(model),
        context,
        None,
        None,
    );
}

pub fn render_system_status(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SystemStatusViewModel,
    theme: &TundraTheme,
) {
    let context = &RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::SystemStatus(model),
        context,
        None,
        None,
    );
}

pub fn render_clock(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &ClockViewModel,
    theme: &TundraTheme,
) {
    let context = &RenderContext::from_theme(theme, Default::default(), Default::default());
    compose(
        frame,
        area,
        chrome,
        ScreenContent::Clock(model),
        context,
        None,
        None,
    );
}
