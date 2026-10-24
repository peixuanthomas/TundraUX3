use crate::{
    NotificationActionViewModel, NotificationLevel, NotificationTone, NotificationViewModel,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageHelpScreen {
    Home,
    Setup,
    Login,
    BootstrapAdmin,
    UserManagement,
    Explorer,
    Launcher,
    CommandLine,
    Editor,
    Settings,
    Logs,
    Management,
    Diagnostics,
    SystemStatus,
    Clock,
}

impl PageHelpScreen {
    pub const ALL: [Self; 15] = [
        Self::Home,
        Self::Setup,
        Self::Login,
        Self::BootstrapAdmin,
        Self::UserManagement,
        Self::Explorer,
        Self::Launcher,
        Self::CommandLine,
        Self::Editor,
        Self::Settings,
        Self::Logs,
        Self::Management,
        Self::Diagnostics,
        Self::SystemStatus,
        Self::Clock,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageHelpRow {
    pub keys: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageHelpSection {
    pub title: String,
    pub rows: Vec<PageHelpRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageHelpViewModel {
    pub title: String,
    pub sections: Vec<PageHelpSection>,
    pub close_label: String,
}

fn row(keys: &str, description: String) -> PageHelpRow {
    PageHelpRow {
        keys: keys.into(),
        description,
    }
}

fn section(title: String, rows: Vec<PageHelpRow>) -> PageHelpSection {
    PageHelpSection { title, rows }
}

fn actions(rows: Vec<PageHelpRow>) -> PageHelpSection {
    section(i18n::tr!("ui-help-actions"), rows)
}

fn advanced(rows: Vec<PageHelpRow>) -> PageHelpSection {
    section(i18n::tr!("ui-help-advanced"), rows)
}

fn navigation() -> PageHelpSection {
    section(
        i18n::tr!("ui-help-navigation"),
        vec![
            row("Esc", i18n::tr!("ui-help-nav-back")),
            row("F1", i18n::tr!("ui-help-nav-help")),
            row("Tab", i18n::tr!("ui-help-nav-tabs")),
            row("Enter", i18n::tr!("ui-help-nav-activate")),
        ],
    )
}

pub fn page_help(screen: PageHelpScreen) -> PageHelpViewModel {
    let (title, pages) = match screen {
        PageHelpScreen::Home => (
            i18n::tr!("ui-help-home-title"),
            vec![
                actions(vec![row("E A S M U", i18n::tr!("ui-help-home-entries"))]),
                advanced(vec![
                    row("←→↑↓", i18n::tr!("ui-help-home-move")),
                    row("Ctrl+L", i18n::tr!("ui-help-home-logout")),
                ]),
            ],
        ),
        PageHelpScreen::Explorer => (
            i18n::tr!("ui-help-explorer-title"),
            vec![
                actions(vec![
                    row("Enter", i18n::tr!("ui-help-explorer-open")),
                    row("N", i18n::tr!("ui-help-explorer-new")),
                    row("F2", i18n::tr!("ui-help-explorer-rename")),
                    row("Del", i18n::tr!("ui-help-explorer-delete")),
                    row("Shift+F10", i18n::tr!("ui-help-explorer-menu")),
                    row("F6", i18n::tr!("ui-help-explorer-sort")),
                    row("O", i18n::tr!("ui-help-explorer-options")),
                ]),
                advanced(vec![
                    row("C X V", i18n::tr!("ui-help-explorer-clipboard")),
                    row("Ctrl+A", i18n::tr!("ui-help-explorer-select-all")),
                    row("F5", i18n::tr!("ui-help-explorer-refresh")),
                    row("/", i18n::tr!("ui-help-explorer-search")),
                ]),
            ],
        ),
        PageHelpScreen::Launcher => (
            i18n::tr!("ui-help-launcher-title"),
            vec![
                actions(vec![
                    row("Enter", i18n::tr!("ui-help-launcher-open")),
                    row("V", i18n::tr!("ui-help-launcher-view")),
                    row("Del", i18n::tr!("ui-help-launcher-remove")),
                ]),
                advanced(vec![
                    row("R/F5", i18n::tr!("ui-help-launcher-refresh")),
                    row("←→↑↓", i18n::tr!("ui-help-launcher-move")),
                ]),
            ],
        ),
        PageHelpScreen::Logs => (
            i18n::tr!("ui-help-logs-title"),
            vec![
                actions(vec![
                    row("Enter", i18n::tr!("ui-help-logs-open")),
                    row("P", i18n::tr!("ui-help-logs-follow")),
                    row("F", i18n::tr!("ui-help-logs-filter")),
                    row("M", i18n::tr!("ui-help-logs-more")),
                ]),
                advanced(vec![
                    row("R", i18n::tr!("ui-help-logs-refresh")),
                    row("Esc", i18n::tr!("ui-help-logs-back")),
                ]),
            ],
        ),
        PageHelpScreen::Settings => (
            i18n::tr!("ui-help-settings-title"),
            vec![
                actions(vec![row("Enter / ␣", i18n::tr!("ui-help-settings-open"))]),
                advanced(vec![
                    row("←→↑↓", i18n::tr!("ui-help-settings-adjust")),
                    row("Tab", i18n::tr!("ui-help-settings-section")),
                    row("←→ / 0–9 / Enter", i18n::tr!("ui-help-settings-number")),
                ]),
            ],
        ),
        PageHelpScreen::UserManagement => (
            i18n::tr!("ui-help-users-title"),
            vec![
                actions(vec![
                    row("N", i18n::tr!("ui-help-users-new")),
                    row("E", i18n::tr!("ui-help-users-edit")),
                    row("R", i18n::tr!("ui-help-users-password")),
                    row("M", i18n::tr!("ui-help-users-more")),
                    row("D / U", i18n::tr!("ui-help-users-enabled")),
                    row("C", i18n::tr!("ui-help-users-role")),
                    row("Del", i18n::tr!("ui-help-users-delete")),
                ]),
                advanced(vec![
                    row("Tab", i18n::tr!("ui-help-users-focus")),
                    row("Enter", i18n::tr!("ui-help-users-activate")),
                ]),
            ],
        ),
        PageHelpScreen::Management => (
            i18n::tr!("ui-help-management-title"),
            vec![
                actions(vec![
                    row("Enter", i18n::tr!("ui-help-management-action")),
                    row("Tab", i18n::tr!("ui-help-management-focus")),
                ]),
                advanced(vec![
                    row("F4", i18n::tr!("ui-help-management-details")),
                    row("Alt+Left Alt+Right", i18n::tr!("ui-help-management-pages")),
                ]),
            ],
        ),
        PageHelpScreen::SystemStatus => (
            i18n::tr!("ui-help-system-status-title"),
            vec![
                actions(vec![
                    row("E", i18n::tr!("ui-help-system-status-edit")),
                    row("R/F5", i18n::tr!("ui-help-system-status-refresh")),
                    row("H", i18n::tr!("ui-help-system-status-diagnostics")),
                    row("Ctrl+S", i18n::tr!("ui-help-system-status-save")),
                    row("A", i18n::tr!("ui-help-system-status-add")),
                    row("F4", i18n::tr!("ui-help-system-status-size")),
                    row("Del", i18n::tr!("ui-help-system-status-remove")),
                    row("Esc", i18n::tr!("ui-help-system-status-cancel")),
                ]),
                advanced(vec![row(
                    "Shift+←→↑↓ / Enter",
                    i18n::tr!("ui-help-system-status-move"),
                )]),
            ],
        ),
        PageHelpScreen::Diagnostics => (
            i18n::tr!("ui-help-diagnostics-title"),
            vec![
                actions(vec![
                    row("R", i18n::tr!("ui-help-diagnostics-rescan")),
                    row("F", i18n::tr!("ui-help-diagnostics-repair")),
                    row("A", i18n::tr!("ui-help-diagnostics-repair-all")),
                ]),
                advanced(vec![
                    row("C", i18n::tr!("ui-help-diagnostics-copy")),
                    row("O", i18n::tr!("ui-help-diagnostics-open")),
                    row("X", i18n::tr!("ui-help-diagnostics-restart")),
                ]),
            ],
        ),
        PageHelpScreen::Clock => (
            i18n::tr!("ui-help-clock-title"),
            vec![
                actions(vec![
                    row("N", i18n::tr!("ui-help-clock-new")),
                    row("M", i18n::tr!("ui-help-clock-manage")),
                ]),
                advanced(vec![
                    row("F3 F4", i18n::tr!("ui-help-clock-create")),
                    row("←→ / Tab / ↑↓ / 0–9", i18n::tr!("ui-help-clock-input")),
                    row("Enter Esc", i18n::tr!("ui-help-clock-confirm")),
                ]),
            ],
        ),
        PageHelpScreen::Setup | PageHelpScreen::Login | PageHelpScreen::BootstrapAdmin => (
            i18n::tr!(match screen {
                PageHelpScreen::Setup => "ui-help-setup-title",
                PageHelpScreen::BootstrapAdmin => "ui-help-bootstrap-title",
                _ => "ui-help-auth-title",
            }),
            vec![
                actions(vec![
                    row("Enter", i18n::tr!("ui-help-auth-submit")),
                    row("Esc", i18n::tr!("ui-help-auth-back")),
                ]),
                advanced(if screen == PageHelpScreen::Login {
                    vec![
                        row("Tab / ←→↑↓", i18n::tr!("ui-help-auth-focus")),
                        row("F2", i18n::tr!("ui-help-auth-password")),
                    ]
                } else {
                    vec![row("Tab / ←→↑↓", i18n::tr!("ui-help-auth-focus"))]
                }),
            ],
        ),
        PageHelpScreen::Editor => (
            i18n::tr!("ui-help-editor-title"),
            vec![
                actions(vec![
                    row("Ctrl+S", i18n::tr!("ui-help-editor-save")),
                    row("Ctrl+F", i18n::tr!("ui-help-editor-find")),
                    row("R", i18n::tr!("ui-help-editor-reload")),
                ]),
                advanced(vec![
                    row("F8", i18n::tr!("ui-help-editor-vim")),
                    row("F6", i18n::tr!("ui-help-editor-focus")),
                    row("Tab / ←→ / Enter", i18n::tr!("ui-help-editor-settings")),
                ]),
            ],
        ),
        PageHelpScreen::CommandLine => (
            i18n::tr!("ui-help-command-line-title"),
            vec![
                actions(vec![row(
                    &i18n::tr!("ui-help-back-button"),
                    i18n::tr!("ui-help-command-line-close"),
                )]),
                advanced(vec![row("F1", i18n::tr!("ui-help-command-line-keys"))]),
            ],
        ),
    };
    let mut sections = pages;
    sections.push(navigation());
    PageHelpViewModel {
        title,
        sections,
        close_label: i18n::tr!("ui-help-close"),
    }
}

pub fn page_help_notification(
    help: &PageHelpViewModel,
    scroll_offset: usize,
) -> NotificationViewModel {
    let mut message = String::new();
    for section in &help.sections {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str(&section.title);
        for row in &section.rows {
            message.push('\n');
            message.push_str(&row.keys);
            message.push_str("  ");
            message.push_str(&row.description);
        }
    }
    let mut model = NotificationViewModel::new(
        "page-help",
        NotificationLevel::Modal,
        NotificationTone::Info,
        help.title.clone(),
        message,
        vec![NotificationActionViewModel::new(
            "close",
            help.close_label.clone(),
        )],
    );
    model.stacked_actions = true;
    model.actions[0].selected = true;
    model.scroll_offset = scroll_offset;
    model.align_left = true;
    model.show_tone_prefix = false;
    model
}

pub fn render_page_help_overlay(
    frame: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    help: &PageHelpViewModel,
    scroll: usize,
    context: &crate::RenderContext,
) {
    let model = page_help_notification(help, scroll);
    crate::screens::notifications::render_notification_overlay_context(
        frame, area, &model, context,
    );
}

#[cfg(test)]
#[path = "../../tests/unit/screens/help/tests.rs"]
mod tests;
