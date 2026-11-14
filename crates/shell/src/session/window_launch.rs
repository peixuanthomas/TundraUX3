use super::*;
use platform::management::{ManagementKind, ManagementQuery};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::Path;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum WindowOwner {
    #[default]
    Local,
    Shell,
    App(WindowApplication),
}

impl WindowOwner {
    pub(super) fn is_app(self) -> bool {
        matches!(self, Self::App(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowApplication {
    Clock,
    CommandLine,
    Editor,
    Explorer,
    Launcher,
    Logs,
    Management(ManagementKind),
    Settings,
    SystemStatus,
    UserManagement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum WindowLaunch {
    Clock,
    CommandLine {
        directory: Option<OsString>,
        external: Option<ExternalTerminalLaunch>,
    },
    Editor,
    EditorPath(OsString),
    EditorReport {
        path: OsString,
        log: bool,
    },
    Explorer,
    ExplorerPath {
        path: OsString,
        diagnostics: bool,
        select: Option<OsString>,
    },
    Launcher {
        select: Option<OsString>,
    },
    Logs,
    ServiceLogs {
        unit: String,
        scope: String,
        boot_id: Option<String>,
        invocation_id: Option<String>,
        since_usec: Option<u64>,
    },
    Management(ManagementQuery),
    Script,
    Settings,
    SystemStatus,
    UserManagement,
}

impl WindowLaunch {
    pub(super) fn application(&self) -> WindowApplication {
        match self {
            Self::Clock => WindowApplication::Clock,
            Self::CommandLine { .. } => WindowApplication::CommandLine,
            Self::Editor | Self::EditorPath(_) | Self::EditorReport { .. } | Self::Script => {
                WindowApplication::Editor
            }
            Self::Explorer | Self::ExplorerPath { .. } => WindowApplication::Explorer,
            Self::Launcher { .. } => WindowApplication::Launcher,
            Self::Logs | Self::ServiceLogs { .. } => WindowApplication::Logs,
            Self::Management(query) => WindowApplication::Management(query.kind),
            Self::Settings => WindowApplication::Settings,
            Self::SystemStatus => WindowApplication::SystemStatus,
            Self::UserManagement => WindowApplication::UserManagement,
        }
    }

    pub(super) fn id(&self) -> &'static str {
        match self {
            Self::Script => "script",
            Self::CommandLine {
                external: Some(_), ..
            } => "terminal",
            _ => match self.application() {
                WindowApplication::Clock => "clock",
                WindowApplication::CommandLine => "command-line",
                WindowApplication::Editor => "editor",
                WindowApplication::Explorer => "explorer",
                WindowApplication::Launcher => "launcher",
                WindowApplication::Logs => "logs",
                WindowApplication::Management(kind) => kind.id(),
                WindowApplication::Settings => "settings",
                WindowApplication::SystemStatus => "system-status",
                WindowApplication::UserManagement => "users",
            },
        }
    }

    pub(super) fn title(&self) -> String {
        match self {
            Self::Script => app::SCRIPT_EDITOR_APPLICATION
                .localized_name()
                .render_current(),
            Self::CommandLine {
                external: Some(external),
                ..
            } => external.title(),
            _ => match self.application() {
                WindowApplication::Clock => i18n::tr!("shell-clock"),
                WindowApplication::CommandLine => i18n::tr!("shell-command-line"),
                WindowApplication::Editor => i18n::tr!("shell-editor"),
                WindowApplication::Explorer => i18n::tr!("shell-explorer"),
                WindowApplication::Launcher => i18n::tr!("shell-launcher"),
                WindowApplication::Logs => i18n::tr!("shell-logs"),
                WindowApplication::Management(kind) => management_title(kind),
                WindowApplication::Settings => i18n::tr!("settings-title"),
                WindowApplication::SystemStatus => i18n::tr!("shell-system-status"),
                WindowApplication::UserManagement => i18n::tr!("shell-user-management"),
            },
        }
    }

    pub(super) fn activate(self, state: &mut ShellSession, platform: &dyn Platform) {
        match self {
            Self::Clock => state.open_clock(),
            Self::CommandLine {
                directory,
                external,
            } => {
                let directory = directory.map(PathBuf::from);
                if let Some(external) = external {
                    state.open_external_terminal(
                        external.program,
                        external.args,
                        directory,
                        external.wait_for_enter,
                    );
                } else {
                    state.open_command_line_at(directory);
                }
            }
            Self::Editor => state.open_editor(),
            Self::EditorPath(path) => {
                state.open_editor_path(path.into());
            }
            Self::EditorReport { path, log } => {
                let path = path.into();
                let reload = if log {
                    EditorReloadPolicy::Log { path }
                } else {
                    EditorReloadPolicy::DiagnosticsReport { path }
                };
                if let Err(error) = state.open_diagnostics_editor(reload) {
                    state.report_editor_error(error);
                }
            }
            Self::Explorer => state.open_explorer(platform),
            Self::ExplorerPath {
                path,
                diagnostics,
                select,
            } => {
                let Some(storage) = state.storage_manager.clone() else {
                    return;
                };
                let purpose = if diagnostics {
                    ExplorerPurpose::DiagnosticsLogs
                } else {
                    ExplorerPurpose::Browse
                };
                state.open_explorer_at(platform, &storage, path.into(), purpose);
                if let Some(select) = select {
                    state.select_explorer_path(Path::new(&select));
                }
            }
            Self::Launcher { select: Some(path) } => {
                state.open_launcher_for_path(path.into(), platform);
            }
            Self::Launcher { select: None } => state.open_launcher(platform),
            Self::Logs => state.open_logs(),
            Self::ServiceLogs {
                unit,
                scope,
                boot_id,
                invocation_id,
                since_usec,
            } => {
                state.open_related_logs(
                    &unit,
                    &scope,
                    boot_id.as_deref(),
                    invocation_id.as_deref(),
                    since_usec,
                );
            }
            Self::Management(query) => state.open_management_query(query),
            Self::Script => state.open_script_editor(),
            Self::Settings => state.open_settings(),
            Self::SystemStatus => state.open_system_status(),
            Self::UserManagement => state.open_user_management(),
        }
    }
}

impl ShellSession {
    pub(in crate::session) fn initialize_editor_vim_mode(&mut self) {
        if self.window_owner.is_app() && self.editor_vim_enabled.is_none() {
            self.editor_vim_enabled = Some(self.app.storage_config().editor.vim_mode_enabled);
        }
    }

    pub(in crate::session) fn select_explorer_path(&mut self, path: &Path) -> bool {
        self.update_explorer_state(|state| {
            let Some(index) = state.entries.iter().position(|entry| entry.path == path) else {
                return false;
            };
            state.select_index(index, app::explorer::ExplorerSelectionMode::Replace);
            true
        })
        .unwrap_or(false)
    }

    pub(in crate::session) fn route_window_launch(&mut self, launch: WindowLaunch) -> bool {
        use WindowApplication::*;
        let route = match self.window_owner {
            WindowOwner::Local => false,
            WindowOwner::Shell => true,
            WindowOwner::App(owner) if owner == launch.application() => false,
            WindowOwner::App(Settings)
                if launch.application() == Management(ManagementKind::SystemSettings) =>
            {
                false
            }
            WindowOwner::App(Logs) if matches!(&launch, WindowLaunch::EditorReport { .. }) => false,
            WindowOwner::App(_) => true,
        };
        if route {
            if launch.application() == WindowApplication::Management(ManagementKind::Tasks) {
                self.window_tasks_requested = true;
            } else {
                self.window_launches.push_back(launch);
            }
        }
        route
    }
}
