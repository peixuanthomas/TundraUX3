//! Context-aware public boundaries for shell wiring. Legacy theme-only entry
//! points remain exported for compatibility tests.

use ratatui::{Frame, layout::Rect};

use crate::*;

pub fn render_home_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &HomeViewModel,
    context: &RenderContext,
) {
    super::home::render_home_with_icons_context(frame, area, chrome, model, context, None);
}
pub fn render_setup_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SetupViewModel,
    context: &RenderContext,
) {
    super::auth::render_setup_context(frame, area, chrome, model, context);
}
pub fn render_bootstrap_admin_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &BootstrapAdminViewModel,
    context: &RenderContext,
) {
    super::auth::render_bootstrap_admin_context(frame, area, chrome, model, context);
}
pub fn render_login_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &LoginViewModel,
    context: &RenderContext,
) {
    super::auth::render_login_context(frame, area, chrome, model, context);
}
pub fn render_launcher_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &LauncherViewModel,
    context: &RenderContext,
) {
    super::launcher::render_launcher_with_icons_context(frame, area, chrome, model, context, None);
}
pub fn render_clock_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &ClockViewModel,
    context: &RenderContext,
) {
    super::clock::render_clock_context(frame, area, chrome, model, context);
}
pub fn render_clock_placeholder_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &ClockViewModel,
    context: &RenderContext,
) {
    super::clock::render_clock_context(frame, area, chrome, model, context);
}
pub fn render_diagnostics_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &DiagnosticsViewModel,
    context: &RenderContext,
) {
    render_diagnostics_contextual(frame, area, chrome, model, context);
}
pub fn render_user_management_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &UserManagementViewModel,
    context: &RenderContext,
) {
    render_user_management_contextual(frame, area, chrome, model, context);
}
pub fn render_command_line_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &CommandLineViewModel,
    context: &RenderContext,
) {
    render_command_line_contextual(frame, area, chrome, model, context);
}

pub fn render_settings_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &SettingsViewModel,
    context: &RenderContext,
) -> SettingsLayout {
    super::settings::render_settings_context(frame, area, chrome, model, context)
}

pub fn render_editor_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &EditorViewModel,
    context: &RenderContext,
) -> EditorLayout {
    render_editor_contextual(frame, area, model, context)
}

pub fn render_editor_app_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    model: &EditorViewModel,
    context: &RenderContext,
) -> EditorLayout {
    render_editor_app_contextual(frame, area, chrome, model, context)
}

pub fn render_notification_overlay_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &NotificationViewModel,
    context: &RenderContext,
) {
    super::notifications::render_notification_overlay_context(frame, area, model, context);
}

pub fn render_exit_confirmation_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExitConfirmViewModel,
    context: &RenderContext,
) {
    render_exit_confirmation_contextual(frame, area, model, context);
}

pub fn render_time_sync_failure_dialog_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &TimeSyncDialogViewModel,
    context: &RenderContext,
) {
    render_time_sync_failure_dialog_contextual(frame, area, model, context);
}
