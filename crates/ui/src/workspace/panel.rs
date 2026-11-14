use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};
use workspace_protocol::{SessionId, Size};

use crate::components::{
    Button, ButtonEmphasis, ComponentState, DialogLayout, Key, PageAction, PageActionBar,
    row_style, terminal_width,
};
use crate::screens::shell::{centered_rect, fit_cell};
use crate::{RenderContext, TundraTheme, dim_modal_backdrop};

use super::WorkspaceSplitMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceAction {
    Dismiss,
    Add,
    SetMode(WorkspaceSplitMode),
    Select(SessionId),
    Switch(SessionId),
    Show(SessionId),
    Background(SessionId),
    Maximize(SessionId),
    Close(SessionId),
    ForceKill(SessionId),
    ScrollUp,
    ScrollDown,
}

impl WorkspaceAction {
    pub fn id(self) -> String {
        match self {
            Self::Dismiss => "workspace.dismiss".into(),
            Self::Add => "workspace.add".into(),
            Self::SetMode(WorkspaceSplitMode::LeftRight) => "workspace.left-right".into(),
            Self::SetMode(WorkspaceSplitMode::TopBottom) => "workspace.top-bottom".into(),
            Self::SetMode(WorkspaceSplitMode::Grid) => "workspace.grid".into(),
            Self::Select(id) => format!("workspace.select.{id}"),
            Self::Switch(id) => format!("workspace.switch.{id}"),
            Self::Show(id) => format!("workspace.show.{id}"),
            Self::Background(id) => format!("workspace.background.{id}"),
            Self::Maximize(id) => format!("workspace.maximize.{id}"),
            Self::Close(id) => format!("workspace.close.{id}"),
            Self::ForceKill(id) => format!("workspace.force-kill.{id}"),
            Self::ScrollUp => "workspace.scroll-up".into(),
            Self::ScrollDown => "workspace.scroll-down".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceWindowViewModel {
    pub id: SessionId,
    pub title: String,
    pub minimum_size: Size,
    pub visible: bool,
    pub unresponsive: bool,
    pub closing: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspacePanelViewModel {
    pub windows: Vec<WorkspaceWindowViewModel>,
    pub focused: Option<SessionId>,
    pub selected: Option<SessionId>,
    pub mode: WorkspaceSplitMode,
    pub size_needed: Size,
    pub capacity: usize,
    pub blocked: bool,
    pub scroll: usize,
    pub action_focus: Option<WorkspaceAction>,
    pub hovered: Option<WorkspaceAction>,
    pub pressed: Option<WorkspaceAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePanel {
    pub area: Rect,
    pub list: Rect,
    pub scroll: usize,
    pub max_scroll: usize,
    pub hits: Vec<(WorkspaceAction, Rect)>,
    available: Rect,
    dialog: DialogLayout,
    summary: Vec<String>,
    selected: Option<SessionId>,
    buttons: Vec<(PanelButton, Rect)>,
}

impl WorkspacePanel {
    pub fn new(available: Rect, model: &WorkspacePanelViewModel) -> Self {
        let width = available.width.min(94);
        let body_width = DialogLayout::content_width(width);
        let selected = model
            .windows
            .iter()
            .find(|window| Some(window.id) == model.selected)
            .or_else(|| {
                model
                    .windows
                    .iter()
                    .find(|window| Some(window.id) == model.focused)
            })
            .or_else(|| model.windows.first());
        let mode_buttons = mode_buttons(model.mode);
        let window_buttons = selected.map(window_buttons).unwrap_or_default();
        let mode_bar = action_bar(&mode_buttons);
        let window_bar = action_bar(&window_buttons);
        let mode_rows = mode_bar.height(body_width);
        let window_rows = window_bar.height(body_width);
        let summary = if model.blocked {
            workspace_resize_message(model.size_needed)
        } else {
            i18n::tr!("ui-workspace-capacity", capacity = model.capacity)
        };
        let summary = crate::management_wrapped_lines(&summary, body_width);
        let body_rows = model.windows.len().max(1).saturating_add(summary.len());
        let height = available.height.min(
            u16::try_from(body_rows)
                .unwrap_or(u16::MAX)
                .saturating_add(mode_rows)
                .saturating_add(window_rows)
                .saturating_add(7),
        );
        let area = centered_rect(available, width, height);
        // Keep selected-window actions ahead of layout actions in a short terminal.
        let footer_rows = (mode_rows + window_rows).min(height.saturating_sub(3));
        let close = Key::Escape.label();
        let dialog = DialogLayout::with_min_body(area, footer_rows, Some(&close), 2);
        let summary_rows = u16::try_from(summary.len())
            .unwrap_or(u16::MAX)
            .min(dialog.body.height.saturating_sub(1));
        let mut list = Rect::new(
            dialog.body.x,
            dialog.body.y + summary_rows,
            dialog.body.width,
            dialog.body.height - summary_rows,
        );
        let viewport_len = usize::from(list.height);
        let max_scroll = model.windows.len().saturating_sub(viewport_len);
        let scroll = model.scroll.min(max_scroll);
        if max_scroll > 0 {
            list.width = list.width.saturating_sub(3);
        }

        let window_height = window_rows.min(dialog.footer.height);
        let window_area = Rect::new(
            dialog.footer.x,
            dialog.footer.bottom() - window_height,
            dialog.footer.width,
            window_height,
        );
        let mode_area = Rect::new(
            dialog.footer.x,
            dialog.footer.y,
            dialog.footer.width,
            dialog.footer.height - window_height,
        );
        let mut buttons = place_buttons(mode_buttons, &mode_bar, mode_area);
        buttons.extend(place_buttons(window_buttons, &window_bar, window_area));
        if max_scroll > 0 && list.height >= 2 {
            buttons.extend([
                (
                    PanelButton::new(WorkspaceAction::ScrollUp, "↑".into())
                        .enabled(scroll > 0)
                        .named(i18n::tr!("ui-workspace-scroll-up")),
                    Rect::new(list.right(), list.y, 3.min(dialog.body.width), 1),
                ),
                (
                    PanelButton::new(WorkspaceAction::ScrollDown, "↓".into())
                        .enabled(scroll < max_scroll)
                        .named(i18n::tr!("ui-workspace-scroll-down")),
                    Rect::new(list.right(), list.bottom() - 1, 3.min(dialog.body.width), 1),
                ),
            ]);
        }
        let mut hits = model
            .windows
            .iter()
            .skip(scroll)
            .take(viewport_len)
            .enumerate()
            .filter(|_| !list.is_empty())
            .map(|(index, window)| {
                (
                    WorkspaceAction::Select(window.id),
                    Rect::new(list.x, list.y + index as u16, list.width, 1),
                )
            })
            .collect::<Vec<_>>();
        hits.extend(button_hits(&buttons));
        if !dialog.close.is_empty() {
            hits.push((WorkspaceAction::Dismiss, dialog.close));
        }
        Self {
            area,
            list,
            scroll,
            max_scroll,
            hits,
            available,
            dialog,
            summary: summary
                .into_iter()
                .take(usize::from(summary_rows))
                .collect(),
            selected: selected.map(|window| window.id),
            buttons,
        }
    }

    pub fn hit_at(&self, point: (u16, u16)) -> Option<WorkspaceAction> {
        hit_at(&self.hits, point)
    }
}

pub fn render_workspace_panel(
    frame: &mut Frame<'_>,
    panel: &WorkspacePanel,
    model: &WorkspacePanelViewModel,
    context: &RenderContext,
) {
    let title = i18n::tr!("ui-workspace-title");
    dim_modal_backdrop(frame.buffer_mut(), panel.available, context);
    let _scope = crate::automation::scope("windows", "dialog", &title, panel.area);
    panel.dialog.render_frame(frame, &title, context);
    let mut theme = DialogLayout::body_theme(context);
    theme.buttons = None;
    let summary = panel.summary.join("\n");
    frame.render_widget(
        Paragraph::new(summary).style(Style::default().fg(if model.blocked {
            context.theme.warning
        } else {
            context.theme.muted
        })),
        Rect::new(
            panel.dialog.body.x,
            panel.dialog.body.y,
            panel.dialog.body.width,
            panel.summary.len() as u16,
        ),
    );
    let state = PanelInteraction {
        focused: model.action_focus,
        hovered: model.hovered,
        pressed: model.pressed,
    };
    if model.windows.is_empty() {
        frame.render_widget(
            Paragraph::new(i18n::tr!("ui-workspace-no-windows")).style(theme.muted_style()),
            panel.list,
        );
    }
    for (offset, window) in model
        .windows
        .iter()
        .skip(panel.scroll)
        .take(usize::from(panel.list.height))
        .enumerate()
    {
        let area = Rect::new(
            panel.list.x,
            panel.list.y + offset as u16,
            panel.list.width,
            1,
        );
        let selected = panel.selected == Some(window.id);
        let current = model.focused == Some(window.id);
        let action = WorkspaceAction::Select(window.id);
        let state = state.for_action(action).selected(selected);
        let style = row_style(state, &theme, context.theme.surface);
        let background = style.bg.unwrap_or(context.theme.surface);
        let visibility = if window.visible {
            i18n::tr!("ui-workspace-visible")
        } else {
            i18n::tr!("ui-workspace-backgrounded")
        };
        let mut details = vec![visibility];
        if current {
            details.push(i18n::tr!("ui-workspace-focused"));
        }
        if window.unresponsive {
            details.push(i18n::tr!("ui-workspace-unresponsive"));
        } else if window.closing {
            details.push(i18n::tr!("ui-workspace-closing"));
        }
        details.push(i18n::tr!(
            "ui-workspace-window-minimum",
            width = window.minimum_size.width,
            height = window.minimum_size.height
        ));
        let details = details.join(" · ");
        let description_width = terminal_width(&details).min(usize::from(area.width) / 2 + 4);
        let label_width = usize::from(area.width).saturating_sub(description_width + 3);
        let title = one_line(&window.title);
        let marker = if selected { "▌ " } else { "  " };
        let detail_color = if window.unresponsive {
            context.theme.warning
        } else {
            context.theme.muted
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(marker, style.remove_modifier(Modifier::UNDERLINED)),
                Span::styled(fit_cell(&title, label_width), style),
                Span::styled(" ", style.remove_modifier(Modifier::UNDERLINED)),
                Span::styled(
                    fit_cell(&details, description_width),
                    Style::default()
                        .fg(crate::theme::readable_foreground(detail_color, background)),
                ),
            ]))
            .style(style.remove_modifier(Modifier::UNDERLINED)),
            area,
        );
        crate::automation::control(&action.id(), "listitem", &title, area, state, &["click"]);
    }
    render_buttons(frame, &panel.buttons, state, &theme);
    PanelButton::new(WorkspaceAction::Dismiss, Key::Escape.label())
        .named(i18n::tr!("ui-workspace-dismiss"))
        .render(frame, panel.dialog.close, state, &theme);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCloseWait {
    pub hits: Vec<(WorkspaceAction, Rect)>,
    available: Rect,
    dialog: DialogLayout,
    title: String,
    message: String,
    buttons: Vec<(PanelButton, Rect)>,
}

impl WorkspaceCloseWait {
    pub fn new(available: Rect, window: &str, id: SessionId, size_blocked: bool) -> Self {
        let width = available.width.min(64);
        let body_width = DialogLayout::content_width(width);
        let buttons = vec![
            PanelButton::new(
                WorkspaceAction::Dismiss,
                i18n::tr!("workspace-close-wait-continue"),
            )
            .emphasis(ButtonEmphasis::Primary),
            PanelButton::new(
                WorkspaceAction::ForceKill(id),
                i18n::tr!("workspace-close-wait-force"),
            )
            .emphasis(ButtonEmphasis::Danger),
        ];
        let bar = action_bar(&buttons);
        let footer_rows = bar.height(body_width);
        let message = if size_blocked {
            i18n::tr!("workspace-close-size-message")
        } else {
            i18n::tr!("workspace-close-wait-message")
        };
        let lines = crate::management_wrapped_lines(&message, body_width);
        let height = available.height.min(
            footer_rows
                .saturating_add(lines.len() as u16)
                .saturating_add(7),
        );
        let area = centered_rect(available, width, height);
        let dialog = DialogLayout::new(area, footer_rows, None);
        let buttons = place_buttons(buttons, &bar, dialog.footer);
        let title = i18n::tr!("workspace-close-wait-title", window = one_line(window));
        Self {
            hits: button_hits(&buttons).collect(),
            available,
            dialog,
            title,
            message: lines.join("\n"),
            buttons,
        }
    }

    pub fn hit_at(&self, point: (u16, u16)) -> Option<WorkspaceAction> {
        hit_at(&self.hits, point)
    }
}

pub fn render_workspace_close_wait(
    frame: &mut Frame<'_>,
    wait: &WorkspaceCloseWait,
    focused: Option<WorkspaceAction>,
    hovered: Option<WorkspaceAction>,
    pressed: Option<WorkspaceAction>,
    context: &RenderContext,
) {
    dim_modal_backdrop(frame.buffer_mut(), wait.available, context);
    let _scope = crate::automation::scope("close-wait", "dialog", &wait.title, wait.dialog.area);
    wait.dialog.render_frame(frame, &wait.title, context);
    frame.render_widget(
        Paragraph::new(wait.message.as_str()).style(Style::default().fg(context.theme.muted)),
        wait.dialog.body,
    );
    let mut theme = DialogLayout::footer_theme(context);
    theme.buttons = None;
    render_buttons(
        frame,
        &wait.buttons,
        PanelInteraction {
            focused: focused.or(Some(WorkspaceAction::Dismiss)),
            hovered,
            pressed,
        },
        &theme,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspacePanePlaceholder {
    Starting,
    Closing,
    Unresponsive,
    Unavailable,
}

pub fn render_workspace_pane_placeholder(
    frame: &mut Frame<'_>,
    area: Rect,
    state: WorkspacePanePlaceholder,
    context: &RenderContext,
) {
    let text = match state {
        WorkspacePanePlaceholder::Starting => i18n::tr!("ui-workspace-starting"),
        WorkspacePanePlaceholder::Closing => i18n::tr!("ui-workspace-closing"),
        WorkspacePanePlaceholder::Unresponsive => i18n::tr!("ui-workspace-unresponsive"),
        WorkspacePanePlaceholder::Unavailable => i18n::tr!("ui-components-unavailable"),
    };
    let color = if state == WorkspacePanePlaceholder::Unresponsive {
        context.theme.warning
    } else {
        context.theme.muted
    };
    render_notice(frame, area, &text, color, context);
}

pub fn render_workspace_resize_notice(
    frame: &mut Frame<'_>,
    area: Rect,
    actual: Size,
    needed: Size,
    context: &RenderContext,
) {
    let message = workspace_resize_message(needed);
    let size = i18n::tr!(
        "ui-workspace-terminal-size",
        width = actual.width,
        height = actual.height
    );
    let text = format!("{message}\n{size}");
    render_notice(frame, area, &text, context.theme.warning, context);
}

fn render_notice(
    frame: &mut Frame<'_>,
    area: Rect,
    text: &str,
    color: ratatui::style::Color,
    context: &RenderContext,
) {
    frame.render_widget(Clear, area);
    frame
        .buffer_mut()
        .set_style(area, Style::default().bg(context.theme.canvas));
    let lines = crate::management_wrapped_lines(text, area.width);
    let height = (lines.len() as u16).min(area.height);
    let area = centered_rect(area, area.width, height);
    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .alignment(HorizontalAlignment::Center)
            .style(Style::default().fg(color)),
        area,
    );
}

fn workspace_resize_message(size: Size) -> String {
    i18n::tr!(
        "ui-workspace-resize-required",
        width = size.width,
        height = size.height
    )
}

pub(super) fn one_line(text: &str) -> String {
    text.replace(char::is_control, " ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PanelButton {
    action: WorkspaceAction,
    label: String,
    name: Option<String>,
    emphasis: ButtonEmphasis,
    enabled: bool,
    current: bool,
}

impl PanelButton {
    fn new(action: WorkspaceAction, label: String) -> Self {
        Self {
            action,
            label,
            name: None,
            emphasis: ButtonEmphasis::Normal,
            enabled: true,
            current: false,
        }
    }

    fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn named(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    fn emphasis(mut self, emphasis: ButtonEmphasis) -> Self {
        self.emphasis = emphasis;
        self
    }

    fn render(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        interaction: PanelInteraction,
        theme: &TundraTheme,
    ) {
        if area.is_empty() {
            return;
        }
        let width = area.width.saturating_sub(u16::from(area.width > 4) * 2);
        let label = crate::management_wrapped_lines(&self.label, width).join("\n");
        let mut button = Button::new(self.action.id(), label)
            .with_capture_name(self.name.as_ref().unwrap_or(&self.label))
            .with_current(self.current)
            .with_emphasis(self.emphasis)
            .with_centered_label(true)
            .with_focus_fill(Some(theme.tokens().accent_soft));
        button.state = interaction.for_action(self.action).disabled(!self.enabled);
        button.render_borderless_frame(frame, area, theme);
    }
}

#[derive(Clone, Copy)]
struct PanelInteraction {
    focused: Option<WorkspaceAction>,
    hovered: Option<WorkspaceAction>,
    pressed: Option<WorkspaceAction>,
}

impl PanelInteraction {
    fn for_action(self, action: WorkspaceAction) -> ComponentState {
        ComponentState::default()
            .focused(self.focused == Some(action))
            .hovered(self.hovered == Some(action))
            .active(self.pressed == Some(action))
    }
}

fn mode_buttons(current: WorkspaceSplitMode) -> Vec<PanelButton> {
    let mut buttons = vec![
        PanelButton::new(WorkspaceAction::Add, i18n::tr!("ui-workspace-add"))
            .emphasis(ButtonEmphasis::Primary),
    ];
    buttons.extend(
        [
            (WorkspaceSplitMode::Grid, i18n::tr!("ui-workspace-grid")),
            (
                WorkspaceSplitMode::TopBottom,
                i18n::tr!("ui-workspace-top-bottom"),
            ),
            (
                WorkspaceSplitMode::LeftRight,
                i18n::tr!("ui-workspace-left-right"),
            ),
        ]
        .into_iter()
        .map(|(mode, label)| {
            let mut button = PanelButton::new(WorkspaceAction::SetMode(mode), label);
            button.current = current == mode;
            button
        }),
    );
    buttons
}

fn window_buttons(window: &WorkspaceWindowViewModel) -> Vec<PanelButton> {
    let id = window.id;
    let show_or_background = if window.visible {
        PanelButton::new(
            WorkspaceAction::Background(id),
            i18n::tr!("ui-workspace-background"),
        )
    } else {
        PanelButton::new(WorkspaceAction::Show(id), i18n::tr!("ui-workspace-show"))
    };
    vec![
        PanelButton::new(
            WorkspaceAction::ForceKill(id),
            i18n::tr!("ui-workspace-force-kill"),
        )
        .emphasis(ButtonEmphasis::Danger),
        PanelButton::new(WorkspaceAction::Close(id), i18n::tr!("ui-workspace-close"))
            .enabled(!window.closing),
        PanelButton::new(
            WorkspaceAction::Maximize(id),
            i18n::tr!("ui-workspace-maximize"),
        )
        .enabled(!window.closing),
        show_or_background.enabled(!window.closing),
        PanelButton::new(
            WorkspaceAction::Switch(id),
            i18n::tr!("ui-workspace-switch"),
        ),
    ]
}

fn action_bar(buttons: &[PanelButton]) -> PageActionBar {
    PageActionBar::new("").with_actions(
        buttons
            .iter()
            .map(|button| PageAction::new(button.action.id(), &button.label))
            .collect(),
    )
}

fn place_buttons(
    buttons: Vec<PanelButton>,
    bar: &PageActionBar,
    area: Rect,
) -> Vec<(PanelButton, Rect)> {
    let mut layout = bar.layout(area).actions;
    layout.sort_by_key(|action| (action.area.y, action.area.x));
    layout
        .into_iter()
        .filter(|action| !action.area.is_empty())
        .map(|action| (buttons[action.index].clone(), action.area))
        .collect()
}

fn render_buttons(
    frame: &mut Frame<'_>,
    buttons: &[(PanelButton, Rect)],
    interaction: PanelInteraction,
    theme: &TundraTheme,
) {
    for (button, area) in buttons {
        button.render(frame, *area, interaction, theme);
    }
}

fn button_hits(buttons: &[(PanelButton, Rect)]) -> impl Iterator<Item = (WorkspaceAction, Rect)> {
    buttons
        .iter()
        .filter(|(button, area)| button.enabled && !area.is_empty())
        .map(|(button, area)| (button.action, *area))
}

fn hit_at(hits: &[(WorkspaceAction, Rect)], point: (u16, u16)) -> Option<WorkspaceAction> {
    hits.iter()
        .find(|(_, area)| area.contains(point.into()))
        .map(|(action, _)| *action)
}
