use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Borders, Clear, Paragraph};
use workspace_protocol::SessionId;

use crate::components::{Button, ButtonEmphasis, ComponentState, terminal_width};
use crate::screens::shell::fit_cell;
use crate::{RenderContext, automation};

use super::WorkspacePaneLayout;

const COMPACT_BUTTON_WIDTH: u16 = 4;
const BUTTON_GAP: u16 = 1;
const MIN_TITLE_WIDTH: u32 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceWindowAction {
    Minimize,
    ToggleMaximize,
    SplitMenu,
    Close,
}

impl WorkspaceWindowAction {
    const ALL: [Self; 4] = [
        Self::Minimize,
        Self::ToggleMaximize,
        Self::SplitMenu,
        Self::Close,
    ];

    pub fn id(self, window: SessionId) -> String {
        let name = match self {
            Self::Minimize => "minimize",
            Self::ToggleMaximize => "maximize",
            Self::SplitMenu => "split",
            Self::Close => "close",
        };
        format!("workspace.{name}.{window}")
    }

    fn label(self, maximized: bool) -> String {
        match self {
            Self::Minimize => i18n::tr!("ui-workspace-minimize"),
            Self::ToggleMaximize if maximized => i18n::tr!("ui-workspace-restore"),
            Self::ToggleMaximize => i18n::tr!("ui-workspace-maximize"),
            Self::SplitMenu => i18n::tr!("ui-workspace-split"),
            Self::Close => i18n::tr!("ui-workspace-close"),
        }
    }

    fn glyph(self, maximized: bool) -> &'static str {
        match self {
            Self::Minimize => "−",
            Self::ToggleMaximize if maximized => "⧉",
            Self::ToggleMaximize => "□",
            Self::SplitMenu => "◫",
            Self::Close => "×",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceWindowControls {
    pub minimize: Rect,
    pub maximize: Rect,
    pub split: Rect,
    pub close: Rect,
}

impl WorkspaceWindowControls {
    const GAPS_WIDTH: u16 = (WorkspaceWindowAction::ALL.len() as u16 - 1) * BUTTON_GAP;
    pub(super) const MIN_WIDTH: u16 =
        WorkspaceWindowAction::ALL.len() as u16 * COMPACT_BUTTON_WIDTH + Self::GAPS_WIDTH;

    pub(super) fn new(top: Rect) -> Self {
        let mut widths = WorkspaceWindowAction::ALL.map(|action| {
            let width =
                terminal_width(&action.label(false)).max(terminal_width(&action.label(true)));
            u16::try_from(width).unwrap_or(u16::MAX).saturating_add(4)
        });
        let buttons_width = widths.iter().map(|width| u32::from(*width)).sum::<u32>();
        if buttons_width + u32::from(Self::GAPS_WIDTH) + MIN_TITLE_WIDTH > u32::from(top.width) {
            widths = [COMPACT_BUTTON_WIDTH; 4];
        }
        let mut right = top.right();
        widths.reverse();
        let [close, split, maximize, minimize] = widths.map(|width| {
            let x = right - width;
            right = x.saturating_sub(BUTTON_GAP);
            Rect::new(x, top.y, width, 1)
        });
        Self {
            minimize,
            maximize,
            split,
            close,
        }
    }

    pub fn rect(self, action: WorkspaceWindowAction) -> Rect {
        match action {
            WorkspaceWindowAction::Minimize => self.minimize,
            WorkspaceWindowAction::ToggleMaximize => self.maximize,
            WorkspaceWindowAction::SplitMenu => self.split,
            WorkspaceWindowAction::Close => self.close,
        }
    }

    pub fn hit_at(self, point: (u16, u16)) -> Option<WorkspaceWindowAction> {
        WorkspaceWindowAction::ALL
            .into_iter()
            .find(|&action| self.rect(action).contains(point.into()))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkspaceWindowButtonsState {
    pub hovered: Option<WorkspaceWindowAction>,
    pub pressed: Option<WorkspaceWindowAction>,
    pub focused: Option<WorkspaceWindowAction>,
    pub close_disabled: bool,
}

pub fn render_workspace_pane_frame(
    frame: &mut Frame<'_>,
    pane: &WorkspacePaneLayout,
    title: &str,
    focused: bool,
    maximized: bool,
    interaction: WorkspaceWindowButtonsState,
    context: &RenderContext,
) {
    let mut theme = context.compatibility_theme();
    theme.buttons = None;
    let background = if focused {
        context.theme.raised
    } else {
        context.theme.surface
    };
    let border = if focused {
        context.theme.accent
    } else {
        context.theme.border
    };
    frame.render_widget(
        theme
            .block()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border).bg(background)),
        pane.area,
    );
    if let Some(area) = pane.title.filter(|area| !area.is_empty()) {
        let title = super::panel::one_line(title);
        automation::control(
            &format!("workspace.title.{}", pane.id),
            "button",
            &title,
            area,
            ComponentState::default().selected(focused),
            &["click"],
        );
        let text = fit_cell(&format!(" {title}"), usize::from(area.width));
        let foreground = crate::theme::readable_foreground(context.theme.text, background);
        frame.render_widget(
            Paragraph::new(text).style(
                Style::default()
                    .fg(foreground)
                    .bg(background)
                    .add_modifier(Modifier::BOLD),
            ),
            area,
        );
    }
    for action in WorkspaceWindowAction::ALL {
        let area = pane.controls.rect(action);
        let label = action.label(maximized);
        let glyph = action.glyph(maximized);
        let text = if terminal_width(&label) + terminal_width(glyph) + 2 <= usize::from(area.width)
        {
            format!("{glyph} {label}")
        } else {
            glyph.into()
        };
        let mut button = Button::new(action.id(pane.id), text)
            .with_capture_name(label)
            .with_centered_label(true)
            .with_emphasis(if action == WorkspaceWindowAction::Close {
                ButtonEmphasis::Danger
            } else {
                ButtonEmphasis::Normal
            });
        button.state = ComponentState::default()
            .focused(interaction.focused == Some(action))
            .hovered(interaction.hovered == Some(action))
            .active(interaction.pressed == Some(action))
            .disabled(interaction.close_disabled && action == WorkspaceWindowAction::Close);
        frame.render_widget(Clear, area);
        button.render_borderless_frame(frame, area, &theme);
    }
}
