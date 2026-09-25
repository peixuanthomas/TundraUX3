//! Composition harness for existing page fixtures. Production frames use ScreenCompositor.
// Each integration-test crate imports only its own page fixture wrappers.
#![allow(dead_code)]
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

macro_rules! themed_page {
    ($name:ident, $model:ty, $variant:ident) => {
        pub fn $name(
            frame: &mut Frame<'_>,
            area: Rect,
            chrome: &ShellChromeViewModel,
            model: &$model,
            theme: &TundraTheme,
        ) {
            let context = RenderContext::from_theme(theme, Default::default(), Default::default());
            compose(
                frame,
                area,
                chrome,
                ScreenContent::$variant(model),
                &context,
                None,
                None,
            );
        }
    };
}

themed_page!(render_home, HomeViewModel, Home);
themed_page!(render_setup, SetupViewModel, Setup);
themed_page!(render_login, LoginViewModel, Login);
themed_page!(
    render_bootstrap_admin,
    BootstrapAdminViewModel,
    BootstrapAdmin
);
themed_page!(
    render_user_management,
    UserManagementViewModel,
    UserManagement
);
themed_page!(render_explorer, ExplorerViewModel, Explorer);
themed_page!(render_launcher, LauncherViewModel, Launcher);
themed_page!(render_command_line, CommandLineViewModel, CommandLine);
themed_page!(render_diagnostics, DiagnosticsViewModel, Diagnostics);
themed_page!(render_system_status, SystemStatusViewModel, SystemStatus);
themed_page!(render_clock, ClockViewModel, Clock);

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

pub fn render_settings(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SettingsViewModel,
    theme: &TundraTheme,
) -> SettingsLayout {
    let context = &RenderContext::from_theme(theme, Default::default(), Default::default());
    let layout = compose(
        frame,
        area,
        chrome,
        ScreenContent::Settings(model),
        context,
        None,
        None,
    );
    settings_layout(layout.main, model)
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
