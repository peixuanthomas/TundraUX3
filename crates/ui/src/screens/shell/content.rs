//! Page-only rendering inputs. The shell compositor owns frame ordering and chrome.
use crate::*;
use ratatui::Frame;

#[derive(Clone, Copy)]
pub enum ScreenContent<'a> {
    Home(&'a HomeViewModel),
    Setup(&'a SetupViewModel),
    Login(&'a LoginViewModel),
    BootstrapAdmin(&'a BootstrapAdminViewModel),
    UserManagement(&'a UserManagementViewModel),
    Explorer(&'a ExplorerViewModel),
    Launcher(&'a LauncherViewModel),
    CommandLine(&'a CommandLineViewModel),
    Editor(&'a EditorViewModel),
    Settings(&'a SettingsViewModel),
    Logs(&'a LogsViewModel),
    Diagnostics(&'a DiagnosticsViewModel),
    SystemStatus(&'a SystemStatusViewModel),
    Clock(&'a ClockViewModel),
}

impl ScreenContent<'_> {
    pub fn renders_in_compact(self) -> bool {
        matches!(self, Self::Editor(_) | Self::Settings(_))
    }
    pub fn render_context(self, context: &RenderContext) -> RenderContext {
        match self {
            Self::Setup(model) => setup_render_context(model, context),
            _ => *context,
        }
    }
    pub fn render_content(
        self,
        frame: &mut Frame<'_>,
        layout: &ShellFrameLayout,
        context: &RenderContext,
        home_icons: Option<&dyn HomeIconRenderer>,
        launcher_icons: Option<&dyn LauncherIconRenderer>,
    ) {
        let main = layout.main;
        match self {
            Self::Home(model) => render_home_content(frame, main, model, context, home_icons),
            Self::Setup(model) => render_setup_content(frame, main, model, context),
            Self::Login(model) => render_login_content(frame, main, model, context),
            Self::BootstrapAdmin(model) => {
                render_bootstrap_admin_content(frame, main, model, context)
            }
            Self::UserManagement(model) => {
                render_user_management_content(frame, main, model, context)
            }
            Self::Explorer(model) => render_explorer_content(frame, main, model, context),
            Self::Launcher(model) => {
                render_launcher_content(frame, main, model, context, launcher_icons)
            }
            Self::CommandLine(model) => render_command_line_content(
                frame,
                main,
                command_line_terminal_area_in(layout),
                model,
                context,
            ),
            Self::Editor(model) => {
                render_editor_contextual(frame, main, model, context);
            }
            Self::Settings(model) => {
                render_settings_content(frame, &settings_layout(main, model), model, context)
            }
            Self::Logs(model) => render_logs_content(frame, main, model, context),
            Self::Diagnostics(model) => {
                render_diagnostics_page_content(frame, main, model, context)
            }
            Self::SystemStatus(model) => render_system_status_content(frame, main, model, context),
            Self::Clock(model) => render_clock_content(frame, main, model, context),
        }
    }
    pub fn render_overlay(
        self,
        frame: &mut Frame<'_>,
        layout: &ShellFrameLayout,
        context: &RenderContext,
    ) {
        let main = layout.main;
        match self {
            Self::Setup(model) => render_setup_overlay(frame, main, model, context),
            Self::UserManagement(model) => {
                render_user_management_overlay(frame, main, model, context)
            }
            Self::Explorer(model) => {
                render_explorer_overlay(frame, main, model, context, &context.compatibility_theme())
            }
            Self::Launcher(model) => render_launcher_overlay(frame, main, model, context),
            Self::Editor(model) => {
                render_editor_overlay(frame, &editor_layout(main, model), model, context)
            }
            Self::Settings(model) => {
                render_settings_overlay(frame, &settings_layout(main, model), model, context)
            }
            Self::Diagnostics(model) => render_diagnostics_overlay(frame, main, model, context),
            Self::SystemStatus(model) => render_system_status_overlay(frame, main, model, context),
            Self::Clock(model) => render_clock_overlay(frame, main, model, context),
            _ => {}
        }
    }
}
